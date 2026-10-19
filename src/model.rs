//! The model: one call is the conversation so far plus the tools on offer, the answer is either text or tool calls.
//!
//! [`ChatModel`] is what the loop needs; [`OpenAiCompat`] speaks the OpenAI-compatible chat API that OpenRouter,
//! Ollama and most others offer (H4). A second client (Anthropic's own API) can stand behind the same trait later.

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::ModelEndpoint;
use crate::error::HarnessError;

/// How much of an error answer is kept for the message: enough to read what the API said, not a whole HTML page.
const ERROR_BODY_CHARS: usize = 500;

/// How long one model call may take before it counts as a failed connection. The run's own timeout is the real
/// limit; this only keeps a hung connection from eating all of it silently.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// The largest answer read from the API. A chat answer is kilobytes; a misconfigured or hostile endpoint streaming
/// gigabytes must not fill the memory.
const MAX_ANSWER_BYTES: usize = 16 * 1024 * 1024;

/// How long a streamed answer may stay silent — no byte, not even a provider's keep-alive comment — before the call
/// counts as stalled and fails with that reason (glut-round CP5): a hung provider is noticed at once, not at the run's
/// own limit.
const STALL_LIMIT: Duration = Duration::from_secs(120);

/// Hosts a key may be sent to over plain HTTP: the machine itself (a local Ollama). Anywhere else the key would cross
/// the network in clear text.
const LOOPBACK_HOSTS: [&str; 3] = ["localhost", "127.0.0.1", "[::1]"];

/// Who said a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// One message of the conversation, in the API's own shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// The text; `None` for an assistant message that only calls tools.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "text_content"
    )]
    pub content: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "null_as_empty"
    )]
    pub tool_calls: Vec<ToolCall>,
    /// For a tool's answer: the call it answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// A message's text as the APIs send it: a string, `null`, or a list of parts of which the `text` ones count (some
/// providers answer in parts even without images).
fn text_content<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => Some(text),
        Value::Array(parts) => {
            let text: String = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect();
            Some(text)
        }
        _ => None,
    })
}

/// `"tool_calls": null`, which several OpenAI-compatible backends send on a plain answer, as no calls.
fn null_as_empty<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<ToolCall>, D::Error> {
    Ok(Option::<Vec<ToolCall>>::deserialize(deserializer)?.unwrap_or_default())
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self::text(Role::System, text)
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }

    /// A tool's answer to the call `id`.
    pub fn tool(id: impl Into<String>, text: impl Into<String>) -> Self {
        Message {
            tool_call_id: Some(id.into()),
            ..Self::text(Role::Tool, text)
        }
    }

    fn text(role: Role, text: impl Into<String>) -> Self {
        Message {
            role,
            content: Some(text.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }
}

/// A tool call the model asks for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_kind")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_kind() -> String {
    "function".into()
}

/// The called tool and its arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// A JSON object as text, as the API sends it; the tool parses it.
    #[serde(default)]
    pub arguments: String,
}

/// A tool as the model is told about it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// A JSON schema of the arguments object.
    pub parameters: Value,
}

impl ToolSpec {
    fn to_api(&self) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

/// Tokens one call used, as the API reports them (zero when it does not).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Usage {
    /// Every prompt token the call processed, the cached ones included.
    pub input_tokens: u64,
    /// The part of [`Self::input_tokens`] the provider read from its prompt cache — billed at a lower price
    /// (glut-round CP4).
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
        self.cached_input_tokens += other.cached_input_tokens;
        self.output_tokens += other.output_tokens;
    }
}

/// The model's answer to one call.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The assistant message, as it goes back into the conversation.
    pub message: Message,
    pub usage: Usage,
}

/// What the loop needs of a model.
pub trait ChatModel: Send + Sync {
    /// One call: the conversation so far and the tools on offer.
    fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
    ) -> impl Future<Output = Result<Reply, HarnessError>> + Send;

    /// [`Self::complete`], with each piece of the answer's text handed to `on_text` as it arrives (glut-round CP5). A
    /// model that does not stream hands nothing over and answers as `complete` does.
    fn complete_streaming(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> impl Future<Output = Result<Reply, HarnessError>> + Send {
        let _ = on_text;
        self.complete(messages, tools, max_output_tokens)
    }

    /// The model's name, for the events.
    fn name(&self) -> &str;
}

/// A model behind an OpenAI-compatible chat API.
#[derive(Debug, Clone)]
pub struct OpenAiCompat {
    endpoint: ModelEndpoint,
    client: reqwest::Client,
    /// [`STALL_LIMIT`]; shorter in the tests.
    stall: Duration,
}

impl OpenAiCompat {
    /// A client for `endpoint`.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Transport`] if the HTTP client cannot be built.
    /// [`HarnessError::Transport`] if the HTTP client cannot be built; [`HarnessError::Protocol`] for a key that would
    /// go over plain HTTP to another machine.
    pub fn new(endpoint: ModelEndpoint) -> Result<Self, HarnessError> {
        if endpoint.api_key.is_some() && !secure_enough(&endpoint.base_url) {
            return Err(HarnessError::Protocol(
                "a key is only sent over https, or over http to this machine".into(),
            ));
        }
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            // An API endpoint has no reason to redirect; following one would send the request somewhere unchecked.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| HarnessError::Transport(err.to_string()))?;
        Ok(OpenAiCompat {
            endpoint,
            client,
            stall: STALL_LIMIT,
        })
    }

    fn url(&self) -> String {
        format!(
            "{}/v1/chat/completions",
            self.endpoint.base_url.trim_end_matches('/')
        )
    }

    fn body(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
    ) -> Value {
        // Streamed (CP5): the answer arrives in pieces, so a stalled provider is seen at once; the usage comes in the
        // last piece. A provider that ignores `stream` answers in one JSON body, which is read as before.
        let mut body = json!({
            "model": self.endpoint.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if !tools.is_empty() {
            body["tools"] = Value::Array(tools.iter().map(ToolSpec::to_api).collect());
        }
        if let Some(max) = max_output_tokens {
            body["max_tokens"] = json!(max);
        }
        body
    }
}

impl ChatModel for OpenAiCompat {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
    ) -> Result<Reply, HarnessError> {
        self.complete_streaming(messages, tools, max_output_tokens, &mut |_| {})
            .await
    }

    async fn complete_streaming(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<Reply, HarnessError> {
        let mut request = self
            .client
            .post(self.url())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(self.body(messages, tools, max_output_tokens).to_string());
        if let Some(key) = &self.endpoint.api_key {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .map_err(|err| HarnessError::Transport(err.without_url().to_string()))?;
        let status = response.status();
        let streamed = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("text/event-stream"));
        let reply = if status.is_success() && streamed {
            read_stream(response, self.stall, on_text).await
        } else if status.is_success() {
            // A provider that streams under another type (`text/plain`) is still read as a stream.
            let text = read_capped(response).await?;
            if reads_as_stream(&text) {
                let mut reader = StreamReader::default();
                if !reader.feed(text.as_bytes(), on_text)? {
                    reader.end(on_text)?;
                }
                reader.stream.finish()
            } else {
                parse_reply(&text)
            }
        } else {
            let text = read_capped(response).await?;
            // Some APIs repeat the offending key in their error ("Incorrect API key provided: sk-…").
            let body: String = text.chars().take(ERROR_BODY_CHARS).collect();
            return Err(HarnessError::Http {
                status: status.as_u16(),
                body: redact(&body, self.endpoint.api_key.as_deref()),
            });
        };
        reply.map_err(|err| match err {
            HarnessError::Protocol(message) => {
                HarnessError::Protocol(redact(&message, self.endpoint.api_key.as_deref()))
            }
            other => other,
        })
    }

    fn name(&self) -> &str {
        &self.endpoint.model
    }
}

/// Whether a key may go to `base_url`: over https, or over http to the machine itself.
///
/// `pub(crate)` for the MCP client's HTTP transport (HS5, M3): an MCP URL carries headers with credentials, so it
/// is held to the same rule as a key.
pub(crate) fn secure_enough(base_url: &str) -> bool {
    if base_url.starts_with("https://") {
        return true;
    }
    let Some(rest) = base_url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = match authority.rsplit_once(':') {
        // `[::1]:11434` splits at the port's colon; a bare `[::1]` has none after its bracket.
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    };
    LOOPBACK_HOSTS.contains(&host)
}

/// The response's body, read in chunks up to [`MAX_ANSWER_BYTES`].
async fn read_capped(mut response: reqwest::Response) -> Result<String, HarnessError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_ANSWER_BYTES as u64)
    {
        return Err(HarnessError::Protocol("the answer is too large".into()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|err| HarnessError::Transport(err.without_url().to_string()))?
    {
        if body.len() + chunk.len() > MAX_ANSWER_BYTES {
            return Err(HarnessError::Protocol("the answer is too large".into()));
        }
        body.extend_from_slice(&chunk);
    }
    String::from_utf8(body).map_err(|_| HarnessError::Protocol("the answer is not UTF-8".into()))
}

/// `text` with the key, and any longer stretch of its start, replaced — so an API that echoes it does not put it into
/// an event or a log.
fn redact(text: &str, key: Option<&str>) -> String {
    /// Enough of a key's start to recognise it in an error, too short to be the key.
    const KEY_PREFIX_CHARS: usize = 8;
    let Some(key) = key.filter(|key| key.chars().count() >= KEY_PREFIX_CHARS) else {
        return text.to_owned();
    };
    let prefix: String = key.chars().take(KEY_PREFIX_CHARS).collect();
    text.replace(key, "<redacted>")
        .replace(&prefix, "<redacted>")
}

/// Reads a chat-completions answer: the first choice's message and the usage.
fn parse_reply(text: &str) -> Result<Reply, HarnessError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|err| HarnessError::Protocol(format!("not JSON: {err}")))?;
    // Some APIs answer an error with status 200 and an `error` object.
    if let Some(message) = api_error(&value) {
        return Err(HarnessError::Protocol(format!(
            "the API reported an error: {message}"
        )));
    }
    let message = value
        .pointer("/choices/0/message")
        .ok_or_else(|| HarnessError::Protocol("no choices[0].message".into()))?;
    let mut message: Message = serde_json::from_value(message.clone())
        .map_err(|err| HarnessError::Protocol(format!("malformed message: {err}")))?;
    // The loop puts the message back into the conversation as the assistant's, whatever role the API wrote.
    message.role = Role::Assistant;
    // An empty text next to tool calls is no text.
    if message
        .content
        .as_deref()
        .is_some_and(|text| text.trim().is_empty())
    {
        message.content = None;
    }
    Ok(Reply {
        message,
        usage: usage_of(&value),
    })
}

/// The `usage` of an answer or of a stream's last piece (zero where it says nothing).
fn usage_of(value: &Value) -> Usage {
    let tokens = |key: &str| {
        value
            .pointer(&format!("/usage/{key}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    let input_tokens = tokens("prompt_tokens");
    Usage {
        input_tokens,
        // OpenAI's and OpenRouter's field, then DeepSeek's own; never more than the prompt itself.
        cached_input_tokens: tokens("prompt_tokens_details/cached_tokens")
            .max(tokens("prompt_cache_hit_tokens"))
            .min(input_tokens),
        output_tokens: tokens("completion_tokens"),
    }
}

/// A streamed answer (server-sent events of chat-completion chunks) read into one [`Reply`], each piece of text handed
/// to `on_text` as it comes. Fails when the stream stays silent for `stall`, when it sends only keep-alive comments for
/// [`PIECE_LIMIT`], when a piece carries an error (a provider that gives up mid-answer, DeepInfra's 400 over its context
/// length), past [`MAX_ANSWER_BYTES`], or when it ends before its end mark or a `finish_reason`.
async fn read_stream(
    mut response: reqwest::Response,
    stall: Duration,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<Reply, HarnessError> {
    let mut reader = StreamReader::default();
    let mut total = 0usize;
    let mut last_piece = std::time::Instant::now();
    loop {
        let chunk = match tokio::time::timeout(stall, response.chunk()).await {
            Err(_) => {
                return Err(HarnessError::Transport(format!(
                    "the model sent nothing for {} s; the call was given up",
                    stall.as_secs()
                )));
            }
            Ok(Err(err)) if err.is_timeout() => {
                return Err(HarnessError::Transport(format!(
                    "the call ran past its {} min limit",
                    REQUEST_TIMEOUT.as_secs() / 60
                )));
            }
            Ok(Err(err)) => return Err(HarnessError::Transport(err.without_url().to_string())),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(chunk))) => chunk,
        };
        total += chunk.len();
        if total > MAX_ANSWER_BYTES {
            return Err(HarnessError::Protocol("the answer is too large".into()));
        }
        let pieces_before = reader.stream.pieces;
        if reader.feed(&chunk, on_text)? {
            return reader.stream.finish();
        }
        // A provider that keeps the line open with comments while its upstream hangs (OpenRouter's `: PROCESSING`)
        // passes the stall limit; a stream of nothing but comments is given up here.
        if reader.stream.pieces > pieces_before {
            last_piece = std::time::Instant::now();
        } else if last_piece.elapsed() > PIECE_LIMIT {
            return Err(HarnessError::Transport(format!(
                "the model sent only keep-alives for {} s; the call was given up",
                PIECE_LIMIT.as_secs()
            )));
        }
    }
    reader.end(on_text)?;
    reader.stream.finish()
}

/// How long a stream may send only keep-alive comments, no piece of an answer, before it counts as stalled.
const PIECE_LIMIT: Duration = Duration::from_secs(300);

/// A stream's bytes cut into whole lines — a line ends at `\n`, so neither a line nor a character split between two
/// pieces of the response is ever cut — and handed to the [`Stream`].
#[derive(Default)]
struct StreamReader {
    pending: Vec<u8>,
    stream: Stream,
}

impl StreamReader {
    /// Takes the next bytes; `true` once the stream's end mark has been read.
    fn feed(
        &mut self,
        bytes: &[u8],
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<bool, HarnessError> {
        self.pending.extend_from_slice(bytes);
        while let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            if self.stream.line(&line, on_text)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The last line, when the stream ended without a line break after it.
    fn end(&mut self, on_text: &mut (dyn FnMut(&str) + Send)) -> Result<(), HarnessError> {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.stream.line(&line, on_text)?;
        }
        Ok(())
    }
}

/// One tool call as its pieces arrive.
#[derive(Default)]
struct CallSlot {
    /// The `index` the provider gave it, if any.
    wire: Option<u64>,
    id: String,
    name: String,
    arguments: String,
}

/// What a stream has said so far.
#[derive(Default)]
struct Stream {
    text: String,
    /// The tool calls in the order they began.
    calls: Vec<CallSlot>,
    usage: Usage,
    /// Whether any piece held a choice: a stream without one has no answer.
    answered: bool,
    /// Whether the stream said it was done — its end mark or a `finish_reason`; one that just stops was cut off.
    ended: bool,
    /// How many `data:` pieces it sent, comments not counted.
    pieces: u64,
}

impl Stream {
    /// Reads one line of the stream; `true` at its end mark (`data: [DONE]`).
    fn line(
        &mut self,
        line: &[u8],
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<bool, HarnessError> {
        let line = std::str::from_utf8(line)
            .map_err(|_| HarnessError::Protocol("the answer is not UTF-8".into()))?
            .trim_start_matches('\u{feff}')
            .trim_end_matches(['\r', '\n']);
        // Comments (`: OPENROUTER PROCESSING`), event names and blank lines carry nothing to read.
        let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
            return Ok(false);
        };
        self.pieces += 1;
        if data == "[DONE]" {
            self.ended = true;
            return Ok(true);
        }
        let piece: Value = serde_json::from_str(data).map_err(|err| {
            HarnessError::Protocol(format!("a piece of the answer is not JSON: {err}"))
        })?;
        if let Some(message) = api_error(&piece) {
            return Err(HarnessError::Protocol(format!(
                "the API reported an error: {message}"
            )));
        }
        // Some providers send a usage in every piece, some an empty one at the end: only a real count replaces one.
        let usage = usage_of(&piece);
        if usage.input_tokens + usage.output_tokens > 0 {
            self.usage = usage;
        }
        let Some(choice) = piece.pointer("/choices/0") else {
            return Ok(false);
        };
        if choice
            .get("finish_reason")
            .is_some_and(|reason| !reason.is_null())
        {
            self.ended = true;
        }
        let Some(delta) = choice.get("delta") else {
            return Ok(false);
        };
        self.answered = true;
        if let Some(text) = delta.get("content").and_then(Value::as_str)
            && !text.is_empty()
        {
            self.text.push_str(text);
            on_text(text);
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            self.call_piece(call);
        }
        Ok(false)
    }

    /// One piece of a tool call, put into the slot it belongs to: by its `index`, and a new slot when it brings an id
    /// other than the one its slot already has (a provider that numbers every call `0`, or none at all).
    fn call_piece(&mut self, call: &Value) {
        let wire = call.get("index").and_then(Value::as_u64);
        let id = call
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty());
        let found = match wire {
            Some(wire) => self.calls.iter().rposition(|slot| slot.wire == Some(wire)),
            None => self.calls.len().checked_sub(1),
        };
        let slot = match found {
            Some(at)
                if id.is_none_or(|id| self.calls[at].id.is_empty() || self.calls[at].id == id) =>
            {
                at
            }
            _ => {
                self.calls.push(CallSlot {
                    wire,
                    ..CallSlot::default()
                });
                self.calls.len() - 1
            }
        };
        let slot = &mut self.calls[slot];
        if let Some(id) = id {
            slot.id = id.to_owned();
        }
        // A name comes whole or in parts; one repeated in every piece is not added again.
        if let Some(name) = call.pointer("/function/name").and_then(Value::as_str)
            && slot.name != name
        {
            slot.name.push_str(name);
        }
        if let Some(arguments) = call.pointer("/function/arguments").and_then(Value::as_str) {
            slot.arguments.push_str(arguments);
        }
    }

    /// The answer the stream made, as [`parse_reply`] would have read it from one body.
    fn finish(self) -> Result<Reply, HarnessError> {
        if !self.answered {
            return Err(HarnessError::Protocol(
                "the stream ended without an answer".into(),
            ));
        }
        // Cut off before its end: half an answer, or a tool call's arguments cut in two, is no answer to act on.
        if !self.ended {
            return Err(HarnessError::Protocol(
                "the stream ended before the answer was complete".into(),
            ));
        }
        let tool_calls = self
            .calls
            .into_iter()
            .enumerate()
            .map(|(index, slot)| ToolCall {
                // A provider that sends no id still gets one the tool's answer can name.
                id: if slot.id.is_empty() {
                    format!("call_{index}")
                } else {
                    slot.id
                },
                kind: function_kind(),
                function: FunctionCall {
                    name: slot.name,
                    arguments: slot.arguments,
                },
            })
            .collect();
        Ok(Reply {
            message: Message {
                role: Role::Assistant,
                content: (!self.text.trim().is_empty()).then_some(self.text),
                tool_calls,
                tool_call_id: None,
            },
            usage: self.usage,
        })
    }
}

/// Whether a body is a stream of server-sent events whatever its type says: its first line is a `data:` piece or a
/// comment.
fn reads_as_stream(text: &str) -> bool {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .is_some_and(|line| line.starts_with("data:") || line.starts_with(':'))
}

/// The message of an `error` an answer or a piece carries; `None` without one (`"error": null` is none).
fn api_error(value: &Value) -> Option<String> {
    let error = value.get("error").filter(|error| !error.is_null())?;
    Some(
        error
            .get("message")
            .and_then(Value::as_str)
            .or_else(|| error.as_str())
            .unwrap_or("no message")
            .to_owned(),
    )
}

#[cfg(test)]
pub(crate) mod fake {
    //! A model API on the loopback that answers each request with the next prepared body, and keeps what it was sent.

    use std::sync::{Arc, Mutex};

    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// One prepared answer: status, body and its type; with `then_hang`, the connection stays open and silent after
    /// the body (a stalled stream).
    pub struct Answer {
        pub status: u16,
        pub body: String,
        pub content_type: &'static str,
        pub then_hang: bool,
    }

    impl Answer {
        pub fn ok(body: Value) -> Self {
            Answer {
                status: 200,
                body: body.to_string(),
                content_type: "application/json",
                then_hang: false,
            }
        }

        /// A streamed answer: each piece as one `data:` event, then the end mark.
        pub fn stream(pieces: &[Value]) -> Self {
            let mut body: String = pieces
                .iter()
                .map(|piece| format!(": keep-alive\n\ndata: {piece}\n\n"))
                .collect();
            body.push_str("data: [DONE]\n\n");
            Answer {
                status: 200,
                body,
                content_type: "text/event-stream",
                then_hang: false,
            }
        }

        pub fn error(status: u16, body: impl Into<String>) -> Self {
            Answer {
                status,
                body: body.into(),
                content_type: "application/json",
                then_hang: false,
            }
        }
    }

    pub struct FakeApi {
        pub base_url: String,
        /// Each request as received: its head and its JSON body.
        pub seen: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl FakeApi {
        pub async fn spawn(answers: Vec<Answer>) -> Self {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base_url = format!("http://{}", listener.local_addr().unwrap());
            let seen = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&seen);
            tokio::spawn(async move {
                for answer in answers {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let (head, body) = read_request(&mut socket).await;
                    log.lock()
                        .unwrap()
                        .push((head, serde_json::from_slice(&body).unwrap_or(Value::Null)));
                    // A hanging answer has no length: the client reads on, and waits, until the stall limit.
                    let length = if answer.then_hang {
                        String::new()
                    } else {
                        format!("content-length: {}\r\n", answer.body.len())
                    };
                    let reply = format!(
                        "HTTP/1.1 {} X\r\ncontent-type: {}\r\n{length}connection: close\r\n\r\n{}",
                        answer.status, answer.content_type, answer.body
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                    if answer.then_hang {
                        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                    }
                    let _ = socket.shutdown().await;
                }
            });
            FakeApi { base_url, seen }
        }

        /// The JSON bodies of the requests so far.
        pub fn bodies(&self) -> Vec<Value> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|(_, body)| body.clone())
                .collect()
        }
    }

    /// Reads one request whole: the head up to the blank line, then `content-length` bytes of body. Reused by
    /// the MCP client's own HTTP tests (HS5).
    pub(crate) async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                break end + 4;
            }
            let n = socket.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return (String::from_utf8_lossy(&buffer).into_owned(), Vec::new());
            }
            buffer.extend_from_slice(&chunk[..n]);
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
        let length = head
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())?
            })
            .unwrap_or(0);
        let mut body = buffer[head_end..].to_vec();
        while body.len() < length {
            let n = socket.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..n]);
        }
        (head, body)
    }
}

#[cfg(test)]
mod tests {
    use super::fake::{Answer, FakeApi};
    use super::*;

    fn endpoint(base_url: &str, key: Option<&str>) -> ModelEndpoint {
        ModelEndpoint {
            base_url: base_url.into(),
            api_key: key.map(str::to_owned),
            model: "test-model".into(),
        }
    }

    fn spec() -> ToolSpec {
        ToolSpec {
            name: "read_file".into(),
            description: "Reads a file.".into(),
            parameters: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        }
    }

    /// glut-round CP4: the cached part is read from OpenAI's/OpenRouter's field or DeepSeek's own, and never exceeds
    /// the prompt.
    #[tokio::test]
    async fn cached_tokens_come_from_either_field_and_never_exceed_the_prompt() {
        let answer = |usage: Value| {
            Answer::ok(json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}],
                "usage": usage
            }))
        };
        let api = FakeApi::spawn(vec![
            answer(json!({"prompt_tokens": 100, "completion_tokens": 1, "prompt_cache_hit_tokens": 70})),
            answer(json!({"prompt_tokens": 10, "completion_tokens": 1,
                          "prompt_tokens_details": {"cached_tokens": 50}})),
        ])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, None)).unwrap();
        for want in [70, 10] {
            let reply = model
                .complete(&[Message::user("hi")], &[], Some(10))
                .await
                .unwrap();
            assert_eq!(reply.usage.cached_input_tokens, want);
        }
    }

    #[tokio::test]
    async fn a_call_sends_model_messages_tools_and_key_and_reads_tool_calls_and_usage() {
        let api = FakeApi::spawn(vec![Answer::ok(json!({
            "choices": [{"message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{"id": "c1", "type": "function",
                                "function": {"name": "read_file", "arguments": "{\"path\":\"a.txt\"}"}}]
            }}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3,
                      "prompt_tokens_details": {"cached_tokens": 8}}
        }))])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, Some("sk-test"))).unwrap();
        let reply = model
            .complete(&[Message::user("hi")], &[spec()], Some(100))
            .await
            .unwrap();

        assert_eq!(reply.message.role, Role::Assistant);
        assert_eq!(
            reply.message.content, None,
            "an empty text beside tool calls is no text"
        );
        assert_eq!(reply.message.tool_calls[0].function.name, "read_file");
        assert_eq!(
            reply.usage,
            Usage {
                input_tokens: 12,
                cached_input_tokens: 8,
                output_tokens: 3
            }
        );

        let (head, body) = api.seen.lock().unwrap()[0].clone();
        assert!(head.starts_with("POST /v1/chat/completions "), "{head}");
        assert!(
            head.to_ascii_lowercase()
                .contains("authorization: bearer sk-test"),
            "{head}"
        );
        assert_eq!(body["model"], "test-model");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["tools"][0]["function"]["name"], "read_file");
        assert_eq!(body["max_tokens"], 100);
    }

    #[tokio::test]
    async fn no_key_sends_no_authorization_and_no_tools_sends_no_tools_field() {
        let api = FakeApi::spawn(vec![Answer::ok(json!({
            "choices": [{"message": {"role": "assistant", "content": "done"}}]
        }))])
        .await;
        let model = OpenAiCompat::new(endpoint(&format!("{}/", api.base_url), None)).unwrap();
        let reply = model
            .complete(&[Message::user("hi")], &[], None)
            .await
            .unwrap();
        assert_eq!(reply.message.content.as_deref(), Some("done"));
        assert_eq!(reply.usage, Usage::default());
        let (head, body) = api.seen.lock().unwrap()[0].clone();
        assert!(
            !head.to_ascii_lowercase().contains("authorization"),
            "{head}"
        );
        assert!(body.get("tools").is_none() && body.get("max_tokens").is_none());
    }

    #[tokio::test]
    async fn an_error_status_keeps_the_start_of_the_answer() {
        let api = FakeApi::spawn(vec![Answer::error(
            401,
            format!("{{\"error\":\"bad key\"}}{}", "x".repeat(2000)),
        )])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, Some("k"))).unwrap();
        match model.complete(&[Message::user("hi")], &[], None).await {
            Err(HarnessError::Http { status, body }) => {
                assert_eq!(status, 401);
                assert!(body.starts_with("{\"error\":\"bad key\"}"));
                assert_eq!(body.chars().count(), ERROR_BODY_CHARS);
            }
            other => panic!("{other:?}"),
        }
    }

    /// CP5: a streamed answer is read into the same reply — text in pieces, tool calls whose arguments arrive in parts,
    /// the usage of the last piece — and each piece of text is handed over as it comes; the request asks for a stream.
    #[tokio::test]
    async fn a_streamed_answer_is_one_reply_and_its_text_arrives_in_pieces() {
        let api = FakeApi::spawn(vec![Answer::stream(&[
            json!({"choices": [{"delta": {"role": "assistant", "content": "Hel"}}]}),
            json!({"choices": [{"delta": {"content": "lo"}}]}),
            json!({"choices": [{"delta": {"tool_calls": [
                {"index": 0, "id": "c1", "type": "function", "function": {"name": "read_file", "arguments": "{\"pa"}}
            ]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"arguments": "th\":\"a\"}"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 1, "function": {"name": "glob", "arguments": "{}"}}]}}]}),
            json!({"choices": [], "usage": {"prompt_tokens": 20, "completion_tokens": 4,
                                             "prompt_tokens_details": {"cached_tokens": 5}}}),
        ])])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, None)).unwrap();
        let mut pieces = Vec::new();
        let reply = model
            .complete_streaming(&[Message::user("hi")], &[spec()], None, &mut |text| {
                pieces.push(text.to_owned());
            })
            .await
            .unwrap();
        assert_eq!(pieces, ["Hel", "lo"]);
        assert_eq!(reply.message.content.as_deref(), Some("Hello"));
        assert_eq!(reply.message.tool_calls.len(), 2);
        assert_eq!(reply.message.tool_calls[0].id, "c1");
        assert_eq!(
            reply.message.tool_calls[0].function.arguments,
            "{\"path\":\"a\"}"
        );
        assert_eq!(
            reply.message.tool_calls[1].id, "call_1",
            "a call without an id still gets one"
        );
        assert_eq!(
            reply.usage,
            Usage {
                input_tokens: 20,
                cached_input_tokens: 5,
                output_tokens: 4
            }
        );
        let body = &api.bodies()[0];
        assert_eq!(body["stream"], true);
        assert_eq!(body["stream_options"]["include_usage"], true);
    }

    /// The CP5 review: pieces cut anywhere — inside a line, inside a character, with `\r\n`, the last line without a
    /// break — read the same as whole lines.
    #[test]
    fn a_stream_cut_anywhere_reads_the_same() {
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"5 €\"}}]}\r\n\r\n: keep-alive\n\
                    data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"error\":null}\n\
                    data: [DONE]";
        let bytes = body.as_bytes();
        for cut in 1..bytes.len() {
            let mut reader = StreamReader::default();
            let mut pieces = String::new();
            let mut on_text = |text: &str| pieces.push_str(text);
            let done = reader.feed(&bytes[..cut], &mut on_text).unwrap()
                || reader.feed(&bytes[cut..], &mut on_text).unwrap();
            if !done {
                reader.end(&mut on_text).unwrap();
            }
            let reply = reader.stream.finish().unwrap();
            assert_eq!(
                reply.message.content.as_deref(),
                Some("5 €"),
                "cut at {cut}"
            );
            assert_eq!(pieces, "5 €");
        }
    }

    /// The CP5 review: tool calls as providers really send them — every call numbered `0`, no index at all, a name
    /// repeated in each piece, an empty id in the later pieces — come out as separate, whole calls.
    #[test]
    fn tool_calls_are_assembled_whatever_numbering_the_provider_uses() {
        let mut stream = Stream::default();
        let mut quiet = |_: &str| {};
        for piece in [
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "a", "function": {"name": "read_file", "arguments": "{\"p"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "", "function": {"name": "read_file", "arguments": "\":1}"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "b", "function": {"name": "glob", "arguments": "{}"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"id": "c", "function": {"name": "gr", "arguments": "{"}}]}}]}),
            json!({"choices": [{"delta": {"tool_calls": [{"function": {"name": "ep", "arguments": "}"}}]}}]}),
            json!({"choices": [{"delta": {}, "finish_reason": "tool_calls"}], "usage": {}}),
        ] {
            stream
                .line(format!("data: {piece}").as_bytes(), &mut quiet)
                .unwrap();
        }
        let calls = stream.finish().unwrap().message.tool_calls;
        let got: Vec<(&str, &str, &str)> = calls
            .iter()
            .map(|c| {
                (
                    c.id.as_str(),
                    c.function.name.as_str(),
                    c.function.arguments.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("a", "read_file", "{\"p\":1}"),
                ("b", "glob", "{}"),
                ("c", "grep", "{}")
            ]
        );
    }

    /// The CP5 review: a stream that just stops — no end mark, no finish reason — is cut off, not an answer; an empty
    /// usage at the end keeps the real one before it.
    #[test]
    fn a_stream_cut_off_is_no_answer_and_an_empty_usage_keeps_the_count() {
        let mut quiet = |_: &str| {};
        let mut cut = Stream::default();
        cut.line(
            br#"data: {"choices":[{"delta":{"content":"half"}}]}"#,
            &mut quiet,
        )
        .unwrap();
        assert!(
            matches!(cut.finish(), Err(HarnessError::Protocol(text)) if text.contains("before the answer"))
        );

        let mut counted = Stream::default();
        for line in [
            &br#"data: {"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":2}}"#[..],
            br#"data: {"choices":[],"usage":{}}"#,
        ] {
            counted.line(line, &mut quiet).unwrap();
        }
        assert_eq!(counted.finish().unwrap().usage.input_tokens, 9);
    }

    /// The CP5 review: a provider that does not stream still answers in one body, and one that streams under another
    /// type is still read as a stream.
    #[tokio::test]
    async fn a_body_answer_and_a_stream_under_another_type_both_read() {
        let mut plain_stream = Answer::stream(&[
            json!({"choices": [{"delta": {"content": "streamed"}, "finish_reason": "stop"}]}),
        ]);
        plain_stream.content_type = "text/plain";
        let api = FakeApi::spawn(vec![
            Answer::ok(
                json!({"choices": [{"message": {"role": "assistant", "content": "whole"}}]}),
            ),
            plain_stream,
        ])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, None)).unwrap();
        for want in ["whole", "streamed"] {
            let reply = model
                .complete(&[Message::user("hi")], &[], None)
                .await
                .unwrap();
            assert_eq!(reply.message.content.as_deref(), Some(want));
        }
    }

    /// CP5: an error inside the stream fails the call at once, and so does a stream without any answer.
    #[tokio::test]
    async fn an_error_in_the_stream_or_no_answer_at_all_fails_the_call() {
        let api = FakeApi::spawn(vec![
            Answer::stream(&[
                json!({"choices": [{"delta": {"content": "par"}}]}),
                json!({"error": {"message": "context length exceeded (131072)"}, "choices": [{"finish_reason": "error", "delta": {}}]}),
            ]),
            Answer::stream(&[]),
        ])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, None)).unwrap();
        for want in ["context length exceeded", "without an answer"] {
            let err = model
                .complete(&[Message::user("hi")], &[], None)
                .await
                .unwrap_err();
            assert!(
                matches!(&err, HarnessError::Protocol(text) if text.contains(want)),
                "{err:?}"
            );
        }
    }

    /// CP5: a stream that goes silent fails at the stall limit with that reason, not at the run's own limit.
    #[tokio::test]
    async fn a_silent_stream_fails_at_the_stall_limit() {
        let mut stalled =
            Answer::stream(&[json!({"choices": [{"delta": {"content": "thinking"}}]})]);
        stalled.body = stalled.body.replace("data: [DONE]\n\n", "");
        stalled.then_hang = true;
        let api = FakeApi::spawn(vec![stalled]).await;
        let mut model = OpenAiCompat::new(endpoint(&api.base_url, None)).unwrap();
        model.stall = Duration::from_millis(300);
        let started = std::time::Instant::now();
        let err = model
            .complete(&[Message::user("hi")], &[], None)
            .await
            .unwrap_err();
        assert!(
            matches!(&err, HarnessError::Transport(text) if text.contains("sent nothing")),
            "{err:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_key_goes_over_https_or_to_this_machine_only() {
        for url in [
            "https://openrouter.ai/api",
            "http://localhost:11434",
            "http://127.0.0.1:8080/x",
            "http://[::1]:1",
        ] {
            assert!(secure_enough(url), "{url}");
        }
        for url in [
            "http://example.com",
            "http://localhost.evil.com",
            "ftp://x",
            "http://10.0.0.2:11434",
        ] {
            assert!(!secure_enough(url), "{url}");
        }
        assert!(matches!(
            OpenAiCompat::new(endpoint("http://example.com", Some("k"))),
            Err(HarnessError::Protocol(_))
        ));
        assert!(OpenAiCompat::new(endpoint("http://example.com", None)).is_ok());
    }

    #[tokio::test]
    async fn an_echoed_key_is_redacted_from_the_error() {
        let api = FakeApi::spawn(vec![Answer::error(
            401,
            r#"{"error":"Incorrect API key provided: sk-live-0123456789"}"#,
        )])
        .await;
        let model = OpenAiCompat::new(endpoint(&api.base_url, Some("sk-live-0123456789"))).unwrap();
        let err = model
            .complete(&[Message::user("hi")], &[], None)
            .await
            .unwrap_err();
        let text = err.to_string();
        assert!(
            !text.contains("sk-live-0123456789") && !text.contains("sk-live-"),
            "{text}"
        );
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn a_malformed_answer_or_an_error_object_is_a_protocol_error() {
        for text in ["not json", "{}", r#"{"error":{"message":"overloaded"}}"#] {
            assert!(
                matches!(parse_reply(text), Err(HarnessError::Protocol(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn null_tool_calls_and_text_in_parts_are_read_as_the_apis_mean_them() {
        let reply = parse_reply(
            &json!({"choices": [{"message": {
                "role": "assistant",
                "content": [{"type": "text", "text": "Hel"}, {"type": "text", "text": "lo"}],
                "tool_calls": null
            }}]})
            .to_string(),
        )
        .unwrap();
        assert_eq!(reply.message.content.as_deref(), Some("Hello"));
        assert!(reply.message.tool_calls.is_empty());
        let empty = parse_reply(r#"{"choices":[{"message":{"role":"assistant","content":null}}]}"#)
            .unwrap();
        assert_eq!(empty.message.content, None);
    }

    #[test]
    fn messages_serialise_in_the_apis_shape() {
        let tool = serde_json::to_value(Message::tool("c1", "ok")).unwrap();
        assert_eq!(
            tool,
            json!({"role": "tool", "content": "ok", "tool_call_id": "c1"})
        );
        let user = serde_json::to_value(Message::user("hi")).unwrap();
        assert_eq!(user, json!({"role": "user", "content": "hi"}));
    }
}
