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
use std::time::Duration;

use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::timeout;

use super::config::{ServerConfig, expand_vars};
use crate::config::API_KEY_ENV;
use crate::error::HarnessError;

/// The protocol version the client proposes. The server answers with the one it speaks, and nothing here is
/// version-specific, so whichever comes back is taken.
const PROTOCOL_VERSION: &str = "2025-06-18";
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

/// One answer of a transport — boxed so two very different transports still fit behind [`Transport`], the way
/// [`crate::tools::ToolFuture`] does for the tools.
pub type TransportFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, HarnessError>> + Send + 'a>>;

/// How a client reaches its server (M3).
pub trait Transport: Send {
    /// Sends a request and returns the `result` of the response carrying its id. Notifications, answers to
    /// requests the harness no longer waits for and lines that are no JSON are on the way to it and skipped —
    /// a chatty server must not be able to wedge a request.
    fn send(&mut self, request: Request) -> TransportFuture<'_, Value>;
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
    fn send(&mut self, request: Request) -> TransportFuture<'_, Value> {
        Box::pin(async move {
            let id = request.id;
            let message = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": request.method,
                "params": request.params,
            });
            self.write(&message).await?;
            loop {
                let line = self.read_line().await?;
                let Ok(response) = serde_json::from_str::<Value>(&line) else {
                    // No JSON: something the server printed of its own accord. Read past it (M3).
                    continue;
                };
                match result_for(&self.server, &response, id)? {
                    Some(result) => return Ok(result),
                    None => continue,
                }
            }
        })
    }

    fn notify(&mut self, method: String, params: Value) -> TransportFuture<'_, ()> {
        Box::pin(async move {
            self.write(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
                .await
        })
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
}

impl Http {
    /// The HTTP transport for one server.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Mcp`] when the config is not an HTTP one, a header cannot be resolved or built, or the
    /// client itself cannot be made — that server fails to start, the run goes on without it.
    pub fn new(server: &str, config: &ServerConfig) -> Result<Self, HarnessError> {
        let ServerConfig::Http { url, headers } = config else {
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
        let client = reqwest::Client::builder()
            .timeout(CALL_TIMEOUT)
            // A redirect would carry the headers to a host the owner never named.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| mcp_err(server, err.to_string()))?;
        Ok(Http {
            server: server.to_owned(),
            url: url.clone(),
            headers: resolved,
            client,
            session: None,
        })
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
        let status = response.status();
        if !status.is_success() {
            let text = read_body(&self.server, response).await?;
            return Err(mcp_err(
                &self.server,
                format!("it answered {status}: {}", excerpt(&text)),
            ));
        }
        Ok(response)
    }
}

/// A header's value with its `${NAME}`s put in from `env` (M3) — except the harness's own model key, which is
/// never a server's to have: a repository's `.mcp.json` asking for `${AXH_API_KEY}` would send it to a host of
/// its choosing.
fn header_value(value: &str, env: impl Fn(&str) -> Option<String>) -> Result<String, String> {
    expand_vars(value, |key| {
        (key != API_KEY_ENV).then(|| env(key)).flatten()
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
    fn send(&mut self, request: Request) -> TransportFuture<'_, Value> {
        Box::pin(async move {
            let id = request.id;
            let body = json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": request.method,
                "params": request.params,
            });
            let response = self.exchange(&body).await?;
            let streaming = is_stream(&response);
            if streaming {
                return read_stream(&self.server, response, id).await;
            }
            let text = read_body(&self.server, response).await?;
            let value: Value = serde_json::from_str(&text)
                .map_err(|_| mcp_err(&self.server, "its answer is no JSON"))?;
            result_for(&self.server, &value, id)?
                .ok_or_else(|| mcp_err(&self.server, "its answer was for another request"))
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

/// Reads a `text/event-stream` answer until the response with `id` arrives (M3): a `data:` line carries part of
/// one event, a blank line ends it. An event that does not answer this request — a server's own notification,
/// on the way to the answer — is read and dropped.
async fn read_stream(
    server: &str,
    mut response: reqwest::Response,
    id: u64,
) -> Result<Value, HarnessError> {
    let mut pending: Vec<u8> = Vec::new();
    let mut event: Vec<String> = Vec::new();
    let mut total = 0usize;
    loop {
        let chunk = response.chunk().await.map_err(|err| {
            mcp_err(
                server,
                format!("reading its stream failed: {}", err.without_url()),
            )
        })?;
        let Some(chunk) = chunk else {
            // The stream ended: an event without its blank line still counts, one last time.
            let answer = event_answer(server, &event, id)?;
            return answer.ok_or_else(|| mcp_err(server, "it ended its stream before answering"));
        };
        total += chunk.len();
        if total > MAX_ANSWER_BYTES {
            return Err(mcp_err(server, "its answer is too large"));
        }
        pending.extend_from_slice(&chunk);
        while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = pending.drain(..=newline).collect();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.is_empty() {
                let answer = event_answer(server, &event, id)?;
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

/// One finished event as the answer to `id`, or [`None`] when it is not that answer.
fn event_answer(server: &str, event: &[String], id: u64) -> Result<Option<Value>, HarnessError> {
    if event.is_empty() {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<Value>(&event.join("\n")) else {
        // Not JSON-RPC: some servers send a keep-alive as data. It is not the answer either.
        return Ok(None);
    };
    result_for(server, &value, id)
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
        Client::connect_within(server, config, START_TIMEOUT, CALL_TIMEOUT).await
    }

    /// [`Self::connect`] with timeouts of the caller's choosing: the tests need a handshake that hangs within
    /// a second rather than within thirty.
    pub(crate) async fn connect_within(
        server: &str,
        config: &ServerConfig,
        start: Duration,
        call: Duration,
    ) -> Result<Client, HarnessError> {
        let transport: Box<dyn Transport> = match config {
            ServerConfig::Stdio { .. } => Box::new(Stdio::spawn(server, config)?),
            ServerConfig::Http { .. } => Box::new(Http::new(server, config)?),
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
            "capabilities": {},
            "clientInfo": {"name": "axh", "version": env!("CARGO_PKG_VERSION")},
        });
        let start = client.start;
        client.request("initialize", initialize, start).await?;
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
            fn send(&mut self, _request: Request) -> TransportFuture<'_, Value> {
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

    /// The tools this server advertised.
    pub fn tools(&self) -> &[RemoteTool] {
        &self.tools
    }

    /// One request on the wire, held to `limit` (M3).
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        limit: Duration,
    ) -> Result<Value, HarnessError> {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let request = Request {
            id,
            method: method.to_owned(),
            params,
        };
        match timeout(limit, self.transport.send(request)).await {
            Ok(answer) => answer,
            Err(_) => Err(HarnessError::McpTimeout {
                server: self.server.clone(),
                timeout: limit,
            }),
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
            let answer = self.request("tools/list", params, limit).await?;
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
        let limit = self.call;
        let result = self
            .request(
                "tools/call",
                json!({"name": tool, "arguments": arguments}),
                limit,
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
            "axh-mcp-{name}-{}-{}",
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
            "AXH_API_KEY" => Some("sk-model-key".to_owned()),
            "GITHUB_TOKEN" => Some("gh-token".to_owned()),
            _ => None,
        };
        assert_eq!(
            header_value("Bearer ${GITHUB_TOKEN}", env).unwrap(),
            "Bearer gh-token"
        );
        let refused = header_value("Bearer ${AXH_API_KEY}", env).unwrap_err();
        assert!(!refused.contains("sk-model-key"), "{refused}");
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

    #[tokio::test]
    async fn a_server_that_never_answers_ends_at_the_timeout() {
        let config = mock("silent", None);
        let started = std::time::Instant::now();
        let err = failure(
            Client::connect_within(
                "mock",
                &config,
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
        };
        let mut client = Client::connect_within(
            "mail",
            &config,
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
        };
        let client = Client::connect_within(
            "mail",
            &config,
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
        };
        let err = failure(
            Client::connect_within(
                "mail",
                &config,
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
