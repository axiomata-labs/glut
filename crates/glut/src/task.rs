//! The `task` tool (HS8): the spec the model sees and the check of a call's arguments.
//!
//! The tool deliberately has no [`crate::tools::Tool`] impl — starting a run needs what only the loop has (the
//! model, the sink, the hooks), so `run` intercepts a `task` call itself (`run.rs`, D2). This module is what
//! that interception shares: name, spec, argument parsing.

use serde_json::{Value, json};

use crate::model::ToolSpec;

/// The tool's name. Reserved at depth 0 (D1): a caller's own tool of this name would be shadowed.
pub(crate) const NAME: &str = "task";

/// The spec as the model sees it.
pub(crate) fn spec() -> ToolSpec {
    ToolSpec {
        name: NAME.into(),
        description: "Start a subagent for a self-contained piece of work. It runs in its own context — yours \
                      stays clean — with the same tools, permissions and folder, and returns only its final \
                      answer; give it everything it needs in `prompt`, it sees nothing else."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "A short label for what the subagent does, for the event stream."
                },
                "prompt": {
                    "type": "string",
                    "description": "The subagent's whole task: context, steps, and what to answer with. \
                                    It sees nothing else."
                }
            },
            "required": ["description", "prompt"]
        }),
    }
}

/// The subagent's user message from a `task` call's arguments (D1).
///
/// Both arguments are required — `description` is what the stream shows beside the call — but only the prompt
/// travels on: the label already reaches every consumer through the `ToolCall` event's arguments.
///
/// # Errors
///
/// A missing or empty `description` or `prompt`: `the task needs a non-empty description and prompt` — the run
/// goes on, the model is told what to fix.
pub(crate) fn prompt_of(arguments: &Value) -> Result<String, String> {
    let text = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
    };
    if text("description").is_empty() || text("prompt").is_empty() {
        return Err("the task needs a non-empty description and prompt".into());
    }
    Ok(text("prompt").to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_spec_is_a_task_with_a_description_and_a_prompt() {
        let spec = spec();
        assert_eq!(spec.name, NAME);
        assert_eq!(
            spec.parameters["required"],
            json!(["description", "prompt"])
        );
        assert!(
            spec.description
                .contains("same tools, permissions and folder")
        );
    }

    #[test]
    fn both_arguments_are_required_and_the_prompt_travels() {
        let full = json!({"description": "look it up", "prompt": "Find the answer."});
        assert_eq!(prompt_of(&full).unwrap(), "Find the answer.");
        // An empty string counts as missing, not as a subagent with nothing to do.
        for arguments in [
            json!({}),
            json!({"description": "x"}),
            json!({"prompt": "x"}),
            json!({"description": "x", "prompt": "  "}),
        ] {
            let reason = prompt_of(&arguments).unwrap_err();
            assert!(reason.contains("description and prompt"), "{reason}");
        }
    }
}
