//! What a run reports, step by step: the contract between the harness and whatever shows it — a log, the `axh`
//! binary's JSON lines, later a TUI, the Studio or an app of its own.

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

/// One step of a run.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Started {
        model: String,
        cwd: String,
        tools: Vec<String>,
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
                output_tokens: 2,
            },
        })
        .unwrap();
        assert_eq!(done["reason"], "max_turns");
        assert_eq!(done["usage"]["input_tokens"], 5);
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
