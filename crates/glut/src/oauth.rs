//! OAuth's parts every sign-in of Glut shares (PV4; first written for MCP, T56): the
//! authorization code with PKCE (S256) and a random `state`, the listener on 127.0.0.1 the browser comes back to, the
//! token endpoint, the browser opened only on the owner's word. What one kind of sign-in adds — MCP's discovery and
//! registration ([`crate::mcp::oauth`]), ChatGPT's own client ([`crate::subscription`]) — stays with it.
//!
//! Every answer is read capped, nothing follows a redirect, and an authorization server's words reach the owner cut
//! and made printable.

use std::io::Read as _;
use std::time::Duration;

use reqwest::header::ACCEPT;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

/// How long one request to an authorization server may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a sign-in waits for the browser to come back: the owner may have to type a password, find a second
/// factor.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The largest metadata or token answer read: a few kilobytes are what they hold.
const MAX_ANSWER_BYTES: usize = 256 * 1024;
/// The largest request the listener reads: the browser's `GET` of the redirect.
const MAX_CALLBACK_BYTES: usize = 16 * 1024;
/// How long the listener waits for one connection to say what it wants: a port scanner that opens and stays silent
/// must not hold the sign-in.
const CALLBACK_READ: Duration = Duration::from_secs(10);
/// The most connections the listener reads at once; more wait until one is done.
const MAX_CALLBACK_READERS: usize = 32;
/// Random bytes in a `state`, a nonce and a PKCE verifier: 256 bits, 43 characters as base64url (RFC 7636 wants
/// 43–128).
const RANDOM_BYTES: usize = 32;
/// How much of an authorization server's error description an error keeps.
const DESCRIPTION_CHARS: usize = 200;

/// Whether a browser of this machine is the owner's: not over SSH, where the owner sits at another one.
pub fn can_open() -> bool {
    !["SSH_CONNECTION", "SSH_TTY", "SSH_CLIENT"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
}

/// Opens `address` in the browser of this machine — never over SSH ([`can_open`]), and only on the owner's word.
/// `false`: the owner opens it.
pub fn open_in_browser(address: &str) -> bool {
    // Only an address of the web: `open` would run whatever else it is handed.
    if !can_open() || !(address.starts_with("https://") || address.starts_with("http://")) {
        return false;
    }
    let program = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(address)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// The client every request to an authorization server goes out with: no redirect followed (it would carry a code
/// or a token elsewhere), a time limit, Glut's name.
pub(crate) fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("glut/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| error.to_string())
}

/// A JSON document at `url`, `None` for anything else: an error status, HTML, an answer too large.
pub(crate) async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
) -> Option<T> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    serde_json::from_str(&read(response).await?).ok()
}

/// A response's body as text, `None` past [`MAX_ANSWER_BYTES`] or when it is no UTF-8.
pub(crate) async fn read(mut response: reqwest::Response) -> Option<String> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).ok()
}

/// What an OAuth error answer says (`error`, `error_description`), cut and made printable — never the whole body,
/// which may echo what was sent.
pub(crate) fn said_error(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
        return String::new();
    };
    let field = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(described)
    };
    match (field("error"), field("error_description")) {
        (Some(error), Some(description)) => format!(": {error}: {description}"),
        (Some(error), None) => format!(": {error}"),
        _ => String::new(),
    }
}

/// A server's words, cut to [`DESCRIPTION_CHARS`] and made printable for the owner's terminal.
pub(crate) fn described(text: &str) -> String {
    crate::permissions::printable(&text.chars().take(DESCRIPTION_CHARS).collect::<String>())
}

/// The listener a sign-in's browser comes back to: on 127.0.0.1 — never `localhost`, which may resolve elsewhere —
/// for this one sign-in, at a path of its own.
pub(crate) struct Callback {
    listener: TcpListener,
    path: String,
    /// The redirect the authorization request names: `http://127.0.0.1:<port><path>`.
    pub redirect: String,
}

impl Callback {
    /// The listener on `port` (0: any free one) for redirects to `path`.
    ///
    /// # Errors
    ///
    /// Why it cannot listen.
    pub(crate) async fn bind(port: u16, path: &str) -> Result<Callback, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .await
            .map_err(|error| format!("cannot listen for the sign-in: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        Ok(Callback {
            listener,
            redirect: format!("http://127.0.0.1:{port}{path}"),
            path: path.to_owned(),
        })
    }

    /// The redirect of the sign-in whose `state` this is, its parameters judged by `judge` — the page the browser
    /// shows says whether that went well. Every connection is read beside the others, so one that stays silent holds
    /// nothing up; a request that is not this sign-in's redirect — another path, a stray query, another `state` — is
    /// answered and passed by, never the end of the sign-in.
    pub(crate) async fn wait<T, E>(
        &self,
        state: &str,
        judge: impl Fn(&[(String, String)]) -> Result<T, E>,
    ) -> Result<T, E> {
        let mut reading = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                accepted = self.listener.accept() => {
                    if let Ok((mut socket, _)) = accepted
                        && reading.len() < MAX_CALLBACK_READERS
                    {
                        reading.spawn(async move {
                            let target = timeout(CALLBACK_READ, request_target(&mut socket))
                                .await
                                .ok()
                                .flatten();
                            (socket, target)
                        });
                    }
                }
                Some(Ok((mut socket, target))) = reading.join_next() => {
                    let Some(target) = target else {
                        continue;
                    };
                    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
                    let pairs = pairs_of(query);
                    let ours = path == self.path
                        && pairs.iter().any(|(key, value)| key == "state" && value == state);
                    if !ours {
                        let _ = respond(&mut socket, "404 Not Found", "").await;
                        continue;
                    }
                    let found = judge(&pairs);
                    let page = match &found {
                        Ok(_) => "Glut is signed in. This tab may be closed.",
                        Err(_) => "Glut's sign-in did not succeed; Glut says why.",
                    };
                    let _ = respond(&mut socket, "200 OK", page).await;
                    return found;
                }
            }
        }
    }
}

/// The request line's target of the browser's `GET`, read up to the end of its head.
async fn request_target(socket: &mut tokio::net::TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 2048];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = socket.read(&mut buffer).await.ok()?;
        if read == 0 || head.len() + read > MAX_CALLBACK_BYTES {
            return None;
        }
        head.extend_from_slice(&buffer[..read]);
    }
    let line = std::str::from_utf8(&head).ok()?.lines().next()?;
    let mut parts = line.split(' ');
    (parts.next()? == "GET").then_some(())?;
    parts.next().map(str::to_owned)
}

/// A short page to the browser, and the connection closed.
async fn respond(
    socket: &mut tokio::net::TcpStream,
    status: &str,
    text: &str,
) -> std::io::Result<()> {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Glut</title><p style=\"font: 16px system-ui; margin: 3em\">\
         {text}</p>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\n\
         cache-control: no-store\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await
}

/// The parameters of a query string, decoded.
pub(crate) fn pairs_of(query: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(&format!("http://x/?{query}"))
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

/// The parameters of an address the owner pasted: a whole URL, or only what follows its `?`.
pub(crate) fn query_of(address: &str) -> Vec<(String, String)> {
    let address = address.trim();
    match address.split_once('?') {
        Some((_, query)) => pairs_of(query.split('#').next().unwrap_or(query)),
        None => pairs_of(address),
    }
}

/// The value of `key` among `pairs`.
pub(crate) fn param<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value.as_str())
}

/// Whether two URLs are the same but for a trailing slash.
pub(crate) fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// Now, in Unix seconds.
pub(crate) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// [`RANDOM_BYTES`] from the system's random source, as base64url.
///
/// # Errors
///
/// Why there are none.
pub(crate) fn random_text() -> Result<String, String> {
    Ok(base64url(&random_bytes::<RANDOM_BYTES>()?))
}

/// `N` bytes from the system's random source.
///
/// # Errors
///
/// Why there are none.
pub(crate) fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    let mut bytes = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|error| format!("no random bytes for the sign-in: {error}"))?;
    Ok(bytes)
}

/// PKCE's S256 challenge of `verifier` (RFC 7636 §4.2).
pub(crate) fn challenge_of(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// The base64url alphabet (RFC 4648 §5).
const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// `bytes` as base64url without padding (RFC 4648 §5).
pub(crate) fn base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let triple = chunk.iter().enumerate().fold(0u32, |acc, (at, byte)| {
            acc | u32::from(*byte) << (16 - 8 * at)
        });
        for at in 0..=chunk.len() {
            out.push(char::from(
                ALPHABET[(triple >> (18 - 6 * at) & 0x3f) as usize],
            ));
        }
    }
    out
}

/// base64url `text` (padding allowed, never needed) as its bytes; `None` for a character outside the alphabet or a
/// length no encoding has.
pub(crate) fn base64url_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim_end_matches('=');
    if text.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut bits = 0u32;
    let mut held = 0;
    for byte in text.bytes() {
        let value = ALPHABET.iter().position(|letter| *letter == byte)? as u32;
        bits = bits << 6 | value;
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((bits >> held) as u8);
            bits &= (1 << held) - 1;
        }
    }
    Some(out)
}

/// A token endpoint's answer (RFC 6749 §5.1; OpenID Connect's `id_token` beside it), what Glut keeps of it.
#[derive(Deserialize)]
pub(crate) struct TokenAnswer {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<i64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub id_token: Option<String>,
}

/// Why a token endpoint gave no tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TokenError {
    /// It refused the grant (`invalid_grant` and its kin): the owner signs in again.
    Refused(String),
    /// Anything else, said for the owner.
    Failed(String),
}

/// The error codes that mean a grant is gone for good — the owner signs in again; ChatGPT names its own beside
/// OAuth's.
const REFUSED_GRANTS: [&str; 3] = [
    "invalid_grant",
    "refresh_token_reused",
    "refresh_token_expired",
];

/// One request to a token endpoint.
///
/// # Errors
///
/// [`TokenError::Refused`] for a grant refused, [`TokenError::Failed`] for anything else.
pub(crate) async fn token_request(
    endpoint: &str,
    form: &[(&str, String)],
) -> Result<TokenAnswer, TokenError> {
    let response = http()
        .map_err(TokenError::Failed)?
        .post(endpoint)
        .header(ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|error| {
            TokenError::Failed(format!(
                "its token endpoint cannot be reached: {}",
                error.without_url()
            ))
        })?;
    let status = response.status();
    let text = read(response)
        .await
        .ok_or_else(|| TokenError::Failed("its token endpoint's answer is too large".into()))?;
    if !status.is_success() {
        let said = said_error(&text);
        if REFUSED_GRANTS.iter().any(|code| text.contains(code)) {
            return Err(TokenError::Refused(said));
        }
        return Err(TokenError::Failed(format!(
            "its token endpoint said no ({status}){said}"
        )));
    }
    let answer: TokenAnswer = serde_json::from_str(&text)
        .map_err(|_| TokenError::Failed("its token endpoint's answer holds no token".into()))?;
    if answer
        .token_type
        .as_deref()
        .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(TokenError::Failed(
            "its token endpoint handed out a token that is no bearer token".into(),
        ));
    }
    if answer.access_token.is_empty() {
        return Err(TokenError::Failed(
            "its token endpoint handed out an empty token".into(),
        ));
    }
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_and_the_pkce_challenge_are_the_rfc_s() {
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        for text in [
            &b""[..],
            b"f",
            b"fo",
            b"foo",
            b"foob",
            b"fooba",
            b"foobar",
            &[0xff, 0xfe, 0x00],
        ] {
            assert_eq!(base64url_decode(&base64url(text)).as_deref(), Some(text));
        }
        assert_eq!(base64url_decode("Zm9vYg=="), Some(b"foob".to_vec()));
        assert_eq!(
            base64url_decode("Zm9v+g"),
            None,
            "+ is base64's, not base64url's"
        );
        assert_eq!(
            base64url_decode("Z"),
            None,
            "no encoding is one character long"
        );
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        let (a, b) = (random_text().unwrap(), random_text().unwrap());
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }

    #[test]
    fn a_pasted_address_gives_its_parameters() {
        let pairs = query_of("http://127.0.0.1:1/callback?code=abc&state=x%20y#frag");
        assert_eq!(param(&pairs, "code"), Some("abc"));
        assert_eq!(param(&pairs, "state"), Some("x y"));
        assert_eq!(param(&query_of("code=1"), "code"), Some("1"));
        assert!(same_url("https://a/", "https://a"));
    }
}
