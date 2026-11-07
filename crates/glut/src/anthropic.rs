//! Anthropic's own Messages API (PV3): Claude by API key, streamed, with the prompt
//! cached, adaptive thinking at the run's effort, and the model's thinking blocks handed back unchanged.
//!
//! The conversation Glut keeps is OpenAI-shaped ([`Message`]); this client turns it into Anthropic's form on every call
//! — system prompt apart, content blocks, tool results as `tool_result` blocks of a user turn — and the answer back.
//! Thinking blocks travel in [`Message::reasoning_details`], as the API sent them (their `signature` included): a
//! block changed or cut short is no longer the one the model signed.
//!
//! Glut compacts a long run by summarizing its older steps and keeping the newer ones; on the newest models a
//! thinking block is bound to the history it was made in, so every request of an adaptive-thinking model sends
//! `prefix_mismatch_behavior: "drop_block"` — after a compaction the API drops a block whose history changed instead
//! of refusing the call (models that do not check accept the field and ignore it).
//!
//! The cost of a call is metered from its tokens: tokens written to the cache count at the input price, though
//! Anthropic bills them a quarter more.

use std::time::Duration;

use serde_json::{Value, json};

use crate::config::{Effort, ModelEndpoint, Prices};
use crate::error::HarnessError;
use crate::model::{
    ChatModel, ERROR_BODY_CHARS, FunctionCall, MAX_ANSWER_BYTES, MODEL_LIST_TIMEOUT, Message,
    PIECE_LIMIT, Piece, REQUEST_TIMEOUT, Reply, Role, STALL_LIMIT, ToolCall, ToolSpec, Usage,
    read_capped, redact, secure_enough,
};

/// The API version every request names.
const API_VERSION: &str = "2023-06-01";
/// The beta that lets a request say what happens to a thinking block whose history changed.
const THINKING_BINDING_BETA: &str = "thinking-binding-controls-2026-08-01";
/// The beta of the server-side fallback: a request a model declines on a safety classifier is answered by another.
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// How long an answer may be when the run sets no limit: room for a long answer, as the API's guidance for streamed
/// calls asks, below every current model's own limit.
const DEFAULT_MAX_TOKENS: u32 = 32_000;
/// The models the server-side fallback applies to (Anthropic's guidance: on by default for these).
const FALLBACK_MODELS: [&str; 4] = [
    "claude-fable-5-1",
    "claude-opus-5-5",
    "claude-opus-5",
    "claude-sonnet-5-5",
];

/// Anthropic's prices per million tokens (input, cache read, output), from its model table of 2026-10-06 — its model
/// list carries none. The owner's own `prices` in the settings come first; a model not here has none.
const PRICES: [(&str, f64, f64, f64); 11] = [
    ("claude-fable-5-1", 10.0, 0.25, 50.0),
    ("claude-fable-5", 10.0, 1.0, 50.0),
    ("claude-opus-5-5", 4.0, 0.20, 20.0),
    ("claude-opus-5", 5.0, 0.50, 25.0),
    ("claude-opus-4-8", 5.0, 0.50, 25.0),
    ("claude-opus-4-7", 5.0, 0.50, 25.0),
    ("claude-opus-4-6", 5.0, 0.50, 25.0),
    ("claude-sonnet-5-5", 2.0, 0.20, 10.0),
    ("claude-sonnet-5", 2.0, 0.20, 10.0),
    ("claude-sonnet-4-6", 3.0, 0.30, 15.0),
    ("claude-haiku-5-5", 0.10, 0.01, 0.50),
];

/// What Anthropic charges for `model`, where Glut knows it.
pub fn prices(model: &str) -> Option<Prices> {
    PRICES
        .iter()
        .find(|(id, ..)| *id == model)
        .map(|&(_, input, cached, output)| Prices {
            input_per_m: input,
            cached_input_per_m: Some(cached),
            output_per_m: output,
        })
}

/// How a model thinks, as far as the request has to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Thinking {
    /// Adaptive thinking (Opus and Sonnet from 4.6 on, Haiku from 5, every Fable and Mythos): the model decides how
    /// much, the effort steers it.
    Adaptive,
    /// An older model: no thinking asked for, no effort sent (Haiku 4.5 refuses one).
    Plain,
}

/// The model's family and version from its id (`claude-opus-5-5` → `("opus", 5, 5)`, `claude-sonnet-4-20250514` →
/// `("sonnet", 4, 0)`).
fn generation(model: &str) -> Option<(&str, u32, u32)> {
    let mut parts = model.strip_prefix("claude-")?.split('-');
    let family = parts.next()?;
    let major = parts.next()?.parse().ok()?;
    // A short number is the minor version; a long one is a date.
    let minor = parts
        .next()
        .filter(|part| part.len() <= 2)
        .and_then(|part| part.parse().ok())
        .unwrap_or(0);
    Some((family, major, minor))
}

fn thinking_of(model: &str) -> Thinking {
    match generation(model) {
        Some(("fable" | "mythos", ..)) => Thinking::Adaptive,
        Some(("opus" | "sonnet", major, minor)) if (major, minor) >= (4, 6) => Thinking::Adaptive,
        Some(("haiku", major, _)) if major >= 5 => Thinking::Adaptive,
        _ => Thinking::Plain,
    }
}

/// The effort as Anthropic spells it for `model`: thinking cannot be switched off on the newest models, so "none"
/// and "minimal" are its lowest level; `xhigh` came with Opus 4.7, an older model gets `high`.
fn effort_for(model: &str, effort: Effort) -> &'static str {
    let xhigh = generation(model).is_none_or(|(family, major, minor)| {
        !matches!(family, "opus" | "sonnet") || (major, minor) >= (4, 7)
    });
    match effort {
        Effort::None | Effort::Minimal | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh if xhigh => "xhigh",
        Effort::Xhigh => "high",
        Effort::Max => "max",
    }
}

/// A model behind Anthropic's Messages API.
#[derive(Debug, Clone)]
pub struct AnthropicMessages {
    endpoint: ModelEndpoint,
    client: reqwest::Client,
    /// The cache's one-hour life instead of five minutes (TUI T36e): every write costs twice the input price then.
    /// Shared by a clone: it is the session's choice, not the call's.
    cache_hour: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// [`STALL_LIMIT`]; shorter in the tests.
    stall: Duration,
}

impl AnthropicMessages {
    /// A client for `endpoint` (its root is `https://api.anthropic.com/v1` in the catalog).
    ///
    /// # Errors
    ///
    /// [`HarnessError::Transport`] if the HTTP client cannot be built; [`HarnessError::Protocol`] for a key that would
    /// go over plain HTTP to another machine.
    pub fn new(endpoint: ModelEndpoint) -> Result<Self, HarnessError> {
        if endpoint.api_key.is_some() && !secure_enough(&endpoint.base_url) {
            return Err(HarnessError::Protocol(
                "a key is only sent over https, or over http to this machine".into(),
            ));
        }
        let client = crate::model::identified(reqwest::Client::builder(), &endpoint.base_url)
            .timeout(REQUEST_TIMEOUT)
            // An API endpoint has no reason to redirect; following one would send the key somewhere unchecked.
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| HarnessError::Transport(err.to_string()))?;
        Ok(AnthropicMessages {
            endpoint,
            client,
            cache_hour: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            stall: STALL_LIMIT,
        })
    }

    /// The cache's life from the next call on: an hour, or the five minutes Anthropic gives by default.
    pub fn set_cache_hour(&self, hour: bool) {
        self.cache_hour
            .store(hour, std::sync::atomic::Ordering::Relaxed);
    }

    fn url(&self, path: &str) -> String {
        let base = self.endpoint.base_url.trim_end_matches('/');
        if self.endpoint.versioned {
            format!("{base}/{path}")
        } else {
            format!("{base}/v1/{path}")
        }
    }

    /// A request with the key and the version every call carries.
    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        betas: &[&str],
    ) -> reqwest::RequestBuilder {
        let mut request = self
            .client
            .request(method, self.url(path))
            .header("anthropic-version", API_VERSION);
        if let Some(key) = &self.endpoint.api_key {
            request = request.header("x-api-key", key_header(key));
        }
        if !betas.is_empty() {
            request = request.header("anthropic-beta", betas.join(","));
        }
        request
    }

    /// The request's body, and the betas it needs.
    fn body(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
    ) -> (Value, Vec<&'static str>) {
        let model = self.endpoint.model.as_str();
        let (system, turns) = to_api(messages);
        let mut betas = Vec::new();
        let mut body = json!({
            "model": model,
            "max_tokens": max_output_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            "messages": turns,
            "stream": true,
            // The whole prefix is cached as the conversation grows: each call reads what the last one wrote.
            "cache_control": { "type": "ephemeral" },
        });
        // The hour is Anthropic's own: a gateway that passes the API on may not take it.
        if self.cache_hour.load(std::sync::atomic::Ordering::Relaxed)
            && !crate::model::is_opencode(&self.endpoint.base_url)
        {
            body["cache_control"]["ttl"] = json!("1h");
        }
        if !system.is_empty() {
            body["system"] = Value::Array(system);
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "name": tool.name,
                            "description": tool.description,
                            "input_schema": tool.parameters,
                            // A file's content streams as it is written, not in one burst at the end.
                            "eager_input_streaming": true,
                        })
                    })
                    .collect(),
            );
        }
        // Betas are Anthropic's own: a gateway that passes the Messages API on (OpenCode's) may refuse them.
        let own = !crate::model::is_opencode(&self.endpoint.base_url);
        if thinking_of(model) == Thinking::Adaptive {
            body["thinking"] = json!({
                "type": "adaptive",
                // The thinking is shown as Glut works: a summary of it, never the raw chain.
                "display": "summarized",
            });
            if own {
                body["thinking"]["block_binding"] =
                    json!({ "prefix_mismatch_behavior": "drop_block" });
                betas.push(THINKING_BINDING_BETA);
            }
            if let Some(effort) = self.endpoint.effort {
                body["output_config"] = json!({ "effort": effort_for(model, effort) });
            }
        }
        if own && FALLBACK_MODELS.contains(&model) {
            body["fallbacks"] = json!("default");
            betas.push(FALLBACK_BETA);
        }
        (body, betas)
    }
}

impl ChatModel for AnthropicMessages {
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
        on_text: &mut (dyn FnMut(Piece<'_>) + Send),
    ) -> Result<Reply, HarnessError> {
        let (body, betas) = self.body(messages, tools, max_output_tokens);
        let response = self
            .request(reqwest::Method::POST, "messages", &betas)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|err| HarnessError::Transport(err.without_url().to_string()))?;
        let key = self.endpoint.api_key.as_deref();
        let status = response.status();
        if !status.is_success() {
            let text = read_capped(response).await?;
            let body: String = text.chars().take(ERROR_BODY_CHARS).collect();
            return Err(HarnessError::Http {
                status: status.as_u16(),
                body: redact(&body, key),
            });
        }
        read_stream(response, self.stall, on_text)
            .await
            .map_err(|err| match err {
                HarnessError::Protocol(message) => HarnessError::Protocol(redact(&message, key)),
                other => other,
            })
    }

    /// Anthropic's own way to keep the cache: the same request with `max_tokens: 0`, not streamed — it reads the
    /// cached head (which renews it) and is billed as that read, with no answer at all.
    async fn warm(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<Usage, HarnessError> {
        let (mut body, betas) = self.body(messages, tools, None);
        body["max_tokens"] = json!(0);
        body["stream"] = json!(false);
        let response = self
            .request(reqwest::Method::POST, "messages", &betas)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_string())
            .send()
            .await
            .map_err(|err| HarnessError::Transport(err.without_url().to_string()))?;
        let key = self.endpoint.api_key.as_deref();
        let status = response.status();
        let text = read_capped(response).await?;
        if !status.is_success() {
            let body: String = text.chars().take(ERROR_BODY_CHARS).collect();
            return Err(HarnessError::Http {
                status: status.as_u16(),
                body: redact(&body, key),
            });
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|_| HarnessError::Protocol("the warming call's answer is no JSON".into()))?;
        Ok(warm_usage(value.get("usage").unwrap_or(&Value::Null)))
    }

    fn name(&self) -> &str {
        &self.endpoint.model
    }

    async fn context_window(&self) -> Option<u32> {
        // The id is one path segment, whatever it holds: `../` or `?` in it cannot reach another path of the host.
        let path = format!("models/{}", path_segment(&self.endpoint.model));
        let response = self
            .request(reqwest::Method::GET, &path, &[])
            .timeout(MODEL_LIST_TIMEOUT)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let text = read_capped(response).await.ok()?;
        let entry: Value = serde_json::from_str(&text).ok()?;
        entry
            .get("max_input_tokens")
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok())
            .filter(|tokens| *tokens > 0)
    }
}

/// The conversation in Anthropic's form: the system prompt's blocks, and the turns. The system messages go together
/// at the top (the API has its system prompt apart); a tool's answer is a `tool_result` block of a user turn; turns
/// of the same role in a row are one turn, as the API wants them alternating. An assistant turn keeps its thinking
/// blocks only after the latest prompt of the owner's (E4): it works on that one still, earlier thinking is gone.
fn to_api(messages: &[Message]) -> (Vec<Value>, Vec<Value>) {
    let latest_prompt = messages
        .iter()
        .rposition(|message| message.role == Role::User)
        .unwrap_or(0);
    let mut system = Vec::new();
    let mut turns: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for (at, message) in messages.iter().enumerate() {
        let text = message.content.as_deref().unwrap_or("");
        let (role, blocks) = match message.role {
            Role::System => {
                if !text.trim().is_empty() {
                    system.push(json!({ "type": "text", "text": text }));
                }
                continue;
            }
            // The owner's images before their words, as Anthropic advises (T45).
            Role::User => {
                let mut blocks = image_blocks(&message.images);
                blocks.extend(text_blocks(text));
                ("user", blocks)
            }
            Role::Tool => {
                // An empty result is still an answer the call is owed.
                let said = if text.is_empty() { "(no output)" } else { text };
                let content = if message.images.is_empty() {
                    json!(said)
                } else {
                    let mut content = vec![json!({ "type": "text", "text": said })];
                    content.extend(image_blocks(&message.images));
                    Value::Array(content)
                };
                (
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": message.tool_call_id.as_deref().unwrap_or(""),
                        "content": content,
                    })],
                )
            }
            Role::Assistant => {
                let mut blocks = Vec::new();
                if at > latest_prompt {
                    blocks.extend(message.reasoning_details.iter().filter_map(thinking_block));
                }
                blocks.extend(text_blocks(text));
                for call in &message.tool_calls {
                    let input: Value = serde_json::from_str(&call.function.arguments)
                        .ok()
                        .filter(Value::is_object)
                        .unwrap_or_else(|| json!({}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": call.id,
                        "name": call.function.name,
                        "input": input,
                    }));
                }
                if blocks.is_empty() {
                    // A turn the API cannot be sent empty; one that said nothing still happened.
                    blocks.push(json!({ "type": "text", "text": "(no answer)" }));
                }
                ("assistant", blocks)
            }
        };
        if blocks.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((last, kept)) if *last == role => kept.extend(blocks),
            _ => turns.push((role, blocks)),
        }
    }
    pair_tool_calls(&mut turns);
    let turns = turns
        .into_iter()
        .map(|(role, content)| json!({ "role": role, "content": content }))
        .collect();
    (system, turns)
}

/// Every `tool_use` answered in the user turn right after it, and no answer without its call — the API refuses either
/// (a call a stopped run never answered, a result whose call a compaction took away). A call without an answer gets
/// one saying it did not run; an answer without its call is left out.
fn pair_tool_calls(turns: &mut Vec<(&'static str, Vec<Value>)>) {
    let ids = |blocks: &[Value], kind: &str, field: &str| -> Vec<String> {
        blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some(kind))
            .filter_map(|block| block.get(field).and_then(Value::as_str).map(str::to_owned))
            .collect()
    };
    let mut at = 0;
    while at < turns.len() {
        let called = if turns[at].0 == "assistant" {
            ids(&turns[at].1, "tool_use", "id")
        } else {
            Vec::new()
        };
        if turns[at].0 == "user" {
            // A user turn after anything but a call answers nothing: its stray results go.
            let after_call = at > 0 && turns[at - 1].0 == "assistant";
            let open: Vec<String> = if after_call {
                ids(&turns[at - 1].1, "tool_use", "id")
            } else {
                Vec::new()
            };
            turns[at].1.retain(|block| {
                block.get("type").and_then(Value::as_str) != Some("tool_result")
                    || block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| open.iter().any(|open| open == id))
            });
            if turns[at].1.is_empty() {
                turns[at]
                    .1
                    .push(json!({ "type": "text", "text": "(continued)" }));
            }
        }
        if !called.is_empty() {
            if turns.get(at + 1).is_none_or(|(role, _)| *role != "user") {
                turns.insert(at + 1, ("user", Vec::new()));
            }
            let answered = ids(&turns[at + 1].1, "tool_result", "tool_use_id");
            let missing: Vec<Value> = called
                .iter()
                .filter(|id| !answered.contains(id))
                .map(|id| json!({ "type": "tool_result", "tool_use_id": id, "content": "(not run)", "is_error": true }))
                .collect();
            let user = &mut turns[at + 1].1;
            user.splice(0..0, missing);
            // The answers come first in their turn, as the API wants them.
            user.sort_by_key(|block| {
                block.get("type").and_then(Value::as_str) != Some("tool_result")
            });
            // A turn that holds answers now needs no placeholder.
            user.retain(|block| block.get("text").and_then(Value::as_str) != Some("(continued)"));
        }
        at += 1;
    }
}

/// A kept thinking block as it goes back: only the fields the API made — the text and signature of a `thinking`
/// block, the data of a `redacted_thinking` one — so a block loaded from a changed file carries nothing else into the
/// request.
fn thinking_block(block: &Value) -> Option<Value> {
    let field = |name: &str| block.get(name).and_then(Value::as_str);
    match field("type")? {
        "thinking" => Some(json!({
            "type": "thinking",
            "thinking": field("thinking")?,
            "signature": field("signature")?,
        })),
        "redacted_thinking" => Some(json!({ "type": "redacted_thinking", "data": field("data")? })),
        _ => None,
    }
}

/// The key as a header value marked sensitive, as `bearer_auth` marks its own: never printed by a debug log.
fn key_header(key: &str) -> reqwest::header::HeaderValue {
    let mut value = reqwest::header::HeaderValue::from_str(key)
        .unwrap_or_else(|_| reqwest::header::HeaderValue::from_static(""));
    value.set_sensitive(true);
    value
}

/// `text` percent-encoded as one segment of a URL's path.
fn path_segment(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~'
                if text != "." && text != ".." =>
            {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// A message's images as Anthropic's image blocks, each after its line (`[image #2 · …]`) so the model can name it;
/// one that cannot be read is said in text.
fn image_blocks(images: &[crate::images::Image]) -> Vec<Value> {
    images
        .iter()
        .flat_map(|image| {
            let block = match crate::images::base64_of(image) {
                Ok((media, data)) => json!({
                    "type": "image",
                    "source": { "type": "base64", "media_type": media, "data": data },
                }),
                Err(reason) => json!({ "type": "text", "text": format!("[{reason}]") }),
            };
            [json!({ "type": "text", "text": image.label() }), block]
        })
        .collect()
}

/// `text` as a text block; none for an empty text (the API refuses an empty one).
fn text_blocks(text: &str) -> Vec<Value> {
    if text.trim().is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": "text", "text": text })]
    }
}

/// One content block of an answer as its pieces arrive.
enum Block {
    Text(String),
    Thinking {
        text: String,
        signature: String,
    },
    Redacted(String),
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    /// A block Glut does not use (a fallback's switch mark, a server tool's): read and left out.
    Other,
}

/// What a streamed answer has said so far.
#[derive(Default)]
struct Answer {
    blocks: Vec<(u64, Block)>,
    /// The prompt's tokens as the API counts them apart: not cached, read from the cache, written to it.
    fresh: u64,
    read: u64,
    written: u64,
    output: u64,
    stop_reason: Option<String>,
    refusal: Option<String>,
    /// Whether `message_stop` came: an answer cut off before it is no answer.
    ended: bool,
    /// How many events it sent, pings not counted.
    events: u64,
}

impl Answer {
    fn block(&mut self, index: u64) -> Option<&mut Block> {
        self.blocks
            .iter_mut()
            .rev()
            .find(|(at, _)| *at == index)
            .map(|(_, block)| block)
    }

    /// One event of the stream; `true` at its end.
    fn event(
        &mut self,
        data: &Value,
        on_text: &mut (dyn FnMut(Piece<'_>) + Send),
    ) -> Result<bool, HarnessError> {
        let kind = data.get("type").and_then(Value::as_str).unwrap_or("");
        if kind != "ping" {
            self.events += 1;
        }
        let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
        match kind {
            "message_start" => {
                if let Some(usage) = data.pointer("/message/usage") {
                    self.take_usage(usage);
                }
            }
            "content_block_start" => {
                let block = data.get("content_block").unwrap_or(&Value::Null);
                let field = |name: &str| {
                    block
                        .get(name)
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_owned()
                };
                let started = match block.get("type").and_then(Value::as_str) {
                    Some("text") => Block::Text(field("text")),
                    Some("thinking") => Block::Thinking {
                        text: field("thinking"),
                        signature: field("signature"),
                    },
                    Some("redacted_thinking") => Block::Redacted(field("data")),
                    Some("tool_use") => Block::ToolUse {
                        id: field("id"),
                        name: field("name"),
                        input: String::new(),
                    },
                    _ => Block::Other,
                };
                self.blocks.push((index, started));
            }
            "content_block_delta" => {
                let delta = data.get("delta").unwrap_or(&Value::Null);
                let piece = |name: &str| delta.get(name).and_then(Value::as_str).unwrap_or("");
                match (delta.get("type").and_then(Value::as_str), self.block(index)) {
                    (Some("text_delta"), Some(Block::Text(text))) => {
                        let more = piece("text");
                        text.push_str(more);
                        on_text(Piece::Text(more));
                    }
                    (Some("thinking_delta"), Some(Block::Thinking { text, .. })) => {
                        let more = piece("thinking");
                        text.push_str(more);
                        on_text(Piece::Reasoning(more));
                    }
                    (Some("signature_delta"), Some(Block::Thinking { signature, .. })) => {
                        signature.push_str(piece("signature"));
                    }
                    (Some("input_json_delta"), Some(Block::ToolUse { input, .. })) => {
                        input.push_str(piece("partial_json"));
                    }
                    _ => {}
                }
            }
            "message_delta" => {
                if let Some(reason) = data.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(reason.to_owned());
                }
                if let Some(details) = data
                    .pointer("/delta/stop_details")
                    .filter(|details| !details.is_null())
                {
                    self.refusal = details
                        .get("category")
                        .and_then(Value::as_str)
                        .or_else(|| details.get("explanation").and_then(Value::as_str))
                        .map(str::to_owned);
                }
                if let Some(usage) = data.get("usage") {
                    self.take_usage(usage);
                }
            }
            "message_stop" => {
                self.ended = true;
                return Ok(true);
            }
            "error" => {
                let message = data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("no message");
                return Err(HarnessError::Protocol(format!(
                    "the API reported an error: {message}"
                )));
            }
            _ => {}
        }
        Ok(false)
    }

    /// A usage of `message_start` or `message_delta`: every count it names replaces the one so far (the output grows
    /// to its total, the input is said once). Glut counts every prompt token processed, the cached ones included.
    fn take_usage(&mut self, usage: &Value) {
        let tokens = |name: &str| usage.get(name).and_then(Value::as_u64);
        for (name, kept) in [
            ("input_tokens", &mut self.fresh),
            ("cache_read_input_tokens", &mut self.read),
            ("cache_creation_input_tokens", &mut self.written),
            ("output_tokens", &mut self.output),
        ] {
            if let Some(count) = tokens(name) {
                *kept = count;
            }
        }
    }

    /// The call's usage as Glut counts it: every prompt token processed, the part read from the cache apart.
    fn usage(&self) -> Usage {
        Usage {
            input_tokens: self.fresh + self.read + self.written,
            cached_input_tokens: self.read,
            output_tokens: self.output,
        }
    }

    /// The answer as the assistant's message.
    fn finish(self) -> Result<Reply, HarnessError> {
        if self.stop_reason.as_deref() == Some("refusal") {
            let why = self
                .refusal
                .map(|why| format!(" ({why})"))
                .unwrap_or_default();
            return Err(HarnessError::Protocol(format!(
                "the model declined to answer{why}"
            )));
        }
        // A tool call cut off at the token limit is half a call: run, it would act on arguments the model never
        // finished.
        let cut_call = self
            .blocks
            .iter()
            .any(|(_, block)| matches!(block, Block::ToolUse { input, .. } if !input.trim().is_empty() && serde_json::from_str::<Value>(input).is_err()));
        if self.stop_reason.as_deref() == Some("max_tokens") && cut_call {
            return Err(HarnessError::Protocol(
                "the answer reached its token limit in the middle of a tool call".into(),
            ));
        }
        if !self.ended {
            return Err(HarnessError::Protocol(
                "the stream ended before the answer was complete".into(),
            ));
        }
        let usage = self.usage();
        let mut text = String::new();
        let mut thought = String::new();
        let mut details = Vec::new();
        let mut tool_calls = Vec::new();
        for (_, block) in self.blocks {
            match block {
                Block::Text(more) => text.push_str(&more),
                Block::Thinking {
                    text: more,
                    signature,
                } => {
                    thought.push_str(&more);
                    details.push(
                        json!({ "type": "thinking", "thinking": more, "signature": signature }),
                    );
                }
                Block::Redacted(data) => {
                    details.push(json!({ "type": "redacted_thinking", "data": data }))
                }
                Block::ToolUse { id, name, input } => tool_calls.push(ToolCall {
                    id,
                    kind: "function".into(),
                    function: FunctionCall {
                        name,
                        // A call without arguments streams none.
                        arguments: if input.trim().is_empty() {
                            "{}".into()
                        } else {
                            input
                        },
                    },
                }),
                Block::Other => {}
            }
        }
        let mut message = Message {
            role: Role::Assistant,
            content: (!text.trim().is_empty()).then_some(text),
            tool_calls,
            tool_call_id: None,
            reasoning: (!thought.is_empty()).then_some(thought),
            reasoning_content: None,
            reasoning_details: details,
            images: Vec::new(),
        };
        message.bound_reasoning();
        Ok(Reply {
            message,
            usage,
            served_by: None,
        })
    }
}

/// A streamed answer (server-sent events) read into one [`Reply`]: each line's `data:` is one event, the rest (event
/// names, blank lines) carries nothing more. Fails when the stream stays silent for `stall`, sends only pings for
/// [`PIECE_LIMIT`], grows past [`MAX_ANSWER_BYTES`], carries an error, or ends before `message_stop`.
async fn read_stream(
    mut response: reqwest::Response,
    stall: Duration,
    on_text: &mut (dyn FnMut(Piece<'_>) + Send),
) -> Result<Reply, HarnessError> {
    let mut answer = Answer::default();
    let mut pending: Vec<u8> = Vec::new();
    let mut total = 0usize;
    let mut last_event = std::time::Instant::now();
    loop {
        let chunk = match tokio::time::timeout(stall, response.chunk()).await {
            Err(_) => {
                return Err(HarnessError::Transport(format!(
                    "{}{} s; the call was given up",
                    crate::model::STALLED,
                    stall.as_secs()
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
        pending.extend_from_slice(&chunk);
        let events_before = answer.events;
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if line_event(&line, &mut answer, on_text)? {
                return answer.finish();
            }
        }
        if answer.events > events_before {
            last_event = std::time::Instant::now();
        } else if last_event.elapsed() > PIECE_LIMIT {
            return Err(HarnessError::Transport(format!(
                "{}{} s; the call was given up",
                crate::model::KEPT_ALIVE,
                PIECE_LIMIT.as_secs()
            )));
        }
    }
    if !pending.is_empty() {
        line_event(&pending, &mut answer, on_text)?;
    }
    answer.finish()
}

/// One line of the stream; `true` at its end.
fn line_event(
    line: &[u8],
    answer: &mut Answer,
    on_text: &mut (dyn FnMut(Piece<'_>) + Send),
) -> Result<bool, HarnessError> {
    let line = std::str::from_utf8(line)
        .map_err(|_| HarnessError::Protocol("the answer is not UTF-8".into()))?
        .trim_end_matches(['\r', '\n']);
    let Some(data) = line.strip_prefix("data:").map(str::trim_start) else {
        return Ok(false);
    };
    let event: Value = serde_json::from_str(data).map_err(|err| {
        HarnessError::Protocol(format!("an event of the answer is not JSON: {err}"))
    })?;
    answer.event(&event, on_text)
}

/// The models of Anthropic's list (`GET /v1/models`, one page of up to a thousand): id, display name, when it came
/// out, its context window — and the prices Glut knows for it.
pub(crate) fn models_of(list: &Value) -> Option<Vec<crate::providers::models::ModelInfo>> {
    let entries = list.get("data")?.as_array()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                let id = entry.get("id")?.as_str()?.trim();
                if id.is_empty() || id.chars().any(char::is_control) {
                    return None;
                }
                Some(crate::providers::models::ModelInfo {
                    id: id.to_owned(),
                    name: entry
                        .get("display_name")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .filter(|name| name != id),
                    context_window: entry
                        .get("max_input_tokens")
                        .and_then(Value::as_u64)
                        .and_then(|tokens| u32::try_from(tokens).ok())
                        .filter(|tokens| *tokens > 0),
                    prices: prices(id),
                    reasoning: Some(thinking_of(id) == Thinking::Adaptive),
                    created: entry
                        .get("created_at")
                        .and_then(Value::as_str)
                        .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                        .map(|at| at.timestamp()),
                    api: None,
                    // Every Claude model Anthropic lists sees images.
                    images: Some(true),
                })
            })
            .collect(),
    )
}

/// The headers a model list is asked with: the key as Anthropic takes it, and the version.
pub(crate) fn list_request(
    request: reqwest::RequestBuilder,
    key: Option<&str>,
) -> reqwest::RequestBuilder {
    let request = request.header("anthropic-version", API_VERSION);
    match key {
        Some(key) => request.header("x-api-key", key_header(key)),
        None => request,
    }
}

/// A warming call's usage as Glut counts it: every prompt token processed, the part read from the cache apart.
fn warm_usage(usage: &Value) -> Usage {
    let tokens = |name: &str| usage.get(name).and_then(Value::as_u64).unwrap_or(0);
    let read = tokens("cache_read_input_tokens");
    Usage {
        input_tokens: tokens("input_tokens") + read + tokens("cache_creation_input_tokens"),
        cached_input_tokens: read,
        output_tokens: tokens("output_tokens"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::fake::{Answer as Prepared, FakeApi};

    fn endpoint(base_url: &str, model: &str) -> ModelEndpoint {
        ModelEndpoint {
            base_url: base_url.to_owned(),
            api_key: Some("sk-ant-test-0123456789".into()),
            model: model.to_owned(),
            effort: Some(Effort::High),
            versioned: false,
        }
    }

    /// A stream of Anthropic events, as the API sends them.
    fn events(events: &[Value]) -> Prepared {
        let body: String = events
            .iter()
            .map(|event| {
                let kind = event["type"].as_str().unwrap_or("");
                format!("event: {kind}\ndata: {event}\n\n")
            })
            .collect();
        Prepared {
            status: 200,
            body,
            content_type: "text/event-stream",
            then_hang: false,
        }
    }

    fn thought_then_call() -> Vec<Value> {
        vec![
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 10, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 90, "output_tokens": 1}}}),
            json!({"type": "ping"}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Read the "}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "file."}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "sig-abc"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Let me look."}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {}}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}}),
            json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"a.rs\"}"}}),
            json!({"type": "content_block_stop", "index": 2}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 42}}),
            json!({"type": "message_stop"}),
        ]
    }

    #[test]
    fn images_go_as_base64_blocks_before_the_words_and_inside_a_tool_result() {
        let (home, image) = crate::images::kept_for_tests("anthropic");
        let mut prompt = Message::user("what is this?");
        prompt.images = vec![image.clone()];
        let (_, turns) = to_api(&[prompt]);
        let blocks = &turns[0]["content"];
        assert!(
            blocks[0]["text"]
                .as_str()
                .unwrap()
                .starts_with("[image #1 · ")
        );
        assert_eq!(blocks[1]["type"], "image");
        assert_eq!(blocks[1]["source"]["media_type"], "image/png");
        assert_eq!(blocks[2]["text"], "what is this?");
        let mut call = Message::user("");
        call.role = Role::Assistant;
        call.content = None;
        call.tool_calls = vec![crate::model::ToolCall {
            id: "a".into(),
            kind: "function".into(),
            function: crate::model::FunctionCall {
                name: "read_file".into(),
                arguments: "{}".into(),
            },
        }];
        let mut seen = Message::tool("a", "an image, 4×3");
        seen.images = vec![image];
        let (_, turns) = to_api(&[Message::user("look"), call, seen]);
        let result = &turns[2]["content"][0];
        assert_eq!(result["type"], "tool_result");
        assert_eq!(result["content"][0]["text"], "an image, 4×3");
        assert_eq!(result["content"][2]["type"], "image");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[tokio::test]
    async fn warming_reads_the_cache_without_an_answer_and_the_hour_is_asked_for_when_chosen() {
        let warmed = Prepared {
            status: 200,
            body: json!({"content": [], "stop_reason": "max_tokens", "usage": {
                "input_tokens": 3, "cache_read_input_tokens": 9000, "cache_creation_input_tokens": 0,
                "output_tokens": 0
            }})
            .to_string(),
            content_type: "application/json",
            then_hang: false,
        };
        let api = FakeApi::spawn(vec![warmed, events(&thought_then_call())]).await;
        let model = AnthropicMessages::new(endpoint(&api.base_url, "claude-opus-5-5")).unwrap();
        let hi = [Message::user("hi")];
        let usage = model.warm(&hi, &[]).await.unwrap();
        assert_eq!(
            usage,
            Usage {
                input_tokens: 9003,
                cached_input_tokens: 9000,
                output_tokens: 0
            }
        );
        model.set_cache_hour(true);
        model.complete(&hi, &[], None).await.unwrap();
        let bodies = api.bodies();
        assert_eq!(
            (bodies[0]["max_tokens"].clone(), bodies[0]["stream"].clone()),
            (json!(0), json!(false))
        );
        assert!(
            bodies[0]["cache_control"].get("ttl").is_none(),
            "five minutes by default"
        );
        assert_eq!(bodies[1]["cache_control"]["ttl"], "1h");
    }

    #[tokio::test]
    async fn a_streamed_answer_brings_its_thinking_text_call_and_cached_usage() {
        let api = FakeApi::spawn(vec![events(&thought_then_call())]).await;
        let model = AnthropicMessages::new(endpoint(&api.base_url, "claude-opus-5-5")).unwrap();
        let mut pieces = Vec::new();
        let reply = model
            .complete_streaming(&[Message::user("Look at a.rs")], &[], None, &mut |piece| {
                pieces.push(format!("{piece:?}"))
            })
            .await
            .unwrap();
        assert_eq!(reply.message.content.as_deref(), Some("Let me look."));
        assert_eq!(
            reply.message.tool_calls[0].function.arguments,
            "{\"path\":\"a.rs\"}"
        );
        assert_eq!(reply.message.reasoning.as_deref(), Some("Read the file."));
        assert_eq!(
            reply.message.reasoning_details,
            [json!({"type": "thinking", "thinking": "Read the file.", "signature": "sig-abc"})]
        );
        assert_eq!(
            reply.usage,
            Usage {
                input_tokens: 1000,
                cached_input_tokens: 900,
                output_tokens: 42
            }
        );
        assert!(
            pieces
                .iter()
                .any(|piece| piece.contains("Reasoning(\"Read the \")"))
        );
        assert!(
            pieces
                .iter()
                .any(|piece| piece.contains("Text(\"Let me look.\")"))
        );
        let (head, body) = api.seen.lock().unwrap()[0].clone();
        let head = head.to_ascii_lowercase();
        assert!(head.contains("x-api-key: sk-ant-test-0123456789"), "{head}");
        assert!(head.contains("anthropic-version: 2023-06-01"));
        assert!(
            head.contains(THINKING_BINDING_BETA) && head.contains(FALLBACK_BETA),
            "{head}"
        );
        assert!(
            !head.contains("authorization"),
            "Anthropic takes the key as x-api-key, not as a bearer"
        );
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(
            body["thinking"]["block_binding"]["prefix_mismatch_behavior"],
            "drop_block"
        );
        assert_eq!(body["output_config"]["effort"], "high");
        assert_eq!(body["fallbacks"], "default");
        assert_eq!(body["cache_control"]["type"], "ephemeral");
        assert_eq!(body["max_tokens"], DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn the_conversation_goes_over_with_its_system_prompt_apart_and_tool_results_in_a_user_turn() {
        let mut thought = Message::user("");
        thought.role = Role::Assistant;
        thought.content = Some("Let me look.".into());
        thought.reasoning_details =
            vec![json!({"type": "thinking", "thinking": "hm", "signature": "s"})];
        thought.tool_calls = vec![ToolCall {
            id: "toolu_1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: "{\"path\":\"a.rs\"}".into(),
            },
        }];
        thought.tool_calls.push(call("toolu_2"));
        let mut earlier = thought.clone();
        earlier.tool_calls.clear();
        let conversation = [
            Message::system("Be brief."),
            Message::user("First"),
            earlier,
            Message::user("Look at a.rs"),
            thought,
            Message::tool("toolu_1", "fn main() {}"),
            Message::tool("toolu_2", ""),
        ];
        let (system, turns) = to_api(&conversation);
        assert_eq!(system, [json!({"type": "text", "text": "Be brief."})]);
        let roles: Vec<&str> = turns
            .iter()
            .map(|turn| turn["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, ["user", "assistant", "user", "assistant", "user"]);
        assert_eq!(
            turns[1]["content"][0]["type"], "text",
            "earlier thinking stays behind (E4)"
        );
        let current = &turns[3]["content"];
        assert_eq!(
            current[0]["type"], "thinking",
            "the thinking goes back first, unchanged"
        );
        assert_eq!(current[0]["signature"], "s");
        assert_eq!(current[2]["type"], "tool_use");
        assert_eq!(current[2]["input"]["path"], "a.rs");
        let results = &turns[4]["content"];
        assert_eq!(results[0]["tool_use_id"], "toolu_1");
        assert_eq!(results[1]["content"], "(no output)");
    }

    #[test]
    fn a_gateway_gets_no_betas_of_anthropics_own() {
        let through = AnthropicMessages::new(endpoint(
            "https://opencode.ai/inference/anthropic/v1",
            "claude-opus-5-5",
        ))
        .unwrap();
        let (body, betas) = through.body(&[Message::user("hi")], &[], Some(100));
        assert!(betas.is_empty(), "{betas:?}");
        assert!(body.get("fallbacks").is_none());
        assert_eq!(
            body["thinking"]["type"], "adaptive",
            "the thinking itself stays"
        );
        assert!(body["thinking"].get("block_binding").is_none());
        let own = AnthropicMessages::new(endpoint("https://api.anthropic.com", "claude-opus-5-5"))
            .unwrap();
        let (_, betas) = own.body(&[Message::user("hi")], &[], Some(100));
        assert!(betas.contains(&THINKING_BINDING_BETA) && betas.contains(&FALLBACK_BETA));
    }

    #[test]
    fn older_models_get_no_thinking_and_efforts_fit_each_model() {
        assert_eq!(thinking_of("claude-opus-5-5"), Thinking::Adaptive);
        assert_eq!(thinking_of("claude-sonnet-4-6"), Thinking::Adaptive);
        assert_eq!(thinking_of("claude-haiku-5-5"), Thinking::Adaptive);
        assert_eq!(thinking_of("claude-fable-5-1"), Thinking::Adaptive);
        assert_eq!(thinking_of("claude-haiku-4-5"), Thinking::Plain);
        assert_eq!(thinking_of("claude-sonnet-4-20250514"), Thinking::Plain);
        assert_eq!(thinking_of("claude-3-7-sonnet-latest"), Thinking::Plain);
        assert_eq!(
            effort_for("claude-opus-5-5", Effort::None),
            "low",
            "thinking cannot be switched off"
        );
        assert_eq!(
            effort_for("claude-opus-4-6", Effort::Xhigh),
            "high",
            "xhigh came with 4.7"
        );
        assert_eq!(effort_for("claude-opus-4-7", Effort::Xhigh), "xhigh");
        let plain =
            AnthropicMessages::new(endpoint("https://api.anthropic.com", "claude-haiku-4-5"))
                .unwrap();
        let (body, betas) = plain.body(&[Message::user("hi")], &[], Some(100));
        assert!(body.get("thinking").is_none() && body.get("output_config").is_none());
        assert!(betas.is_empty() && body.get("fallbacks").is_none());
        assert_eq!(body["max_tokens"], 100);
    }

    #[tokio::test]
    async fn a_refusal_an_error_event_a_cut_stream_and_an_error_status_are_said_without_the_key() {
        let refused = vec![
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "refusal", "stop_details": {"type": "refusal", "category": "cyber"}}, "usage": {"output_tokens": 0}}),
            json!({"type": "message_stop"}),
        ];
        let failed = vec![
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
        ];
        let mut cut = thought_then_call();
        cut.truncate(9);
        let api = FakeApi::spawn(vec![
            events(&refused),
            events(&failed),
            events(&cut),
            Prepared::error(
                401,
                "{\"error\":{\"message\":\"invalid x-api-key sk-ant-test-0123456789\"}}",
            ),
        ])
        .await;
        let model = AnthropicMessages::new(endpoint(&api.base_url, "claude-opus-5-5")).unwrap();
        let hi = [Message::user("hi")];
        let ask = || model.complete(&hi, &[], None);
        let refusal = ask().await.unwrap_err().to_string();
        assert!(
            refusal.contains("declined") && refusal.contains("cyber"),
            "{refusal}"
        );
        assert!(ask().await.unwrap_err().to_string().contains("Overloaded"));
        assert!(
            ask()
                .await
                .unwrap_err()
                .to_string()
                .contains("before the answer was complete")
        );
        let status = ask().await.unwrap_err();
        assert!(matches!(status, HarnessError::Http { status: 401, .. }));
        assert!(!status.to_string().contains("0123456789"), "{status}");
    }

    #[test]
    fn a_model_id_stays_one_path_segment_and_kept_thinking_goes_back_with_its_own_fields_only() {
        assert_eq!(path_segment("claude-opus-5-5"), "claude-opus-5-5");
        assert_eq!(path_segment("../organizations"), "..%2Forganizations");
        assert_eq!(path_segment("a?b#c"), "a%3Fb%23c");
        assert_eq!(path_segment(".."), "%2E%2E");
        let tampered = json!({"type": "thinking", "thinking": "t", "signature": "s", "cache_control": {"type": "x"}});
        assert_eq!(
            thinking_block(&tampered),
            Some(json!({"type": "thinking", "thinking": "t", "signature": "s"}))
        );
        assert_eq!(
            thinking_block(&json!({"type": "thinking", "thinking": "unsigned"})),
            None
        );
        assert_eq!(
            thinking_block(&json!({"type": "reasoning.text", "text": "x"})),
            None,
            "another provider's block"
        );
    }

    #[test]
    fn the_usage_keeps_each_count_apart_whichever_event_brings_it() {
        let mut answer = Answer::default();
        answer.take_usage(&json!({"input_tokens": 10, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 90}));
        answer.take_usage(&json!({"input_tokens": 12, "output_tokens": 40}));
        answer.take_usage(&json!({"cache_read_input_tokens": 950}));
        assert_eq!(
            answer.usage(),
            Usage {
                input_tokens: 12 + 950 + 90,
                cached_input_tokens: 950,
                output_tokens: 40
            },
            "a later count replaces only its own"
        );
    }

    fn call(id: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "bash".into(),
                arguments: "{}".into(),
            },
        }
    }

    #[test]
    fn every_call_gets_its_answer_and_no_answer_stays_without_its_call() {
        let mut asked = Message::user("");
        asked.role = Role::Assistant;
        asked.tool_calls = vec![call("toolu_1"), call("toolu_2")];
        let conversation = [
            Message::tool("toolu_gone", "from before a compaction"),
            Message::user("Go on"),
            asked,
            Message::tool("toolu_1", "done"),
            Message::user("and then?"),
        ];
        let (_, turns) = to_api(&conversation);
        let first = &turns[0]["content"];
        assert_eq!(
            first.as_array().unwrap().len(),
            1,
            "the orphan result went: {first}"
        );
        assert_eq!(first[0]["text"], "Go on");
        let answers = turns[2]["content"].as_array().unwrap();
        let kinds: Vec<&str> = answers
            .iter()
            .map(|block| block["type"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            ["tool_result", "tool_result", "text"],
            "answers first, then the owner's words"
        );
        let not_run = answers
            .iter()
            .find(|block| block["tool_use_id"] == "toolu_2")
            .unwrap();
        assert_eq!(not_run["content"], "(not run)");
        assert_eq!(not_run["is_error"], true);
        let mut trailing = Message::user("");
        trailing.role = Role::Assistant;
        trailing.tool_calls = vec![call("toolu_9")];
        let (_, turns) = to_api(&[Message::user("hi"), trailing]);
        assert_eq!(
            turns.last().unwrap()["role"],
            "user",
            "a call at the very end is answered too"
        );
    }

    #[tokio::test]
    async fn a_tool_call_cut_at_the_token_limit_is_no_call_and_redacted_thinking_comes_through() {
        let cut = vec![
            json!({"type": "message_start", "message": {"usage": {"input_tokens": 5}}}),
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "redacted_thinking", "data": "enc"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "write_file", "input": {}}}),
            json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"path\": \"a"}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "max_tokens"}, "usage": {"output_tokens": 100}}),
            json!({"type": "message_stop"}),
        ];
        let mut whole = cut.clone();
        whole[4] = json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{}"}});
        whole[5] = json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 100}});
        let api = FakeApi::spawn(vec![events(&cut), events(&whole)]).await;
        let model = AnthropicMessages::new(endpoint(&api.base_url, "claude-opus-5-5")).unwrap();
        let hi = [Message::user("hi")];
        let err = model
            .complete(&hi, &[], None)
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("token limit"), "{err}");
        let reply = model.complete(&hi, &[], None).await.unwrap();
        assert_eq!(
            reply.message.reasoning_details,
            [json!({"type": "redacted_thinking", "data": "enc"})]
        );
        let mut next = reply.message.clone();
        next.tool_calls.clear();
        let (_, turns) = to_api(&[Message::user("hi"), next]);
        assert_eq!(
            turns[1]["content"][0],
            json!({"type": "redacted_thinking", "data": "enc"}),
            "back unchanged"
        );
    }

    #[test]
    fn model_ids_old_and_new_are_told_apart() {
        for old in [
            "claude-3-5-haiku-latest",
            "claude-opus-4-1-20250805",
            "claude-sonnet-4-5",
            "claude-3-haiku-20240307",
        ] {
            assert_eq!(thinking_of(old), Thinking::Plain, "{old}");
        }
        for new in ["claude-sonnet-5", "claude-opus-4-8", "claude-mythos-5-1"] {
            assert_eq!(thinking_of(new), Thinking::Adaptive, "{new}");
        }
    }

    #[tokio::test]
    async fn the_model_list_is_asked_as_anthropic_wants_it() {
        let api = FakeApi::spawn(vec![Prepared::ok(json!({"data": [
            {"id": "claude-sonnet-5-5", "display_name": "Claude Sonnet 5.5", "created_at": "2026-09-15T00:00:00Z", "max_input_tokens": 1000000}
        ], "has_more": false}))])
        .await;
        let list = crate::providers::models::fetch(
            &endpoint(&api.base_url, "-"),
            crate::providers::catalog::Protocol::AnthropicMessages,
        )
        .await
        .unwrap();
        assert_eq!(list.models[0].id, "claude-sonnet-5-5");
        assert_eq!(
            list.models[0].prices.map(|prices| prices.output_per_m),
            Some(10.0)
        );
        let (head, _) = api.seen.lock().unwrap()[0].clone();
        let head = head.to_ascii_lowercase();
        assert!(head.starts_with("get /v1/models?limit=1000"), "{head}");
        assert!(
            head.contains("x-api-key:") && !head.contains("authorization"),
            "{head}"
        );
    }

    #[test]
    fn a_key_never_goes_over_plain_http_to_another_machine() {
        assert!(AnthropicMessages::new(endpoint("http://example.com", "claude-opus-5-5")).is_err());
        assert!(AnthropicMessages::new(endpoint("http://127.0.0.1:9", "claude-opus-5-5")).is_ok());
    }

    #[test]
    fn the_model_list_reads_windows_dates_and_known_prices() {
        let list = json!({"data": [
            {"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "created_at": "2026-09-01T00:00:00Z", "max_input_tokens": 1000000},
            {"id": "claude-haiku-4-5", "display_name": "Claude Haiku 4.5", "created_at": "2025-10-01T00:00:00Z", "max_input_tokens": 200000},
        ], "has_more": false});
        let models = models_of(&list).unwrap();
        assert_eq!(models[0].context_window, Some(1_000_000));
        assert_eq!(models[0].name.as_deref(), Some("Claude Opus 5.5"));
        assert_eq!(models[0].prices.map(|prices| prices.input_per_m), Some(4.0));
        assert!(models[0].created.is_some_and(|at| at > 1_700_000_000));
        assert_eq!(models[1].prices, None, "no price Glut does not know");
        assert_eq!(models[1].reasoning, Some(false));
    }
}
