//! The tools a run offers the model, and the box that holds them.
//!
//! A tool answers with text either way: what it found, or what went wrong. An error is not the end of the run —
//! it goes back to the model, which can correct its call (a wrong path, an ambiguous edit).

pub mod editor;
pub mod files;
pub mod glut_files;
pub mod memory;
pub(crate) mod outline;
pub(crate) mod output;
pub mod search;
pub mod shell;
pub mod web;

use std::future::Future;
use std::ops::Range;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

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
    /// Where a tool says more about its answer than the text: a `bash` chain where each part's output lies (Glut lean,
    /// N7).
    pub report: Report,
}

/// One part of a chain's output (Glut lean, N7): the command as the model wrote it and where its output lies in the
/// call's answer, in bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    /// The part's command, as the model wrote it.
    pub command: String,
    /// Where its output lies in the answer, in bytes.
    pub range: Range<usize>,
}

/// What a tool tells the loop beside its answer: shared with the toolbox, filled at most once a call.
#[derive(Debug, Clone, Default)]
pub struct Report(Arc<Mutex<Said>>);

/// What a [`Report`] holds.
#[derive(Debug, Default)]
struct Said {
    parts: Option<Vec<Part>>,
    /// Images the answer shows (T45, `read_file`): the file's bytes, which the loop keeps and the model is sent —
    /// never in the answer's text, which a file could forge.
    images: Vec<Vec<u8>>,
}

impl Report {
    /// Says where the parts of the answer lie.
    pub fn parts(&self, parts: Vec<Part>) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .parts = Some(parts);
    }

    /// An image the answer shows.
    pub fn image(&self, bytes: Vec<u8>) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .images
            .push(bytes);
    }

    fn take(&self) -> (Option<Vec<Part>>, Vec<Vec<u8>>) {
        let mut said = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        (said.parts.take(), std::mem::take(&mut said.images))
    }
}

/// A call's answer as the toolbox hands it to the loop: whether it succeeded, its whole text, and — for a chain —
/// where each part's output lies in that text.
#[derive(Debug)]
pub struct Called {
    /// Whether the call succeeded.
    pub ok: bool,
    /// Its whole answer; an error's starts with `error: `.
    pub text: String,
    /// For a `bash` chain, where the output of each part that ran lies in `text`.
    pub parts: Option<Vec<Part>>,
    /// The images the answer shows (T45), as the files' bytes.
    pub images: Vec<Vec<u8>>,
    /// The owner wrote their own version of the edit (T91): the path and what they changed against the model's.
    pub owners: Option<(String, String)>,
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
    /// Whether the tool is shown to the model now: one that needs something not always there (an editor, T64c) says
    /// no while it is missing.
    fn offered_now(&self) -> bool {
        true
    }
    /// Whether a call with these arguments may run again after the run was interrupted while it ran (durable runs,
    /// D4): by default only what reads — a file below the folder, a page on the web.
    fn replay_safe(&self, arguments: &Value) -> bool {
        matches!(
            self.access(arguments),
            Access::Read { .. } | Access::Network { .. }
        )
    }
    /// A tool that keeps a folder between calls (the shell) starts its next call in the run's folder again.
    fn back_to_top(&self) {}
    /// The folder the tool's next call starts in (the shell's): where the gate reads what a command's glob would find.
    fn here(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// The tools of a run.
#[derive(Default)]
pub struct Toolbox {
    tools: Vec<Arc<dyn Tool>>,
    /// The MCP servers' tools (T56): a register of their own, so a server signed in later joins the session's
    /// toolbox at once and one signed out leaves it — while every turn and subagent holding this box sees the same.
    servers: crate::mcp::Servers,
    /// Judges every call; without one, every call runs (a library caller that checks on its own, the tests).
    gate: Option<Arc<Gate>>,
    /// The folders beside the working one (T60c), shared by the file tools, the shell and the gate.
    folders: crate::folders::Folders,
    /// The Keychain's list and switch (T74), shared by the gate and the shell.
    keychain: crate::keychain::Keychain,
    /// An arena model's confinement (T61), shared with the shell ([`Toolbox::contain`]).
    confinement: crate::sandbox::Confinement,
    /// The editor the file tools ask about its open files (T64b), when Glut is an editor's agent.
    editor: crate::editor::Editor,
}

impl Toolbox {
    pub fn new() -> Self {
        Self::default()
    }

    /// The next shell command starts in the run's folder again, wherever the last one ended: a command of the
    /// screen's own (Glut-TUI's arena check, T61) runs from the top without naming the folder's path, which the gate
    /// would judge as written.
    pub fn back_to_top(&self) {
        for tool in &self.tools {
            tool.back_to_top();
        }
    }

    /// The confinement the shell reads before every command (T61): what is set here holds from its next command.
    pub fn confinement(&self) -> &crate::sandbox::Confinement {
        &self.confinement
    }

    /// Makes this box an arena model's (T61): its gate asks no one, and with a private `home` its commands write their
    /// own folder alone and read nothing of `hidden` ([`crate::sandbox::Confinement`]) — which then lets the gate run
    /// a heavy command that only the folder holds. Without a home, only the asking stops. Never taken back.
    pub fn contain(&self, home: Option<std::path::PathBuf>, hidden: Vec<std::path::PathBuf>) {
        let confined = home.is_some();
        if let Some(home) = home {
            self.confinement.confine(home, hidden);
        }
        if let Some(gate) = &self.gate {
            gate.set_contained(confined);
        }
    }

    /// Puts every call under `gate` (HS3a).
    pub fn set_gate(&mut self, gate: Arc<Gate>) {
        gate.set_beside(self.folders.clone());
        gate.set_keychain(self.keychain.clone());
        self.gate = Some(gate);
        self.tie_here();
    }

    /// Tells the gate which tool's folder its commands run in, once both are there.
    fn tie_here(&self) {
        let (Some(gate), Some(tool)) = (
            &self.gate,
            self.tools.iter().find(|tool| tool.here().is_some()),
        ) else {
            return;
        };
        gate.set_here(Arc::downgrade(tool));
    }

    /// The folders beside the working one (T60c): what is added here, every tool of the box reaches at its next call.
    pub fn folders(&self) -> &crate::folders::Folders {
        &self.folders
    }

    /// The editor the file tools ask about its open files (T64b): set here, every file tool of the box asks it at its
    /// next call.
    pub fn editor(&self) -> &crate::editor::Editor {
        &self.editor
    }

    /// The Keychain's list and switch (T74): what is changed here, the gate and the shell see at their next call.
    pub fn keychain(&self) -> &crate::keychain::Keychain {
        &self.keychain
    }

    /// Puts the Keychain's state under `keychain` — before [`Self::set_gate`] and the shell, which share it.
    pub fn set_keychain(&mut self, keychain: crate::keychain::Keychain) {
        self.keychain = keychain;
    }

    pub fn gate(&self) -> Option<&Arc<Gate>> {
        self.gate.as_ref()
    }

    /// Adds `tools`.
    pub fn extend(&mut self, tools: impl IntoIterator<Item = Box<dyn Tool>>) {
        self.tools.extend(tools.into_iter().map(Arc::from));
        self.tie_here();
    }

    /// The MCP servers whose tools this box offers beside its own (T56).
    pub fn servers(&self) -> &crate::mcp::Servers {
        &self.servers
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

    /// Every tool, the box's own first, then the servers' as they stand now.
    fn all(&self) -> Vec<Arc<dyn Tool>> {
        let mut all = self.tools.clone();
        all.extend(self.servers.tools());
        all
    }

    fn find(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.all().into_iter().find(|tool| tool.spec().name == name)
    }

    fn offered(&self) -> impl Iterator<Item = Arc<dyn Tool>> {
        self.all()
            .into_iter()
            .filter(|tool| self.offers(tool.as_ref()))
    }

    fn offers(&self, tool: &dyn Tool) -> bool {
        tool.offered_now()
            && self
                .gate
                .as_ref()
                .is_none_or(|gate| gate.offers(&tool.spec().name, &tool.offered_as()))
    }

    /// What a call of `name` with these arguments would touch, when the tool exists and the arguments are an object —
    /// for a step that wants to know beforehand (Glut 2: a snapshot before a change).
    pub fn access_of(&self, name: &str, arguments: &str) -> Option<Access> {
        let tool = self.find(name)?;
        arguments_of(tool.as_ref(), arguments)
            .ok()
            .map(|parsed| tool.access(&parsed))
    }

    /// Whether a call of `name` with these arguments may run again after an interruption ([`Tool::replay_safe`]); an
    /// unknown tool or arguments that are no object may not.
    pub fn replay_safe(&self, name: &str, arguments: &str) -> bool {
        self.find(name).is_some_and(|tool| {
            arguments_of(tool.as_ref(), arguments).is_ok_and(|parsed| tool.replay_safe(&parsed))
        })
    }

    /// Runs the tool `name` with the model's argument text. Returns whether it succeeded and its whole text; the loop
    /// bounds it for the conversation.
    pub async fn call(&self, name: &str, arguments: &str) -> (bool, String) {
        self.call_until(name, arguments, &StopFlag::default()).await
    }

    /// [`Self::call`] within a run: refused once `stop` is set, and handed to the tool so a long one can stop too.
    pub async fn call_until(&self, name: &str, arguments: &str, stop: &StopFlag) -> (bool, String) {
        let called = self.call_reporting(name, arguments, stop).await;
        (called.ok, called.text)
    }

    /// [`Self::call_until`] with what the tool reported beside its text (Glut lean, N7).
    pub async fn call_reporting(&self, name: &str, arguments: &str, stop: &StopFlag) -> Called {
        let refused = |text: String| Called {
            ok: false,
            text,
            parts: None,
            images: Vec::new(),
            owners: None,
        };
        if stop.load(Ordering::SeqCst) {
            return refused("error: the run was stopped".into());
        }
        let Some(tool) = self.find(name) else {
            return refused(format!("error: there is no tool named {name:?}"));
        };
        // A tool the run does not offer stays refused when the model guesses its name.
        if !self.offers(tool.as_ref()) {
            // In plan mode a change waits for the owner's yes: the model is told the way there, not only the no.
            let in_plan = self
                .gate
                .as_ref()
                .is_some_and(|gate| gate.mode() == crate::permissions::Mode::Plan);
            if in_plan {
                return refused(format!(
                    "error: not permitted in plan mode: {name} changes nothing until the owner approves a plan — \
                     send your plan with {}, and once approved you can carry it out",
                    crate::planning::EXIT_PLAN
                ));
            }
            return refused(format!(
                "error: not permitted: this run does not offer {name}"
            ));
        }
        let parsed = match arguments_of(tool.as_ref(), arguments) {
            Ok(parsed) => parsed,
            Err(text) => return refused(text),
        };
        // The gate judges the arguments as the tool will read them: after `unquoted`, never before.
        if let Some(gate) = &self.gate
            && let Err(refusal) = gate
                .judge_call(name, &tool.access(&parsed), Some(&parsed))
                .await
        {
            return refused(format!("error: {refusal}"));
        }
        // T91: the owner changed the edit in their editor before allowing it — their text is written, whole, and only
        // over the file the diff showed: as an edit of all of it, which fails where the file changed meanwhile.
        let owners = self
            .gate
            .as_ref()
            .and_then(|gate| gate.take_owners_text(&parsed))
            .zip(
                parsed
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
        let (tool, parsed, owners) = match owners {
            Some((owners, path)) => {
                let changed = crate::planning::changed_lines(&owners.proposed, &owners.text);
                let (name, arguments) = match owners.based_on.filter(|old| !old.is_empty()) {
                    Some(old) => (
                        "edit_file",
                        serde_json::json!({"path": path, "old_string": old, "new_string": owners.text, "whole": true}),
                    ),
                    None => (
                        "write_file",
                        serde_json::json!({"path": path, "content": owners.text}),
                    ),
                };
                match self.find(name).filter(|tool| self.offers(tool.as_ref())) {
                    Some(tool) => (tool, arguments, Some((path, changed))),
                    None => {
                        return refused(format!(
                            "error: the owner's own version of {path} was not written: this run does not offer {name}"
                        ));
                    }
                }
            }
            None => (tool, parsed, None),
        };
        let report = Report::default();
        let context = CallContext {
            stop: Arc::clone(stop),
            gate: self.gate.clone(),
            report: report.clone(),
        };
        const ERROR: &str = "error: ";
        let (ok, text, shift) = match tool.call(parsed, context).await {
            Ok(text) => (true, text, 0),
            Err(text) => (false, format!("{ERROR}{text}"), ERROR.len()),
        };
        // The owner's version not written (the file changed while they looked): said as such, not as the edit's own.
        let (text, owners) = match owners {
            Some((path, _)) if !ok => (
                format!(
                    "{text}\n\nThe owner changed your edit of {path} in their editor, but their version was not \
                     written: the file changed while they looked at it. Read it again."
                ),
                None,
            ),
            owners => (text, owners),
        };
        // The parts lie in the tool's own text; an error's prefix moves them along.
        let (parts, images) = report.take();
        let parts = parts.map(|parts| {
            parts
                .into_iter()
                .map(|part| Part {
                    range: part.range.start + shift..part.range.end + shift,
                    ..part
                })
                .collect()
        });
        Called {
            ok,
            text,
            parts,
            images,
            owners,
        }
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

/// A call's argument text as `tool` reads it — what the gate judges, the snapshot and the replay weigh, and the tool
/// runs with, read one way for all of them (T88).
///
/// # Errors
///
/// The model's text: no JSON, or JSON that is no object.
fn arguments_of(tool: &dyn Tool, arguments: &str) -> Result<Value, String> {
    // Some models send no arguments at all for a tool without required ones.
    let arguments = if arguments.trim().is_empty() {
        "{}"
    } else {
        arguments
    };
    match serde_json::from_str::<Value>(arguments) {
        Ok(value @ Value::Object(_)) => Ok(unquoted(value, &tool.spec().parameters)),
        Ok(_) => Err("error: the arguments must be a JSON object".into()),
        Err(err) => Err(format!("error: the arguments are not valid JSON: {err}")),
    }
}

/// `arguments` with a list or an object the model sent as text holding its JSON read as what the schema asks for
/// there (T88: MiMo wrote `"items": "[…]"`, as other harnesses repair too). A field the schema wants as text, or text
/// that is no such JSON, stays as it came; a wrong field name is never guessed.
pub(crate) fn unquoted(mut arguments: Value, schema: &Value) -> Value {
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return arguments;
    };
    if let Some(fields) = arguments.as_object_mut() {
        for (name, value) in fields.iter_mut() {
            let wanted = properties
                .get(name)
                .and_then(|property| property.get("type"))
                .and_then(Value::as_str);
            let Value::String(text) = value else {
                continue;
            };
            let Ok(inner) = serde_json::from_str::<Value>(text.trim()) else {
                continue;
            };
            if matches!(
                (wanted, &inner),
                (Some("array"), Value::Array(_)) | (Some("object"), Value::Object(_))
            ) {
                *value = inner;
            }
        }
    }
    arguments
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_list_sent_as_text_is_read_where_the_schema_wants_a_list() {
        let schema = json!({"properties": {
            "items": {"type": "array"},
            "options": {"type": "object"},
            "text": {"type": "string"},
        }});
        let sent = json!({
            "items": " [{\"text\": \"a\"}] ",
            "options": "{\"deep\": true}",
            "text": "[1, 2]",
            "other": "[3]",
        });
        assert_eq!(
            unquoted(sent, &schema),
            json!({
                "items": [{"text": "a"}],
                "options": {"deep": true},
                "text": "[1, 2]",
                "other": "[3]",
            }),
            "only where the schema asks for a list or an object"
        );
        let broken = json!({"items": "[{\"text\": "});
        assert_eq!(
            unquoted(broken.clone(), &schema),
            broken,
            "no JSON, no change"
        );
        let wrong = json!({"items": "{\"a\": 1}"});
        assert_eq!(
            unquoted(wrong.clone(), &schema),
            wrong,
            "an object is no list"
        );
    }

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

    /// A tool that would change a file.
    struct Writer;

    impl Tool for Writer {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "writer".into(),
                description: "Writes `path`.".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, arguments: &Value) -> Access {
            Access::Edit {
                path: arguments["path"].as_str().unwrap_or_default().to_owned(),
            }
        }

        fn offered_as(&self) -> Access {
            Access::Edit {
                path: String::new(),
            }
        }

        fn call(&self, _arguments: Value, _context: CallContext) -> ToolFuture<'_> {
            Box::pin(async { Ok(String::new()) })
        }
    }

    #[tokio::test]
    async fn a_change_asked_in_plan_mode_points_the_model_to_its_plan() {
        use crate::permissions::{Mode, Nobody, RuleSet};
        let mut tools = toolbox();
        tools.extend([Box::new(Writer) as Box<dyn Tool>]);
        tools.set_gate(Arc::new(Gate::new(
            RuleSet::default(),
            Mode::Plan,
            Arc::new(Nobody),
        )));
        let (ok, text) = tools.call("writer", r#"{"path": "a.rs"}"#).await;
        assert!(
            !ok && text.contains("plan mode") && text.contains(crate::planning::EXIT_PLAN),
            "{text}"
        );
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
