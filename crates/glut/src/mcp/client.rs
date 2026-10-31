//! The MCP protocol (`docs/plans/harness.md`, HS5, M3): one connected server behind one of two transports, the
//! handshake, the tool list and `tools/call`.
//!
//! Over stdio the protocol is JSON-RPC 2.0, one message per line; over HTTP every message is one `POST`,
//! answered either as JSON or as a Server-Sent Events stream. Both are read by hand here: the surface is three
//! methods and a cursor, and a protocol library would pull a hundred crates into a crate meant to stand alone
//! — the same reason the client Axiomata once had (`54173bf`) was written by hand.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio as Piped;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

use super::config::{ServerConfig, expand_vars};
use super::elicit;
use super::keys::{Keys, Signed};
use crate::config::API_KEY_ENV;
use crate::error::HarnessError;
use crate::permissions::Gate;

/// The protocol version the client proposes: the first with a page among the questions a server may ask (T67). The
/// server answers with the one it speaks, and nothing here is version-specific, so whichever comes back is taken.
const PROTOCOL_VERSION: &str = "2025-11-25";
/// How long a server may take to start and to list its tools (M3): a handshake that hangs must not eat the
/// run's own time limit.
const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long one tool call may take (M3): a connector can legitimately take a while — a mail search over a slow
/// connection, a search across a big repository.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);
/// The most pages of `tools/list` read (M3): a server that answers with a cursor for ever is not humoured.
const MAX_PAGES: usize = 10;
/// The most tools one server may offer (M3): the model's tool list has room for a bounded number of them, and
/// what runs past this is a misconfigured server rather than a connector worth having.
const MAX_TOOLS: usize = 200;
/// The largest answer read from a server (M3), the same cap the model API gets: a tool result is text, not
/// gigabytes.
const MAX_ANSWER_BYTES: usize = 16 * 1024 * 1024;
/// How much of a server's error answer an error keeps: enough to read what it said, not a whole HTML page.
const ERROR_BODY_CHARS: usize = 500;
/// The header a Streamable HTTP server issues its session id with, and which every later request sends back.
const SESSION_HEADER: &str = "mcp-session-id";
/// JSON-RPC's code for a method nobody here answers.
const METHOD_NOT_FOUND: i64 = -32601;
/// The most questions a server may ask the owner during one request (T67): a tool asks what it needs, it does not
/// keep a call alive by asking.
const MAX_QUESTIONS: usize = 5;
/// How often a request whose clock stands still looks whether it runs again.
const STOPPED_POLL: Duration = Duration::from_secs(1);

/// A failure of one MCP server: it is reported and left out, the run goes on without it.
fn mcp_err(server: &str, message: impl Into<String>) -> HarnessError {
    HarnessError::Mcp {
        server: server.to_owned(),
        message: message.into(),
    }
}

/// One JSON-RPC message as a transport sends it: the id the answer is expected under.
#[derive(Debug, Clone)]
pub struct Request {
    pub id: u64,
    pub method: String,
    pub params: Value,
}

/// What a request answers the server's own requests with while it waits for its answer (T67): a `ping`, and a
/// question to the owner through the gate of the call it serves — none during the handshake, so a server asking then
/// hears a decline. Everything else is a method Glut does not offer.
pub struct Asks {
    gate: Option<Arc<Gate>>,
    clock: Clock,
    /// How many questions the server asked so far, and whether the owner turned one down — after which every
    /// further one of this request is declined unasked.
    asked: std::sync::atomic::AtomicUsize,
    refused: std::sync::atomic::AtomicBool,
}

impl Asks {
    /// Nobody to ask: every question is declined.
    pub fn nobody() -> Self {
        Asks::through(None)
    }

    fn through(gate: Option<Arc<Gate>>) -> Self {
        Asks {
            gate,
            clock: Clock::default(),
            asked: std::sync::atomic::AtomicUsize::new(0),
            refused: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// The response to `message` when it is the server's own request, [`None`] for anything else (a notification,
    /// an answer).
    async fn answer(&self, server: &str, message: &Value) -> Option<Value> {
        let id = message
            .get("id")
            .filter(|id| id.is_number() || id.is_string())?
            .clone();
        let method = message.get("method").and_then(Value::as_str)?;
        let outcome = match method {
            "ping" => Ok(json!({})),
            "elicitation/create" => Ok(self
                .elicit(server, message.get("params").unwrap_or(&Value::Null))
                .await),
            _ => {
                Err(json!({"code": METHOD_NOT_FOUND, "message": "glut does not offer this method"}))
            }
        };
        Some(match outcome {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err(error) => json!({"jsonrpc": "2.0", "id": id, "error": error}),
        })
    }

    /// The owner's answer to a server's question — a decline when it is none Glut shows, or nobody is there. The
    /// request's clock stands still while the owner answers: a form is not the server's slowness.
    async fn elicit(&self, server: &str, params: &Value) -> Value {
        use std::sync::atomic::Ordering;
        let asked = self.asked.fetch_add(1, Ordering::SeqCst) + 1;
        if asked > MAX_QUESTIONS || self.refused.load(Ordering::SeqCst) {
            return elicit::Reply::Decline.result();
        }
        let reply = match (&self.gate, elicit::parse(params)) {
            (Some(gate), Ok(what)) => {
                let _stopped = self.clock.stop();
                gate.fill(elicit::Asked {
                    server: server.to_owned(),
                    from: None,
                    what,
                })
                .await
            }
            _ => elicit::Reply::Decline,
        };
        if !matches!(reply, elicit::Reply::Accept(_)) {
            self.refused.store(true, Ordering::SeqCst);
        }
        reply.result()
    }
}

/// What a request dropped half way left unsaid to its server (T67) — a stop while it ran, or while the owner filled
/// in a form: said before the next request goes out, so the server neither waits for ever nor carries on.
#[derive(Default)]
struct Owed {
    /// The request under way: it is cancelled if it never came back.
    open: Option<u64>,
    /// The ids of the server's questions put to the owner and not yet answered: each hears a cancel.
    questions: Vec<Value>,
}

impl Owed {
    /// The messages that settle what was left: one per unanswered question, then the cancel of the request.
    fn settle(&mut self) -> Vec<Value> {
        let mut messages: Vec<Value> = std::mem::take(&mut self.questions)
            .into_iter()
            .map(|id| json!({"jsonrpc": "2.0", "id": id, "result": elicit::Reply::Cancel.result()}))
            .collect();
        if let Some(id) = self.open.take() {
            messages.push(json!({
                "jsonrpc": "2.0",
                "method": "notifications/cancelled",
                "params": {"requestId": id, "reason": "the run was stopped"},
            }));
        }
        messages
    }

    /// The question `message` is put to the owner now, if it is one.
    fn asking(&mut self, message: &Value) -> bool {
        let question = message.get("method").and_then(Value::as_str) == Some("elicitation/create");
        if let (true, Some(id)) = (question, message.get("id")) {
            self.questions.push(id.clone());
        }
        question
    }
}

/// How long a request's clock stood still for the owner (T67).
#[derive(Default)]
struct Clock(Mutex<Stopped>);

#[derive(Default)]
struct Stopped {
    since: Option<Instant>,
    before: Duration,
}

impl Clock {
    /// Stops the clock until the guard is dropped.
    fn stop(&self) -> Stop<'_> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).since = Some(Instant::now());
        Stop(self)
    }

    /// How long it stood still so far, the stop under way included.
    fn is_stopped(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .since
            .is_some()
    }

    fn stopped(&self) -> Duration {
        let stopped = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        stopped.before
            + stopped
                .since
                .map_or(Duration::ZERO, |since| since.elapsed())
    }
}

/// A stopped [`Clock`]; it runs again when this is dropped.
struct Stop<'a>(&'a Clock);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        let mut stopped = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(since) = stopped.since.take() {
            stopped.before += since.elapsed();
        }
    }
}

/// One answer of a transport — boxed so two very different transports still fit behind [`Transport`], the way
/// [`crate::tools::ToolFuture`] does for the tools.
pub type TransportFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, HarnessError>> + Send + 'a>>;

/// How a client reaches its server (M3).
pub trait Transport: Send {
    /// Sends a request and returns the `result` of the response carrying its id. Notifications, answers to
    /// requests the harness no longer waits for and lines that are no JSON are on the way to it and skipped —
    /// a chatty server must not be able to wedge a request. A request of the server's own on the way is answered
    /// through `asks` (T67).
    fn send<'a>(&'a mut self, request: Request, asks: &'a Asks) -> TransportFuture<'a, Value>;
    /// Sends a message nobody answers (`notifications/initialized`).
    fn notify(&mut self, method: String, params: Value) -> TransportFuture<'_, ()>;
}

/// The `result` of `response` when it answers `id`, [`None`] when it is not that answer — a server's own
/// notification, or a response to a request the harness already gave up on. A JSON-RPC `error` object is an
/// error, never an answer.
fn result_for(server: &str, response: &Value, id: u64) -> Result<Option<Value>, HarnessError> {
    if response.get("id") != Some(&Value::from(id)) {
        return Ok(None);
    }
    if let Some(error) = response.get("error").filter(|error| !error.is_null()) {
        return Err(mcp_err(server, format!("it refused: {error}")));
    }
    Ok(Some(response.get("result").cloned().unwrap_or(Value::Null)))
}

/// A server as a child process: JSON-RPC one message per line on its stdin and stdout, stderr discarded (M3) —
/// a server that writes over the run's own diagnostics makes both unreadable.
pub struct Stdio {
    server: String,
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    owed: Owed,
}

impl Stdio {
    /// Starts the server and returns its transport.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] when the config is not a stdio one or the process cannot be started.
    pub fn spawn(server: &str, config: &ServerConfig) -> Result<Self, HarnessError> {
        let ServerConfig::Stdio { command, args, env } = config else {
            return Err(mcp_err(server, "not a stdio server"));
        };
        let mut program = Command::new(command);
        program
            .args(args)
            .env_clear()
            .envs(server_env(env))
            .stdin(Piped::piped())
            .stdout(Piped::piped())
            .stderr(Piped::null())
            // A process group of its own: what the server starts goes down with it (M4).
            .process_group(0)
            .kill_on_drop(true);
        let mut child = program
            .spawn()
            .map_err(|err| mcp_err(server, format!("cannot start {command:?}: {err}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| mcp_err(server, "its stdin is not piped"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| mcp_err(server, "its stdout is not piped"))?;
        Ok(Stdio {
            server: server.to_owned(),
            child,
            stdin,
            stdout: BufReader::new(stdout),
            owed: Owed::default(),
        })
    }

    /// One line on the wire, ending in the newline the protocol wants.
    async fn write(&mut self, message: &Value) -> Result<(), HarnessError> {
        let text = serde_json::to_string(message).map_err(|err| {
            mcp_err(
                &self.server,
                format!("a message cannot be written down: {err}"),
            )
        })?;
        let mut line = text.into_bytes();
        line.push(b'\n');
        let written = self.stdin.write_all(&line).await;
        written.map_err(|err| mcp_err(&self.server, format!("its stdin broke: {err}")))?;
        let flushed = self.stdin.flush().await;
        flushed.map_err(|err| mcp_err(&self.server, format!("its stdin broke: {err}")))
    }

    /// The next line it says, held to [`MAX_ANSWER_BYTES`] as an HTTP answer is: a server that writes without
    /// ever ending a line must not fill the harness's memory. The end of its stdout is an error: the server is gone.
    async fn read_line(&mut self) -> Result<String, HarnessError> {
        let mut line = String::new();
        // The limit, plus room for the `\r\n` that ends a line of exactly the limit.
        let read = (&mut self.stdout)
            .take(MAX_ANSWER_BYTES as u64 + 2)
            .read_line(&mut line)
            .await;
        match read {
            Ok(0) => Err(mcp_err(
                &self.server,
                "it closed its stdout before answering",
            )),
            Ok(_) if line.trim_end_matches(['\n', '\r']).len() > MAX_ANSWER_BYTES => {
                // The rest of the line is read past, so the next read starts at a message again.
                if !line.ends_with('\n') {
                    self.discard_line().await?;
                }
                Err(mcp_err(
                    &self.server,
                    format!("it wrote a line longer than {MAX_ANSWER_BYTES} bytes"),
                ))
            }
            Ok(_) => Ok(line),
            Err(err) => Err(mcp_err(&self.server, format!("its stdout broke: {err}"))),
        }
    }

    /// Reads past the rest of the current line without keeping it. A server that never ends it is stopped by the
    /// call's own time limit, which drops this wait.
    async fn discard_line(&mut self) -> Result<(), HarnessError> {
        loop {
            let buffer = self
                .stdout
                .fill_buf()
                .await
                .map_err(|err| mcp_err(&self.server, format!("its stdout broke: {err}")))?;
            if buffer.is_empty() {
                return Ok(());
            }
            match buffer.iter().position(|&byte| byte == b'\n') {
                Some(at) => {
                    self.stdout.consume(at + 1);
                    return Ok(());
                }
                None => {
                    let all = buffer.len();
                    self.stdout.consume(all);
                }
            }
        }
    }
}

impl Drop for Stdio {
    fn drop(&mut self) {
        // `kill_on_drop` reaches only the child; the group is what takes a server it started down with it (M4).
        if let Some(pid) = self.child.id()
            && let Some(pid) = rustix::process::Pid::from_raw(pid as i32)
        {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
    }
}

impl Transport for Stdio {
    fn send<'a>(&'a mut self, request: Request, asks: &'a Asks) -> TransportFuture<'a, Value> {
        Box::pin(async move {
            let id = request.id;
            let message = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": request.method,
                "params": request.params,
            });
            for settled in self.owed.settle() {
                self.write(&settled).await?;
            }
            self.owed.open = Some(id);
            self.write(&message).await?;
            let answer = self.answer_to(id, asks).await;
            self.owed.open = None;
            answer
        })
    }

    fn notify(&mut self, method: String, params: Value) -> TransportFuture<'_, ()> {
        Box::pin(async move {
            self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
                .await
        })
    }
}

impl Stdio {
    /// Reads until the answer to `id`, answering the server's own requests on the way.
    async fn answer_to(&mut self, id: u64, asks: &Asks) -> Result<Value, HarnessError> {
        loop {
            let line = self.read_line().await?;
            let Ok(response) = serde_json::from_str::<Value>(&line) else {
                // No JSON: something the server printed of its own accord. Read past it (M3).
                continue;
            };
            // A message with a method is the server's own — never the answer, whatever its id.
            if response.get("method").is_some() {
                let question = self.owed.asking(&response);
                let reply = asks.answer(&self.server, &response).await;
                if question {
                    self.owed.questions.pop();
                }
                if let Some(reply) = reply {
                    self.write(&reply).await?;
                }
                continue;
            }
            match result_for(&self.server, &response, id)? {
                Some(result) => return Ok(result),
                None => continue,
            }
        }
    }
}

/// The environment a server starts in (M3): the cleaned tool environment the shell gives its own commands
/// (`axiomata_files::toolenv`, so not the harness's keys), then the server's own `env` on top — that is what it
/// is there for.
fn server_env(extra: &BTreeMap<String, String>) -> Vec<(OsString, OsString)> {
    let ambient: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    let search: Vec<PathBuf> = ambient
        .iter()
        .find(|(key, _)| key == "PATH")
        .map(|(_, value)| std::env::split_paths(value).collect())
        .unwrap_or_default();
    let mut env = axiomata_files::toolenv::tool_env_from(ambient, &search);
    env.extend(
        extra
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value))),
    );
    env
}

/// Streamable HTTP (M3): one `POST` per message, answered either as JSON or as a Server-Sent Events stream,
/// read until the response with the request's id arrives.
pub struct Http {
    server: String,
    url: String,
    /// The headers as they go out: `${NAME}` resolved (M3), and each one checked to be a legal header before
    /// anything is sent.
    headers: BTreeMap<String, String>,
    client: reqwest::Client,
    /// The session the server issued with `initialize`, sent back with every later message.
    session: Option<String>,
    /// What the server is signed in with (T56); none where it needs no sign-in, or a header of the owner's carries
    /// the authorization.
    signed: Option<Signed>,
    /// Where a refreshed grant is kept.
    keys: Keys,
    owed: Owed,
}

impl Http {
    /// The HTTP transport for one server.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] when the config is not an HTTP one, a header cannot be resolved or built, or the
    /// client itself cannot be made — that server fails to start, the run goes on without it.
    pub fn new(server: &str, config: &ServerConfig, keys: &Keys) -> Result<Self, HarnessError> {
        let ServerConfig::Http { url, headers, .. } = config else {
            return Err(mcp_err(server, "not an http server"));
        };
        let mut resolved = BTreeMap::new();
        for (name, value) in headers {
            let value = header_value(value, |key| std::env::var(key).ok())
                .map_err(|reason| mcp_err(server, format!("the header {name:?}: {reason}")))?;
            // Checked here, not when it is sent: a value holding a line break would be a second header, and no
            // server must be able to add one at call time.
            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| mcp_err(server, format!("{name:?} is no header name")))?;
            reqwest::header::HeaderValue::from_str(&value).map_err(|_| {
                mcp_err(
                    server,
                    format!("the value of the header {name:?} cannot be sent"),
                )
            })?;
            resolved.insert(name.clone(), value);
        }
        // No time limit on the whole exchange: the client's own ([`Client::request`]) holds it, and stands still while
        // the owner fills in a form the server asked for in the middle of a stream (T67).
        let mut builder = reqwest::Client::builder()
            .connect_timeout(START_TIMEOUT)
            // A redirect would carry the headers to a host the owner never named.
            .redirect(reqwest::redirect::Policy::none());
        // A server on this machine is reached directly: a proxy of the environment would carry its token and what is
        // said to it off the machine (T64c's audit).
        let loopback = reqwest::Url::parse(url).is_ok_and(|parsed| {
            matches!(
                parsed.host_str(),
                Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
            )
        });
        if loopback {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|err| mcp_err(server, err.to_string()))?;
        // The owner's own header says how to sign in; only without one is a kept sign-in looked up.
        let signed = if headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
        {
            None
        } else {
            keys.signed(server, url)?
        };
        Ok(Http {
            server: server.to_owned(),
            url: url.clone(),
            headers: resolved,
            client,
            session: None,
            signed,
            keys: keys.clone(),
            owed: Owed::default(),
        })
    }

    /// Whether the owner's entry carries an `Authorization` header of its own.
    fn own_authorization(&self) -> bool {
        self.headers
            .keys()
            .any(|name| name.eq_ignore_ascii_case("authorization"))
    }

    /// The bearer token the requests carry, where the server is signed in.
    fn bearer(&self) -> Option<&str> {
        match &self.signed {
            Some(Signed::Token(token)) => Some(token),
            Some(Signed::OAuth(grant)) => Some(&grant.access_token),
            None => None,
        }
    }

    /// Whether the sign-in can be renewed without the owner: a grant with a refresh token.
    fn renewable(&self) -> bool {
        matches!(&self.signed, Some(Signed::OAuth(grant)) if grant.refresh_token.is_some())
    }

    /// The grant refreshed and kept (T56). A store that cannot be written keeps the old one: this session goes on
    /// with the new token, the next one may have to sign in again.
    async fn renew(&mut self) -> Result<(), HarnessError> {
        let Some(Signed::OAuth(grant)) = &self.signed else {
            return Err(HarnessError::McpSignIn {
                server: self.server.clone(),
            });
        };
        let fresh = super::oauth::refresh(&self.server, grant).await?;
        let _ = self.keys.keep_grant(&self.server, &self.url, &fresh);
        self.signed = Some(Signed::OAuth(fresh));
        Ok(())
    }

    /// One `POST` with the headers every message carries.
    fn post(&self, body: &Value) -> reqwest::RequestBuilder {
        let mut call = self
            .client
            .post(self.url.as_str())
            .header(CONTENT_TYPE, "application/json")
            .header(ACCEPT, "application/json, text/event-stream")
            .body(body.to_string());
        for (name, value) in &self.headers {
            call = call.header(name.as_str(), value.as_str());
        }
        if let Some(token) = self.bearer() {
            call = call.bearer_auth(token);
        }
        if let Some(session) = &self.session {
            call = call.header(SESSION_HEADER, session.as_str());
        }
        call
    }

    /// The session id the answer came with, if it brought one.
    fn note_session(&mut self, response: &reqwest::Response) {
        if let Some(session) = response.headers().get(SESSION_HEADER)
            && let Ok(session) = session.to_str()
            && !session.is_empty()
        {
            self.session = Some(session.to_owned());
        }
    }

    /// Posts `body` and hands back a successful response, its session noted; a server that cannot be reached
    /// or answers with an error status is the error.
    async fn exchange(&mut self, body: &Value) -> Result<reqwest::Response, HarnessError> {
        if matches!(&self.signed, Some(Signed::OAuth(grant)) if grant.spent(chrono::Utc::now().timestamp()))
            && self.renewable()
        {
            self.renew().await?;
        }
        let mut response = self.send_once(body).await?;
        // A token the server no longer takes is renewed once, where it can be; else the owner signs in again.
        if response.status() == reqwest::StatusCode::UNAUTHORIZED && self.renewable() {
            self.renew().await?;
            response = self.send_once(body).await?;
        }
        let status = response.status();
        let refused = status == reqwest::StatusCode::UNAUTHORIZED
            || (status == reqwest::StatusCode::FORBIDDEN && super::oauth::wants_scope(&response));
        // The owner's own header is theirs to mend: no sign-in of Glut's would help.
        if refused && self.signed.is_none() && self.own_authorization() {
            return Err(mcp_err(
                &self.server,
                format!("it refused the token of its Authorization header ({status})"),
            ));
        }
        if refused {
            return Err(HarnessError::McpSignIn {
                server: self.server.clone(),
            });
        }
        if !status.is_success() {
            let text = read_body(&self.server, response).await?;
            return Err(mcp_err(
                &self.server,
                format!("it answered {status}: {}", excerpt(&text)),
            ));
        }
        Ok(response)
    }

    /// One `POST` of `body`, its session noted.
    async fn send_once(&mut self, body: &Value) -> Result<reqwest::Response, HarnessError> {
        let response = self.post(body).send().await.map_err(|err| {
            mcp_err(
                &self.server,
                format!(
                    "cannot reach {}: {}",
                    shown_url(&self.url),
                    err.without_url()
                ),
            )
        })?;
        self.note_session(&response);
        Ok(response)
    }
}

/// A header's value with its `${NAME}`s put in from `env` (M3) — except the model keys, which are never a server's
/// to have: a repository's `.mcp.json` asking for `${GLUT_API_KEY}`, `${OPENROUTER_API_KEY}` or a variable the
/// owner's `providers.json` names would send it to a host of its choosing.
fn header_value(value: &str, env: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    header_value_without(value, &crate::providers::configured_key_envs(), env)
}

/// [`header_value`] with the owner's configured key variables handed in, so it is testable without a home.
fn header_value_without(
    value: &str,
    configured: &[String],
    env: impl Fn(&str) -> Option<String>,
) -> Result<String, String> {
    expand_vars(value, |key| {
        let model_key = key == API_KEY_ENV
            || crate::providers::catalog::key_envs().any(|name| name == key)
            || configured.iter().any(|name| name == key);
        (!model_key).then(|| env(key)).flatten()
    })
}

/// `url` as an error may show it: its scheme, host and port only. A token typed into `.mcp.json` can sit in the
/// user info, the query, the fragment — or the path, as hosted servers' `/mcp/<token>` URLs have it — and the
/// reason travels into events and a run's record.
fn shown_url(url: &str) -> String {
    let Ok(url) = reqwest::Url::parse(url) else {
        return "its URL".to_owned();
    };
    let Some(host) = url.host_str() else {
        return "its URL".to_owned();
    };
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let elided = if url.path() == "/" && url.query().is_none() {
        ""
    } else {
        "/…"
    };
    format!("{}://{host}{port}{elided}", url.scheme())
}

impl Transport for Http {
    fn send<'a>(&'a mut self, request: Request, asks: &'a Asks) -> TransportFuture<'a, Value> {
        Box::pin(async move {
            let id = request.id;
            let body = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": request.method,
                "params": request.params,
            });
            // What a request dropped half way left unsaid goes first; a server that takes it no more is no reason
            // to fail this one.
            for settled in self.owed.settle() {
                if let Ok(response) = self.exchange(&settled).await {
                    let _ = read_body(&self.server, response).await;
                }
            }
            self.owed.open = Some(id);
            let answer = self.answer_to(&body, id, asks).await;
            self.owed.open = None;
            answer
        })
    }

    fn notify(&mut self, method: String, params: Value) -> TransportFuture<'_, ()> {
        Box::pin(async move {
            // No id, so no response will ever carry one: the answer to the POST itself is taken as the word
            // that it arrived (M3).
            let body = json!({"jsonrpc": "2.0", "method": method, "params": params});
            let response = self.exchange(&body).await?;
            read_body(&self.server, response).await.map(|_| ())
        })
    }
}

impl Http {
    /// Posts `body` and reads its answer, a stream's or a plain one.
    async fn answer_to(
        &mut self,
        body: &Value,
        id: u64,
        asks: &Asks,
    ) -> Result<Value, HarnessError> {
        let response = self.exchange(body).await?;
        let streaming = is_stream(&response);
        if streaming {
            return self.read_stream(response, id, asks).await;
        }
        let text = read_body(&self.server, response).await?;
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| mcp_err(&self.server, "its answer is no JSON"))?;
        // A plain answer cannot ask: a message with a method is none, whatever its id.
        if value.get("method").is_some() {
            return Err(mcp_err(&self.server, "its answer was a request of its own"));
        }
        result_for(&self.server, &value, id)?
            .ok_or_else(|| mcp_err(&self.server, "its answer was for another request"))
    }
}

/// Whether the answer comes as a Server-Sent Events stream.
fn is_stream(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
}

/// A response's body, read in chunks up to [`MAX_ANSWER_BYTES`].
async fn read_body(server: &str, mut response: reqwest::Response) -> Result<String, HarnessError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ANSWER_BYTES as u64)
    {
        return Err(mcp_err(server, "its answer is too large"));
    }
    let mut body: Vec<u8> = Vec::new();
    loop {
        let chunk = response.chunk().await.map_err(|err| {
            mcp_err(
                server,
                format!("reading its answer failed: {}", err.without_url()),
            )
        })?;
        let Some(chunk) = chunk else {
            break;
        };
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return Err(mcp_err(server, "its answer is too large"));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| mcp_err(server, "its answer is no UTF-8"))
}

impl Http {
    /// Reads a `text/event-stream` answer until the response with `id` arrives (M3): a `data:` line carries part of
    /// one event, a blank line ends it. An event that does not answer this request — a server's own notification,
    /// on the way to the answer — is read and dropped; a request of the server's own is answered with a `POST` of its
    /// own (T67).
    async fn read_stream(
        &mut self,
        mut response: reqwest::Response,
        id: u64,
        asks: &Asks,
    ) -> Result<Value, HarnessError> {
        let mut pending: Vec<u8> = Vec::new();
        let mut event: Vec<String> = Vec::new();
        let mut total = 0usize;
        loop {
            let chunk = response.chunk().await.map_err(|err| {
                mcp_err(
                    &self.server,
                    format!("reading its stream failed: {}", err.without_url()),
                )
            })?;
            let Some(chunk) = chunk else {
                // The stream ended: an event without its blank line still counts, one last time.
                let answer = self.take_event(&event, id, asks).await?;
                return answer
                    .ok_or_else(|| mcp_err(&self.server, "it ended its stream before answering"));
            };
            total += chunk.len();
            if total > MAX_ANSWER_BYTES {
                return Err(mcp_err(&self.server, "its answer is too large"));
            }
            pending.extend_from_slice(&chunk);
            while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line: Vec<u8> = pending.drain(..=newline).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if line.is_empty() {
                    let answer = self.take_event(&event, id, asks).await?;
                    event.clear();
                    if let Some(answer) = answer {
                        return Ok(answer);
                    }
                } else if let Some(data) = data_of(&line) {
                    event.push(data);
                }
                // `event:`, `id:`, `retry:` and comments (`: …`) say nothing this waits for; HS5 reads the answer
                // only.
            }
        }
    }

    /// One finished event as the answer to `id`, or [`None`] when it is not that answer — a request of the
    /// server's own answered on the way.
    async fn take_event(
        &mut self,
        event: &[String],
        id: u64,
        asks: &Asks,
    ) -> Result<Option<Value>, HarnessError> {
        if event.is_empty() {
            return Ok(None);
        }
        let Ok(value) = serde_json::from_str::<Value>(&event.join("\n")) else {
            // Not JSON-RPC: some servers send a keep-alive as data. It is not the answer either.
            return Ok(None);
        };
        if value.get("method").is_some() {
            let question = self.owed.asking(&value);
            let reply = asks.answer(&self.server, &value).await;
            if question {
                self.owed.questions.pop();
            }
            if let Some(reply) = reply {
                let posted = self.exchange(&reply).await?;
                read_body(&self.server, posted).await?;
            }
            return Ok(None);
        }
        result_for(&self.server, &value, id)
    }
}

/// The payload of one SSE line, [`None`] for every line that carries no `data:`.
fn data_of(line: &[u8]) -> Option<String> {
    let line = std::str::from_utf8(line).ok()?;
    let data = line.strip_prefix("data:")?;
    Some(data.strip_prefix(' ').unwrap_or(data).to_owned())
}

/// The first [`ERROR_BODY_CHARS`] characters of an error's answer.
fn excerpt(text: &str) -> String {
    text.chars().take(ERROR_BODY_CHARS).collect()
}

/// One tool a server advertised (M3, M5).
#[derive(Debug, Clone)]
pub struct RemoteTool {
    /// Its own name on the server — what `tools/call` names, and what a rule is written with.
    pub name: String,
    pub description: Option<String>,
    /// The server's own `inputSchema`, handed to the model as the call's parameters.
    pub input_schema: Value,
    /// The server's `annotations.readOnlyHint`: the tool only reads, so an interrupted call may run again (durable
    /// runs, D7). A hint, as the MCP spec says — only what the owner's own servers say, and only for a replay.
    pub read_only: bool,
}

impl RemoteTool {
    /// One entry of a `tools/list` answer; an entry without a name is not a tool and is passed by.
    fn read(tool: &Value) -> Option<Self> {
        Some(RemoteTool {
            name: tool.get("name").and_then(Value::as_str)?.to_owned(),
            description: tool
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_owned),
            input_schema: tool
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
            read_only: tool.pointer("/annotations/readOnlyHint") == Some(&Value::Bool(true)),
        })
    }

    /// A tool as `tools/list` would give it, for the tests that only need one to offer.
    #[cfg(test)]
    pub(crate) fn for_test(name: &str) -> Self {
        RemoteTool {
            name: name.to_owned(),
            description: Some(format!("the tool {name}")),
            input_schema: json!({"type": "object"}),
            read_only: false,
        }
    }
}

/// A connected MCP server: handshake done, tools known, ready to be called.
pub struct Client {
    server: String,
    transport: Box<dyn Transport>,
    next_id: u64,
    tools: Vec<RemoteTool>,
    /// How long a handshake or a tool list may take, and how long one call may (M3).
    start: Duration,
    call: Duration,
}

impl Client {
    /// Connects to `config`'s server: `initialize`, the `notifications/initialized` that follows it, then
    /// `tools/list` — after which the server's tools are [`Self::tools`] and one of them can be
    /// [`Self::call`]ed.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] when the server cannot be started or does not speak the protocol,
    /// [`HarnessError::McpTimeout`] when it says nothing within [`START_TIMEOUT`]. Either way that server is
    /// left out; the run goes on without it.
    pub async fn connect(server: &str, config: &ServerConfig) -> Result<Client, HarnessError> {
        Client::connect_signed(server, config, &Keys::none()).await
    }

    /// [`Self::connect`] with what `keys` holds for the server (T56): its kept sign-in, refreshed when spent.
    ///
    /// # Errors
    ///
    /// As [`Self::connect`]; [`HarnessError::McpSignIn`] when the server wants a sign-in Glut does not hold.
    pub async fn connect_signed(
        server: &str,
        config: &ServerConfig,
        keys: &Keys,
    ) -> Result<Client, HarnessError> {
        Client::connect_within(server, config, keys, START_TIMEOUT, CALL_TIMEOUT).await
    }

    /// [`Self::connect`] with timeouts of the caller's choosing: the tests need a handshake that hangs within
    /// a second rather than within thirty.
    pub(crate) async fn connect_within(
        server: &str,
        config: &ServerConfig,
        keys: &Keys,
        start: Duration,
        call: Duration,
    ) -> Result<Client, HarnessError> {
        let transport: Box<dyn Transport> = match config {
            ServerConfig::Stdio { .. } => Box::new(Stdio::spawn(server, config)?),
            ServerConfig::Http { .. } => Box::new(Http::new(server, config, keys)?),
        };
        let mut client = Client {
            server: server.to_owned(),
            transport,
            next_id: 1,
            tools: Vec::new(),
            start,
            call,
        };
        let initialize = json!({
            "protocolVersion": PROTOCOL_VERSION,
            // A form and a page to open (T67): only the owner answers either, through the gate of the call.
            "capabilities": {"elicitation": {"form": {}, "url": {}}},
            "clientInfo": {"name": "glut", "version": env!("CARGO_PKG_VERSION")},
        });
        let start = client.start;
        client
            .request("initialize", initialize, start, None)
            .await?;
        client.notify("notifications/initialized").await?;
        let tools = client.list().await?;
        client.tools = tools;
        Ok(client)
    }

    /// A client connected to nothing, for the tests that only need one to hang a tool off.
    #[cfg(test)]
    pub(crate) fn for_test() -> Client {
        /// A transport that answers nothing; these tests never send through it.
        struct Nowhere;
        impl Transport for Nowhere {
            fn send<'a>(
                &'a mut self,
                _request: Request,
                _asks: &'a Asks,
            ) -> TransportFuture<'a, Value> {
                Box::pin(async { Err(mcp_err("nowhere", "not connected")) })
            }
            fn notify(&mut self, _method: String, _params: Value) -> TransportFuture<'_, ()> {
                Box::pin(async { Err(mcp_err("nowhere", "not connected")) })
            }
        }
        Client {
            server: "nowhere".to_owned(),
            transport: Box::new(Nowhere),
            next_id: 1,
            tools: Vec::new(),
            start: START_TIMEOUT,
            call: CALL_TIMEOUT,
        }
    }

    /// Lets one tool call take up to `limit`: a call that waits for the owner on the server's side (an editor's diff,
    /// T64c) takes as long as a question of the gate may.
    pub fn set_call_timeout(&mut self, limit: Duration) {
        self.call = limit;
    }

    /// The tools this server advertised.
    pub fn tools(&self) -> &[RemoteTool] {
        &self.tools
    }

    /// One request on the wire, held to `limit` (M3) — the time the owner takes over a question the server asks on
    /// the way through `gate` not counted (T67).
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        limit: Duration,
        gate: Option<Arc<Gate>>,
    ) -> Result<Value, HarnessError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let request = Request {
            id,
            method: method.to_owned(),
            params,
        };
        let asks = Asks::through(gate);
        let started = Instant::now();
        let mut answer = self.transport.send(request, &asks);
        loop {
            let mut deadline = started + limit + asks.clock.stopped();
            // While the owner answers the deadline moves on with the clock: look again a while later, not at once.
            if asks.clock.is_stopped() {
                deadline = deadline.max(Instant::now() + STOPPED_POLL);
            }
            tokio::select! {
                biased;
                result = &mut answer => return result,
                () = tokio::time::sleep_until(deadline.into()) => {
                    if Instant::now() >= started + limit + asks.clock.stopped() {
                        return Err(HarnessError::McpTimeout {
                            server: self.server.clone(),
                            timeout: limit,
                        });
                    }
                }
            }
        }
    }

    /// One message nobody answers, held to the same limit as the handshake (M3).
    async fn notify(&mut self, method: &str) -> Result<(), HarnessError> {
        let limit = self.start;
        match timeout(limit, self.transport.notify(method.to_owned(), json!({}))).await {
            Ok(answer) => answer,
            Err(_) => Err(HarnessError::McpTimeout {
                server: self.server.clone(),
                timeout: limit,
            }),
        }
    }

    /// Every tool of the server, following `nextCursor` across at most [`MAX_PAGES`] pages until
    /// [`MAX_TOOLS`] of them are in.
    async fn list(&mut self) -> Result<Vec<RemoteTool>, HarnessError> {
        let limit = self.start;
        let mut tools = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut params = json!({});
            if let Some(page) = &cursor {
                params["cursor"] = Value::String(page.clone());
            }
            let answer = self.request("tools/list", params, limit, None).await?;
            let Some(list) = answer.get("tools").and_then(Value::as_array) else {
                return Err(mcp_err(
                    &self.server,
                    "its tools/list answer holds no tools array",
                ));
            };
            for tool in list.iter().take(MAX_TOOLS.saturating_sub(tools.len())) {
                if let Some(tool) = RemoteTool::read(tool) {
                    tools.push(tool);
                }
            }
            cursor = answer
                .get("nextCursor")
                .and_then(Value::as_str)
                .filter(|cursor| !cursor.is_empty())
                .map(str::to_owned);
            if cursor.is_none() || tools.len() >= MAX_TOOLS {
                break;
            }
        }
        Ok(tools)
    }

    /// Calls one of the server's tools and returns its text (M3): the `text` items joined by newlines, every
    /// other item as `[<type> omitted]` on a line of its own. `isError: true` fails the call with that text,
    /// so the model is told what the tool said and can correct its call.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] for a refused or broken answer, [`HarnessError::McpTimeout`] when none comes
    /// within [`CALL_TIMEOUT`].
    pub async fn call(&mut self, tool: &str, arguments: Value) -> Result<String, HarnessError> {
        self.call_asking(tool, arguments, None).await
    }

    /// [`Self::call`] where the server may ask the owner through `gate` while its tool runs (T67): a form, a page to
    /// open. Without a gate every such question is declined.
    ///
    /// # Errors
    ///
    /// As [`Self::call`]; the owner's time over a question does not count towards [`CALL_TIMEOUT`].
    pub async fn call_asking(
        &mut self,
        tool: &str,
        arguments: Value,
        gate: Option<Arc<Gate>>,
    ) -> Result<String, HarnessError> {
        let limit = self.call;
        let result = self
            .request(
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
                limit,
                gate,
            )
            .await?;
        let text = result_text(&result);
        if result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            return Err(mcp_err(
                &self.server,
                if text.is_empty() {
                    "the tool reported an error".to_owned()
                } else {
                    text
                },
            ));
        }
        Ok(text)
    }
}

/// The text a `tools/call` answer holds: its `text` items joined by newlines, every other item a line naming
/// what was left out — an image is not a thing to paste into a model's context as bytes.
fn result_text(result: &Value) -> String {
    let Some(items) = result.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    items
        .iter()
        .map(|item| match item.get("type").and_then(Value::as_str) {
            Some("text") => item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            Some(kind) => format!("[{kind} omitted]"),
            None => "[unknown omitted]".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use super::*;

    /// The environment the mock server reads its mode from, and the file it writes its own process id to.
    const MOCK_MODE: &str = "AXH_MCP_MOCK";
    const MOCK_PID: &str = "AXH_MCP_MOCK_PID";
    /// The test this very binary runs when it is a mock server.
    const MOCK_TEST: &str = "mcp::client::tests::mock_server_self_entry";

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glut-mcp-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Durable runs (D7): the server's `readOnlyHint` is read, and only a plain `true` counts.
    #[test]
    fn a_tool_is_read_only_only_when_its_server_says_so() {
        let tool = |annotations: Value| {
            RemoteTool::read(&json!({"name": "t", "annotations": annotations})).unwrap()
        };
        assert!(tool(json!({"readOnlyHint": true})).read_only);
        assert!(!tool(json!({"readOnlyHint": "true"})).read_only);
        assert!(!tool(json!({"destructiveHint": false})).read_only);
        assert!(!RemoteTool::read(&json!({"name": "t"})).unwrap().read_only);
    }

    #[test]
    fn a_header_never_receives_the_harness_key_even_where_it_is_set() {
        let env = |key: &str| match key {
            "GLUT_API_KEY" => Some("sk-model-key".to_owned()),
            "GITHUB_TOKEN" => Some("gh-token".to_owned()),
            _ => None,
        };
        assert_eq!(
            header_value("Bearer ${GITHUB_TOKEN}", env).unwrap(),
            "Bearer gh-token"
        );
        let refused = header_value("Bearer ${GLUT_API_KEY}", env).unwrap_err();
        assert!(!refused.contains("sk-model-key"), "{refused}");
    }

    #[test]
    fn a_header_never_receives_a_providers_key_of_the_catalog_or_the_owners_own() {
        let env = |key: &str| match key {
            "OPENROUTER_API_KEY" => Some("sk-or".to_owned()),
            "MY_PROXY_KEY" => Some("sk-proxy".to_owned()),
            "BRAVE_API_KEY" => Some("brave".to_owned()),
            _ => None,
        };
        let configured = ["MY_PROXY_KEY".to_owned()];
        for header in ["Bearer ${OPENROUTER_API_KEY}", "Bearer ${MY_PROXY_KEY}"] {
            let refused = header_value_without(header, &configured, env).unwrap_err();
            assert!(!refused.contains("sk-"), "{refused}");
        }
        assert_eq!(
            header_value_without("${BRAVE_API_KEY}", &configured, env).unwrap(),
            "brave",
            "a server's own key still reaches it"
        );
    }

    #[tokio::test]
    async fn a_line_over_the_limit_is_an_error_and_the_next_one_is_read_whole() {
        // A line of exactly the limit, one a byte over it, then an ordinary one.
        let script = format!(
            "head -c {max} /dev/zero | tr '\\0' x; echo; head -c {over} /dev/zero | tr '\\0' y; echo; echo ok",
            max = MAX_ANSWER_BYTES,
            over = MAX_ANSWER_BYTES + 1,
        );
        let config = ServerConfig::Stdio {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), script],
            env: BTreeMap::new(),
        };
        let mut stdio = Stdio::spawn("long", &config).unwrap();
        let exact = stdio.read_line().await.unwrap();
        assert_eq!(
            exact.trim_end().len(),
            MAX_ANSWER_BYTES,
            "the limit itself fits"
        );
        let err = stdio.read_line().await.unwrap_err().to_string();
        assert!(err.contains("longer than"), "{err}");
        assert_eq!(
            stdio.read_line().await.unwrap(),
            "ok\n",
            "the rest of the long line was read past"
        );
    }

    #[test]
    fn an_error_shows_the_url_without_what_could_be_a_token() {
        assert_eq!(
            shown_url("https://user:pw@mcp.example.com/v1?key=sk-1#frag"),
            "https://mcp.example.com/…"
        );
        assert_eq!(
            shown_url("https://mcp.example.com/mcp/sk-in-the-path"),
            "https://mcp.example.com/…"
        );
        assert_eq!(shown_url("http://127.0.0.1:9/"), "http://127.0.0.1:9");
        assert_eq!(shown_url("not a url ?key=sk-1"), "its URL");
    }

    /// The error of a connect that must not succeed. `unwrap_err` would need the client to be `Debug`, which a
    /// client holding a transport can hardly be.
    fn failure(result: Result<Client, HarnessError>) -> HarnessError {
        match result {
            Err(err) => err,
            Ok(_) => panic!("the server was not supposed to connect"),
        }
    }

    /// The config of a server that is this very test binary in mock mode: spawned as the client would spawn
    /// anything, so the stdio transport is exercised as it really runs.
    fn mock(mode: &str, pid: Option<&Path>) -> ServerConfig {
        let mut env = BTreeMap::new();
        env.insert(MOCK_MODE.to_owned(), mode.to_owned());
        if let Some(pid) = pid {
            env.insert(MOCK_PID.to_owned(), pid.display().to_string());
        }
        ServerConfig::Stdio {
            command: std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            args: vec![
                // The mock runs alone, and --nocapture lets its JSON reach the client's pipe; --quiet drops
                // libtest's "test <name> …" prefix, which would otherwise merge with the first answer line.
                "--exact".into(),
                MOCK_TEST.into(),
                "--test-threads".into(),
                "1".into(),
                "--quiet".into(),
                "--nocapture".into(),
            ],
            env,
        }
    }

    /// Entered by [`mock_server_self_entry`] in the child: the stdio server of these tests.
    fn serve_mock() {
        use std::io::{BufRead, BufWriter, Write as _};
        let mode = std::env::var(MOCK_MODE).unwrap_or_default();
        if mode == "silent" {
            // Never answers a word: the client's own timeout is what ends this (M3).
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
        }
        if let Ok(path) = std::env::var(MOCK_PID) {
            let _ = std::fs::write(path, std::process::id().to_string());
        }
        let stdin = std::io::stdin();
        let stdout = std::io::stdout();
        let mut reader = stdin.lock();
        let mut writer = BufWriter::new(stdout.lock());
        let mut pages = 0usize;
        loop {
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            let Ok(message) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            let id = message.get("id").and_then(Value::as_u64);
            let method = message
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            // Before its own answer: a notification on the way, which the client must read past rather than
            // take for the response it waits for (M3).
            if id.is_some() {
                let notification = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/message",
                    "params": {"level": "info", "data": "a chatty server"},
                });
                let _ = writeln!(writer, "{notification}");
            }
            // A tool that asks the owner first (T67): the question goes out, its answer comes back as the tool's text.
            if method == "tools/call"
                && message.pointer("/params/name").and_then(Value::as_str) == Some("ask")
            {
                let question = json!({
                    "jsonrpc": "2.0", "id": 1, "method": "elicitation/create",
                    "params": {"message": "Who?", "requestedSchema": {
                        "type": "object", "properties": {"name": {"type": "string"}}, "required": ["name"]}},
                });
                let _ = writeln!(writer, "{question}");
                let _ = writer.flush();
                let mut reply = String::new();
                if reader.read_line(&mut reply).is_err() {
                    return;
                }
                let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
                let text = format!("{}|{}", reply["id"], reply["result"]);
                let answer = json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": text}]}});
                let _ = writeln!(writer, "{answer}");
                let _ = writer.flush();
                continue;
            }
            let result = match (id, method.as_str()) {
                (Some(_), "initialize") => Some(json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "mock", "version": "1"},
                })),
                (Some(_), "tools/list") => {
                    pages += 1;
                    Some(if pages == 1 {
                        json!({
                            "tools": [{
                                "name": "alpha",
                                "description": "the first",
                                "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}
                            }],
                            "nextCursor": "page2",
                        })
                    } else {
                        json!({
                            "tools": [{"name": "beta", "inputSchema": {"type": "object"}}],
                        })
                    })
                }
                (Some(_), "tools/call") => {
                    let name = message
                        .pointer("/params/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    Some(match name {
                        "alpha" | "beta" | "echo" => json!({
                            "content": [{
                                "type": "text",
                                "text": message.pointer("/params/arguments/text")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default(),
                            }],
                        }),
                        "boom" => json!({
                            "content": [{"type": "text", "text": "boom called"}],
                            "isError": true,
                        }),
                        "mixed" => json!({
                            "content": [
                                {"type": "text", "text": "one"},
                                {"type": "image", "data": "aW1hZ2U="},
                                {"type": "text", "text": "two"},
                            ],
                        }),
                        other => json!({
                            "content": [{"type": "text", "text": format!("no tool named {other}")}],
                            "isError": true,
                        }),
                    })
                }
                _ => None,
            };
            let (Some(id), Some(result)) = (id, result) else {
                continue;
            };
            let answer = json!({"jsonrpc": "2.0", "id": id, "result": result});
            let _ = writeln!(writer, "{answer}");
            let _ = writer.flush();
        }
    }

    /// This test, run a second time as the mock server (see [`mock`]).
    #[test]
    fn mock_server_self_entry() {
        if std::env::var(MOCK_MODE).is_ok() {
            serve_mock();
        }
    }

    #[tokio::test]
    async fn a_stdio_server_is_handshaken_listed_and_called() {
        let config = mock("serve", None);
        let mut client = Client::connect_within(
            "mock",
            &config,
            &Keys::none(),
            Duration::from_secs(30),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        // Two pages of tools/list, and every notification on the way read past rather than taken for an answer.
        let names: Vec<&str> = client
            .tools()
            .iter()
            .map(|tool| tool.name.as_str())
            .collect();
        assert_eq!(names, ["alpha", "beta"]);
        assert_eq!(
            client.tools()[0].input_schema,
            json!({"type": "object", "properties": {"text": {"type": "string"}}})
        );

        // The answer's text comes back whole.
        let text = client.call("echo", json!({"text": "hello"})).await.unwrap();
        assert_eq!(text, "hello");

        // An `isError` answer fails the call with what the tool said, which is what the model needs to hear.
        let refused = client.call("boom", json!({})).await.unwrap_err();
        assert!(refused.to_string().contains("boom called"), "{refused}");

        // A content item that is no text becomes a line naming what was left out.
        let mixed = client.call("mixed", json!({})).await.unwrap();
        assert_eq!(mixed, "one\n[image omitted]\ntwo");
    }

    /// An asker that fills every form with a name, after `delay`, and remembers what it was asked.
    struct Fills {
        delay: Duration,
        asked: StdMutex<Vec<elicit::Asked>>,
    }

    impl crate::permissions::Asker for Fills {
        fn ask<'a>(
            &'a self,
            _request: &'a crate::permissions::AskRequest,
        ) -> crate::permissions::AnswerFuture<'a> {
            Box::pin(async { crate::permissions::Answer::Deny })
        }

        fn fill<'a>(&'a self, asked: &'a elicit::Asked) -> elicit::ReplyFuture<'a> {
            self.asked.lock().unwrap().push(asked.clone());
            Box::pin(async move {
                tokio::time::sleep(self.delay).await;
                let mut content = serde_json::Map::new();
                content.insert("name".into(), json!("Ada"));
                elicit::Reply::Accept(content)
            })
        }
    }

    fn gate_filling(delay: Duration) -> (Arc<Gate>, Arc<Fills>) {
        let fills = Arc::new(Fills {
            delay,
            asked: StdMutex::new(Vec::new()),
        });
        let gate = Gate::new(
            crate::permissions::RuleSet::default(),
            crate::permissions::Mode::Default,
            Arc::clone(&fills) as Arc<dyn crate::permissions::Asker>,
        );
        (Arc::new(gate), fills)
    }

    #[tokio::test]
    async fn a_servers_question_goes_to_the_owner_and_the_wait_is_not_the_servers() {
        let config = mock("serve", None);
        // A call may take a second; the owner takes longer over the form, which must not end it.
        let mut client = Client::connect_within(
            "mock",
            &config,
            &Keys::none(),
            Duration::from_secs(30),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        let (gate, fills) = gate_filling(Duration::from_millis(1_500));
        let text = client
            .call_asking("ask", json!({}), Some(gate))
            .await
            .unwrap();
        assert_eq!(text, r#"1|{"action":"accept","content":{"name":"Ada"}}"#);
        let asked = fills.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1);
        assert_eq!(asked[0].server, "mock");
        assert!(matches!(&asked[0].what, elicit::What::Form(form) if form.message == "Who?"));

        // Without a gate nobody is asked: the server hears a decline at once.
        let text = client.call("ask", json!({})).await.unwrap();
        assert_eq!(text, r#"1|{"action":"decline"}"#);
    }

    #[tokio::test]
    async fn a_request_asks_the_owner_a_few_times_at_most_and_never_after_a_no() {
        let form = json!({"message": "m", "requestedSchema": {"type": "object", "properties": {}}});
        let (gate, fills) = gate_filling(Duration::ZERO);
        let asks = Asks::through(Some(gate));
        for _ in 0..MAX_QUESTIONS + 2 {
            asks.elicit("mock", &form).await;
        }
        assert_eq!(fills.asked.lock().unwrap().len(), MAX_QUESTIONS);

        struct Declines(StdMutex<usize>);
        impl crate::permissions::Asker for Declines {
            fn ask<'a>(
                &'a self,
                _request: &'a crate::permissions::AskRequest,
            ) -> crate::permissions::AnswerFuture<'a> {
                Box::pin(async { crate::permissions::Answer::Deny })
            }
            fn fill<'a>(&'a self, _asked: &'a elicit::Asked) -> elicit::ReplyFuture<'a> {
                *self.0.lock().unwrap() += 1;
                Box::pin(async { elicit::Reply::Decline })
            }
        }
        let declines = Arc::new(Declines(StdMutex::new(0)));
        let gate = Gate::new(
            crate::permissions::RuleSet::default(),
            crate::permissions::Mode::Default,
            Arc::clone(&declines) as Arc<dyn crate::permissions::Asker>,
        );
        let asks = Asks::through(Some(Arc::new(gate)));
        assert_eq!(
            asks.elicit("mock", &form).await,
            json!({"action": "decline"})
        );
        assert_eq!(
            asks.elicit("mock", &form).await,
            json!({"action": "decline"})
        );
        assert_eq!(
            *declines.0.lock().unwrap(),
            1,
            "the second question was declined unasked"
        );
    }

    #[test]
    fn what_a_dropped_request_left_is_settled_once() {
        let mut owed = Owed {
            open: Some(7),
            questions: vec![json!("q1")],
        };
        assert_eq!(
            owed.settle(),
            vec![
                json!({"jsonrpc": "2.0", "id": "q1", "result": {"action": "cancel"}}),
                json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
                       "params": {"requestId": 7, "reason": "the run was stopped"}}),
            ]
        );
        assert!(owed.settle().is_empty());
    }

    #[tokio::test]
    async fn a_server_that_never_answers_ends_at_the_timeout() {
        let config = mock("silent", None);
        let started = std::time::Instant::now();
        let err = failure(
            Client::connect_within(
                "mock",
                &config,
                &Keys::none(),
                Duration::from_millis(500),
                Duration::from_secs(5),
            )
            .await,
        );
        assert!(
            matches!(err, HarnessError::McpTimeout { .. }),
            "a hanging handshake is a timeout, not a hang: {err}"
        );
        assert!(started.elapsed() < Duration::from_secs(20));
    }

    #[tokio::test]
    async fn the_server_is_gone_once_its_client_is_dropped() {
        let dir = temp("drop");
        let pid_file = dir.join("pid");
        let config = mock("serve", Some(&pid_file));
        let client = Client::connect_within(
            "mock",
            &config,
            &Keys::none(),
            Duration::from_secs(30),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap())
                .is_ok(),
            "the server is running while its client holds it"
        );
        drop(client);
        // Dropping the transport kills its process group (M4); give the reaper a moment to catch up.
        for _ in 0..200 {
            if rustix::process::test_kill_process(rustix::process::Pid::from_raw(pid).unwrap())
                .is_err()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("the server outlived its client");
    }

    // ── the HTTP transport ────────────────────────────────────────────────────────────────────────────

    /// What a fake HTTP server answers one request with.
    struct Answer {
        status: u16,
        content_type: &'static str,
        headers: Vec<(&'static str, String)>,
        body: String,
    }

    impl Answer {
        fn json(value: Value) -> Self {
            Answer {
                status: 200,
                content_type: "application/json",
                headers: Vec::new(),
                body: value.to_string(),
            }
        }

        fn sse(events: &[Value]) -> Self {
            Answer {
                status: 200,
                content_type: "text/event-stream",
                headers: Vec::new(),
                body: events
                    .iter()
                    .map(|event| format!("data: {event}\n\n"))
                    .collect(),
            }
        }
    }

    /// A fake MCP server over HTTP on the loopback, answering each request through `answer` and remembering
    /// what it was asked (the request head, for the headers the test checks).
    async fn fake_http(
        answer: impl Fn(&str, u64) -> Answer + Send + Sync + 'static,
    ) -> (String, Arc<StdMutex<Vec<String>>>) {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/mcp", listener.local_addr().unwrap());
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let seen_in_task = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let answer = &answer;
                let (head, body) = crate::model::fake::read_request(&mut socket).await;
                let value: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
                let method = value
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                seen_in_task.lock().unwrap().push(head);
                let Answer {
                    status,
                    content_type,
                    headers,
                    body,
                } = answer(&method, id);
                let mut response = format!(
                    "HTTP/1.1 {status} {}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n",
                    match status {
                        200 => "OK",
                        202 => "Accepted",
                        _ => "Failed",
                    },
                    body.len(),
                );
                for (name, value) in headers {
                    response.push_str(&format!("{name}: {value}\r\n"));
                }
                response.push_str("\r\n");
                response.push_str(&body);
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (url, seen)
    }

    /// The `tools/list` and `initialize` answers the fake gives.
    fn handshake(method: &str, id: u64) -> Answer {
        match method {
            "initialize" => Answer::json(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "serverInfo": {"name": "fake", "version": "1"}},
            })),
            "tools/list" => Answer::json(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"tools": [{"name": "ping", "description": "answers", "inputSchema": {"type": "object"}}]},
            })),
            "tools/call" => Answer::json(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"content": [{"type": "text", "text": "pong"}]},
            })),
            _ => Answer::json(Value::Null),
        }
    }

    #[tokio::test]
    async fn an_http_server_is_called_over_post_and_its_session_comes_back() {
        let (url, seen) = fake_http(|method, id| {
            let mut answer = handshake(method, id);
            if method == "initialize" {
                answer
                    .headers
                    .push(("mcp-session-id", "session-1".to_owned()));
            }
            answer
        })
        .await;
        let config = ServerConfig::Http {
            url,
            headers: BTreeMap::new(),
            oauth: None,
        };
        let mut client = Client::connect_within(
            "mail",
            &config,
            &Keys::none(),
            Duration::from_secs(10),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(client.tools().len(), 1);
        assert_eq!(client.call("ping", json!({})).await.unwrap(), "pong");

        let heads = seen.lock().unwrap().clone();
        assert_eq!(heads.len(), 4, "initialize, initialized, tools/list, call");
        // The session id the server issued is sent with every message after the one it came with (M3).
        for head in &heads[1..] {
            assert!(
                head.to_ascii_lowercase()
                    .contains("mcp-session-id: session-1"),
                "a session id was not sent back: {head}"
            );
        }
        assert!(heads[0].starts_with("POST /mcp "), "{}", heads[0]);
    }

    #[tokio::test]
    async fn an_sse_answer_is_read_until_the_response_with_its_id() {
        // The first event is the server's own notification: it has no id and must be read past (M3).
        let (url, _seen) = fake_http(|method, id| match method {
            "initialize" => Answer::sse(&[
                json!({"jsonrpc": "2.0", "method": "notifications/message", "params": {"level": "info"}}),
                json!({"jsonrpc": "2.0", "id": id, "result": {"protocolVersion": PROTOCOL_VERSION, "capabilities": {}, "serverInfo": {"name": "fake", "version": "1"}}}),
            ]),
            _ => handshake(method, id),
        })
        .await;
        let config = ServerConfig::Http {
            url,
            headers: BTreeMap::new(),
            oauth: None,
        };
        let client = Client::connect_within(
            "mail",
            &config,
            &Keys::none(),
            Duration::from_secs(10),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        assert_eq!(
            client
                .tools()
                .iter()
                .map(|tool| &tool.name)
                .collect::<Vec<_>>(),
            ["ping"],
            "the stream was read to its answer, not to its first event"
        );
    }

    #[tokio::test]
    async fn a_question_in_a_stream_is_answered_with_a_post_of_its_own() {
        let replied = Arc::new(StdMutex::new(false));
        let replied_in_server = Arc::clone(&replied);
        let (url, _seen) = fake_http(move |method, id| match method {
            "tools/call" => Answer::sse(&[
                json!({"jsonrpc": "2.0", "id": 77, "method": "elicitation/create", "params": {
                    "mode": "url", "message": "Sign in", "url": "https://example.com/in", "elicitationId": "e"}}),
                json!({"jsonrpc": "2.0", "id": id, "result": {"content": [{"type": "text", "text": "done"}]}}),
            ]),
            // The owner's answer: a message without a method, under the server's id.
            "" if id == 77 => {
                *replied_in_server.lock().unwrap() = true;
                Answer {
                    status: 202,
                    content_type: "application/json",
                    headers: Vec::new(),
                    body: String::new(),
                }
            }
            _ => handshake(method, id),
        })
        .await;
        let config = ServerConfig::Http {
            url,
            headers: BTreeMap::new(),
            oauth: None,
        };
        let mut client = Client::connect_within(
            "mail",
            &config,
            &Keys::none(),
            Duration::from_secs(10),
            Duration::from_secs(10),
        )
        .await
        .unwrap();
        let (gate, fills) = gate_filling(Duration::ZERO);
        assert_eq!(
            client
                .call_asking("ping", json!({}), Some(gate))
                .await
                .unwrap(),
            "done"
        );
        assert!(*replied.lock().unwrap(), "the answer was posted back");
        assert!(matches!(
            &fills.asked.lock().unwrap()[0].what,
            elicit::What::Page { url, .. } if url == "https://example.com/in"
        ));
    }

    #[tokio::test]
    async fn a_500_from_a_server_is_an_error_with_the_start_of_its_answer() {
        let (url, _seen) = fake_http(|_method, _id| Answer {
            status: 500,
            content_type: "text/html",
            headers: Vec::new(),
            body: "<html><body>the connector is not running</body></html>".repeat(50),
        })
        .await;
        let config = ServerConfig::Http {
            url,
            headers: BTreeMap::new(),
            oauth: None,
        };
        let err = failure(
            Client::connect_within(
                "mail",
                &config,
                &Keys::none(),
                Duration::from_secs(10),
                Duration::from_secs(10),
            )
            .await,
        );
        let text = err.to_string();
        assert!(text.contains("500"), "{text}");
        assert!(text.contains("the connector is not running"), "{text}");
        assert!(
            text.len() < 2_000,
            "an error keeps only the start of an answer, not a whole page"
        );
    }
}
