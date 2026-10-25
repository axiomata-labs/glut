//! MCP's authorization, as the spec has it (`docs/plans/glut-tui.md`, T56): a hosted server that answers 401 names
//! its protected resource's metadata (RFC 9728), that names its authorization server, whose metadata (RFC 8414)
//! names the endpoints; Glut registers itself there (RFC 7591) unless the owner registered a client of their own,
//! and signs in with OAuth 2.1's authorization code and PKCE (S256), a random `state`, and the redirect to a listener
//! on 127.0.0.1 for this one sign-in. The tokens are asked for the server's resource (RFC 8707) and the scopes the
//! server names, never more; they are refreshed by themselves and revoked at a sign-out (RFC 7009).
//!
//! Glut never opens a browser unasked: a sign-in is begun by the owner ([`begin`]), the address is always shown as
//! text, and where the browser is on another machine (over SSH) the owner pastes back the address it ended on.
//! Every endpoint must be https (or http on this machine), every answer is read capped, and nothing follows a
//! redirect.

use std::future::Future;
use std::io::Read as _;
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE, WWW_AUTHENTICATE};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::timeout;

use super::config::ServerConfig;
use super::keys::{Grant, Keys};
use crate::error::HarnessError;

/// How long one request to an authorization server may take.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a sign-in waits for the browser to come back: the owner may have to type a password, find a second
/// factor.
pub const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// The path the redirect comes back to.
const CALLBACK: &str = "/callback";
/// The largest metadata or token answer read: a few kilobytes are what they hold.
const MAX_ANSWER_BYTES: usize = 256 * 1024;
/// The largest request the listener reads: the browser's `GET` of the redirect.
const MAX_CALLBACK_BYTES: usize = 16 * 1024;
/// How long the listener waits for one connection to say what it wants: a port scanner that opens and stays silent
/// must not hold the sign-in.
const CALLBACK_READ: Duration = Duration::from_secs(10);
/// The most connections the listener reads at once; more wait until one is done.
const MAX_CALLBACK_READERS: usize = 32;
/// Random bytes in a `state` and a PKCE verifier: 256 bits, 43 characters as base64url (RFC 7636 wants 43–128).
const RANDOM_BYTES: usize = 32;
/// How much of an authorization server's error description an error keeps.
const DESCRIPTION_CHARS: usize = 200;
/// The well-known suffix of a protected resource's metadata (RFC 9728).
const RESOURCE_METADATA: &str = "/.well-known/oauth-protected-resource";
/// The well-known suffixes of an authorization server's metadata: OAuth's (RFC 8414) and OpenID Connect's.
const SERVER_METADATA: [&str; 2] = [
    "/.well-known/oauth-authorization-server",
    "/.well-known/openid-configuration",
];

fn err(server: &str, message: impl Into<String>) -> HarnessError {
    HarnessError::Mcp {
        server: server.to_owned(),
        message: message.into(),
    }
}

/// What a 401's `WWW-Authenticate: Bearer …` says (RFC 6750, RFC 9728 §5.1).
#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

/// The `Bearer` challenge of a `WWW-Authenticate` header, `None` where it holds none.
pub(crate) fn challenge(header: &str) -> Option<Challenge> {
    let lower = header.to_ascii_lowercase();
    let at = lower
        .match_indices("bearer")
        .find(|(at, _)| *at == 0 || header[..*at].ends_with([' ', ',']))
        .map(|(at, _)| at)?;
    let mut challenge = Challenge::default();
    let mut rest = header[at + "bearer".len()..].trim_start();
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };
        let key = key
            .trim()
            .trim_start_matches(',')
            .trim()
            .to_ascii_lowercase();
        // A key with a space in it is the next challenge's scheme: this one ended.
        if key.contains(' ') {
            break;
        }
        let after = after.trim_start();
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            let mut value = String::new();
            let mut chars = quoted.char_indices();
            let mut end = quoted.len();
            while let Some((at, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, escaped)) = chars.next() {
                            value.push(escaped);
                        }
                    }
                    '"' => {
                        end = at + 1;
                        break;
                    }
                    c => value.push(c),
                }
            }
            (value, &quoted[end..])
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (after[..end].trim().to_owned(), &after[end..])
        };
        match key.as_str() {
            "resource_metadata" => challenge.resource_metadata = Some(value),
            "scope" => challenge.scope = Some(value),
            "error" => challenge.error = Some(value),
            _ => {}
        }
        rest = next.trim_start().trim_start_matches(',').trim_start();
    }
    Some(challenge)
}

/// Whether a 401 or 403 answer says the token lacks a scope: signing in again is the way out.
pub(crate) fn wants_scope(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .and_then(challenge)
        .is_some_and(|challenge| challenge.error.as_deref() == Some("insufficient_scope"))
}

/// A protected resource's metadata (RFC 9728 §2), what Glut reads of it.
#[derive(Debug, Deserialize)]
struct ResourceMeta {
    #[serde(default)]
    resource: Option<String>,
    #[serde(default)]
    authorization_servers: Vec<String>,
    #[serde(default)]
    scopes_supported: Vec<String>,
}

/// An authorization server's metadata (RFC 8414 §2), what Glut reads of it.
#[derive(Debug, Clone, Deserialize)]
struct ServerMeta {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    #[serde(default)]
    registration_endpoint: Option<String>,
    #[serde(default)]
    revocation_endpoint: Option<String>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

/// What a server's discovery found: the resource the tokens are for, its authorization server, and the scopes to
/// ask for.
#[derive(Debug, Clone)]
struct Discovered {
    resource: String,
    server: ServerMeta,
    scope: Option<String>,
}

/// The client a sign-in uses: registered by Glut, or the owner's own.
#[derive(Clone)]
struct Registered {
    id: String,
    secret: Option<String>,
}

/// How a sign-in for a server begins.
pub enum Begun {
    /// The browser's part: open [`SignIn::address`], then [`SignIn::finish`].
    SignIn(Box<SignIn>),
    /// The server speaks no OAuth Glut can use — none at all, or only for clients registered beforehand
    /// (`registered_only`, GitHub's): a token is the way in (or the owner's own client, `"oauth": {"clientId"}`).
    Token { registered_only: bool },
    /// The server answers without a sign-in.
    Open,
}

/// A sign-in begun: the listener waits on 127.0.0.1 for the browser to come back with the code.
pub struct SignIn {
    server: String,
    address: String,
    host: String,
    listener: TcpListener,
    redirect: String,
    state: String,
    verifier: String,
    client: Registered,
    meta: ServerMeta,
    resource: String,
    scope: Option<String>,
}

impl SignIn {
    /// The address the owner opens in a browser.
    pub fn address(&self) -> &str {
        &self.address
    }

    /// The authorization server's host, which the owner is shown before anything opens: where they will sign in.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The token endpoint's host where it is not [`Self::host`]: the owner sees where the tokens come from too.
    pub fn token_host(&self) -> Option<String> {
        let host = host_of(&self.meta.token_endpoint);
        (host != self.host).then_some(host)
    }

    /// Waits for the browser to come back — at the listener, or as the address the owner pastes (`pasted`, over
    /// SSH: the browser's machine cannot reach this one's 127.0.0.1) — and trades the code for the tokens.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] when the sign-in was refused, came back with another `state`, did not come back within
    /// [`SIGN_IN_TIMEOUT`], or the token endpoint said no; [`HarnessError::McpTimeout`] for the last.
    pub async fn finish(
        self,
        pasted: impl Future<Output = Option<String>>,
    ) -> Result<Grant, HarnessError> {
        let code = tokio::select! {
            came = timeout(SIGN_IN_TIMEOUT, self.callback()) => came.map_err(|_| HarnessError::McpTimeout {
                server: self.server.clone(),
                timeout: SIGN_IN_TIMEOUT,
            })??,
            Some(address) = pasted => self.code_of(&query_of(&address))?,
        };
        self.exchange(&code).await
    }

    /// The code the browser brings to the listener. Every connection is read beside the others, so one that stays
    /// silent holds nothing up; a request that is not this sign-in's redirect — another path, a stray query, another
    /// `state` — is answered and passed by, never the end of the sign-in.
    async fn callback(&self) -> Result<String, HarnessError> {
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
                    let ours = path == CALLBACK
                        && pairs
                            .iter()
                            .any(|(key, value)| key == "state" && *value == self.state);
                    if !ours {
                        let _ = respond(&mut socket, "404 Not Found", "").await;
                        continue;
                    }
                    let found = self.code_of(&pairs);
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

    /// The code among the redirect's parameters, once its `state` is this sign-in's and its issuer (RFC 9207, where
    /// the server sends one) this server's.
    fn code_of(&self, pairs: &[(String, String)]) -> Result<String, HarnessError> {
        let get = |key: &str| {
            pairs
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };
        if get("state") != Some(self.state.as_str()) {
            return Err(err(
                &self.server,
                "the sign-in came back for another request (its state is not this one's)",
            ));
        }
        if let Some(issuer) = get("iss")
            && !same_url(issuer, &self.meta.issuer)
        {
            return Err(err(
                &self.server,
                "the sign-in came back from another authorization server",
            ));
        }
        if let Some(error) = get("error") {
            let said = get("error_description")
                .map(|text| format!(": {}", described(text)))
                .unwrap_or_default();
            return Err(err(
                &self.server,
                format!("the sign-in was refused ({}){said}", described(error)),
            ));
        }
        get("code")
            .filter(|code| !code.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| err(&self.server, "the sign-in came back without a code"))
    }

    /// The code traded for the tokens.
    async fn exchange(self, code: &str) -> Result<Grant, HarnessError> {
        let mut form = vec![
            ("grant_type", "authorization_code".to_owned()),
            ("code", code.to_owned()),
            ("redirect_uri", self.redirect.clone()),
            ("client_id", self.client.id.clone()),
            ("code_verifier", self.verifier.clone()),
            ("resource", self.resource.clone()),
        ];
        if let Some(secret) = &self.client.secret {
            form.push(("client_secret", secret.clone()));
        }
        let answer = token_request(&self.server, &self.meta.token_endpoint, &form).await?;
        Ok(Grant {
            access_token: answer.access_token,
            refresh_token: answer.refresh_token,
            expires_at: answer.expires_in.map(|secs| now() + secs),
            client_id: self.client.id,
            client_secret: self.client.secret,
            token_endpoint: self.meta.token_endpoint,
            revocation_endpoint: self.meta.revocation_endpoint,
            resource: self.resource,
            scope: answer.scope.or(self.scope),
        })
    }
}

/// Begins the sign-in of the server `name`: asks it once without one, follows its metadata to its authorization
/// server, registers Glut there (or takes the owner's client), and opens the listener the browser comes back to.
/// Nothing is opened, nothing is kept: the caller shows the address and calls [`SignIn::finish`].
///
/// # Errors
///
/// [`HarnessError::Mcp`] for a server that is not reached over HTTP, metadata that does not hold together (a
/// resource for another server, an issuer that is not the one asked, an endpoint in clear text), an authorization
/// server without PKCE, or a registration it refused.
pub async fn begin(name: &str, config: &ServerConfig, keys: &Keys) -> Result<Begun, HarnessError> {
    let ServerConfig::Http { url, oauth, .. } = config else {
        return Err(err(name, "a server Glut starts itself signs in on its own"));
    };
    let client = http(name)?;
    let probe = client
        .post(url.as_str())
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json, text/event-stream")
        .body(
            json!({
                "jsonrpc": "2.0", "id": 0, "method": "initialize",
                "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                           "clientInfo": {"name": "glut", "version": env!("CARGO_PKG_VERSION")}},
            })
            .to_string(),
        )
        .send()
        .await
        .map_err(|error| err(name, format!("cannot reach it: {}", error.without_url())))?;
    if probe.status().is_success() {
        return Ok(Begun::Open);
    }
    let said = probe
        .headers()
        .get(WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .and_then(challenge)
        .unwrap_or_default();
    let Some(found) = discover(name, &client, url, &said).await? else {
        return Ok(Begun::Token {
            registered_only: false,
        });
    };
    if !found
        .server
        .code_challenge_methods_supported
        .iter()
        .any(|method| method == "S256")
    {
        return Err(err(
            name,
            format!(
                "its authorization server {} does not offer PKCE (S256); Glut signs in with it only",
                host_of(&found.server.issuer)
            ),
        ));
    }
    let listener = TcpListener::bind((
        "127.0.0.1",
        oauth
            .as_ref()
            .and_then(|client| client.callback_port)
            .unwrap_or(0),
    ))
    .await
    .map_err(|error| err(name, format!("cannot listen for the sign-in: {error}")))?;
    let port = listener
        .local_addr()
        .map_err(|error| err(name, error.to_string()))?
        .port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK}");
    let registered = match oauth {
        Some(own) => Registered {
            id: own.client_id.clone(),
            secret: keys.client_secret(name, url)?,
        },
        None => match &found.server.registration_endpoint {
            Some(endpoint) => register(name, &client, endpoint, &redirect).await?,
            None => {
                return Ok(Begun::Token {
                    registered_only: true,
                });
            }
        },
    };
    let state = random_text()?;
    let verifier = random_text()?;
    let address = authorization_address(&found, &registered.id, &redirect, &state, &verifier)
        .map_err(|reason| err(name, reason))?;
    Ok(Begun::SignIn(Box::new(SignIn {
        server: name.to_owned(),
        host: host_of(&found.server.authorization_endpoint),
        address,
        listener,
        redirect,
        state,
        verifier,
        client: registered,
        meta: found.server,
        resource: found.resource,
        scope: found.scope,
    })))
}

/// The resource's metadata, then its authorization server's: `None` where the server names none — it takes no
/// OAuth.
async fn discover(
    name: &str,
    client: &reqwest::Client,
    url: &str,
    said: &Challenge,
) -> Result<Option<Discovered>, HarnessError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| err(name, "its URL is no URL"))?;
    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path().trim_end_matches('/');
    // Where the 401 points first, then both well-known places: below the path, and at the root (GitLab's served
    // HTML at one of them for a while — an answer that is no JSON is passed by).
    let mut places: Vec<String> = said.resource_metadata.iter().cloned().collect();
    if !path.is_empty() {
        places.push(format!("{origin}{RESOURCE_METADATA}{path}"));
    }
    places.push(format!("{origin}{RESOURCE_METADATA}"));
    let mut meta = None;
    for place in places {
        if !reachable(url, &place) {
            continue;
        }
        if let Some(found) = get_json::<ResourceMeta>(client, &place).await {
            meta = Some(found);
            break;
        }
    }
    let (resource, issuer, scopes, named) = match meta {
        Some(meta) => {
            let resource = meta.resource.unwrap_or_else(|| url.to_owned());
            // A resource's metadata speaks for that resource only: one for another server is somebody else's.
            if !covers(&resource, url) {
                return Err(err(
                    name,
                    format!(
                        "its metadata speaks for another resource ({})",
                        host_of(&resource)
                    ),
                ));
            }
            let Some(issuer) = meta.authorization_servers.into_iter().next() else {
                return Ok(None);
            };
            (resource, issuer, meta.scopes_supported, true)
        }
        // A server of the protocol's first version names no resource metadata: its own origin is its authorization
        // server.
        None => (url.to_owned(), origin.clone(), Vec::new(), false),
    };
    if !reachable(url, &issuer) {
        return Err(err(
            name,
            "its authorization server is not reached over https",
        ));
    }
    let Some(server) = server_meta(client, &issuer).await else {
        // A resource that names its authorization server wants OAuth: metadata that cannot be had now is a failure
        // to say, not a reason to ask for a token.
        if named {
            return Err(err(
                name,
                format!(
                    "the metadata of its authorization server {} cannot be read",
                    host_of(&issuer)
                ),
            ));
        }
        return Ok(None);
    };
    if !same_url(&server.issuer, &issuer) {
        return Err(err(
            name,
            "its authorization server's metadata names another issuer",
        ));
    }
    for endpoint in [
        Some(&server.authorization_endpoint),
        Some(&server.token_endpoint),
        server.registration_endpoint.as_ref(),
        server.revocation_endpoint.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if !reachable(url, endpoint) {
            return Err(err(
                name,
                "an endpoint of its authorization server is not reached over https",
            ));
        }
    }
    let scope = said
        .scope
        .clone()
        .filter(|scope| !scope.trim().is_empty())
        .or_else(|| (!scopes.is_empty()).then(|| scopes.join(" ")));
    Ok(Some(Discovered {
        resource,
        server,
        scope,
    }))
}

/// The authorization server's metadata, at the places RFC 8414 and OpenID Connect put it for `issuer`.
async fn server_meta(client: &reqwest::Client, issuer: &str) -> Option<ServerMeta> {
    let parsed = reqwest::Url::parse(issuer).ok()?;
    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path().trim_end_matches('/');
    let mut places = Vec::new();
    for suffix in SERVER_METADATA {
        places.push(format!("{origin}{suffix}{path}"));
    }
    if !path.is_empty() {
        places.push(format!("{origin}{path}{}", SERVER_METADATA[1]));
    }
    for place in places {
        if let Some(meta) = get_json::<ServerMeta>(client, &place).await {
            return Some(meta);
        }
    }
    None
}

/// Glut registered with the authorization server (RFC 7591) as a public client for this redirect.
async fn register(
    name: &str,
    client: &reqwest::Client,
    endpoint: &str,
    redirect: &str,
) -> Result<Registered, HarnessError> {
    #[derive(Deserialize)]
    struct Answer {
        client_id: String,
        #[serde(default)]
        client_secret: Option<String>,
    }
    let response = client
        .post(endpoint)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .body(
            json!({
                "client_name": "Glut",
                "redirect_uris": [redirect],
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
            })
            .to_string(),
        )
        .send()
        .await
        .map_err(|error| {
            err(
                name,
                format!(
                    "its registration cannot be reached: {}",
                    error.without_url()
                ),
            )
        })?;
    let status = response.status();
    let text = read(response)
        .await
        .ok_or_else(|| err(name, "its registration's answer is too large"))?;
    if !status.is_success() {
        return Err(err(
            name,
            format!(
                "its authorization server refused Glut's registration ({status}){}",
                said_error(&text)
            ),
        ));
    }
    let answer: Answer = serde_json::from_str(&text)
        .map_err(|_| err(name, "its registration's answer holds no client id"))?;
    Ok(Registered {
        id: answer.client_id,
        secret: answer.client_secret.filter(|secret| !secret.is_empty()),
    })
}

/// The authorization endpoint with the request's parameters.
fn authorization_address(
    found: &Discovered,
    client: &str,
    redirect: &str,
    state: &str,
    verifier: &str,
) -> Result<String, String> {
    let mut address = reqwest::Url::parse(&found.server.authorization_endpoint)
        .map_err(|_| "its authorization endpoint is no URL".to_owned())?;
    {
        let mut query = address.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client)
            .append_pair("redirect_uri", redirect)
            .append_pair("state", state)
            .append_pair("code_challenge", &challenge_of(verifier))
            .append_pair("code_challenge_method", "S256")
            .append_pair("resource", &found.resource);
        if let Some(scope) = &found.scope {
            query.append_pair("scope", scope);
        }
    }
    Ok(address.into())
}

/// A token endpoint's answer (RFC 6749 §5.1), what Glut keeps of it.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: String,
    #[serde(default)]
    token_type: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// One request to a token endpoint; a refused grant (`invalid_grant`) is a sign-in to do again.
async fn token_request(
    name: &str,
    endpoint: &str,
    form: &[(&str, String)],
) -> Result<TokenAnswer, HarnessError> {
    let response = http(name)?
        .post(endpoint)
        .header(ACCEPT, "application/json")
        .form(form)
        .send()
        .await
        .map_err(|error| {
            err(
                name,
                format!(
                    "its token endpoint cannot be reached: {}",
                    error.without_url()
                ),
            )
        })?;
    let status = response.status();
    let text = read(response)
        .await
        .ok_or_else(|| err(name, "its token endpoint's answer is too large"))?;
    if !status.is_success() {
        if text.contains("invalid_grant") {
            return Err(HarnessError::McpSignIn {
                server: name.to_owned(),
            });
        }
        return Err(err(
            name,
            format!("its token endpoint said no ({status}){}", said_error(&text)),
        ));
    }
    let answer: TokenAnswer = serde_json::from_str(&text)
        .map_err(|_| err(name, "its token endpoint's answer holds no token"))?;
    if answer
        .token_type
        .as_deref()
        .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(err(
            name,
            "its token endpoint handed out a token that is no bearer token",
        ));
    }
    if answer.access_token.is_empty() {
        return Err(err(name, "its token endpoint handed out an empty token"));
    }
    Ok(answer)
}

/// `grant` refreshed (RFC 6749 §6): the new access token, the new refresh token where the server rotates it.
///
/// # Errors
///
/// [`HarnessError::McpSignIn`] when there is nothing to refresh with or the server refused the grant;
/// [`HarnessError::Mcp`] when the token endpoint failed otherwise.
pub(crate) async fn refresh(name: &str, grant: &Grant) -> Result<Grant, HarnessError> {
    let Some(refresh) = &grant.refresh_token else {
        return Err(HarnessError::McpSignIn {
            server: name.to_owned(),
        });
    };
    let mut form = vec![
        ("grant_type", "refresh_token".to_owned()),
        ("refresh_token", refresh.clone()),
        ("client_id", grant.client_id.clone()),
        ("resource", grant.resource.clone()),
    ];
    if let Some(secret) = &grant.client_secret {
        form.push(("client_secret", secret.clone()));
    }
    let answer = token_request(name, &grant.token_endpoint, &form).await?;
    Ok(Grant {
        access_token: answer.access_token,
        refresh_token: answer.refresh_token.or_else(|| grant.refresh_token.clone()),
        expires_at: answer.expires_in.map(|secs| now() + secs),
        scope: answer.scope.or_else(|| grant.scope.clone()),
        ..grant.clone()
    })
}

/// Revokes `grant` at its server (RFC 7009) — the refresh token, which takes the access tokens with it, then the
/// access token. `false`: the server offers no revocation.
///
/// # Errors
///
/// [`HarnessError::Mcp`] when the revocation endpoint cannot be reached or refuses.
pub async fn revoke(name: &str, grant: &Grant) -> Result<bool, HarnessError> {
    let Some(endpoint) = &grant.revocation_endpoint else {
        return Ok(false);
    };
    let client = http(name)?;
    let tokens = grant
        .refresh_token
        .iter()
        .map(|token| (token, "refresh_token"))
        .chain([(&grant.access_token, "access_token")]);
    for (token, hint) in tokens {
        let mut form = vec![
            ("token", token.clone()),
            ("token_type_hint", hint.to_owned()),
            ("client_id", grant.client_id.clone()),
        ];
        if let Some(secret) = &grant.client_secret {
            form.push(("client_secret", secret.clone()));
        }
        let response = client
            .post(endpoint.as_str())
            .form(&form)
            .send()
            .await
            .map_err(|error| {
                err(
                    name,
                    format!("its revocation cannot be reached: {}", error.without_url()),
                )
            })?;
        if !response.status().is_success() {
            return Err(err(
                name,
                format!("its revocation said no ({})", response.status()),
            ));
        }
    }
    Ok(true)
}

/// What a sign-out did (T56).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedOut {
    /// Nothing was kept for the server.
    Nothing,
    /// A token the owner typed, forgotten here (it is theirs to withdraw where they made it).
    Token,
    /// The grant revoked at the server and forgotten here.
    Revoked,
    /// The grant forgotten here; the server offers no revocation.
    NotOffered,
    /// The grant forgotten here; the server's revocation failed with this reason.
    RevokeFailed(String),
}

/// Signs the server `name` at `url` out: what `keys` holds for it forgotten, a grant revoked at its server first
/// where it offers that (RFC 7009) — forgotten here either way.
///
/// # Errors
///
/// [`HarnessError::Settings`] when the store cannot be read or written.
pub async fn sign_out(name: &str, url: &str, keys: &Keys) -> Result<SignedOut, HarnessError> {
    let had = keys.signed(name, url).ok().flatten();
    let grant = keys.forget(name, url)?;
    Ok(match (grant, had) {
        (Some(grant), _) => match revoke(name, &grant).await {
            Ok(true) => SignedOut::Revoked,
            Ok(false) => SignedOut::NotOffered,
            Err(error) => SignedOut::RevokeFailed(error.to_string()),
        },
        (None, Some(_)) => SignedOut::Token,
        (None, None) => SignedOut::Nothing,
    })
}

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
/// or a token elsewhere), a time limit.
fn http(name: &str) -> Result<reqwest::Client, HarnessError> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| err(name, error.to_string()))
}

/// A JSON document at `url`, `None` for anything else: an error status, HTML, an answer too large.
async fn get_json<T: DeserializeOwned>(client: &reqwest::Client, url: &str) -> Option<T> {
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
async fn read(mut response: reqwest::Response) -> Option<String> {
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
fn said_error(text: &str) -> String {
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
fn described(text: &str) -> String {
    crate::permissions::printable(&text.chars().take(DESCRIPTION_CHARS).collect::<String>())
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
fn pairs_of(query: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(&format!("http://x/?{query}"))
        .map(|url| url.query_pairs().into_owned().collect())
        .unwrap_or_default()
}

/// The parameters of an address the owner pasted: a whole URL, or only what follows its `?`.
fn query_of(address: &str) -> Vec<(String, String)> {
    let address = address.trim();
    match address.split_once('?') {
        Some((_, query)) => pairs_of(query.split('#').next().unwrap_or(query)),
        None => pairs_of(address),
    }
}

/// Whether `resource` speaks for the server at `url`: the same origin, and `url`'s path at or below its path.
fn covers(resource: &str, url: &str) -> bool {
    let (Ok(resource), Ok(url)) = (reqwest::Url::parse(resource), reqwest::Url::parse(url)) else {
        return false;
    };
    let base = resource.path().trim_end_matches('/');
    let path = url.path().trim_end_matches('/');
    resource.origin() == url.origin() && (path == base || path.starts_with(&format!("{base}/")))
}

/// Whether `endpoint`, which the server's metadata named, may be reached for the server at `url`: over https, and on
/// this machine only for a server on this machine — a remote server must not steer Glut's requests, nor the owner's
/// browser, at what listens on the loopback.
fn reachable(url: &str, endpoint: &str) -> bool {
    let local = |url: &str| {
        reqwest::Url::parse(url).is_ok_and(|parsed| {
            let host = parsed.host_str().unwrap_or_default();
            let host = host.trim_start_matches('[').trim_end_matches(']');
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified())
        })
    };
    crate::model::secure_enough(endpoint) && (local(url) || !local(endpoint))
}

/// Whether two URLs are the same but for a trailing slash.
fn same_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

fn host_of(url: &str) -> String {
    super::keys::host_of(url)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// [`RANDOM_BYTES`] from the system's random source, as base64url.
fn random_text() -> Result<String, HarnessError> {
    let mut bytes = [0u8; RANDOM_BYTES];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|error| err("glut", format!("no random bytes for the sign-in: {error}")))?;
    Ok(base64url(&bytes))
}

/// PKCE's S256 challenge of `verifier` (RFC 7636 §4.2).
fn challenge_of(verifier: &str) -> String {
    use sha2::{Digest, Sha256};
    base64url(&Sha256::digest(verifier.as_bytes()))
}

/// `bytes` as base64url without padding (RFC 4648 §5).
fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_and_the_pkce_challenge_are_the_rfc_s() {
        assert_eq!(base64url(b""), "");
        assert_eq!(base64url(b"f"), "Zg");
        assert_eq!(base64url(b"fo"), "Zm8");
        assert_eq!(base64url(b"foo"), "Zm9v");
        assert_eq!(base64url(&[0xfb, 0xff]), "-_8");
        // RFC 7636, appendix B.
        assert_eq!(
            challenge_of("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (a, b) = (random_text().unwrap(), random_text().unwrap());
        assert_eq!(a.len(), 43);
        assert_ne!(a, b);
    }

    #[test]
    fn a_bearer_challenge_is_read_with_its_quoted_values() {
        let said = challenge(concat!(
            r#"Bearer error="invalid_token", "#,
            r#"resource_metadata="https://x.example/.well-known/oauth-protected-resource/mcp", "#,
            r#"scope="mcp read""#
        ))
        .unwrap();
        assert_eq!(
            said.resource_metadata.as_deref(),
            Some("https://x.example/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(said.scope.as_deref(), Some("mcp read"));
        assert_eq!(said.error.as_deref(), Some("invalid_token"));
        let plain = challenge(r#"Basic realm="x", Bearer error=insufficient_scope"#).unwrap();
        assert_eq!(plain.error.as_deref(), Some("insufficient_scope"));
        let escaped = challenge(r#"bearer scope="a \"b\"""#).unwrap();
        assert_eq!(escaped.scope.as_deref(), Some(r#"a "b""#));
        assert_eq!(challenge(r#"Basic realm="bearer""#), None);
    }

    #[test]
    fn a_remote_server_cannot_point_at_the_loopback() {
        assert!(reachable(
            "https://mcp.example/mcp",
            "https://auth.example/token"
        ));
        assert!(!reachable(
            "https://mcp.example/mcp",
            "http://127.0.0.1:8080/token"
        ));
        assert!(!reachable(
            "https://mcp.example/mcp",
            "https://localhost/token"
        ));
        assert!(!reachable("https://mcp.example/mcp", "https://[::1]/token"));
        assert!(!reachable(
            "https://mcp.example/mcp",
            "http://auth.example/token"
        ));
        assert!(reachable(
            "http://127.0.0.1:3000/mcp",
            "http://127.0.0.1:3000/token"
        ));
        assert!(reachable(
            "http://localhost:3000/mcp",
            "https://auth.example/token"
        ));
    }

    #[test]
    fn a_resource_speaks_for_its_own_origin_and_path_only() {
        assert!(covers(
            "https://mcp.notion.com",
            "https://mcp.notion.com/mcp"
        ));
        assert!(covers("https://x.example/mcp/", "https://x.example/mcp"));
        assert!(covers(
            "https://x.example/api/v4/mcp",
            "https://x.example/api/v4/mcp"
        ));
        assert!(!covers("https://x.example/mcp", "https://x.example/mcpx"));
        assert!(!covers("https://evil.example", "https://x.example/mcp"));
        assert!(!covers("http://x.example", "https://x.example/mcp"));
    }

    #[test]
    fn a_pasted_address_gives_its_parameters() {
        let pairs = query_of(" http://127.0.0.1:5555/callback?code=a%2Bb&state=s1#x ");
        assert_eq!(
            pairs,
            [
                ("code".to_owned(), "a+b".to_owned()),
                ("state".to_owned(), "s1".to_owned())
            ]
        );
        assert_eq!(query_of("code=c&state=s").len(), 2);
    }

    // ── a whole sign-in against a fake server on the loopback ─────────────────────────────────────────────────

    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex as StdMutex};

    use serde_json::Value;

    use crate::mcp::keys::tests::Memory;

    /// What the fake was asked: method, path, the authorization it carried, the body.
    type Seen = Arc<StdMutex<Vec<(String, String, Option<String>, String)>>>;

    /// A hosted MCP server with its authorization server on one loopback port: it takes only the access token
    /// `valid`, hands out `at-1` for the code and `at-2` for a refresh.
    async fn fake(valid: &'static str) -> (String, Seen) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let (base, record) = (root.clone(), Arc::clone(&seen));
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let (head, body) = crate::model::fake::read_request(&mut socket).await;
                let mut line = head.lines().next().unwrap_or_default().split(' ');
                let (method, path) = (
                    line.next().unwrap_or_default().to_owned(),
                    line.next().unwrap_or_default().to_owned(),
                );
                let bearer = head.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("authorization")
                        .then(|| value.trim().trim_start_matches("Bearer ").to_owned())
                });
                let body = String::from_utf8_lossy(&body).into_owned();
                record.lock().unwrap().push((
                    method.clone(),
                    path.clone(),
                    bearer.clone(),
                    body.clone(),
                ));
                let json = |value: Value| (200, value.to_string(), String::new());
                let (status, text, extra) = match path.as_str() {
                    "/mcp" if bearer.as_deref() == Some(valid) => {
                        let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                        let id = request.get("id").cloned().unwrap_or(Value::Null);
                        match request.get("method").and_then(Value::as_str) {
                            Some("initialize") => {
                                json(serde_json::json!({"jsonrpc": "2.0", "id": id,
                                "result": {"protocolVersion": "2025-06-18", "capabilities": {}}}))
                            }
                            Some("tools/list") => {
                                json(serde_json::json!({"jsonrpc": "2.0", "id": id,
                                "result": {"tools": [{"name": "search", "inputSchema": {"type": "object"}}]}}))
                            }
                            _ => (202, String::new(), String::new()),
                        }
                    }
                    "/mcp" => (
                        401,
                        String::new(),
                        format!(
                            "www-authenticate: Bearer error=\"invalid_token\", \
                             resource_metadata=\"{base}/.well-known/oauth-protected-resource/mcp\"\r\n"
                        ),
                    ),
                    "/.well-known/oauth-protected-resource/mcp" => json(serde_json::json!({
                        "resource": format!("{base}/mcp"),
                        "authorization_servers": [format!("{base}/auth")],
                        "scopes_supported": ["mcp"],
                    })),
                    "/.well-known/oauth-authorization-server/auth" => json(serde_json::json!({
                        "issuer": format!("{base}/auth"),
                        "authorization_endpoint": format!("{base}/auth/authorize"),
                        "token_endpoint": format!("{base}/auth/token"),
                        "registration_endpoint": format!("{base}/auth/register"),
                        "revocation_endpoint": format!("{base}/auth/revoke"),
                        "code_challenge_methods_supported": ["S256"],
                    })),
                    "/auth/register" => json(serde_json::json!({"client_id": "c-9"})),
                    "/auth/token" if body.contains("grant_type=authorization_code") => {
                        json(serde_json::json!({
                            "access_token": "at-1", "refresh_token": "rt-1", "expires_in": 3600, "token_type": "Bearer",
                        }))
                    }
                    "/auth/token" if body.contains("refresh_token=rt-1") => {
                        json(serde_json::json!({
                            "access_token": "at-2", "expires_in": 3600, "token_type": "bearer",
                        }))
                    }
                    "/auth/token" => (
                        400,
                        r#"{"error":"invalid_grant"}"#.to_owned(),
                        String::new(),
                    ),
                    "/auth/revoke" => (200, String::new(), String::new()),
                    _ => (404, "<html>not here</html>".to_owned(), String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\
                     connection: close\r\n{extra}\r\n{text}",
                    text.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (root, seen)
    }

    fn http_config(root: &str) -> ServerConfig {
        ServerConfig::Http {
            url: format!("{root}/mcp"),
            headers: BTreeMap::new(),
            oauth: None,
        }
    }

    /// The address's parameters, as the browser would follow it.
    fn query(address: &str) -> BTreeMap<String, String> {
        reqwest::Url::parse(address)
            .unwrap()
            .query_pairs()
            .into_owned()
            .collect()
    }

    #[tokio::test]
    async fn a_sign_in_follows_the_metadata_registers_and_comes_back_to_the_listener() {
        let (root, seen) = fake("at-1").await;
        let config = http_config(&root);
        let keys = Keys::of(Box::new(Memory::default()));
        let Begun::SignIn(sign_in) = begin("notes", &config, &keys).await.unwrap() else {
            panic!("a sign-in was expected");
        };
        assert_eq!(sign_in.host(), host_of(&root));
        let asked = query(sign_in.address());
        assert!(
            sign_in
                .address()
                .starts_with(&format!("{root}/auth/authorize?"))
        );
        assert_eq!(asked["client_id"], "c-9");
        assert_eq!(asked["code_challenge_method"], "S256");
        assert_eq!(asked["scope"], "mcp", "the scopes the server names");
        assert_eq!(asked["resource"], format!("{root}/mcp"));
        assert_eq!(asked["code_challenge"].len(), 43);
        let redirect = asked["redirect_uri"].clone();
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with(CALLBACK));
        // The browser: first a stray request, then one with another state, then the right one.
        let (state, browser) = (asked["state"].clone(), redirect.clone());
        let browse = tokio::spawn(async move {
            let client = reqwest::Client::new();
            let base = browser.trim_end_matches(CALLBACK);
            let _ = client.get(format!("{base}/favicon.ico")).send().await;
            // A page elsewhere that knocks at the callback with a state of its own ends nothing.
            let _ = client
                .get(format!("{browser}?code=stolen&state=forged"))
                .send()
                .await;
            client
                .get(format!("{browser}?code=the-code&state={state}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        });
        let grant = sign_in.finish(std::future::pending()).await.unwrap();
        assert!(browse.await.unwrap().contains("signed in"));
        assert_eq!(grant.access_token, "at-1");
        assert_eq!(grant.refresh_token.as_deref(), Some("rt-1"));
        assert_eq!(grant.client_id, "c-9");
        assert_eq!(grant.scope.as_deref(), Some("mcp"));
        assert!(grant.expires_at.unwrap() > now() + 3000);
        let token_call = seen
            .lock()
            .unwrap()
            .iter()
            .find(|(_, path, _, body)| path == "/auth/token" && body.contains("authorization_code"))
            .map(|(_, _, _, body)| body.clone())
            .unwrap();
        assert!(token_call.contains("code=the-code"), "{token_call}");
        assert!(token_call.contains("code_verifier="), "{token_call}");
        assert!(token_call.contains("redirect_uri="), "{token_call}");

        // Kept, the server is reached with it; a sign-out revokes it there and forgets it here.
        keys.keep_grant("notes", &format!("{root}/mcp"), &grant)
            .unwrap();
        let client = crate::mcp::Client::connect_signed("notes", &config, &keys)
            .await
            .unwrap();
        assert_eq!(client.tools().len(), 1);
        assert_eq!(
            sign_out("notes", &format!("{root}/mcp"), &keys)
                .await
                .unwrap(),
            SignedOut::Revoked
        );
        assert_eq!(keys.signed("notes", &format!("{root}/mcp")).unwrap(), None);
        assert_eq!(
            sign_out("notes", &format!("{root}/mcp"), &keys)
                .await
                .unwrap(),
            SignedOut::Nothing
        );
        let revoked = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, path, _, _)| path == "/auth/revoke")
            .count();
        assert_eq!(revoked, 2, "the refresh token and the access token");
    }

    #[tokio::test]
    async fn a_pasted_address_finishes_and_a_wrong_state_or_a_refusal_does_not() {
        let (root, _) = fake("at-1").await;
        let config = http_config(&root);
        let keys = Keys::of(Box::new(Memory::default()));
        let begun = |keys: Keys| {
            let config = config.clone();
            async move {
                match begin("notes", &config, &keys).await.unwrap() {
                    Begun::SignIn(sign_in) => sign_in,
                    _ => panic!("a sign-in was expected"),
                }
            }
        };
        let sign_in = begun(keys.clone()).await;
        let state = query(sign_in.address())["state"].clone();
        let pasted = format!("http://127.0.0.1:1/callback?code=pasted&state={state}");
        let grant = sign_in.finish(async { Some(pasted) }).await.unwrap();
        assert_eq!(grant.access_token, "at-1");

        let sign_in = begun(keys.clone()).await;
        let wrong = sign_in
            .finish(async { Some("http://127.0.0.1:1/callback?code=x&state=forged".to_owned()) })
            .await
            .unwrap_err()
            .to_string();
        assert!(wrong.contains("another request"), "{wrong}");

        let sign_in = begun(keys).await;
        let state = query(sign_in.address())["state"].clone();
        let refused = sign_in
            .finish(async move {
                Some(format!(
                    "?error=access_denied&error_description=no%1b[2Jway&state={state}"
                ))
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(refused.contains("access_denied"), "{refused}");
        assert!(
            !refused.contains('\u{1b}'),
            "a server's words are made printable: {refused:?}"
        );
    }

    #[tokio::test]
    async fn a_spent_token_is_refreshed_by_itself_and_a_server_without_a_sign_in_wants_one() {
        let (root, seen) = fake("at-2").await;
        let config = http_config(&root);
        let url = format!("{root}/mcp");
        let keys = Keys::of(Box::new(Memory::default()));
        let none = crate::mcp::Client::connect_signed("notes", &config, &keys).await;
        assert!(matches!(none, Err(HarnessError::McpSignIn { .. })));
        // A grant the server no longer takes (at-1): renewed once with its refresh token, then kept.
        let mut grant = crate::mcp::keys::tests::grant("at-1");
        grant.expires_at = None;
        grant.refresh_token = Some("rt-1".into());
        grant.token_endpoint = format!("{root}/auth/token");
        keys.keep_grant("notes", &url, &grant).unwrap();
        let client = crate::mcp::Client::connect_signed("notes", &config, &keys)
            .await
            .unwrap();
        assert_eq!(client.tools().len(), 1);
        let Some(crate::mcp::keys::Signed::OAuth(kept)) = keys.signed("notes", &url).unwrap()
        else {
            panic!("the grant stays kept");
        };
        assert_eq!(kept.access_token, "at-2");
        assert_eq!(
            kept.refresh_token.as_deref(),
            Some("rt-1"),
            "kept where the server rotates none"
        );
        // A refresh the server refuses wants a sign-in.
        let mut stale = crate::mcp::keys::tests::grant("at-0");
        stale.refresh_token = Some("rt-gone".into());
        stale.token_endpoint = format!("{root}/auth/token");
        keys.keep_grant("notes", &url, &stale).unwrap();
        let refused = crate::mcp::Client::connect_signed("notes", &config, &keys).await;
        assert!(matches!(refused, Err(HarnessError::McpSignIn { .. })));
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|(_, path, bearer, _)| path == "/mcp" && bearer.as_deref() == Some("at-2"))
        );
    }

    #[tokio::test]
    async fn the_owners_own_header_wins_over_a_kept_sign_in_and_its_refusal_is_no_sign_in() {
        let (root, seen) = fake("at-1").await;
        let url = format!("{root}/mcp");
        let keys = Keys::of(Box::new(Memory::default()));
        keys.keep_token("notes", &url, "kept").unwrap();
        let own = |token: &str| ServerConfig::Http {
            url: url.clone(),
            headers: BTreeMap::from([("Authorization".to_owned(), format!("Bearer {token}"))]),
            oauth: None,
        };
        let client = crate::mcp::Client::connect_signed("notes", &own("at-1"), &keys)
            .await
            .unwrap();
        assert_eq!(client.tools().len(), 1);
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .all(|(_, _, bearer, _)| bearer.as_deref() != Some("kept")),
            "the kept token never goes out beside the owner's header"
        );
        let refused = crate::mcp::Client::connect_signed("notes", &own("wrong"), &keys).await;
        assert!(
            matches!(&refused, Err(HarnessError::Mcp { message, .. }) if message.contains("Authorization header")),
            "{:?}",
            refused.err()
        );
    }

    #[tokio::test]
    async fn metadata_for_another_resource_or_issuer_or_without_pkce_is_refused() {
        // A server whose metadata names another resource is somebody else's.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let base = root.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let (head, _) = crate::model::fake::read_request(&mut socket).await;
                let path = head.split(' ').nth(1).unwrap_or_default().to_owned();
                let (status, text) = match path.as_str() {
                    "/mcp" => (401, String::new()),
                    "/.well-known/oauth-protected-resource/mcp" => (
                        200,
                        serde_json::json!({"resource": "https://elsewhere.example/mcp",
                            "authorization_servers": [base.clone()]})
                        .to_string(),
                    ),
                    _ => (404, String::new()),
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                    text.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        let refused = begin("x", &http_config(&root), &Keys::none()).await;
        assert!(
            matches!(&refused, Err(error) if error.to_string().contains("another resource")),
            "{:?}",
            refused.err()
        );
    }
}
