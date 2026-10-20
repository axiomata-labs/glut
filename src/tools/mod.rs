//! The tools a run offers the model, and the box that holds them.
//!
//! A tool answers with text either way: what it found, or what went wrong. An error is not the end of the run —
//! it goes back to the model, which can correct its call (a wrong path, an ambiguous edit).

pub mod files;
pub(crate) mod outline;
pub(crate) mod output;
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

/// The most characters of one tool result the conversation gets (Glut lean, N2). A bigger result (a huge file, a broad
/// search) would eat the context window in one step: the loop keeps its head and tail and relocates the whole into the
/// run's store (`results::bounded`), where `read_full_result` reads it back. The toolbox itself hands back the whole.
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
    /// What the tool's calls touch in general, for [`Gate::offers`] to decide whether the run shows it to the model
    /// at all (glut-round CP4): by default a call without arguments — its kind of access, with no path or host.
    fn offered_as(&self) -> Access {
        self.access(&Value::Object(Default::default()))
    }
    /// Whether a call with these arguments may run again after the run was interrupted while it ran (durable runs,
    /// `docs/plans/durable.md` D4): by default only what reads — a file below the folder, a page on the web.
    fn replay_safe(&self, arguments: &Value) -> bool {
        matches!(
            self.access(arguments),
            Access::Read { .. } | Access::Network { .. }
        )
    }
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

    /// Takes the tool `name` out of the box (a skill run has no use for the `skill` tool: its body is the task).
    pub fn withdraw(&mut self, name: &str) {
        self.tools.retain(|tool| tool.spec().name != name);
    }

    /// The specs to send to the model: the tools the gate offers (glut-round CP4), every one without a gate.
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.offered().map(|tool| tool.spec()).collect()
    }

    /// [`Self::specs`] split in two (Glut 2, G3): the tools the model calls directly, and the MCP tools a run in code
    /// mode keeps behind `codemode`.
    pub fn split_specs(&self) -> (Vec<ToolSpec>, Vec<ToolSpec>) {
        self.offered()
            .map(|tool| (matches!(tool.offered_as(), Access::Mcp { .. }), tool.spec()))
            .fold(
                (Vec::new(), Vec::new()),
                |(mut direct, mut behind), (mcp, spec)| {
                    if mcp {
                        behind.push(spec);
                    } else {
                        direct.push(spec);
                    }
                    (direct, behind)
                },
            )
    }

    pub fn names(&self) -> Vec<String> {
        self.offered().map(|tool| tool.spec().name).collect()
    }

    fn offered(&self) -> impl Iterator<Item = &dyn Tool> {
        self.tools
            .iter()
            .map(Box::as_ref)
            .filter(|tool| self.offers(*tool))
    }

    fn offers(&self, tool: &dyn Tool) -> bool {
        self.gate
            .as_ref()
            .is_none_or(|gate| gate.offers(&tool.spec().name, &tool.offered_as()))
    }

    /// What a call of `name` with these arguments would touch, when the tool exists and the arguments are an object —
    /// for a step that wants to know beforehand (Glut 2: a snapshot before a change).
    pub fn access_of(&self, name: &str, arguments: &str) -> Option<Access> {
        let tool = self.tools.iter().find(|tool| tool.spec().name == name)?;
        let parsed = serde_json::from_str::<Value>(if arguments.trim().is_empty() {
            "{}"
        } else {
            arguments
        })
        .ok()?;
        parsed.is_object().then(|| tool.access(&parsed))
    }

    /// Whether a call of `name` with these arguments may run again after an interruption ([`Tool::replay_safe`]); an
    /// unknown tool or arguments that are no object may not.
    pub fn replay_safe(&self, name: &str, arguments: &str) -> bool {
        let Some(tool) = self.tools.iter().find(|tool| tool.spec().name == name) else {
            return false;
        };
        let text = if arguments.trim().is_empty() {
            "{}"
        } else {
            arguments
        };
        serde_json::from_str::<Value>(text)
            .is_ok_and(|parsed| parsed.is_object() && tool.replay_safe(&parsed))
    }

    /// Runs the tool `name` with the model's argument text. Returns whether it succeeded and its whole text; the loop
    /// bounds it for the conversation.
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
        // A tool the run does not offer stays refused when the model guesses its name.
        if !self.offers(tool.as_ref()) {
            return (
                false,
                format!("error: not permitted: this run does not offer {name}"),
            );
        }
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
        (ok, text)
    }
}

/// Text from outside (a web page, search results) as the model gets it between a data frame and its `end_mark`: no
/// control characters but line breaks and tabs, none of the invisible ones that hide or reorder text, and no copy of
/// the end mark — a page that writes `[end of page content]` must not be able to close the frame early and speak
/// after it (glut-round CP3, security audit).
pub(crate) fn as_data(text: &str, end_mark: &str) -> String {
    let plain: String = text
        .chars()
        .filter(|c| {
            matches!(c, '\n' | '\t') || !(c.is_control() || crate::permissions::is_invisible(*c))
        })
        .collect();
    plain.replace(end_mark, "[…]")
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

    /// glut-round CP4: a tool the gate does not offer is neither shown to the model nor callable by its name.
    struct Reader;

    impl Tool for Reader {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "reader".into(),
                description: "Reads `path`.".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, arguments: &Value) -> Access {
            Access::Read {
                path: arguments["path"].as_str().unwrap_or_default().to_owned(),
            }
        }

        fn call(&self, _arguments: Value, _context: CallContext) -> ToolFuture<'_> {
            Box::pin(async { Ok(String::new()) })
        }
    }

    /// Durable runs (D4): only what reads runs again after an interruption — an unknown tool or arguments that are no
    /// object never do.
    #[test]
    fn only_a_reading_call_may_run_again() {
        let mut tools = toolbox();
        tools.extend([Box::new(Reader) as Box<dyn Tool>]);
        assert!(tools.replay_safe("reader", r#"{"path": "a.rs"}"#));
        assert!(tools.replay_safe("reader", ""));
        assert!(!tools.replay_safe("reader", "[1]"));
        assert!(!tools.replay_safe("echo", "{}"));
        assert!(!tools.replay_safe("nope", "{}"));
    }

    #[tokio::test]
    async fn a_tool_the_gate_does_not_offer_is_not_there_for_the_model() {
        use crate::permissions::{Mode, Nobody, Rule, RuleSet};
        let narrowed = |declared: &str| {
            // As a skill's declaration does: the same rules allow and narrow.
            let declared = vec![Rule::parse(declared).unwrap()];
            let allow = RuleSet {
                allow: declared.clone(),
                ..RuleSet::default()
            };
            let mut gate = Gate::new(allow, Mode::Default, Arc::new(Nobody));
            gate.narrow("the skill s", declared);
            let mut tools = toolbox();
            tools.set_gate(Arc::new(gate));
            tools
        };
        let reader = narrowed("Read");
        assert!(reader.specs().is_empty() && reader.names().is_empty());
        let (ok, text) = reader.call("echo", r#"{"text":"hi"}"#).await;
        assert!(!ok && text.contains("does not offer echo"), "{text}");
        let named = narrowed("echo");
        assert_eq!(named.names(), ["echo"]);
        assert_eq!(
            named.call("echo", r#"{"text":"hi"}"#).await,
            (true, "hi".into())
        );

        let mut withdrawn = toolbox();
        withdrawn.withdraw("echo");
        assert!(withdrawn.names().is_empty());
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
    async fn the_toolbox_hands_back_a_huge_result_whole() {
        let long = "y".repeat(MAX_TOOL_OUTPUT_CHARS + 10);
        let (_, text) = toolbox()
            .call("echo", &json!({"text": long}).to_string())
            .await;
        assert_eq!(text.chars().count(), MAX_TOOL_OUTPUT_CHARS + 10);
    }
}
