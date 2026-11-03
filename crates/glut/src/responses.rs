//! OpenAI's Responses API (`docs/plans/glut-providers.md`, PV4): the ChatGPT plan's way in, OpenAI's newer models by
//! key, and OpenCode Zen's models that speak it. Every request is streamed and stored nowhere (`store: false`): the
//! whole conversation goes as `input` each time, the system prompt as `instructions`, the model's reasoning as the
//! encrypted items the API hands out (`include: reasoning.encrypted_content`), sent back unchanged while the model
//! works on the same prompt.
//!
//! On the ChatGPT plan ([`ResponsesApi::signed`]) the bearer token comes from the sign-in at every call
//! ([`crate::subscription::Credential`]), and what the plan does not take is never sent (`max_output_tokens`,
//! `temperature`); its usage limit ends the turn with the owner's way to it — never a guess when it resets.

use std::time::Duration;

use serde_json::{Value, json};

use crate::config::{Effort, ModelEndpoint};
use crate::error::HarnessError;
use crate::model::Message;
use crate::model::{
    ChatModel, ERROR_BODY_CHARS, FunctionCall, MAX_ANSWER_BYTES, PIECE_LIMIT, Piece,
    REQUEST_TIMEOUT, Reply, Role, STALL_LIMIT, ToolCall, ToolSpec, Usage, read_capped, redact,
    secure_enough,
};
use crate::subscription::Credential;

/// The error code of a ChatGPT plan whose usage limit is reached: the turn ends, the owner is shown where to see it.
pub const PLAN_LIMIT: &str = "subscription_sharing_usage_limit_exceeded";
/// The codes that say the plan's service is busy: tried again after a pause, a bounded number of times.
const BUSY: [&str; 2] = [
    "subscription_sharing_usage_unavailable",
    "subscription_sharing_user_unavailable",
];
/// How often a busy plan is tried again, and the first pause (doubled each time).
const BUSY_TRIES: u32 = 3;
const BUSY_PAUSE: Duration = Duration::from_secs(2);

/// A model behind OpenAI's Responses API.
#[derive(Debug, Clone)]
pub struct ResponsesApi {
    endpoint: ModelEndpoint,
    client: reqwest::Client,
    /// The ChatGPT plan's sign-in; `None` for a key (or none).
    credential: Option<Credential>,
    stall: Duration,
}

impl ResponsesApi {
    /// A client for `endpoint` with its key.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Transport`] if the HTTP client cannot be built; [`HarnessError::Protocol`] for a key that would
    /// go over plain HTTP to another machine.
    pub fn new(endpoint: ModelEndpoint) -> Result<Self, HarnessError> {
        Self::make(endpoint, None)
    }

    /// A client for `endpoint` on a signed-in plan: the bearer token from `credential` at every call.
    ///
    /// # Errors
    ///
    /// As [`Self::new`].
    pub fn signed(endpoint: ModelEndpoint, credential: Credential) -> Result<Self, HarnessError> {
        Self::make(endpoint, Some(credential))
    }

    fn make(endpoint: ModelEndpoint, credential: Option<Credential>) -> Result<Self, HarnessError> {
        if (endpoint.api_key.is_some() || credential.is_some())
            && !secure_enough(&endpoint.base_url)
        {
            return Err(HarnessError::Protocol(
                "a key is only sent over https, or over http to this machine".into(),
            ));
        }
        let client = crate::model::identified(reqwest::Client::builder(), &endpoint.base_url)
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|err| HarnessError::Transport(err.to_string()))?;
        Ok(ResponsesApi {
            endpoint,
            client,
            credential,
            stall: STALL_LIMIT,
        })
    }

    /// Whether it runs on a signed-in plan.
    fn plan(&self) -> bool {
        self.credential.is_some()
    }

    fn url(&self, path: &str) -> String {
        let base = self.endpoint.base_url.trim_end_matches('/');
        if self.endpoint.versioned {
            format!("{base}/{path}")
        } else {
            format!("{base}/v1/{path}")
        }
    }

    /// The bearer token of the next call: the plan's, fresh; else the key.
    async fn bearer(&self) -> Result<Option<String>, HarnessError> {
        match &self.credential {
            Some(credential) => credential.bearer().await.map(Some),
            None => Ok(self.endpoint.api_key.clone()),
        }
    }

    fn body(
        &self,
        messages: &[Message],
        tools: &[ToolSpec],
        max_output_tokens: Option<u32>,
    ) -> Value {
        let (instructions, input) = to_input(messages);
        let mut body = json!({
            "model": self.endpoint.model,
            "input": input,
            "stream": true,
            "store": false,
            "include": ["reasoning.encrypted_content"],
        });
        if !instructions.is_empty() {
            body["instructions"] = json!(instructions);
        }
        if !tools.is_empty() {
            body["tools"] = Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        json!({
                            "type": "function",
                            "name": tool.name,
                            "description": tool.description,
                            "parameters": tool.parameters,
                            "strict": false,
                        })
                    })
                    .collect(),
            );
        }
        // The plan takes no output limit (OpenAI's list of what it refuses).
        if let Some(max) = max_output_tokens.filter(|_| !self.plan()) {
            body["max_output_tokens"] = json!(max);
        }
        if let Some(effort) = self.endpoint.effort {
            body["reasoning"] = json!({ "effort": effort_word(effort) });
            // Summaries need a verified organisation by key; the plan has them.
            if self.plan() {
                body["reasoning"]["summary"] = json!("auto");
            }
        }
        body
    }
}

/// An effort as the Responses API names it: its top level is `xhigh`.
fn effort_word(effort: Effort) -> &'static str {
    match effort {
        Effort::Max => "xhigh",
        other => other.as_str(),
    }
}

impl ChatModel for ResponsesApi {
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
        let body = self.body(messages, tools, max_output_tokens).to_string();
        let mut pause = BUSY_PAUSE;
        let mut tries = 0;
        // A plan's token the API turns away is renewed once and the call asked again.
        let mut refused: Option<String> = None;
        loop {
            let bearer = match (&self.credential, &refused) {
                (Some(credential), Some(token)) => Some(credential.instead_of(token).await?),
                _ => self.bearer().await?,
            };
            let mut request = self
                .client
                .post(self.url("responses"))
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body.clone());
            if let Some(token) = &bearer {
                request = request.bearer_auth(token);
            }
            let response = request
                .send()
                .await
                .map_err(|err| HarnessError::Transport(err.without_url().to_string()))?;
            let status = response.status();
            if status.is_success() {
                // The stream's errors carry the API's words: the token never goes back out in them.
                return read_stream(response, self.stall, on_text)
                    .await
                    .map_err(|err| match err {
                        HarnessError::Protocol(message) => {
                            HarnessError::Protocol(redact(&message, bearer.as_deref()))
                        }
                        other => other,
                    });
            }
            let text = read_capped(response).await?;
            if status == reqwest::StatusCode::UNAUTHORIZED && self.plan() && refused.is_none() {
                refused = bearer;
                continue;
            }
            let code = error_code(&text);
            if code.as_deref().is_some_and(|code| BUSY.contains(&code)) && tries < BUSY_TRIES {
                tries += 1;
                tokio::time::sleep(pause).await;
                pause *= 2;
                continue;
            }
            return Err(match code {
                Some(code) => failure(&code, &redact(&error_message(&text), bearer.as_deref())),
                None => {
                    let body: String = text.chars().take(ERROR_BODY_CHARS).collect();
                    HarnessError::Http {
                        status: status.as_u16(),
                        body: redact(&body, bearer.as_deref()),
                    }
                }
            });
        }
    }

    /// The plan has no call that only reads the cache, and one of a token is no answer it takes: there is nothing to
    /// keep warm on it. By key, the default — one token of answer.
    async fn warm(&self, messages: &[Message], tools: &[ToolSpec]) -> Result<Usage, HarnessError> {
        if self.plan() {
            return Err(HarnessError::Protocol(
                "a ChatGPT plan's cache cannot be kept warm".into(),
            ));
        }
        self.complete(messages, tools, Some(16))
            .await
            .map(|reply| reply.usage)
    }

    fn name(&self) -> &str {
        &self.endpoint.model
    }
}

/// The `error.code` of an error answer (`{"error": {"code": …}}`), if it has one.
fn error_code(text: &str) -> Option<String> {
    let value: Value = serde_json::from_str(text).ok()?;
    value
        .pointer("/error/code")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The `error.message` (or `detail`) of an error answer, cut and made printable.
fn error_message(text: &str) -> String {
    let value: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let said = value
        .pointer("/error/message")
        .or_else(|| value.get("detail"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    crate::permissions::printable(&said.chars().take(ERROR_BODY_CHARS).collect::<String>())
}

/// What the owner is told of an error the API named.
fn failure(code: &str, message: &str) -> HarnessError {
    if code == PLAN_LIMIT {
        return HarnessError::Protocol(format!(
            "the ChatGPT plan's usage limit is reached ({PLAN_LIMIT}) — see and manage it at {}",
            crate::subscription::chatgpt::USAGE_PAGE
        ));
    }
    let code = crate::permissions::printable(code);
    if message.is_empty() {
        HarnessError::Protocol(format!("the API refused the call: {code}"))
    } else {
        HarnessError::Protocol(format!("the API refused the call: {code}: {message}"))
    }
}

/// Glut's conversation as the Responses API takes it: the system prompt apart as `instructions` (a later system
/// message as a developer message — the plan refuses system items), the owner's words and images, the model's words,
/// its calls and their answers as items of their own, its reasoning items back while it works on the same prompt.
fn to_input(messages: &[Message]) -> (String, Vec<Value>) {
    let latest_prompt = messages
        .iter()
        .rposition(|message| message.role == Role::User)
        .unwrap_or(0);
    let mut instructions = Vec::new();
    let mut input = Vec::new();
    // The images of tool answers (T45): an answer is text alone, so they follow the run of answers as the owner's.
    let mut shown: Vec<Value> = Vec::new();
    let mut began = false;
    for (at, message) in messages.iter().enumerate() {
        if message.role != Role::Tool && !shown.is_empty() {
            input.push(
                json!({ "type": "message", "role": "user", "content": std::mem::take(&mut shown) }),
            );
        }
        let text = message.content.as_deref().unwrap_or_default();
        match message.role {
            Role::System if !began => instructions.push(text.to_owned()),
            Role::System => input.push(json!({
                "type": "message",
                "role": "developer",
                "content": [{ "type": "input_text", "text": text }],
            })),
            Role::User => {
                began = true;
                let mut content = Vec::new();
                if !text.trim().is_empty() {
                    content.push(json!({ "type": "input_text", "text": text }));
                }
                content.extend(image_parts(&message.images));
                if content.is_empty() {
                    content.push(json!({ "type": "input_text", "text": " " }));
                }
                input.push(json!({ "type": "message", "role": "user", "content": content }));
            }
            Role::Assistant => {
                began = true;
                if at > latest_prompt {
                    input.extend(
                        message
                            .reasoning_details
                            .iter()
                            .filter(|item| {
                                item.get("type").and_then(Value::as_str) == Some("reasoning")
                            })
                            .cloned(),
                    );
                }
                if !text.trim().is_empty() {
                    input.push(json!({
                        "type": "message",
                        "role": "assistant",
                        "content": [{ "type": "output_text", "text": text }],
                    }));
                }
                for call in &message.tool_calls {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.function.name,
                        "arguments": call.function.arguments,
                    }));
                }
            }
            Role::Tool => {
                began = true;
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": message.tool_call_id.clone().unwrap_or_default(),
                    "output": text,
                }));
                if !message.images.is_empty() {
                    if shown.is_empty() {
                        shown.push(json!({ "type": "input_text", "text": "The images the tools above returned:" }));
                    }
                    shown.extend(image_parts(&message.images));
                }
            }
        }
    }
    if !shown.is_empty() {
        input.push(json!({ "type": "message", "role": "user", "content": shown }));
    }
    (instructions.join("\n\n"), input)
}

/// Images as input parts, each after its line, so the model can name it (`image #2`).
fn image_parts(images: &[crate::images::Image]) -> Vec<Value> {
    images
        .iter()
        .flat_map(|image| {
            let part = match crate::images::base64_of(image) {
                Ok((media, data)) => json!({
                    "type": "input_image",
                    "image_url": format!("data:{media};base64,{data}"),
                }),
                Err(reason) => json!({ "type": "input_text", "text": format!("[{reason}]") }),
            };
            [json!({ "type": "input_text", "text": image.label() }), part]
        })
        .collect()
}

/// What a streamed answer has said so far.
#[derive(Default)]
struct Answer {
    /// The finished output items, in their order.
    items: Vec<(u64, Value)>,
    /// The text as it streamed, for an answer whose items never came.
    text: String,
    thought: String,
    usage: Usage,
    /// Whether `response.completed` (or `response.incomplete`) came: a stream cut off before it is no answer.
    ended: bool,
    /// Why the answer stopped short, where it did (`max_output_tokens`).
    incomplete: Option<String>,
    events: u64,
}

impl Answer {
    /// One event of the stream; `true` at its end.
    fn event(
        &mut self,
        data: &Value,
        on_text: &mut (dyn FnMut(Piece<'_>) + Send),
    ) -> Result<bool, HarnessError> {
        self.events += 1;
        let kind = data.get("type").and_then(Value::as_str).unwrap_or("");
        let delta = || data.get("delta").and_then(Value::as_str).unwrap_or("");
        match kind {
            "response.output_text.delta" => {
                self.text.push_str(delta());
                on_text(Piece::Text(delta()));
            }
            "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                self.thought.push_str(delta());
                on_text(Piece::Reasoning(delta()));
            }
            "response.output_item.done" => {
                let index = data
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(self.items.len() as u64);
                if let Some(item) = data.get("item") {
                    self.items.push((index, item.clone()));
                }
            }
            "response.completed" | "response.incomplete" => {
                let response = data.get("response").unwrap_or(&Value::Null);
                self.usage = usage_of(response.get("usage").unwrap_or(&Value::Null));
                // The finished response's own list wins where it carries one: it is the whole answer.
                if let Some(output) = response
                    .get("output")
                    .and_then(Value::as_array)
                    .filter(|output| !output.is_empty())
                {
                    self.items = output
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(at, item)| (at as u64, item))
                        .collect();
                }
                if kind == "response.incomplete" {
                    self.incomplete = Some(
                        response
                            .pointer("/incomplete_details/reason")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_owned(),
                    );
                }
                self.ended = true;
                return Ok(true);
            }
            "response.failed" => {
                let error = data.pointer("/response/error").unwrap_or(&Value::Null);
                return Err(failure(
                    error
                        .get("code")
                        .and_then(Value::as_str)
                        .unwrap_or("failed"),
                    &crate::permissions::printable(
                        error.get("message").and_then(Value::as_str).unwrap_or(""),
                    ),
                ));
            }
            "error" => {
                return Err(failure(
                    data.get("code").and_then(Value::as_str).unwrap_or("error"),
                    &crate::permissions::printable(
                        data.get("message").and_then(Value::as_str).unwrap_or(""),
                    ),
                ));
            }
            _ => {}
        }
        Ok(false)
    }

    /// The answer as the assistant's message.
    fn finish(mut self) -> Result<Reply, HarnessError> {
        if !self.ended {
            return Err(HarnessError::Protocol(
                "the stream ended before the answer was complete".into(),
            ));
        }
        self.items.sort_by_key(|(at, _)| *at);
        let mut text = String::new();
        let mut thought = String::new();
        let mut details = Vec::new();
        let mut tool_calls = Vec::new();
        let field = |item: &Value, key: &str| {
            item.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        for (_, item) in &self.items {
            match item.get("type").and_then(Value::as_str) {
                Some("message") => {
                    for part in item
                        .get("content")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(more) = part.get("text").and_then(Value::as_str) {
                            text.push_str(more);
                        }
                    }
                }
                Some("function_call") => {
                    let arguments = field(item, "arguments");
                    if self.incomplete.is_some()
                        && serde_json::from_str::<Value>(&arguments).is_err()
                    {
                        return Err(HarnessError::Protocol(
                            "the answer stopped short in the middle of a tool call".into(),
                        ));
                    }
                    tool_calls.push(ToolCall {
                        id: field(item, "call_id"),
                        kind: "function".into(),
                        function: FunctionCall {
                            name: field(item, "name"),
                            arguments: if arguments.trim().is_empty() {
                                "{}".into()
                            } else {
                                arguments
                            },
                        },
                    });
                }
                Some("reasoning") => {
                    for part in item
                        .get("summary")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                    {
                        if let Some(more) = part.get("text").and_then(Value::as_str) {
                            thought.push_str(more);
                        }
                    }
                    // Back as it came but for its id: with nothing stored, an id names an item the API never kept.
                    let mut kept = item.clone();
                    if let Some(fields) = kept.as_object_mut() {
                        fields.remove("id");
                        fields.remove("status");
                    }
                    details.push(kept);
                }
                _ => {}
            }
        }
        if text.is_empty() && tool_calls.is_empty() {
            text = std::mem::take(&mut self.text);
        }
        if thought.is_empty() {
            thought = std::mem::take(&mut self.thought);
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
            usage: self.usage,
            served_by: None,
        })
    }
}

/// A response's usage as Glut counts it: every prompt token processed, the part read from the cache apart.
fn usage_of(usage: &Value) -> Usage {
    let tokens = |pointer: &str| usage.pointer(pointer).and_then(Value::as_u64).unwrap_or(0);
    Usage {
        input_tokens: tokens("/input_tokens"),
        cached_input_tokens: tokens("/input_tokens_details/cached_tokens"),
        output_tokens: tokens("/output_tokens"),
    }
}

/// A streamed answer (server-sent events) read into one [`Reply`]: each line's `data:` is one event. Fails when the
/// stream stays silent for `stall`, carries an error, grows past [`MAX_ANSWER_BYTES`], or ends before
/// `response.completed`.
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
        let before = answer.events;
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if line_event(&line, &mut answer, on_text)? {
                return answer.finish();
            }
        }
        if answer.events > before {
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
    if data == "[DONE]" {
        return Ok(answer.ended);
    }
    let event: Value = serde_json::from_str(data).map_err(|err| {
        HarnessError::Protocol(format!("an event of the answer is not JSON: {err}"))
    })?;
    answer.event(&event, on_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Server-sent events, one `data:` line each.
    fn events(events: &[Value]) -> String {
        events
            .iter()
            .map(|event| format!("data: {event}\n"))
            .collect()
    }

    fn feed(lines: &str) -> Result<(Reply, String), HarnessError> {
        let mut answer = Answer::default();
        let mut seen = String::new();
        for line in lines.lines() {
            let mut on_text = |piece: Piece<'_>| {
                if let Piece::Text(text) = piece {
                    seen.push_str(text);
                }
            };
            if line_event(line.as_bytes(), &mut answer, &mut on_text)? {
                break;
            }
        }
        answer.finish().map(|reply| (reply, seen))
    }

    #[test]
    fn the_conversation_goes_as_instructions_and_items_its_reasoning_only_for_the_prompt_at_hand() {
        let reasoning = json!({"type": "reasoning", "summary": [], "encrypted_content": "e1"});
        let mut old = Message::text(Role::Assistant, "first");
        old.reasoning_details = vec![reasoning.clone()];
        let mut calling = Message::text(Role::Assistant, "");
        calling.content = None;
        calling.reasoning_details = vec![
            reasoning.clone(),
            json!({"type": "thinking", "thinking": "x"}),
        ];
        calling.tool_calls = vec![ToolCall {
            id: "call_1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: "{\"path\":\"a\"}".into(),
            },
        }];
        let messages = [
            Message::system("be brief"),
            Message::user("one"),
            old,
            Message::user("two"),
            calling,
            Message::tool("call_1", "contents"),
            Message::system("a note"),
        ];
        let (instructions, input) = to_input(&messages);
        assert_eq!(instructions, "be brief");
        let kinds: Vec<String> = input
            .iter()
            .map(|item| {
                format!(
                    "{}/{}",
                    item["type"].as_str().unwrap(),
                    item.get("role").and_then(Value::as_str).unwrap_or("")
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "message/user",
                "message/assistant",
                "message/user",
                "reasoning/",
                "function_call/",
                "function_call_output/",
                "message/developer"
            ],
            "the old prompt's reasoning left out, another provider's thinking never sent"
        );
        assert_eq!(input[4]["call_id"], "call_1");
        assert_eq!(input[5]["output"], "contents");
        assert_eq!(input[3]["encrypted_content"], "e1");
    }

    #[test]
    fn a_streamed_answer_gives_its_text_its_calls_its_reasoning_and_its_usage() {
        let stream = format!(
            "event: response.output_text.delta\n{}",
            events(&[
                json!({"type": "response.output_text.delta", "delta": "Hel"}),
                json!({"type": "response.output_text.delta", "delta": "lo"}),
                json!({"type": "response.reasoning_summary_text.delta", "delta": "thinking"}),
                json!({"type": "response.output_item.done", "output_index": 1,
                       "item": {"type": "message", "content": [{"type": "output_text", "text": "Hello"}]}}),
                json!({"type": "response.output_item.done", "output_index": 0,
                       "item": {"id": "rs_1", "type": "reasoning", "encrypted_content": "e2", "status": "completed",
                                "summary": [{"type": "summary_text", "text": "thinking"}]}}),
                json!({"type": "response.output_item.done", "output_index": 2,
                       "item": {"type": "function_call", "call_id": "call_9", "name": "bash",
                                "arguments": "{\"command\":\"ls\"}"}}),
                json!({"type": "response.completed", "response": {"usage": {"input_tokens": 100,
                       "input_tokens_details": {"cached_tokens": 40}, "output_tokens": 7}}}),
            ])
        );
        let stream = stream.as_str();
        let (reply, seen) = feed(stream).unwrap();
        assert_eq!(seen, "Hello");
        assert_eq!(reply.message.content.as_deref(), Some("Hello"));
        assert_eq!(reply.message.reasoning.as_deref(), Some("thinking"));
        assert_eq!(reply.message.tool_calls[0].id, "call_9");
        assert_eq!(reply.message.tool_calls[0].function.name, "bash");
        let kept = &reply.message.reasoning_details[0];
        assert_eq!(kept["encrypted_content"], "e2");
        assert!(kept.get("id").is_none(), "nothing stored: no id to name");
        assert_eq!(
            reply.usage,
            Usage {
                input_tokens: 100,
                cached_input_tokens: 40,
                output_tokens: 7
            }
        );
    }

    #[test]
    fn a_plans_limit_says_where_to_see_it_and_a_cut_stream_or_call_is_no_answer() {
        let failed = events(&[json!({"type": "response.failed",
                                     "response": {"error": {"code": PLAN_LIMIT, "message": "x"}}})]);
        let said = feed(&failed).unwrap_err().to_string();
        assert!(
            said.contains(PLAN_LIMIT) && said.contains("chatgpt.com/settings/usage"),
            "{said}"
        );
        assert!(
            feed(r#"data: {"type":"response.output_text.delta","delta":"Hel"}"#).is_err(),
            "never completed"
        );
        let cut = events(&[
            json!({"type": "response.output_item.done", "output_index": 0,
                   "item": {"type": "function_call", "call_id": "c", "name": "bash", "arguments": "{\"comm"}}),
            json!({"type": "response.incomplete",
                   "response": {"incomplete_details": {"reason": "max_output_tokens"}}}),
        ]);
        assert!(feed(&cut).is_err());
        assert_eq!(
            error_code(r#"{"error":{"code":"subscription_sharing_usage_unavailable"}}"#).as_deref(),
            Some(BUSY[0])
        );
        assert_eq!(effort_word(Effort::Max), "xhigh");
    }

    #[test]
    fn the_plan_is_sent_nothing_it_refuses() {
        let endpoint = ModelEndpoint {
            base_url: "https://api.openai.com/v1".into(),
            api_key: None,
            model: "gpt-6.1-sol".into(),
            effort: Some(Effort::High),
            versioned: true,
        };
        let credential = Credential::new(
            std::path::Path::new("/nowhere"),
            "chatgpt",
            "chatgpt@api.openai.com",
        );
        let plan = ResponsesApi::signed(endpoint.clone(), credential).unwrap();
        let body = plan.body(&[Message::user("hi")], &[], Some(1));
        assert_eq!(
            (body["store"].clone(), body["stream"].clone()),
            (json!(false), json!(true))
        );
        assert!(body.get("max_output_tokens").is_none() && body.get("temperature").is_none());
        assert_eq!(body["reasoning"]["effort"], "high");
        let keyed = ResponsesApi::new(endpoint).unwrap();
        assert_eq!(
            keyed.body(&[Message::user("hi")], &[], Some(1))["max_output_tokens"],
            1
        );
        assert_eq!(
            keyed.url("responses"),
            "https://api.openai.com/v1/responses"
        );
    }
}
