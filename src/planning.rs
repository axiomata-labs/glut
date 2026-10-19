//! Planning a run (Glut 2, D, after Opencode): `todo_write` keeps the run's task list, `exit_plan` asks the owner to
//! leave Plan mode with a plan.
//!
//! Like `task`, neither has a [`crate::tools::Tool`] impl: both touch only the run itself (its list, its mode), so the
//! loop answers them (`run.rs`).
//!
//! * **`todo_write`** replaces the whole list each time (as Opencode's `todowrite`): the model sends the list as it now
//!   stands, and the run reports it as an event the Studio can show. The answer repeats the open items, so the list is
//!   in the conversation where the model next looks.
//! * **`exit_plan`** is offered only in Plan mode: the plan goes to the owner as an ask; yes switches the gate to the
//!   run's build mode (and the edit tools appear), no keeps Plan. Headless nobody answers, so the run stays in Plan.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::model::ToolSpec;

pub(crate) const TODO: &str = "todo_write";
pub(crate) const EXIT_PLAN: &str = "exit_plan";

/// Most items a list may hold; a longer one is a plan to write down, not a list to track.
const MAX_ITEMS: usize = 50;
/// Longest item text, in characters.
const MAX_ITEM_CHARS: usize = 300;
/// What of the plan the owner's question shows, in characters; the whole plan stays in the conversation.
const PLAN_PREVIEW_CHARS: usize = 600;

/// Where an item stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

/// One item of the run's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TodoItem {
    pub text: String,
    pub status: TodoStatus,
}

pub(crate) fn todo_spec() -> ToolSpec {
    ToolSpec {
        name: TODO.into(),
        description: "Keep the task list of this run: send the whole list as it now stands (it replaces the last one). \
                      Use it for work of several steps — one item in_progress at a time, mark items done as you \
                      finish them."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" },
                            "status": { "type": "string", "enum": ["pending", "in_progress", "done"] }
                        },
                        "required": ["text", "status"]
                    }
                }
            },
            "required": ["items"]
        }),
    }
}

pub(crate) fn exit_plan_spec() -> ToolSpec {
    ToolSpec {
        name: EXIT_PLAN.into(),
        description: "You are in plan mode: you can read and plan, but not change anything. When your plan is ready, \
                      call this with it — the owner decides whether you may carry it out."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "plan": { "type": "string", "description": "The plan, short and concrete: what changes, where, why." }
            },
            "required": ["plan"]
        }),
    }
}

/// The list from a `todo_write` call, checked.
///
/// # Errors
///
/// A text the model reads: no list, an item without text or with an unknown status, too many or too long items.
pub(crate) fn parse_todos(arguments: &str) -> Result<Vec<TodoItem>, String> {
    #[derive(Deserialize)]
    struct Args {
        items: Vec<TodoItem>,
    }
    let args: Args = serde_json::from_str(arguments).map_err(|err| {
        format!("the arguments must be {{\"items\": [{{\"text\", \"status\"}}]}}: {err}")
    })?;
    if args.items.len() > MAX_ITEMS {
        return Err(format!("at most {MAX_ITEMS} items"));
    }
    if args
        .items
        .iter()
        .any(|item| item.text.trim().is_empty() || item.text.chars().count() > MAX_ITEM_CHARS)
    {
        return Err(format!(
            "every item needs a text of at most {MAX_ITEM_CHARS} characters"
        ));
    }
    Ok(args.items)
}

/// What the model is told after a `todo_write`: the counts and the items still open.
pub(crate) fn todo_answer(items: &[TodoItem]) -> String {
    let open: Vec<&TodoItem> = items
        .iter()
        .filter(|i| i.status != TodoStatus::Done)
        .collect();
    let done = items.len() - open.len();
    let mut answer = format!("list saved: {} items, {done} done", items.len());
    for item in open {
        let mark = if item.status == TodoStatus::InProgress {
            "▸"
        } else {
            "·"
        };
        answer.push_str(&format!("\n{mark} {}", item.text));
    }
    answer
}

/// The plan from an `exit_plan` call, and the line the owner is asked with.
///
/// # Errors
///
/// A text the model reads: no plan.
pub(crate) fn plan_of(arguments: &str) -> Result<(String, String), String> {
    let parsed: Value = serde_json::from_str(arguments)
        .map_err(|err| format!("the arguments are not valid JSON: {err}"))?;
    let plan = parsed
        .get("plan")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|plan| !plan.is_empty())
        .ok_or("`plan` (the plan to carry out) is required")?;
    let mut preview: String = plan.chars().take(PLAN_PREVIEW_CHARS).collect();
    if plan.chars().count() > PLAN_PREVIEW_CHARS {
        preview.push('…');
    }
    Ok((
        plan.to_owned(),
        format!("leave plan mode and carry out: {preview}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_is_checked_and_its_open_items_come_back() {
        let items = parse_todos(
            r#"{"items":[{"text":"read","status":"done"},{"text":"fix","status":"in_progress"},{"text":"test","status":"pending"}]}"#,
        )
        .unwrap();
        assert_eq!(
            todo_answer(&items),
            "list saved: 3 items, 1 done\n▸ fix\n· test"
        );
        assert!(parse_todos(r#"{"items":[{"text":"x","status":"later"}]}"#).is_err());
        assert!(parse_todos(r#"{"items":[{"text":" ","status":"done"}]}"#).is_err());
        assert!(parse_todos(r#"{"todos":[]}"#).is_err());
        let many = format!(
            r#"{{"items":[{}]}}"#,
            vec![r#"{"text":"a","status":"done"}"#; MAX_ITEMS + 1].join(",")
        );
        assert!(parse_todos(&many).is_err());
    }

    #[test]
    fn the_plan_is_required_and_its_preview_is_bounded() {
        let (plan, line) = plan_of(r#"{"plan":"  change a.rs  "}"#).unwrap();
        assert_eq!(plan, "change a.rs");
        assert_eq!(line, "leave plan mode and carry out: change a.rs");
        let long = "p".repeat(PLAN_PREVIEW_CHARS + 5);
        assert!(
            plan_of(&json!({ "plan": long }).to_string())
                .unwrap()
                .1
                .ends_with('…')
        );
        assert!(plan_of(r#"{"plan":""}"#).is_err());
    }
}
