//! The tools a run offers the model, and the box that holds them.
//!
//! A tool answers with text either way: what it found, or what went wrong. An error is not the end of the run —
//! it goes back to the model, which can correct its call (a wrong path, an ambiguous edit).

pub mod files;
pub mod search;
pub mod shell;
pub mod web;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::model::ToolSpec;
use crate::permissions::{Access, Gate};

/// The most characters of one tool result the model gets. A bigger result (a huge file, a broad search) would eat the
/// context window in one step; the model is told it was cut and can narrow its call.
pub const MAX_TOOL_OUTPUT_CHARS: usize = 30_000;

/// What a tool call returns: its text, or the text of its error.
pub type ToolFuture<'a> = Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>;

/// Set when the run stops (cancelled, out of time) while a tool works: a tool that walks a tree checks it and stops,
/// and no tool starts once it is set. A tool's future is dropped at that moment, but work already handed to a
/// blocking thread — one file write — still finishes; nothing new begins after it.
pub type StopFlag = Arc<AtomicBool>;

/// What a tool call gets besides its arguments: the run's stop signal and, if the run has one, its gate (a search
/// leaves out what the read rules hold back).
#[derive(Clone, Default)]
pub struct CallContext {
    pub stop: StopFlag,
    pub gate: Option<Arc<Gate>>,
}

/// A tool the model can call.
pub trait Tool: Send + Sync {
    /// The name, description and argument schema the model is shown.
    fn spec(&self) -> ToolSpec;
    /// What a call with these arguments would touch, for the run's [`Gate`] to judge before it runs. Required, not
    /// defaulted: a new tool must say what it does before any rule can speak about it.
    fn access(&self, arguments: &Value) -> Access;
    /// Runs the tool with the model's arguments (a JSON object).
    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_>;
}

/// The tools of a run.
#[derive(Default)]
pub struct Toolbox {
    tools: Vec<Box<dyn Tool>>,
    /// Judges every call; without one, every call runs (a library caller that checks on its own, the tests).
    gate: Option<Arc<Gate>>,
}

impl Toolbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// Puts every call under `gate` (HS3a).
    pub fn set_gate(&mut self, gate: Arc<Gate>) {
        self.gate = Some(gate);
    }

    pub fn gate(&self) -> Option<&Arc<Gate>> {
        self.gate.as_ref()
    }

    /// Adds `tools`.
    pub fn extend(&mut self, tools: impl IntoIterator<Item = Box<dyn Tool>>) {
        self.tools.extend(tools);
    }

    /// The specs to send to the model.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.iter().map(|tool| tool.spec()).collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.spec().name).collect()
    }

    /// Runs the tool `name` with the model's argument text. Returns whether it succeeded and the text for the model,
    /// cut to [`MAX_TOOL_OUTPUT_CHARS`].
    pub async fn call(&self, name: &str, arguments: &str) -> (bool, String) {
        self.call_until(name, arguments, &StopFlag::default()).await
    }

    /// [`Self::call`] within a run: refused once `stop` is set, and handed to the tool so a long one can stop too.
    pub async fn call_until(&self, name: &str, arguments: &str, stop: &StopFlag) -> (bool, String) {
        if stop.load(Ordering::SeqCst) {
            return (false, "error: the run was stopped".into());
        }
        let Some(tool) = self.tools.iter().find(|tool| tool.spec().name == name) else {
            return (false, format!("error: there is no tool named {name:?}"));
        };
        // Some models send no arguments at all for a tool without required ones.
        let arguments = if arguments.trim().is_empty() {
            "{}"
        } else {
            arguments
        };
        let parsed = match serde_json::from_str::<Value>(arguments) {
            Ok(value @ Value::Object(_)) => value,
            Ok(_) => return (false, "error: the arguments must be a JSON object".into()),
            Err(err) => {
                return (
                    false,
                    format!("error: the arguments are not valid JSON: {err}"),
                );
            }
        };
        if let Some(gate) = &self.gate
            && let Err(refused) = gate.judge(name, &tool.access(&parsed)).await
        {
            return (false, format!("error: {refused}"));
        }
        let context = CallContext {
            stop: Arc::clone(stop),
            gate: self.gate.clone(),
        };
        let (ok, text) = match tool.call(parsed, context).await {
            Ok(text) => (true, text),
            Err(text) => (false, format!("error: {text}")),
        };
        (ok, cap(text))
    }
}

/// `text` cut to [`MAX_TOOL_OUTPUT_CHARS`], with a note for the model when it was. `pub(crate)` since HS8: the
/// loop's own `task` arm caps a subagent's answer with it, like every tool result.
pub(crate) fn cap(text: String) -> String {
    if text.chars().count() <= MAX_TOOL_OUTPUT_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_TOOL_OUTPUT_CHARS).collect();
    cut.push_str(&format!(
        "\n[output cut at {MAX_TOOL_OUTPUT_CHARS} characters; narrow the call to see the rest]"
    ));
    cut
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Echo;

    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "echo".into(),
                description: "Echoes `text`.".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> Access {
            Access::Other
        }

        fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
            Box::pin(async move {
                match arguments.get("text").and_then(Value::as_str) {
                    Some(text) => Ok(text.to_owned()),
                    None => Err("missing text".into()),
                }
            })
        }
    }

    fn toolbox() -> Toolbox {
        let mut toolbox = Toolbox::new();
        toolbox.extend([Box::new(Echo) as Box<dyn Tool>]);
        toolbox
    }

    #[tokio::test]
    async fn a_call_runs_the_named_tool_and_errors_come_back_as_text() {
        let toolbox = toolbox();
        assert_eq!(
            toolbox.call("echo", r#"{"text":"hi"}"#).await,
            (true, "hi".into())
        );
        assert_eq!(
            toolbox.call("echo", "").await,
            (false, "error: missing text".into())
        );
        let (ok, text) = toolbox.call("nope", "{}").await;
        assert!(!ok && text.contains("no tool named"));
        let (ok, text) = toolbox.call("echo", "[1]").await;
        assert!(!ok && text.contains("JSON object"));
        let (ok, text) = toolbox.call("echo", "{oops").await;
        assert!(!ok && text.contains("not valid JSON"));
    }

    #[tokio::test]
    async fn a_huge_result_is_cut_with_a_note() {
        let long = "y".repeat(MAX_TOOL_OUTPUT_CHARS + 10);
        let (_, text) = toolbox()
            .call("echo", &json!({"text": long}).to_string())
            .await;
        assert!(text.starts_with(&"y".repeat(MAX_TOOL_OUTPUT_CHARS)));
        assert!(text.ends_with("narrow the call to see the rest]"));
    }
}
