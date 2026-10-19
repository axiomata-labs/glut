//! What a run reports, step by step: the contract between the harness and whatever shows it — a log, the `axh`
//! binary's JSON lines, later a TUI, the Studio or an app of its own.

use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::model::Usage;

/// How much of a tool's result an event carries. The model gets the whole (already capped) result; an event is for
/// showing what happened, and a viewer can always read the file itself.
pub const EVENT_RESULT_CHARS: usize = 2_000;

/// Why a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model answered without asking for a tool.
    Answered,
    /// The turn limit was reached.
    MaxTurns,
    /// The run's time limit passed.
    Timeout,
    /// The caller called the run off.
    Cancelled,
    /// The model API failed; the `error` event before says how.
    Failed,
}

/// Whether an MCP server of the run came up (HS5, M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum McpStatus {
    /// Handshake and tool list done: its tools are offered to the model.
    Connected,
    /// It could not be started or did not speak the protocol: left out, the run goes on without it.
    Failed,
}

/// Whether a skill became a run or was loaded while one went on (HS6, K5/K1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillHow {
    /// The run started with the skill: its body is the run's instructions (K1.1).
    Run,
    /// The model called the `skill` tool and got the skill's body back (K1.2).
    Loaded,
}

/// One step of a run.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Started {
        model: String,
        cwd: String,
        tools: Vec<String>,
    },
    /// A slash command became the run's prompt (HS4): its name, where the command came from and how many characters
    /// the prompt it became holds. For a built-in, which is answered without a model, `prompt_chars` is `0` — its
    /// text never becomes a prompt.
    Command {
        name: String,
        source: String,
        prompt_chars: usize,
    },
    /// A piece of the text the model is writing, as it streams in (glut-round CP5); the whole text follows as
    /// [`Event::AssistantText`] once the call is done, so a sink that does not show the answer growing ignores these.
    TextDelta {
        turn: u32,
        text: String,
    },
    /// Text the model wrote, its final answer or a remark beside tool calls.
    AssistantText {
        turn: u32,
        text: String,
    },
    ToolCall {
        turn: u32,
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        turn: u32,
        id: String,
        name: String,
        ok: bool,
        output: String,
        truncated: bool,
    },
    /// Tokens of one model call.
    Usage {
        turn: u32,
        input_tokens: u64,
        output_tokens: u64,
    },
    /// How a tool call was judged (P8): allowed or refused, and why — a rule, the mode, the owner's answer.
    Permission {
        turn: u32,
        id: String,
        tool: String,
        target: String,
        allowed: bool,
        reason: String,
    },
    /// An MCP server of this run came up or failed to (HS5). `tools` is how many tools it offered; `reason` is
    /// why it failed, on a failure.
    McpServer {
        name: String,
        status: McpStatus,
        tools: usize,
        reason: Option<String>,
    },
    /// A skill was used (HS6): as the run itself (K1.1) or loaded by the model through the `skill` tool (K1.2).
    Skill {
        name: String,
        how: SkillHow,
    },
    /// A hook of this run ran (HS7): which of Claude's four events it belongs to, its command (cut), whether it
    /// blocked, how much context it left behind, and — for a failure (B3) or a block — the note saying why.
    /// `turn` is `None` for `SessionStart`, which runs before the first model turn.
    Hook {
        turn: Option<u32>,
        hook_event: String,
        command: String,
        blocked: bool,
        context_chars: usize,
        note: Option<String>,
    },
    /// One event of a nested run (HS8, D8): everything a subagent reports arrives wrapped in its depth —
    /// nothing dropped, the parent's own `ToolCall(task)` … `ToolResult(task)` bracketing the whole stream.
    /// The inner event serialises as `inner`, because the outer tag already owns the field name `event`.
    Sub {
        depth: u32,
        #[serde(rename = "inner")]
        event: Box<Event>,
    },
    Error {
        message: String,
    },
    Finished {
        reason: FinishReason,
        turns: u32,
        usage: Usage,
    },
}

impl Event {
    /// A judgement of the run's gate as an event; the target cut like a tool call's arguments.
    pub fn permission(turn: u32, id: &str, judged: &crate::permissions::Judgement) -> Self {
        Event::Permission {
            turn,
            id: id.to_owned(),
            tool: judged.tool.clone(),
            target: judged.target.chars().take(EVENT_RESULT_CHARS).collect(),
            allowed: judged.allowed,
            reason: judged.reason.clone(),
        }
    }

    /// A tool call for an event: its arguments cut to [`EVENT_RESULT_CHARS`] (a whole file's content in a
    /// `write_file` call is for the tool, not for the event).
    pub fn tool_call(turn: u32, id: &str, name: &str, arguments: &str) -> Self {
        Event::ToolCall {
            turn,
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.chars().take(EVENT_RESULT_CHARS).collect(),
        }
    }

    /// A tool result for an event: cut to [`EVENT_RESULT_CHARS`].
    pub fn tool_result(turn: u32, id: &str, name: &str, ok: bool, output: &str) -> Self {
        let truncated = output.chars().count() > EVENT_RESULT_CHARS;
        Event::ToolResult {
            turn,
            id: id.to_owned(),
            name: name.to_owned(),
            ok,
            output: output.chars().take(EVENT_RESULT_CHARS).collect(),
            truncated,
        }
    }
}

/// Where a run's events go.
pub trait EventSink: Send {
    fn emit(&mut self, event: Event);
}

/// A sink that keeps every event — for tests and for a caller that reads them afterwards.
impl EventSink for Vec<Event> {
    fn emit(&mut self, event: Event) {
        self.push(event);
    }
}

/// One sink behind shared handles (HS6): the run loop holds one, and a tool that reports from inside a call — the
/// `skill` tool — holds a clone. Their lines reach the same place, in the order they were sent.
///
/// The inner sink sits behind a [`std::sync::Mutex`], so every send is one whole event's write (a JSON line's, a
/// human-readable line's), never half of two.
#[derive(Clone)]
pub struct SharedSink(Arc<Mutex<Box<dyn EventSink>>>);

impl SharedSink {
    /// Wraps `sink` so more than one place can send to it.
    pub fn new(sink: Box<dyn EventSink>) -> Self {
        SharedSink(Arc::new(Mutex::new(sink)))
    }

    /// An event through the shared sink — takes a shared handle, so a tool with only `&self` can report too. A sink
    /// that panicked while writing leaves the lock poisoned; the events behind it are lost either way, so the next
    /// send carries on rather than taking the run down with it.
    pub fn send(&self, event: Event) {
        let mut sink = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        sink.emit(event);
    }
}

impl EventSink for SharedSink {
    fn emit(&mut self, event: Event) {
        self.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_serialise_as_tagged_json_lines() {
        let line = serde_json::to_value(Event::ToolCall {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
        })
        .unwrap();
        assert_eq!(
            line,
            json!({"event": "tool_call", "turn": 1, "id": "c1", "name": "read_file", "arguments": "{}"})
        );
        let done = serde_json::to_value(Event::Finished {
            reason: FinishReason::MaxTurns,
            turns: 3,
            usage: Usage {
                input_tokens: 5,
                cached_input_tokens: 0,
                output_tokens: 2,
            },
        })
        .unwrap();
        assert_eq!(done["reason"], "max_turns");
        assert_eq!(done["usage"]["input_tokens"], 5);
    }

    #[test]
    fn a_command_event_is_a_tagged_line_like_the_others() {
        let line = serde_json::to_value(Event::Command {
            name: "review".into(),
            source: "user".into(),
            prompt_chars: 12,
        })
        .unwrap();
        assert_eq!(
            line,
            json!({"event": "command", "name": "review", "source": "user", "prompt_chars": 12})
        );
    }

    #[test]
    fn an_mcp_server_event_is_a_tagged_line_like_the_others() {
        let line = serde_json::to_value(Event::McpServer {
            name: "mail".into(),
            status: McpStatus::Connected,
            tools: 27,
            reason: None,
        })
        .unwrap();
        assert_eq!(
            line,
            json!({"event": "mcp_server", "name": "mail", "status": "connected", "tools": 27, "reason": null})
        );
        let failed = serde_json::to_value(Event::McpServer {
            name: "broken".into(),
            status: McpStatus::Failed,
            tools: 0,
            reason: Some("it never answered".into()),
        })
        .unwrap();
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["reason"], "it never answered");
    }

    #[test]
    fn a_skill_event_is_a_tagged_line_like_the_others() {
        let line = serde_json::to_value(Event::Skill {
            name: "demo".into(),
            how: SkillHow::Run,
        })
        .unwrap();
        // Tagged `event` like every event; the `{"type": …}` in the HS6 plan was the slip of the pen the HS4 and
        // HS5 plans made too (`docs/plans/harness.md`, "HS5 — what exists").
        assert_eq!(
            line,
            json!({"event": "skill", "name": "demo", "how": "run"})
        );
        let loaded = serde_json::to_value(Event::Skill {
            name: "demo".into(),
            how: SkillHow::Loaded,
        })
        .unwrap();
        assert_eq!(loaded["how"], "loaded");
    }

    #[test]
    fn a_hook_event_is_a_tagged_line_like_the_others() {
        let line = serde_json::to_value(Event::Hook {
            turn: Some(2),
            hook_event: "PreToolUse".into(),
            command: "echo hi".into(),
            blocked: true,
            context_chars: 9,
            note: Some("not today".into()),
        })
        .unwrap();
        assert_eq!(
            line,
            json!({"event": "hook", "turn": 2, "hook_event": "PreToolUse", "command": "echo hi",
                   "blocked": true, "context_chars": 9, "note": "not today"})
        );
        // SessionStart runs before the first turn and carries no one.
        let start = serde_json::to_value(Event::Hook {
            turn: None,
            hook_event: "SessionStart".into(),
            command: "true".into(),
            blocked: false,
            context_chars: 5,
            note: None,
        })
        .unwrap();
        assert_eq!(start["turn"], serde_json::Value::Null);
    }

    #[test]
    fn a_sub_event_holds_its_inner_event_under_inner() {
        let line = serde_json::to_value(Event::Sub {
            depth: 1,
            event: Box::new(Event::AssistantText {
                turn: 1,
                text: "found".into(),
            }),
        })
        .unwrap();
        // Tagged `event` like every event; the nested one arrives as `inner` — one field may not claim the
        // tag's name twice.
        assert_eq!(line["event"], "sub");
        assert_eq!(line["depth"], 1);
        assert_eq!(line["inner"]["event"], "assistant_text");
        assert_eq!(line["inner"]["text"], "found");
    }

    #[test]
    fn one_shared_sink_reaches_the_sink_of_every_handle() {
        let kept = Arc::new(Mutex::new(Vec::new()));
        #[derive(Clone)]
        struct Collector(Arc<Mutex<Vec<Event>>>);
        impl EventSink for Collector {
            fn emit(&mut self, event: Event) {
                self.0.lock().unwrap().push(event);
            }
        }
        let sink = SharedSink::new(Box::new(Collector(Arc::clone(&kept))));
        let tool_side = sink.clone();
        tool_side.send(Event::Skill {
            name: "demo".into(),
            how: SkillHow::Loaded,
        });
        let mut run_side = sink;
        run_side.emit(Event::Finished {
            reason: FinishReason::Answered,
            turns: 1,
            usage: Usage::default(),
        });
        let events = kept.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(
            events[0],
            Event::Skill {
                how: SkillHow::Loaded,
                ..
            }
        ));
        assert!(matches!(events[1], Event::Finished { .. }));
    }

    #[test]
    fn a_long_tool_result_is_cut_for_the_event() {
        let Event::ToolResult {
            output, truncated, ..
        } = Event::tool_result(1, "c", "grep", true, &"x".repeat(EVENT_RESULT_CHARS + 1))
        else {
            unreachable!()
        };
        assert!(truncated);
        assert_eq!(output.chars().count(), EVENT_RESULT_CHARS);
    }
}
