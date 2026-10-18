//! Hooks (HS7, `docs/plans/harness-hs7.md`): Claude Code's four events — `SessionStart`, `PreToolUse`,
//! `PostToolUse`, `Stop` — run from the owner's `~/.axh/settings.json` around the loop's own steps.
//!
//! What a hook may do is a veto (B2): it can block a call or the answer, and `SessionStart` can add context;
//! it never lets a call through that the gate would refuse. A hook that crashes, exits with anything but 0/2
//! or runs into its timeout is a note and the run goes on (B3). A project's hooks are parsed but never run
//! (B1) — that happens in `permissions::settings::load`, which keeps only the owner's.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::event::{EVENT_RESULT_CHARS, Event, EventSink};
use crate::permissions::Gate;
use crate::tools::StopFlag;
use crate::tools::shell::{HookOutput, Session};

/// How long a hook may run when its entry names no timeout (Claude's default).
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// The `hooks` section of one settings file, as written: one list per event, in Claude's shape. Unknown event
/// names are kept but never run — a Claude update must not brick a settings file (D1).
#[derive(Debug, Default, Clone, Deserialize)]
pub struct HooksSection(pub std::collections::HashMap<String, Vec<Entry>>);

/// One matcher with the commands under it.
#[derive(Debug, Clone, Deserialize)]
pub struct Entry {
    #[serde(default)]
    pub matcher: Option<String>,
    #[serde(default)]
    pub hooks: Vec<CommandEntry>,
}

/// One hook command as written. Fields we do not know are ignored (forward compatible, D1); `type` must be
/// `command` and `command` must be there.
#[derive(Debug, Clone, Deserialize)]
pub struct CommandEntry {
    #[serde(rename = "type")]
    pub kind: String,
    pub command: String,
    #[serde(default)]
    pub timeout: Option<u64>,
}

/// The four hook events the harness runs (Claude's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    PreToolUse,
    PostToolUse,
    Stop,
}

impl HookEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::SessionStart => "SessionStart",
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::Stop => "Stop",
        }
    }
}

/// One hook command, compiled: its owner text beside its time limit.
#[derive(Debug, Clone)]
struct Command {
    command: String,
    timeout: Duration,
}

/// One matcher and its commands: an unanchored regex like Claude's, `None` for every target.
#[derive(Debug, Clone)]
struct List {
    matcher: Option<regex::Regex>,
    commands: Vec<Command>,
}

impl List {
    /// Whether this list fires for any of `names` (the tool's own name and its Claude alias, D2).
    fn matches(&self, names: &[&str]) -> bool {
        self.matcher
            .as_ref()
            .is_none_or(|regex| names.iter().any(|name| regex.is_match(name)))
    }
}

/// The owner's hooks, by event, compiled once at load. Empty: the run has none, and nothing is built for
/// them (no session, no events).
#[derive(Debug, Clone, Default)]
pub struct Hooks {
    session_start: Vec<List>,
    pre_tool_use: Vec<List>,
    post_tool_use: Vec<List>,
    stop: Vec<List>,
}

impl Hooks {
    /// Compiles the `hooks` section of a settings file (D1): `type` must be `command` and every matcher a
    /// regex the engine can build — a broken hooks section stops the run like a broken deny list (P4); the
    /// caller adds the file's name to the reason.
    ///
    /// # Errors
    ///
    /// A reason naming the entry that does not parse.
    pub(crate) fn compile(section: &HooksSection) -> Result<Self, String> {
        let known = [
            ("SessionStart", HookEvent::SessionStart),
            ("PreToolUse", HookEvent::PreToolUse),
            ("PostToolUse", HookEvent::PostToolUse),
            ("Stop", HookEvent::Stop),
        ];
        let compile_list = |entries: &[Entry]| -> Result<Vec<List>, String> {
            entries
                .iter()
                .map(|entry| {
                    let matcher = match &entry.matcher {
                        Some(text) if !text.trim().is_empty() => Some(
                            regex::Regex::new(text)
                                .map_err(|err| format!("matcher {text:?} is no regex: {err}"))?,
                        ),
                        _ => None,
                    };
                    let commands = entry
                        .hooks
                        .iter()
                        .map(|hook| {
                            if hook.kind != "command" {
                                return Err(format!(
                                    "unknown hook type {:?}; only \"command\" runs here",
                                    hook.kind
                                ));
                            }
                            if hook.command.trim().is_empty() {
                                return Err("a hook command is empty".to_owned());
                            }
                            Ok(Command {
                                command: hook.command.clone(),
                                timeout: hook.timeout.map_or(DEFAULT_TIMEOUT, |secs| {
                                    Duration::from_secs(secs.max(1))
                                }),
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Ok(List { matcher, commands })
                })
                .collect()
        };
        let mut hooks = Hooks::default();
        for (name, event) in known {
            let Some(entries) = section.0.get(name) else {
                continue;
            };
            let lists = compile_list(entries)?;
            let into = match event {
                HookEvent::SessionStart => &mut hooks.session_start,
                HookEvent::PreToolUse => &mut hooks.pre_tool_use,
                HookEvent::PostToolUse => &mut hooks.post_tool_use,
                HookEvent::Stop => &mut hooks.stop,
            };
            *into = lists;
        }
        Ok(hooks)
    }

    /// Whether there is anything to run at all.
    pub fn is_empty(&self) -> bool {
        self.session_start.is_empty()
            && self.pre_tool_use.is_empty()
            && self.post_tool_use.is_empty()
            && self.stop.is_empty()
    }

    fn for_event(&self, event: HookEvent) -> &[List] {
        match event {
            HookEvent::SessionStart => &self.session_start,
            HookEvent::PreToolUse => &self.pre_tool_use,
            HookEvent::PostToolUse => &self.post_tool_use,
            HookEvent::Stop => &self.stop,
        }
    }
}

/// The Claude Code name of one of our tools, for a hook's matcher (D2): to Claude `bash` is `Bash` and our
/// `read_file` is `Read`. MCP tools already carry Claude's `mcp__server__tool` spelling and need none.
pub(crate) fn claude_alias(tool: &str) -> Option<&'static str> {
    match tool {
        "bash" => Some("Bash"),
        "read_file" => Some("Read"),
        "write_file" => Some("Write"),
        "edit_file" => Some("Edit"),
        "list_dir" => Some("LS"),
        "grep" => Some("Grep"),
        "glob" => Some("Glob"),
        // HS8, D9: our subagent tool is Claude's `Task`, matcher and hook payload alike.
        "task" => Some("Task"),
        _ => None,
    }
}

/// The stdin payload of one hook call (D4): Claude's fields with ours in it — `session_id` (there are no
/// sessions yet), `transcript_path` (there is no transcript), and the event's own fields beside them.
pub(crate) fn payload(session_id: &str, cwd: &Path, event: HookEvent, extra: Value) -> Value {
    let mut payload = json!({
        "session_id": session_id,
        "transcript_path": "",
        "cwd": cwd.display().to_string(),
        "hook_event_name": event.as_str(),
    });
    if let Value::Object(extra) = extra
        && let Some(fields) = payload.as_object_mut()
    {
        fields.extend(extra);
    }
    payload
}

/// The model's argument text as a payload's `tool_input`; text that is no JSON object stays no object (the
/// call it belongs to fails at the tools anyway).
pub(crate) fn tool_input(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or(Value::Null)
}

/// What one event's hooks decided (B2: block or context, nothing else).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HookOutcome {
    /// A hook blocked: nothing runs (or the model's result is replaced), the reason is in `context`.
    pub blocked: bool,
    /// `SessionStart` context, or the block reason.
    pub context: Option<String>,
}

/// Runs the hooks of one event in configured order (D5): the first block ends the rest of the event, every
/// hook sees the same payload on stdin, and every failure is a note — nothing here ever fails the run (B3).
/// `limit` is the time the run has left; a hook's own timeout counts against it, whichever is shorter.
// One parameter per thing a hook call needs; a context struct would only hide them.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_event(
    turn: Option<u32>,
    event: HookEvent,
    target: &str,
    payload: &Value,
    hooks: &Hooks,
    session: &Session,
    gate: Option<std::sync::Arc<Gate>>,
    stop: &StopFlag,
    limit: Duration,
    sink: &mut dyn EventSink,
) -> HookOutcome {
    let mut outcome = HookOutcome::default();
    let lists = hooks.for_event(event);
    let mut names = vec![target];
    if let Some(alias) = claude_alias(target) {
        names.push(alias);
    }
    let payload = serde_json::to_string(payload).unwrap_or_default();
    for list in lists {
        // Stop has no matcher (D2); everything else fires where the regex finds the tool name or its alias.
        if event != HookEvent::Stop && !list.matches(&names) {
            continue;
        }
        for command in &list.commands {
            let timeout = command.timeout.min(limit);
            let (note, text, blocked) = match session
                .run_hook(&command.command, &payload, timeout, stop, gate.clone())
                .await
            {
                Err(reason) => (Some(reason), None, false),
                Ok(output) if output.timed_out => (
                    Some(format!("timed out after {} s", timeout.as_secs())),
                    None,
                    false,
                ),
                Ok(output) => answer_of(event, &output),
            };
            let mut note = note;
            if let Some(text) = text {
                // SessionStart collects the context of every hook; a block takes the first reason (and ends
                // the event right after).
                outcome.context = Some(match outcome.context.take() {
                    Some(have) if !blocked => format!("{have}\n{text}"),
                    _ => text,
                });
            }
            outcome.blocked |= blocked;
            // A block leaves its reason behind as the note too (D6): it is what a record shows — the core's
            // `Notes` collector folds it into the run's `stderr`.
            if blocked && note.is_none() {
                note = outcome
                    .context
                    .as_deref()
                    .map(|text| text.chars().take(EVENT_RESULT_CHARS).collect());
            }
            sink.emit(Event::Hook {
                turn,
                hook_event: event.as_str().to_owned(),
                command: command.command.chars().take(EVENT_RESULT_CHARS).collect(),
                blocked,
                context_chars: outcome
                    .context
                    .as_deref()
                    .map_or(0, |text| text.chars().count()),
                note,
            });
            if blocked {
                return outcome;
            }
        }
    }
    outcome
}

/// What one hook answered (Claude's channels, D3): its note when it is no answer at all, the text it leaves
/// behind — `SessionStart` context or a block reason — and whether it blocks. Only an explicit block blocks
/// (B3): `allow` and `ask` parse fine and then mean nothing here (B2), and anything unexpected is a note.
fn answer_of(event: HookEvent, output: &HookOutput) -> (Option<String>, Option<String>, bool) {
    let parsed = answer(&output.stdout);
    if event == HookEvent::SessionStart {
        // Never a block: context from Claude's three channels — the JSON field, plain stdout, exit 2's
        // stderr. Anything else is a note, and the run answers normally.
        if let Some(text) = parsed
            .specific
            .context
            .filter(|text| !text.trim().is_empty())
        {
            return (None, Some(text), false);
        }
        if output.code == Some(0) && !output.stdout.trim().is_empty() {
            return (None, Some(output.stdout.trim().to_owned()), false);
        }
        if output.code == Some(2) && !output.stderr.trim().is_empty() {
            return (None, Some(output.stderr.trim().to_owned()), false);
        }
        let note = (!(output.code.is_some_and(|code| code == 0)))
            .then(|| format!("ended with {}; nothing to add", describe(output)));
        return (note, None, false);
    }
    if output.code == Some(2) {
        // Exit 2: stderr is the block reason (empty stderr still blocks — the exit code is the answer).
        let reason = output.stderr.trim();
        return (
            None,
            Some(if reason.is_empty() {
                "a hook exited with 2".to_owned()
            } else {
                reason.to_owned()
            }),
            true,
        );
    }
    if let Some(reason) = parsed.block_reason() {
        return (None, Some(reason), true);
    }
    let note = (!(output.code.is_some_and(|code| code == 0)))
        .then(|| format!("ended with {}; its answer was left out", describe(output)));
    (note, None, false)
}

/// How a hook ended, for a note.
fn describe(output: &HookOutput) -> String {
    match output.code {
        Some(code) => format!("exit code {code}"),
        None => "a signal".to_owned(),
    }
}

/// What a hook printed on stdout, read the way Claude reads it: `hookSpecificOutput.permissionDecision`
/// first, the older top-level `decision`/`reason` beside it. Anything that is no such JSON is no answer.
#[derive(Default, Deserialize)]
struct Answer {
    #[serde(default, rename = "hookSpecificOutput")]
    specific: Specific,
    #[serde(default, rename = "decision")]
    decision: Option<String>,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Default, Deserialize)]
struct Specific {
    #[serde(default, rename = "permissionDecision")]
    permission: Option<String>,
    #[serde(default, rename = "permissionDecisionReason")]
    permission_reason: Option<String>,
    #[serde(default, rename = "additionalContext")]
    context: Option<String>,
}

impl Answer {
    /// The block reason when the answer blocks; `allow` and `ask` are no-ops (B2) and mean nothing here.
    fn block_reason(&self) -> Option<String> {
        let denies = self.specific.permission.as_deref() == Some("deny")
            || matches!(self.decision.as_deref(), Some("block"));
        denies.then(|| {
            self.specific
                .permission_reason
                .clone()
                .or_else(|| self.reason.clone())
                .unwrap_or_else(|| "a hook refused this".to_owned())
        })
    }
}

/// A hook's stdout as its JSON answer, when it is one.
fn answer(stdout: &str) -> Answer {
    serde_json::from_str(stdout).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::tools::shell::ShellOptions;

    /// A session for a scratch folder, without a profile, over the test process's own PATH and HOME.
    fn session(tag: &str) -> (PathBuf, Session) {
        let dir = std::env::temp_dir().join(format!(
            "axh-hooks-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let ambient: Vec<(std::ffi::OsString, std::ffi::OsString)> = std::env::vars_os()
            .filter(|(key, _)| key == "PATH" || key == "HOME")
            .collect();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/bash"),
            profile: false,
            ambient,
            sandbox: None,
        };
        let session = Session::new(&dir, options).unwrap();
        (dir, session)
    }

    /// Hooks straight from a Claude-shaped section, as a settings file would compile them.
    fn hooks(events: Value) -> Hooks {
        let section: HooksSection = serde_json::from_value(events).unwrap();
        Hooks::compile(&section).unwrap()
    }

    /// One `command` hook for `event`, with the matcher given.
    fn one(event: &str, matcher: Option<&str>, command: &str) -> Value {
        json!({(event): [{"matcher": matcher, "hooks": [{"type": "command", "command": command}]}]})
    }

    async fn fire(
        event: HookEvent,
        target: &str,
        hooks: &Hooks,
        session: &Session,
        extra: Value,
    ) -> (HookOutcome, Vec<Event>) {
        let payload = payload("axh-test", Path::new("/w"), event, extra);
        let mut events = Vec::new();
        let outcome = run_event(
            Some(1),
            event,
            target,
            &payload,
            hooks,
            session,
            None,
            &StopFlag::default(),
            Duration::from_secs(10),
            &mut events,
        )
        .await;
        (outcome, events)
    }

    #[test]
    fn a_claude_shaped_section_parses_and_compiles_and_a_broken_one_stops_the_run() {
        let section: HooksSection = serde_json::from_value(json!({
            "SessionStart": [{"hooks": [{"type": "command", "command": "true"}]}],
            "PreToolUse": [
                {"matcher": "Bash|Write", "hooks": [{"type": "command", "command": "true", "timeout": 30}]},
                {"hooks": [{"type": "command", "command": "true"}]}
            ],
            "PostToolUse": [{"matcher": "Read", "hooks": [{"type": "command", "command": "true"}]}],
            "Stop": [{"hooks": [{"type": "command", "command": "true"}]}],
            "SomeFutureEvent": [{"hooks": [{"type": "command", "command": "true"}]}]
        }))
        .unwrap();
        let hooks = Hooks::compile(&section).unwrap();
        assert!(!hooks.is_empty(), "an unknown event is ignored, not fatal");
        assert!(Hooks::default().is_empty());

        // An unknown type and an unparsable matcher are settings errors (D1), naming the entry.
        for bad in [
            json!({"PreToolUse": [{"hooks": [{"type": "prompt", "command": "true"}]}]}),
            json!({"PreToolUse": [{"matcher": "([", "hooks": [{"type": "command", "command": "true"}]}]}),
            json!({"PreToolUse": [{"hooks": [{"type": "command", "command": "  "}]}]}),
        ] {
            let section: HooksSection = serde_json::from_value(bad.clone()).unwrap();
            assert!(Hooks::compile(&section).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_matcher_fires_on_the_tool_name_and_its_claude_alias() {
        assert_eq!(claude_alias("bash"), Some("Bash"));
        assert_eq!(claude_alias("read_file"), Some("Read"));
        assert_eq!(claude_alias("list_dir"), Some("LS"));
        assert_eq!(claude_alias("mcp__mail__send"), None);
        // HS8, D9: the subagent tool matches Claude's own name in a hook's matcher.
        assert_eq!(claude_alias("task"), Some("Task"));
        let hits = hooks(one("PreToolUse", Some("Bash"), "true"));
        assert!(!hits.for_event(HookEvent::PreToolUse)[0].matches(&["read_file", "Read"]));
        assert!(hits.for_event(HookEvent::PreToolUse)[0].matches(&["bash", "Bash"]));
        let every = hooks(one("PreToolUse", None, "true"));
        assert!(every.for_event(HookEvent::PreToolUse)[0].matches(&["anything"]));
    }

    #[tokio::test]
    async fn a_deny_blocks_with_its_reason_and_allow_changes_nothing() {
        let (_dir, session) = session("deny");
        let deny = hooks(one(
            "PreToolUse",
            Some("Bash"),
            r#"echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"not today"}}' "#,
        ));
        let (outcome, events) = fire(
            HookEvent::PreToolUse,
            "bash",
            &deny,
            &session,
            json!({"tool_name": "bash", "tool_input": {}}),
        )
        .await;
        assert!(outcome.blocked);
        assert_eq!(outcome.context.as_deref(), Some("not today"));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Hook { blocked: true, .. }))
        );

        // B2: `allow` is a no-op — it never carries a call past the gate, so it changes nothing here either.
        let allow = hooks(one(
            "PreToolUse",
            None,
            r#"echo '{"hookSpecificOutput":{"permissionDecision":"allow","permissionDecisionReason":"let it pass"}}' "#,
        ));
        let (outcome, events) = fire(
            HookEvent::PreToolUse,
            "bash",
            &allow,
            &session,
            json!({"tool_name": "bash", "tool_input": {}}),
        )
        .await;
        assert!(!outcome.blocked);
        assert_eq!(outcome.context, None);
        assert!(
            events
                .iter()
                .all(|e| !matches!(e, Event::Hook { blocked: true, .. }))
        );

        // A hook that does not match the tool never fires.
        let elsewhere = hooks(one(
            "PreToolUse",
            Some("Write"),
            r#"echo '{"decision":"block","reason":"no"}'"#,
        ));
        let (outcome, _) = fire(
            HookEvent::PreToolUse,
            "bash",
            &elsewhere,
            &session,
            json!({}),
        )
        .await;
        assert!(!outcome.blocked);
    }

    #[tokio::test]
    async fn exit_2_blocks_with_stderr_and_session_start_only_ever_adds_context() {
        let (_dir, session) = session("exit2");
        let block = hooks(one("PreToolUse", None, r#"echo "no way" >&2; exit 2"#));
        let (outcome, _) = fire(HookEvent::PreToolUse, "bash", &block, &session, json!({})).await;
        assert!(outcome.blocked);
        assert_eq!(outcome.context.as_deref(), Some("no way"));

        // The same exit code is context for SessionStart, never a block (D3).
        let start = hooks(one("SessionStart", None, r#"echo "morning" >&2; exit 2"#));
        let (outcome, _) = fire(
            HookEvent::SessionStart,
            "startup",
            &start,
            &session,
            json!({"source": "startup"}),
        )
        .await;
        assert!(!outcome.blocked);
        assert_eq!(outcome.context.as_deref(), Some("morning"));
    }

    #[tokio::test]
    async fn session_start_takes_context_from_the_json_field_plain_stdout_and_notes_the_rest() {
        let (_dir, session) = session("start");
        let json_context = hooks(one(
            "SessionStart",
            None,
            r#"echo '{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"from json"}}'"#,
        ));
        let (outcome, _) = fire(
            HookEvent::SessionStart,
            "startup",
            &json_context,
            &session,
            json!({"source": "startup"}),
        )
        .await;
        assert_eq!(outcome.context.as_deref(), Some("from json"));

        let plain = hooks(one("SessionStart", None, r#"echo "plain text""#));
        let (outcome, _) = fire(
            HookEvent::SessionStart,
            "startup",
            &plain,
            &session,
            json!({"source": "startup"}),
        )
        .await;
        assert_eq!(outcome.context.as_deref(), Some("plain text"));

        // B3: a hook that ends some other way is a note, and the run goes on.
        let broken = hooks(one("SessionStart", None, "kill -9 $$"));
        let (outcome, events) = fire(
            HookEvent::SessionStart,
            "startup",
            &broken,
            &session,
            json!({"source": "startup"}),
        )
        .await;
        assert!(!outcome.blocked && outcome.context.is_none());
        assert!(
            events.iter().any(
                |e| matches!(e, Event::Hook { note: Some(note), .. } if note.contains("signal"))
            )
        );
    }

    #[tokio::test]
    async fn a_broken_hook_is_a_note_and_the_run_goes_on_and_the_first_block_ends_the_event() {
        let (_dir, session) = session("broken");
        // A hook running into its timeout: a note, not a block, and the group is gone with it.
        let slow = hooks(one("PreToolUse", None, "sleep 30"));
        let stdin = payload(
            "axh-test",
            Path::new("/w"),
            HookEvent::PreToolUse,
            json!({}),
        );
        let mut events = Vec::new();
        let started = std::time::Instant::now();
        let outcome = run_event(
            Some(1),
            HookEvent::PreToolUse,
            "bash",
            &stdin,
            &slow,
            &session,
            None,
            &StopFlag::default(),
            Duration::from_secs(1),
            &mut events,
        )
        .await;
        assert!(!outcome.blocked);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(events.iter().any(
            |e| matches!(e, Event::Hook { note: Some(note), .. } if note.contains("timed out"))
        ));

        // The first block short-circuits the rest of the event: the second hook never runs.
        let two = hooks(json!({"PreToolUse": [
            {"hooks": [{"type": "command", "command": r#"echo '{"decision":"block","reason":"first"}'"#}]},
            {"hooks": [{"type": "command", "command": "echo second"}]}
        ]}));
        let mut events = Vec::new();
        let stdin = payload(
            "axh-test",
            Path::new("/w"),
            HookEvent::PreToolUse,
            json!({}),
        );
        let outcome = run_event(
            Some(1),
            HookEvent::PreToolUse,
            "bash",
            &stdin,
            &two,
            &session,
            None,
            &StopFlag::default(),
            Duration::from_secs(10),
            &mut events,
        )
        .await;
        assert_eq!(
            (outcome.blocked, outcome.context.as_deref()),
            (true, Some("first"))
        );
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Hook { .. }))
                .count(),
            1,
            "the second hook never ran"
        );
    }

    #[tokio::test]
    async fn stop_hooks_run_whatever_the_matcher_says() {
        let (_dir, session) = session("stop");
        let stubborn = hooks(one(
            "Stop",
            Some("NothingMatchesThis"),
            "echo stop it >&2; exit 2",
        ));
        let (outcome, _) = fire(
            HookEvent::Stop,
            "",
            &stubborn,
            &session,
            json!({"stop_hook_active": false}),
        )
        .await;
        assert!(outcome.blocked, "Stop has no matcher (D2)");
    }

    #[test]
    fn the_payload_carries_claudes_fields_with_ours_in_it() {
        let value = payload(
            "axh-1-2",
            Path::new("/w"),
            HookEvent::PreToolUse,
            json!({"tool_name": "bash", "tool_input": {"command": "ls"}}),
        );
        assert_eq!(value["hook_event_name"], "PreToolUse");
        assert_eq!(value["session_id"], "axh-1-2");
        assert_eq!(value["transcript_path"], "");
        assert_eq!(value["cwd"], "/w");
        assert_eq!(value["tool_input"]["command"], "ls");
        // Text that is no JSON object stays no object.
        assert_eq!(tool_input("not json"), Value::Null);
        assert_eq!(tool_input(r#"{"a":1}"#)["a"], 1);
    }
}
