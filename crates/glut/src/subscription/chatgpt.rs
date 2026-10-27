//! "Sign in with ChatGPT" for open-source apps (`docs/plans/glut-providers.md`, PV4; OpenAI's guide under
//! `developers.openai.com/siwc/token-sharing-open-source/`): the owner's ChatGPT plan instead of an API key.
//!
//! The first sign-in registers Glut by itself: the client `dynamic_agent_client`, this host's id and Glut's name; the
//! callback brings the client id OpenAI issued (`oaiapp_…`), which every later sign-in of the account on this host
//! sends again. The redirect is `http://127.0.0.1:<port>/auth/callback` — only the port may change. The ID token is
//! verified in full (the owner's word in the grilling, Q16): its signature against OpenAI's published keys, then its
//! issuer, audience, expiry and nonce; its `sub` is the account. No call goes out before the grant holds
//! `chatgpt.tokens.use.direct`.

use std::future::Future;

use serde::Deserialize;
use serde_json::Value;
use tokio::time::timeout;

use super::Record;
use crate::oauth::{self, Callback, TokenError, param};

/// The scopes asked for: who signed in, and the plan's use by this app.
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope without which the plan cannot be used: a grant without it is no sign-in.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
/// The API the tokens are for.
pub const RESOURCE: &str = "https://api.openai.com/v1";
/// The client a first sign-in registers through.
const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
/// The name OpenAI shows the owner for this app.
const APP_NAME: &str = "Glut";
/// The path the redirect comes back to; the guide wants it the same at every sign-in.
const CALLBACK: &str = "/auth/callback";
/// Where the owner sees and limits what apps use of the plan.
pub const USAGE_PAGE: &str = "https://chatgpt.com/settings/usage";
/// How far a clock may be off before an ID token counts as expired.
const LEEWAY_SECS: i64 = 300;
/// How long an access token is taken to hold where the answer does not say: OpenAI's own hour.
const DEFAULT_LIFETIME_SECS: i64 = 3600;
/// The file of this host's id towards OpenAI, below `~/.glut`: random, made once.
const HOST_ID_FILE: &str = "agent-host-id";

/// OpenAI's sign-in endpoints (`auth.openai.com/.well-known/openid-configuration`, read 2026-10-27) — a test's own.
#[derive(Debug, Clone)]
pub struct Server {
    /// Who issues the ID tokens (their `iss`).
    pub issuer: String,
    /// Where the browser signs in.
    pub authorize: String,
    /// Where a code or a refresh token becomes tokens.
    pub token: String,
    /// Where a sign-out revokes the refresh token.
    pub revoke: String,
    /// The keys the ID tokens are signed with.
    pub jwks: String,
}

impl Server {
    /// OpenAI's own.
    pub fn openai() -> Server {
        Server {
            issuer: "https://auth.openai.com".into(),
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            revoke: "https://auth.openai.com/api/accounts/oauth/revoke".into(),
            jwks: "https://auth.openai.com/.well-known/jwks.json".into(),
        }
    }

    /// The authorization server's host, which the owner is shown before the browser opens.
    pub fn host(&self) -> String {
        reqwest::Url::parse(&self.authorize)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

/// Why a renewal or a sign-in gave no tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The grant is gone: the owner signs in again.
    SignInAgain(String),
    /// Anything else, said for the owner.
    Other(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::SignInAgain(reason) | Failure::Other(reason) => f.write_str(reason),
        }
    }
}

impl From<TokenError> for Failure {
    fn from(error: TokenError) -> Failure {
        match error {
            TokenError::Refused(said) => {
                Failure::SignInAgain(format!("the sign-in is no longer valid{said}"))
            }
            TokenError::Failed(reason) => Failure::Other(reason),
        }
    }
}

/// This host's id towards OpenAI: the one kept in `~/.glut`, else a new random one kept from now on.
///
/// # Errors
///
/// Why it cannot be made or kept.
fn host_id(home: &std::path::Path) -> Result<String, String> {
    let dir = home.join(".glut");
    let path = dir.join(HOST_ID_FILE);
    if let Ok(kept) = std::fs::read_to_string(&path) {
        let kept = kept.trim();
        if !kept.is_empty()
            && kept.len() <= 128
            && kept
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
        {
            return Ok(kept.to_owned());
        }
    }
    let made = oauth::random_text()?;
    crate::providers::make_private_dir(&dir)
        .and_then(|()| crate::providers::write_replacing(&dir, &path, made.as_bytes(), 0o600))
        .map_err(|err| format!("{}: {err}", path.display()))?;
    Ok(made)
}

/// A sign-in begun: the listener waits on 127.0.0.1 for the browser to come back with the code.
pub struct SignIn {
    server: Server,
    callback: Callback,
    address: String,
    state: String,
    nonce: String,
    verifier: String,
    /// The client issued at an earlier sign-in of this account; `None` for the first, which registers.
    client: Option<String>,
    host_id: String,
    /// What was kept for the account: its client, who it was, its last ID token.
    kept: Option<Record>,
}

/// Begins a sign-in to ChatGPT for the account kept as `kept` (none: the first): opens the listener and makes the
/// address the owner opens. Nothing opens by itself.
///
/// # Errors
///
/// Why it cannot begin: no listener, no random bytes, no host id.
pub async fn begin(
    home: &std::path::Path,
    kept: Option<Record>,
    server: Server,
) -> Result<SignIn, String> {
    let callback = Callback::bind(0, CALLBACK).await?;
    let host_id = match kept.as_ref().map(|record| record.host_id.clone()) {
        Some(id) if !id.is_empty() => id,
        _ => host_id(home)?,
    };
    let client = kept
        .as_ref()
        .map(|record| record.client_id.clone())
        .filter(|id| !id.is_empty());
    let (state, nonce, verifier) = (
        oauth::random_text()?,
        oauth::random_text()?,
        oauth::random_text()?,
    );
    let mut address = reqwest::Url::parse(&server.authorize)
        .map_err(|_| "the authorization endpoint is no URL")?;
    {
        let mut query = address.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client.as_deref().unwrap_or(DYNAMIC_CLIENT))
            .append_pair("redirect_uri", &callback.redirect)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge", &oauth::challenge_of(&verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("ext_agent_host_id", &host_id);
        match &kept {
            // Returning: the account named, no name hint (the guide's rule).
            // The ID token stays out of it (an optional hint): the address is shown to the owner and kept in the
            // stream.
            Some(record) if client.is_some() => {
                if let Some(email) = &record.email {
                    query.append_pair("login_hint", email);
                }
            }
            _ => {
                query.append_pair("agent_name_hint", APP_NAME);
            }
        }
    }
    Ok(SignIn {
        server,
        callback,
        address: address.into(),
        state,
        nonce,
        verifier,
        client,
        host_id,
        kept,
    })
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
    /// the browser's machine cannot reach this one's 127.0.0.1) — trades the code for the tokens and verifies who
    /// signed in. The record it gives is the caller's to keep.
    ///
    /// # Errors
    ///
    /// Why there is no sign-in: refused, timed out, another account than the one kept, no plan scope, an ID token
    /// that does not hold.
    pub async fn finish(
        self,
        pasted: impl Future<Output = Option<String>>,
    ) -> Result<Record, Failure> {
        let (code, client) = tokio::select! {
            came = timeout(oauth::SIGN_IN_TIMEOUT, self.callback.wait(&self.state, |pairs| self.judge(pairs))) => {
                came.map_err(|_| Failure::Other("the browser did not come back in time".into()))??
            }
            Some(address) = pasted => {
                let pairs = oauth::query_of(&address);
                if param(&pairs, "state") != Some(self.state.as_str()) {
                    return Err(Failure::Other("the pasted address is not this sign-in's (its state differs)".into()));
                }
                self.judge(&pairs)?
            }
        };
        self.exchange(&code, &client).await
    }

    /// The code and the client among the redirect's parameters (its `state` already this sign-in's).
    fn judge(&self, pairs: &[(String, String)]) -> Result<(String, String), Failure> {
        if let Some(error) = param(pairs, "error") {
            let said = param(pairs, "error_description")
                .map(|text| format!(": {}", oauth::described(text)))
                .unwrap_or_default();
            return Err(Failure::Other(format!(
                "the sign-in was refused ({}){said}",
                oauth::described(error)
            )));
        }
        let code = param(pairs, "code")
            .filter(|code| !code.is_empty())
            .ok_or_else(|| Failure::Other("the sign-in came back without a code".into()))?;
        let issued = param(pairs, "client_id").filter(|id| !id.is_empty() && *id != DYNAMIC_CLIENT);
        let client = match (&self.client, issued) {
            // A returning sign-in keeps its client; one handed back must be the same.
            (Some(kept), Some(issued)) if kept != issued => {
                return Err(Failure::Other(
                    "the sign-in came back for another client".into(),
                ));
            }
            (Some(kept), _) => kept.clone(),
            (None, Some(issued)) => issued.to_owned(),
            (None, None) => {
                return Err(Failure::Other(
                    "OpenAI issued no client for Glut: the registration did not complete".into(),
                ));
            }
        };
        Ok((code.to_owned(), client))
    }

    /// The code traded for the tokens; the ID token verified, the plan's scope there, the account the kept one.
    async fn exchange(self, code: &str, client: &str) -> Result<Record, Failure> {
        let form = [
            ("grant_type", "authorization_code".to_owned()),
            ("client_id", client.to_owned()),
            ("code", code.to_owned()),
            ("code_verifier", self.verifier.clone()),
            ("redirect_uri", self.callback.redirect.clone()),
            ("resource", RESOURCE.to_owned()),
        ];
        let answer = oauth::token_request(&self.server.token, &form).await?;
        let scopes: Vec<String> = answer
            .scope
            .as_deref()
            .unwrap_or_default()
            .split_whitespace()
            .map(str::to_owned)
            .collect();
        if !scopes.iter().any(|scope| scope == PLAN_SCOPE) {
            return Err(Failure::Other(
                "the sign-in did not grant the use of the ChatGPT plan (Plus and Pro only)".into(),
            ));
        }
        let id_token = answer
            .id_token
            .ok_or_else(|| Failure::Other("the sign-in came back without an ID token".into()))?;
        let keys = jwks(&self.server).await.map_err(Failure::Other)?;
        let claims = verify(
            &id_token,
            &keys,
            &self.server.issuer,
            client,
            &self.nonce,
            oauth::now(),
        )
        .map_err(|reason| Failure::Other(format!("the ID token does not hold: {reason}")))?;
        if let Some(kept) = &self.kept
            && !kept.subject.is_empty()
            && kept.subject != claims.sub
        {
            return Err(Failure::Other(
                "this is another ChatGPT account than the one signed in here before: add it as a provider of its own \
                 (glut providers add chatgpt --id chatgpt-2)"
                    .into(),
            ));
        }
        let refresh_token = answer.refresh_token.ok_or_else(|| {
            Failure::Other("the sign-in came back without a refresh token".into())
        })?;
        let now = oauth::now();
        Ok(Record {
            client_id: client.to_owned(),
            host_id: self.host_id,
            subject: claims.sub,
            email: claims.email,
            id_token: Some(id_token),
            access_token: Some(answer.access_token),
            refresh_token: Some(refresh_token),
            expires_at: Some(now + answer.expires_in.unwrap_or(DEFAULT_LIFETIME_SECS)),
            scopes,
            saved_at: now,
        })
    }
}

/// `record`'s tokens renewed: the new access token and the replacement refresh token, kept together.
///
/// # Errors
///
/// [`Failure::SignInAgain`] when there is nothing to renew with or OpenAI refused the grant.
pub async fn renew(server: &Server, record: &Record) -> Result<Record, Failure> {
    let refresh = record
        .refresh_token
        .clone()
        .ok_or_else(|| Failure::SignInAgain("there is no sign-in to renew".into()))?;
    let form = [
        ("grant_type", "refresh_token".to_owned()),
        ("client_id", record.client_id.clone()),
        ("refresh_token", refresh),
        ("resource", RESOURCE.to_owned()),
    ];
    let answer = oauth::token_request(&server.token, &form).await?;
    let now = oauth::now();
    let scopes = match answer.scope.as_deref() {
        Some(scope) => scope.split_whitespace().map(str::to_owned).collect(),
        None => record.scopes.clone(),
    };
    Ok(Record {
        access_token: Some(answer.access_token),
        // The guide: the refresh token rotates; one that comes back none keeps the old.
        refresh_token: answer
            .refresh_token
            .or_else(|| record.refresh_token.clone()),
        expires_at: Some(now + answer.expires_in.unwrap_or(DEFAULT_LIFETIME_SECS)),
        scopes,
        saved_at: now,
        ..record.clone()
    })
}

/// What a sign-out did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedOut {
    /// Nothing was signed in.
    Nothing,
    /// Revoked at OpenAI and forgotten here.
    Revoked,
    /// Forgotten here; OpenAI's revocation failed with this reason — the owner disconnects Glut in ChatGPT's settings.
    NotConfirmed(String),
}

/// `record` signed out: its refresh token revoked at OpenAI, the tokens and the ID token gone; the client, the host's
/// id and the account kept for the next sign-in (the guide's rule).
pub async fn sign_out(server: &Server, record: &Record) -> (Record, SignedOut) {
    let kept = Record {
        client_id: record.client_id.clone(),
        host_id: record.host_id.clone(),
        subject: record.subject.clone(),
        email: record.email.clone(),
        ..Record::default()
    };
    let Some(token) = &record.refresh_token else {
        return (kept, SignedOut::Nothing);
    };
    let form = [
        ("token", token.clone()),
        ("token_type_hint", "refresh_token".to_owned()),
        ("client_id", record.client_id.clone()),
    ];
    let done = match oauth::http() {
        Ok(client) => match client.post(&server.revoke).form(&form).send().await {
            Ok(response) if response.status().is_success() => SignedOut::Revoked,
            Ok(response) => {
                SignedOut::NotConfirmed(format!("OpenAI answered {}", response.status()))
            }
            Err(error) => SignedOut::NotConfirmed(error.without_url().to_string()),
        },
        Err(reason) => SignedOut::NotConfirmed(reason),
    };
    (kept, done)
}

/// One RSA key of a JWKS.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Key {
    #[serde(default)]
    kid: Option<String>,
    kty: String,
    #[serde(default)]
    n: String,
    #[serde(default)]
    e: String,
}

/// A JWKS.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Keys {
    keys: Vec<Key>,
}

/// OpenAI's published keys.
async fn jwks(server: &Server) -> Result<Keys, String> {
    let client = oauth::http()?;
    oauth::get_json::<Keys>(&client, &server.jwks)
        .await
        .ok_or_else(|| "OpenAI's signing keys cannot be read".to_owned())
}

/// What Glut reads of a verified ID token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Claims {
    sub: String,
    email: Option<String>,
}

/// `token` verified (OpenID Connect Core §3.1.3.7): an RS256 signature by one of `keys`, the issuer `issuer`, the
/// audience `client`, not expired at `now`, the nonce `nonce`.
pub(crate) fn verify(
    token: &str,
    keys: &Keys,
    issuer: &str,
    client: &str,
    nonce: &str,
    now: i64,
) -> Result<Claims, String> {
    let mut parts = token.split('.');
    let (Some(head), Some(body), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("it is no signed token".into());
    };
    let decode = |part: &str| {
        oauth::base64url_decode(part)
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .ok_or_else(|| "a part of it is unreadable".to_owned())
    };
    let header = decode(head)?;
    if header.get("alg").and_then(Value::as_str) != Some("RS256") {
        return Err("it is not signed with RS256".into());
    }
    let kid = header.get("kid").and_then(Value::as_str);
    let signature = oauth::base64url_decode(signature).ok_or("its signature is unreadable")?;
    let signed = format!("{head}.{body}");
    let verified = keys
        .keys
        .iter()
        .filter(|key| key.kty == "RSA" && (kid.is_none() || key.kid.as_deref() == kid))
        .any(|key| {
            let (Some(n), Some(e)) = (
                oauth::base64url_decode(&key.n),
                oauth::base64url_decode(&key.e),
            ) else {
                return false;
            };
            ring::signature::RsaPublicKeyComponents { n, e }
                .verify(
                    &ring::signature::RSA_PKCS1_2048_8192_SHA256,
                    signed.as_bytes(),
                    &signature,
                )
                .is_ok()
        });
    if !verified {
        return Err("its signature is none of OpenAI's keys'".into());
    }
    let claims = decode(body)?;
    let text = |key: &str| claims.get(key).and_then(Value::as_str);
    if text("iss").map(|iss| oauth::same_url(iss, issuer)) != Some(true) {
        return Err("another issuer made it".into());
    }
    let audience = match claims.get("aud") {
        Some(Value::String(aud)) => aud == client,
        Some(Value::Array(auds)) => auds.iter().any(|aud| aud.as_str() == Some(client)),
        _ => false,
    };
    if !audience {
        return Err("it is meant for another client".into());
    }
    if claims
        .get("exp")
        .and_then(Value::as_i64)
        .is_none_or(|exp| exp + LEEWAY_SECS < now)
    {
        return Err("it has expired".into());
    }
    if text("nonce") != Some(nonce) {
        return Err("it is not this sign-in's (its nonce differs)".into());
    }
    let sub = text("sub")
        .filter(|sub| !sub.is_empty())
        .ok_or("it names no account")?
        .to_owned();
    Ok(Claims {
        sub,
        email: text("email").map(oauth::described),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// A test-only RSA key (made with `openssl genpkey` for these tests): signs the fake server's ID tokens.
    const TEST_KEY: &[u8] = include_bytes!("test-key.der");

    fn pair() -> RsaKeyPair {
        RsaKeyPair::from_pkcs8(TEST_KEY).unwrap()
    }

    fn keys_of(pair: &RsaKeyPair) -> Keys {
        let public: ring::signature::RsaPublicKeyComponents<Vec<u8>> = pair.public().into();
        Keys {
            keys: vec![Key {
                kid: Some("k1".into()),
                kty: "RSA".into(),
                n: oauth::base64url(&public.n),
                e: oauth::base64url(&public.e),
            }],
        }
    }

    fn token(pair: &RsaKeyPair, claims: &Value) -> String {
        let head = oauth::base64url(
            json!({"alg": "RS256", "kid": "k1", "typ": "JWT"})
                .to_string()
                .as_bytes(),
        );
        let body = oauth::base64url(claims.to_string().as_bytes());
        let signed = format!("{head}.{body}");
        let mut signature = vec![0; pair.public().modulus_len()];
        pair.sign(
            &RSA_PKCS1_SHA256,
            &ring::rand::SystemRandom::new(),
            signed.as_bytes(),
            &mut signature,
        )
        .unwrap();
        format!("{signed}.{}", oauth::base64url(&signature))
    }

    fn claims(now: i64) -> Value {
        json!({"iss": "https://auth.openai.com", "aud": "oaiapp_1", "exp": now + 3600, "nonce": "n1",
               "sub": "user-1", "email": "o@example.com"})
    }

    #[test]
    fn an_id_token_holds_only_signed_by_openai_for_glut_unexpired_and_this_sign_ins() {
        let pair = pair();
        let keys = keys_of(&pair);
        let now = 1_800_000_000;
        let check = |claims: &Value| {
            verify(
                &token(&pair, claims),
                &keys,
                "https://auth.openai.com",
                "oaiapp_1",
                "n1",
                now,
            )
        };
        assert_eq!(
            check(&claims(now)),
            Ok(Claims {
                sub: "user-1".into(),
                email: Some("o@example.com".into())
            })
        );
        let changed = |key: &str, value: Value| {
            let mut claims = claims(now);
            claims[key] = value;
            check(&claims)
        };
        assert!(changed("iss", json!("https://evil.example")).is_err());
        assert!(changed("aud", json!("oaiapp_2")).is_err());
        assert!(
            changed("aud", json!(["x", "oaiapp_1"])).is_ok(),
            "an audience list holding Glut's client"
        );
        assert!(changed("exp", json!(now - LEEWAY_SECS - 1)).is_err());
        assert!(changed("nonce", json!("n2")).is_err());
        assert!(changed("sub", json!("")).is_err());
        // A token changed after it was signed, or signed by another key, holds nothing.
        let good = token(&pair, &claims(now));
        let mut parts: Vec<&str> = good.split('.').collect();
        let forged = oauth::base64url(
            claims(now)
                .to_string()
                .replace("user-1", "user-2")
                .as_bytes(),
        );
        parts[1] = &forged;
        assert!(
            verify(
                &parts.join("."),
                &keys,
                "https://auth.openai.com",
                "oaiapp_1",
                "n1",
                now
            )
            .is_err()
        );
        let other = Keys {
            keys: vec![Key {
                n: oauth::base64url(&[0xc5; 256]),
                ..keys.keys[0].clone()
            }],
        };
        assert!(
            verify(
                &good,
                &other,
                "https://auth.openai.com",
                "oaiapp_1",
                "n1",
                now
            )
            .is_err()
        );
        let none = oauth::base64url(json!({"alg": "none"}).to_string().as_bytes());
        assert!(
            verify(
                &format!("{none}.{}.", parts[1]),
                &keys,
                "https://auth.openai.com",
                "oaiapp_1",
                "n1",
                now
            )
            .is_err()
        );
    }

    /// What the fake server saw: the token requests' forms and the revocations.
    #[derive(Default)]
    struct Seen {
        forms: Vec<BTreeMap<String, String>>,
        revoked: Vec<String>,
    }

    /// A fake OpenAI: the token endpoint (an ID token signed with the test key, for the nonce of the last authorize
    /// request the test hands it), the JWKS, the revocation.
    async fn fake(nonce: Arc<Mutex<String>>, scope: &'static str) -> (Server, Arc<Mutex<Seen>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Seen::default()));
        let kept = Arc::clone(&seen);
        tokio::spawn(async move {
            let pair = pair();
            let keys = keys_of(&pair);
            let jwks = json!({"keys": [{"kid": "k1", "kty": "RSA", "n": keys.keys[0].n, "e": keys.keys[0].e}]});
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = vec![0; 16 * 1024];
                let read = socket.read(&mut buffer).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let line = request.lines().next().unwrap_or_default().to_owned();
                let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
                let form: BTreeMap<String, String> = oauth::pairs_of(body).into_iter().collect();
                let answer = if line.starts_with("GET /jwks") {
                    jwks.to_string()
                } else if line.starts_with("POST /revoke") {
                    kept.lock()
                        .unwrap()
                        .revoked
                        .push(form.get("token").cloned().unwrap_or_default());
                    String::new()
                } else {
                    kept.lock().unwrap().forms.push(form.clone());
                    let now = oauth::now();
                    let id = token(
                        &pair,
                        &json!({"iss": "https://auth.openai.com", "aud": form.get("client_id"), "exp": now + 3600,
                                "nonce": nonce.lock().unwrap().clone(), "sub": "user-1", "email": "o@example.com"}),
                    );
                    json!({"access_token": format!("access-{}", form.len()), "refresh_token": "refresh-2",
                           "id_token": id, "token_type": "Bearer", "expires_in": 3600, "scope": scope})
                    .to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                     connection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        let server = Server {
            issuer: "https://auth.openai.com".into(),
            authorize: format!("{root}/authorize"),
            token: format!("{root}/token"),
            revoke: format!("{root}/revoke"),
            jwks: format!("{root}/jwks"),
        };
        (server, seen)
    }

    fn query(address: &str) -> BTreeMap<String, String> {
        oauth::query_of(address).into_iter().collect()
    }

    fn home() -> std::path::PathBuf {
        let home = std::env::temp_dir().join(format!(
            "glut-chatgpt-{}-{}",
            std::process::id(),
            oauth::random_text().unwrap()
        ));
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[tokio::test]
    async fn a_first_sign_in_registers_glut_takes_the_issued_client_and_verifies_who_came_back() {
        let nonce = Arc::new(Mutex::new(String::new()));
        let (server, seen) = fake(Arc::clone(&nonce), SCOPES).await;
        let home = home();
        let sign_in = begin(&home, None, server.clone()).await.unwrap();
        let asked = query(sign_in.address());
        assert_eq!(asked["client_id"], DYNAMIC_CLIENT);
        assert_eq!(asked["agent_name_hint"], "Glut");
        assert_eq!(asked["scope"], SCOPES);
        assert_eq!(asked["resource"], RESOURCE);
        assert_eq!(asked["code_challenge_method"], "S256");
        assert!(
            asked["redirect_uri"].starts_with("http://127.0.0.1:")
                && asked["redirect_uri"].ends_with(CALLBACK)
        );
        let host_id = asked["ext_agent_host_id"].clone();
        assert_eq!(host_id, host_id_of(&home), "kept for the next sign-in");
        *nonce.lock().unwrap() = asked["nonce"].clone();
        let redirect = format!(
            "{}?code=c1&state={}&client_id=oaiapp_1&scope=x",
            asked["redirect_uri"], asked["state"]
        );
        let record = sign_in.finish(async { Some(redirect) }).await.unwrap();
        assert_eq!(
            (record.client_id.as_str(), record.subject.as_str()),
            ("oaiapp_1", "user-1")
        );
        assert_eq!(record.host_id, host_id);
        assert!(record.signed_in() && record.access_token.is_some());
        let form = seen.lock().unwrap().forms[0].clone();
        assert_eq!(form["grant_type"], "authorization_code");
        assert_eq!(form["client_id"], "oaiapp_1");
        assert_eq!(form["redirect_uri"], asked["redirect_uri"]);
        assert_eq!(form["resource"], RESOURCE);
        assert!(!form["code_verifier"].is_empty());
        // Returning: the issued client, the account named, no name hint.
        let again = begin(&home, Some(record.clone()), server.clone())
            .await
            .unwrap();
        let asked = query(again.address());
        assert_eq!(asked["client_id"], "oaiapp_1");
        assert_eq!(asked["login_hint"], "o@example.com");
        assert!(!asked.contains_key("id_token_hint") && !asked.contains_key("agent_name_hint"));
        // A callback for another client than the kept one is refused.
        let wrong = format!("x?code=c&state={}&client_id=oaiapp_9", asked["state"]);
        assert!(again.finish(async { Some(wrong) }).await.is_err());
        // Renewed: the replacement refresh token kept with the new access token.
        let renewed = renew(&server, &record).await.unwrap();
        assert_eq!(renewed.refresh_token.as_deref(), Some("refresh-2"));
        assert_eq!(
            seen.lock().unwrap().forms.last().unwrap()["grant_type"],
            "refresh_token"
        );
        // Signed out: revoked, the client and the host's id kept.
        let (kept, done) = sign_out(&server, &renewed).await;
        assert_eq!(done, SignedOut::Revoked);
        assert_eq!(seen.lock().unwrap().revoked, ["refresh-2"]);
        assert!(
            !kept.signed_in()
                && kept.client_id == "oaiapp_1"
                && kept.host_id == host_id
                && kept.id_token.is_none()
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn a_sign_in_without_the_plans_scope_or_an_issued_client_or_with_a_refusal_is_none() {
        let nonce = Arc::new(Mutex::new(String::new()));
        let (server, _) = fake(Arc::clone(&nonce), "openid email").await;
        let home = home();
        let sign_in = begin(&home, None, server.clone()).await.unwrap();
        let asked = query(sign_in.address());
        *nonce.lock().unwrap() = asked["nonce"].clone();
        let back = format!("x?code=c&state={}&client_id=oaiapp_1", asked["state"]);
        let refused = sign_in.finish(async { Some(back) }).await.unwrap_err();
        assert!(refused.to_string().contains("Plus and Pro"), "{refused}");
        let sign_in = begin(&home, None, server.clone()).await.unwrap();
        let state = query(sign_in.address())["state"].clone();
        let unissued = format!("x?code=c&state={state}");
        assert!(sign_in.finish(async { Some(unissued) }).await.is_err());
        let sign_in = begin(&home, None, server.clone()).await.unwrap();
        let state = query(sign_in.address())["state"].clone();
        let denied = format!("x?error=access_denied&state={state}");
        assert!(
            sign_in
                .finish(async { Some(denied) })
                .await
                .unwrap_err()
                .to_string()
                .contains("access_denied")
        );
        let sign_in = begin(&home, None, server).await.unwrap();
        assert!(
            sign_in
                .finish(async { Some("x?code=c&state=other".into()) })
                .await
                .is_err()
        );
        let _ = std::fs::remove_dir_all(home);
    }

    fn host_id_of(home: &std::path::Path) -> String {
        std::fs::read_to_string(home.join(".glut").join(HOST_ID_FILE)).unwrap()
    }
}
