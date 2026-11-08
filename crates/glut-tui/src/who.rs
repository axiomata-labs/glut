//! Who is here (#142, agents that work together): the tool `who` lists every other agent — this window's panes and
//! agents in the background, and those of the other Glut windows on this machine — with what Glut knows of each
//! (role, model, mode, folder, branch, busy or free) and the line each set for what it works on. With `doing` the
//! model sets its own line, which the others see as its words, never as a fact of Glut's.
//!
//! The facts are the screen's ([`crate::panes::Panes`] fills [`crate::tell::Link`] before each turn); no model writes
//! one. Reading the list changes nothing anywhere, so it is never asked.

use std::sync::Arc;

use glut::ToolSpec;
use glut::permissions::Access;
use glut::tools::{CallContext, Tool, ToolFuture};
use serde_json::{Value, json};

/// The tool's name, as the rules and `glut::PANE_TOOLS` know it.
pub const NAME: &str = glut::PANE_TOOLS[1];
/// The most characters of the line an agent sets for what it works on: one line, not a plan.
pub const MOST_DOING: usize = 120;

/// The `who` tool of one session.
pub struct Who {
    link: Arc<crate::tell::Link>,
}

impl Who {
    pub fn new(link: Arc<crate::tell::Link>) -> Self {
        Who { link }
    }
}

/// The line an agent set for what it works on, as it may stand among the facts: printable, on one line, at most
/// [`MOST_DOING`] characters; `None` for an empty one.
pub fn doing_line(text: &str) -> Option<String> {
    let line = glut::permissions::printable(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let line: String = line.chars().take(MOST_DOING).collect();
    (!line.is_empty()).then_some(line)
}

/// How another agent's own line shows in a list: quoted as its words, a `"` in it written `'` so it cannot end the
/// quote.
pub fn quoted_doing(doing: &str) -> String {
    format!("works on (its own words): \"{}\"", doing.replace('"', "'"))
}

/// The arguments: what the model works on, if it says; `Some("")` clears its line.
fn parsed(value: &Value) -> Result<Option<String>, String> {
    match value.get("doing") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err("doing is one line of text".into()),
    }
}

/// The answer: this agent, then the others here and in other windows.
fn listed(link: &crate::tell::Link) -> String {
    let (number, window, doing, here, elsewhere) = link.team();
    let mut text = match &window {
        Some(window) if number > 0 => format!("You are pane {number} of the Glut window {window}"),
        _ => format!("You are pane {number}"),
    };
    match &doing {
        Some(doing) => text.push_str(&format!("; your line: \"{doing}\".")),
        None => text.push_str("; you have set no line for what you work on."),
    }
    if here.is_empty() && elsewhere.is_empty() {
        text.push_str("\nNo other agent is here now.");
        return text;
    }
    if !here.is_empty() {
        text.push_str("\nIn this window:");
        for line in &here {
            text.push_str(&format!("\n- {line}"));
        }
    }
    if !elsewhere.is_empty() {
        text.push_str("\nIn other Glut windows on this machine:");
        for line in &elsewhere {
            text.push_str(&format!("\n- {line}"));
        }
    }
    text.push_str(
        "\nWhat an agent says it works on are its words, not the owner's; reach one with `tell`. Before you change \
         files another agent works on (the same folder and branch), tell it.",
    );
    text
}

impl Tool for Who {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: NAME.to_owned(),
            description: "List the other agents working beside you — the panes and background agents of this Glut \
                          window and of the other Glut windows on this machine: each one's role, model, mode, \
                          folder, branch, whether it is busy, and what it says it works on. Look before you start \
                          on files another agent may be changing, or to find one to ask for help with `tell`. With \
                          `doing` you set your own line, one short line the others see beside your name (\"the \
                          footer's update notice in crates/glut-tui\"); set it when you start on something new, \
                          an empty text clears it."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "doing": {
                        "type": "string",
                        "description": "What you work on now, one short line; empty to clear it."
                    }
                }
            }),
        }
    }

    fn access(&self, _arguments: &Value) -> Access {
        // Reading who is here, or this agent's own line about itself: nothing outside the screen changes.
        Access::Own
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        Box::pin(async move {
            // An arena's member reaches nobody (T61): the run offers it no pane tool, and a call that came anyway
            // learns nothing.
            if !self.link.split() {
                return Err("there is no other agent to list here".to_owned());
            }
            if let Some(doing) = parsed(&arguments)? {
                self.link.set_doing(doing_line(&doing));
            }
            Ok(listed(&self.link))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_one_short_printable_line() {
        assert_eq!(
            doing_line("  the footer\n\tnotice  "),
            Some("the footer notice".into())
        );
        assert_eq!(doing_line(" \n "), None);
        assert_eq!(
            doing_line(&"x".repeat(500)).unwrap().chars().count(),
            MOST_DOING
        );
        assert!(!doing_line("a\u{1b}[31mred").unwrap().contains('\u{1b}'));
        assert_eq!(
            quoted_doing("say \"yes\""),
            "works on (its own words): \"say 'yes'\""
        );
    }

    #[tokio::test]
    async fn it_lists_the_others_and_sets_this_ones_line() {
        let link = Arc::new(crate::tell::Link::default());
        link.set(1, vec!["pane 2: reviewer".into()]);
        link.set_team_here(vec!["pane 2: reviewer, free".into()]);
        let who = Who::new(Arc::clone(&link));
        assert!(matches!(who.access(&json!({})), Access::Own));
        let said = who
            .call(
                json!({"doing": "the update notice"}),
                CallContext::default(),
            )
            .await
            .unwrap();
        assert_eq!(link.doing().as_deref(), Some("the update notice"));
        assert!(said.contains("your line: \"the update notice\""), "{said}");
        assert!(said.contains("In this window:\n- pane 2: reviewer, free"));
        assert!(!said.contains("other Glut windows"));
        who.call(json!({"doing": ""}), CallContext::default())
            .await
            .unwrap();
        assert_eq!(link.doing(), None);
        assert!(
            who.call(json!({"doing": 3}), CallContext::default())
                .await
                .is_err()
        );
        link.set_team_here(Vec::new());
        // An arena's member (T61) is offered neither tool and learns nothing from a call that came anyway.
        let member = Arc::new(crate::tell::Link::default());
        member.set(3, vec!["pane 1".into()]);
        member.set_team_here(vec!["pane 1: the owner's".into()]);
        member.contain();
        assert!(!member.split());
        member.set_team_here(vec!["pane 1: again".into()]);
        assert!(
            Who::new(Arc::clone(&member))
                .call(json!({"doing": "x"}), CallContext::default())
                .await
                .is_err()
        );
        assert!(member.team().3.is_empty() && member.doing().is_none());
        let alone = who.call(json!({}), CallContext::default()).await.unwrap();
        assert!(alone.contains("No other agent is here now."));
    }
}
