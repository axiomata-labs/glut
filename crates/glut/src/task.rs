//! The `task` tool (HS8): the spec the model sees and the check of a call's arguments.
//!
//! The tool deliberately has no [`crate::tools::Tool`] impl — starting a run needs what only the loop has (the
//! model, the sink, the hooks), so `run` intercepts a `task` call itself (`run.rs`, D2). This module is what
//! that interception shares: name, spec, argument parsing.

use serde_json::{Value, json};

use crate::model::ToolSpec;

/// The tool's name. Reserved at depth 0 (D1): a caller's own tool of this name would be shadowed.
pub(crate) const NAME: &str = "task";

/// The spec as the model sees it; with named agents (TUI T37a) it lists them and takes an `agent`.
pub(crate) fn spec(agents: &crate::agents::Catalog) -> ToolSpec {
    let mut description = "Start a subagent for a self-contained piece of work. It runs in its own context — yours \
                           stays clean — with the same tools, permissions and folder, and returns only its final \
                           answer; give it everything it needs in `prompt`, it sees nothing else. Independent parts \
                           go as several `task` calls in one reply: they run at once."
        .to_owned();
    let mut parameters = plain_parameters();
    if !agents.is_empty() {
        description.push_str(
            "\n\nNamed agents — set `agent` to hand the work to one whose description fits, or when the owner \
             names it; without `agent` a general subagent runs:\n",
        );
        description.push_str(&agents.described());
        let names: Vec<&str> = agents
            .list()
            .iter()
            .map(|agent| agent.name.as_str())
            .collect();
        parameters["properties"]["agent"] = json!({
            "type": "string",
            "enum": names,
            "description": "The named agent to run; leave it out for a general subagent."
        });
    }
    ToolSpec {
        name: NAME.into(),
        description,
        parameters,
    }
}

/// The parameters every `task` call has.
fn plain_parameters() -> Value {
    json!({
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
    })
}

/// The named agent a `task` call asks for, if any.
///
/// # Errors
///
/// An `agent` that is no string, or names none of `agents` — the model is told which there are.
pub(crate) fn agent_of<'a>(
    arguments: &Value,
    agents: &'a crate::agents::Catalog,
) -> Result<Option<&'a crate::agents::Agent>, String> {
    match arguments.get("agent") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(name)) if name.trim().is_empty() => Ok(None),
        Some(Value::String(name)) => agents
            .get(name.trim())
            .map(Some)
            .ok_or_else(|| format!("no agent {name:?}: {}", agents.known())),
        Some(_) => Err("`agent` must be the name of an agent".into()),
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
    fn named_agents_are_listed_and_a_call_names_one_of_them() {
        let reviewer = crate::agents::Agent::parse(
            "reviewer",
            "---\nname: reviewer\ndescription: Reviews diffs.\n---\nReview.",
        )
        .unwrap();
        let agents = crate::agents::Catalog::of(vec![reviewer]);
        let spec = spec(&agents);
        assert!(spec.description.contains("- reviewer: Reviews diffs."));
        assert_eq!(
            spec.parameters["properties"]["agent"]["enum"],
            json!(["reviewer"])
        );
        let named = |agent: Value| {
            agent_of(&json!({"agent": agent}), &agents).map(|found| found.map(|a| a.name.clone()))
        };
        assert_eq!(named(json!("reviewer")), Ok(Some("reviewer".to_owned())));
        assert_eq!(named(json!("")), Ok(None));
        assert!(
            named(json!("other"))
                .unwrap_err()
                .contains("the agents are reviewer")
        );
        assert!(named(json!(3)).is_err());
        assert_eq!(
            agent_of(&json!({}), &agents).map(|found| found.is_none()),
            Ok(true)
        );
    }

    #[test]
    fn the_spec_is_a_task_with_a_description_and_a_prompt() {
        let spec = spec(&crate::agents::Catalog::default());
        assert_eq!(spec.name, NAME);
        assert!(spec.parameters["properties"].get("agent").is_none());
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
