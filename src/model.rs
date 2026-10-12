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
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, other: Usage) {
        self.input_tokens += other.input_tokens;
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

    /// The model's name, for the events.
    fn name(&self) -> &str;
}

/// A model behind an OpenAI-compatible chat API.
#[derive(Debug, Clone)]
pub struct OpenAiCompat {
    endpoint: ModelEndpoint,
    client: reqwest::Client,
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
        Ok(OpenAiCompat { endpoint, client })
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
        let mut body = json!({
            "model": self.endpoint.model,
            "messages": messages,
            "stream": false,
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
        let text = read_capped(response).await?;
        if !status.is_success() {
            // Some APIs repeat the offending key in their error ("Incorrect API key provided: sk-…").
            let body: String = text.chars().take(ERROR_BODY_CHARS).collect();
            return Err(HarnessError::Http {
                status: status.as_u16(),
                body: redact(&body, self.endpoint.api_key.as_deref()),
            });
        }
        parse_reply(&text).map_err(|err| match err {
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
fn secure_enough(base_url: &str) -> bool {
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
    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("no message");
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
    let tokens = |key: &str| {
        value
            .pointer(&format!("/usage/{key}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    Ok(Reply {
        message,
        usage: Usage {
            input_tokens: tokens("prompt_tokens"),
            output_tokens: tokens("completion_tokens"),
        },
    })
}

#[cfg(test)]
pub(crate) mod fake {
    //! A model API on the loopback that answers each request with the next prepared body, and keeps what it was sent.

    use std::sync::{Arc, Mutex};

    use serde_json::Value;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// One prepared answer: status and body.
    pub struct Answer {
        pub status: u16,
        pub body: String,
    }

    impl Answer {
        pub fn ok(body: Value) -> Self {
            Answer {
                status: 200,
                body: body.to_string(),
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
                    let reply = format!(
                        concat!(
                            "HTTP/1.1 {} X\r\ncontent-type: application/json\r\n",
                            "content-length: {}\r\nconnection: close\r\n\r\n{}"
                        ),
                        answer.status,
                        answer.body.len(),
                        answer.body
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
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

    /// Reads one request whole: the head up to the blank line, then `content-length` bytes of body.
    async fn read_request(socket: &mut tokio::net::TcpStream) -> (String, Vec<u8>) {
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

    #[tokio::test]
    async fn a_call_sends_model_messages_tools_and_key_and_reads_tool_calls_and_usage() {
        let api = FakeApi::spawn(vec![Answer::ok(json!({
            "choices": [{"message": {
                "role": "assistant",
                "content": "",
                "tool_calls": [{"id": "c1", "type": "function",
                                "function": {"name": "read_file", "arguments": "{\"path\":\"a.txt\"}"}}]
            }}],
            "usage": {"prompt_tokens": 12, "completion_tokens": 3}
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
        let api = FakeApi::spawn(vec![Answer {
            status: 401,
            body: format!("{{\"error\":\"bad key\"}}{}", "x".repeat(2000)),
        }])
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
        let api = FakeApi::spawn(vec![Answer {
            status: 401,
            body: r#"{"error":"Incorrect API key provided: sk-live-0123456789"}"#.into(),
        }])
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
