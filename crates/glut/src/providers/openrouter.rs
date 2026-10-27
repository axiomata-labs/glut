//! OpenRouter's sign-in (`docs/plans/glut-providers.md`, PV4b; `openrouter.ai/docs/guides/overview/auth/oauth`, read
//! 2026-10-27): the owner signs in in the browser and OpenRouter hands Glut a key of its own — no key copied. The
//! address names a PKCE challenge (S256) and a random `state`; the browser comes back to 127.0.0.1 with a code,
//! which `POST /api/v1/auth/keys` trades for the key. The key is then a key like any: kept under
//! `openrouter@openrouter.ai` in the store the owner chose.

use std::future::Future;

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde::Deserialize;
use tokio::time::timeout;

use crate::oauth::{self, Callback, param};

/// The path the browser comes back to.
const CALLBACK: &str = "/callback";
/// How every OpenRouter key begins.
const KEY_PREFIX: &str = "sk-or-";
/// The label the new key gets in the owner's OpenRouter account, where it can be limited or deleted.
const KEY_LABEL: &str = "Glut";

/// OpenRouter's sign-in endpoints — a test's own.
#[derive(Debug, Clone)]
pub struct Server {
    /// Where the browser signs in.
    pub authorize: String,
    /// Where a code becomes a key.
    pub keys: String,
}

impl Server {
    /// OpenRouter's own.
    pub fn openrouter() -> Server {
        Server {
            authorize: "https://openrouter.ai/auth".into(),
            keys: "https://openrouter.ai/api/v1/auth/keys".into(),
        }
    }

    /// Where the owner signs in, for the line that names it.
    pub fn host(&self) -> String {
        reqwest::Url::parse(&self.authorize)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

/// A sign-in begun: the listener waits on 127.0.0.1 for the browser to come back with the code.
pub struct SignIn {
    server: Server,
    callback: Callback,
    address: String,
    state: String,
    verifier: String,
}

/// Begins a sign-in to OpenRouter: opens the listener and makes the address the owner opens. Nothing opens by itself.
///
/// # Errors
///
/// Why it cannot begin: no listener, no random bytes.
pub async fn begin(server: Server) -> Result<SignIn, String> {
    let callback = Callback::bind(0, CALLBACK).await?;
    let (state, verifier) = (oauth::random_text()?, oauth::random_text()?);
    let mut address = reqwest::Url::parse(&server.authorize)
        .map_err(|_| "the authorization endpoint is no URL")?;
    address
        .query_pairs_mut()
        .append_pair("callback_url", &callback.redirect)
        .append_pair("code_challenge", &oauth::challenge_of(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", &state)
        .append_pair("key_label", KEY_LABEL);
    Ok(SignIn {
        server,
        callback,
        address: address.into(),
        state,
        verifier,
    })
}

/// What `POST /auth/keys` answers.
#[derive(Deserialize)]
struct Issued {
    key: String,
}

impl SignIn {
    /// The address the owner opens in a browser.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Where the owner signs in.
    pub fn host(&self) -> String {
        self.server.host()
    }

    /// Waits for the browser to come back — at the listener, or as the address the owner pastes (`pasted`, over SSH:
    /// the browser's machine cannot reach this one's 127.0.0.1) — and trades the code for the key, the caller's to
    /// keep.
    ///
    /// # Errors
    ///
    /// Why there is no key: refused, timed out, another sign-in's address, a code OpenRouter does not take.
    pub async fn finish(
        self,
        pasted: impl Future<Output = Option<String>>,
    ) -> Result<String, String> {
        let code = tokio::select! {
            came = timeout(oauth::SIGN_IN_TIMEOUT, self.callback.wait(&self.state, judge)) => {
                // A refusal comes back without the state, so it ends here as well.
                came.map_err(|_| "the browser did not come back in time (refused there? sign in again)".to_owned())??
            }
            Some(address) = pasted => {
                let pairs = oauth::query_of(&address);
                if param(&pairs, "state") != Some(self.state.as_str()) {
                    return Err("the pasted address is not this sign-in's (its state differs)".into());
                }
                judge(&pairs)?
            }
        };
        self.exchange(&code).await
    }

    /// The code traded for the key.
    async fn exchange(self, code: &str) -> Result<String, String> {
        let response = oauth::http()?
            .post(&self.server.keys)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/json")
            .body(
                serde_json::json!({
                    "code": code,
                    "code_verifier": self.verifier,
                    "code_challenge_method": "S256",
                })
                .to_string(),
            )
            .send()
            .await
            .map_err(|error| format!("OpenRouter cannot be reached: {}", error.without_url()))?;
        let status = response.status();
        let text = oauth::read(response)
            .await
            .ok_or("OpenRouter's answer is too large")?;
        if !status.is_success() {
            // Its words, never the body: an answer may echo what was sent.
            return Err(format!("OpenRouter took no code ({status}){}", said(&text)));
        }
        let issued: Issued = serde_json::from_str(&text)
            .map_err(|_| "OpenRouter's answer holds no key".to_owned())?;
        let key = issued.key.trim();
        // OpenRouter's keys begin `sk-or-`: anything else is no key of theirs, kept or not.
        if !key.starts_with(KEY_PREFIX) || key.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err("OpenRouter handed out a key that is none".into());
        }
        Ok(key.to_owned())
    }
}

/// The code among the redirect's parameters (its `state` already this sign-in's).
fn judge(pairs: &[(String, String)]) -> Result<String, String> {
    if let Some(error) = param(pairs, "error") {
        return Err(format!(
            "the sign-in was refused ({})",
            oauth::described(error)
        ));
    }
    param(pairs, "code")
        .filter(|code| !code.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "the sign-in came back without a code".into())
}

/// What an OpenRouter error answer says (`{"error": {"message": …}}`), cut and made printable.
fn said(text: &str) -> String {
    serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .and_then(serde_json::Value::as_str)
                .map(|message| format!(": {}", oauth::described(message)))
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// A key endpoint that answers `status` and `body` once, with the request it got.
    async fn keys_server(
        status: &'static str,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}/api/v1/auth/keys", listener.local_addr().unwrap());
        let served = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some((head, rest)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .map(str::to_owned)
                        })
                        .and_then(|value| value.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if rest.len() >= length {
                        break;
                    }
                }
            }
            let answer = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                 connection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(answer.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (address, served)
    }

    fn server(keys: String) -> Server {
        Server {
            authorize: "https://openrouter.test/auth".into(),
            keys,
        }
    }

    #[tokio::test]
    async fn the_address_names_the_challenge_and_the_state_and_a_pasted_code_becomes_the_key() {
        let (keys, served) = keys_server("200 OK", r#"{"key": "sk-or-v1-abc"}"#).await;
        let sign_in = begin(server(keys)).await.unwrap();
        let pairs = oauth::query_of(sign_in.address());
        assert!(
            param(&pairs, "callback_url")
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );
        assert_eq!(param(&pairs, "code_challenge_method"), Some("S256"));
        assert_eq!(
            param(&pairs, "code_challenge"),
            Some(oauth::challenge_of(&sign_in.verifier).as_str())
        );
        assert_eq!(sign_in.host(), "openrouter.test");
        let pasted = format!(
            "http://127.0.0.1:1/callback?code=the-code&state={}",
            sign_in.state
        );
        let verifier = sign_in.verifier.clone();
        let key = sign_in.finish(async { Some(pasted) }).await.unwrap();
        assert_eq!(key, "sk-or-v1-abc");
        let request = served.await.unwrap();
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["code"], "the-code");
        assert_eq!(body["code_verifier"], verifier.as_str());
        assert_eq!(body["code_challenge_method"], "S256");
    }

    #[tokio::test]
    async fn another_state_a_refusal_or_a_code_not_taken_gives_no_key() {
        let sign_in = begin(server("http://127.0.0.1:9/never".into()))
            .await
            .unwrap();
        let pasted = "http://127.0.0.1:1/callback?code=x&state=other".to_owned();
        assert!(
            sign_in
                .finish(async { Some(pasted) })
                .await
                .unwrap_err()
                .contains("state")
        );
        assert!(
            judge(&oauth::query_of("error=access_denied&state=s"))
                .unwrap_err()
                .contains("refused")
        );
        assert!(judge(&oauth::query_of("state=s")).is_err());

        let (keys, _served) = keys_server(
            "403 Forbidden",
            r#"{"error": {"message": "Invalid code", "code": 403}}"#,
        )
        .await;
        let sign_in = begin(server(keys)).await.unwrap();
        let pasted = format!("code=c&state={}", sign_in.state);
        let refused = sign_in.finish(async { Some(pasted) }).await.unwrap_err();
        assert!(
            refused.contains("403") && refused.contains("Invalid code"),
            "{refused}"
        );

        let (keys, _served) = keys_server("200 OK", r#"{"key": "sk-or-two words"}"#).await;
        let sign_in = begin(server(keys)).await.unwrap();
        let pasted = format!("code=c&state={}", sign_in.state);
        assert!(sign_in.finish(async { Some(pasted) }).await.is_err());
    }
}
