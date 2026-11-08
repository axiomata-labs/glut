//! Agents that work together (#142): what a host shows of them, in one shape for every screen — Glut's terminal UI
//! today, a window app later. A [`Profile`] is one agent as Glut knows it (its facts are Glut's, only `doing` is the
//! model's own words); a [`Note`] is one thing that passed between agents — a message, a request, its answer, Glut's
//! word that none comes, an agent arriving or leaving.
//!
//! These are plain values: the host fills them from its panes and windows and draws them as it likes.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Whether an agent can take something now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// It can take something now.
    Free,
    /// It is at work.
    Busy,
    /// A question of its gate waits for its owner.
    WaitsForOwner,
}

/// One agent: a pane or an agent in the background, of this window or another Glut window on the machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// How it is reached: `pane 2` or an agent's name here, a window's address (`name/2`) elsewhere.
    pub address: String,
    /// The Glut window it runs in when that is another one; `None` for this window.
    pub window: Option<String>,
    /// It is the one the profiles are shown to.
    pub you: bool,
    /// The role it plays (`reviewer`, `docs`); `None` for an all-rounder.
    pub role: Option<String>,
    /// Its model's name, and its mode as the screen names it.
    pub model: String,
    pub mode: String,
    /// Its folder's name — never the whole path of another window's.
    pub folder: String,
    /// The branch of its folder's repository, or a detached `HEAD`'s short id.
    pub branch: Option<String>,
    /// Whether it can take something now.
    pub state: State,
    /// What its model said it works on: its own words, not a fact.
    pub doing: Option<String>,
}

/// What passed between agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "request", rename_all = "snake_case")]
pub enum NoteKind {
    /// A message, nothing asked back.
    Message,
    /// A request: the turn it starts answers it; the asker's number for it.
    Request(u32),
    /// The answer to that request.
    Answer(u32),
    /// Glut's word that the request gets no answer (set aside, refused, closed, ended, too old).
    Unanswered(u32),
    /// An agent or a window came.
    Arrived,
    /// An agent or a window went.
    Left,
    /// Glut told an agent that another works where it does: the same branch of the same working tree, or the same
    /// file changed lately.
    Overlap,
}

/// One thing that passed between agents, as the screen's log keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub at: DateTime<Utc>,
    /// Who said it and to whom, as the profiles name them (`pane 1`, `docs-ab/2`, `glut`); `to` is empty for an
    /// arrival or a departure.
    pub from: String,
    pub to: String,
    pub kind: NoteKind,
    /// Its text, on one line and short: the whole stands in the conversations.
    pub text: String,
}

/// The most characters of a note's text the log keeps.
pub const NOTE_TEXT: usize = 160;

impl Note {
    /// A note of now, its text cut to one line of at most [`NOTE_TEXT`] characters.
    pub fn now(from: &str, to: &str, kind: NoteKind, text: &str) -> Note {
        let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = if line.chars().count() > NOTE_TEXT {
            let cut: String = line.chars().take(NOTE_TEXT - 1).collect();
            format!("{cut}…")
        } else {
            line
        };
        Note {
            at: Utc::now(),
            from: from.to_owned(),
            to: to.to_owned(),
            kind,
            text,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_keeps_one_short_line_and_reads_back() {
        let note = Note::now(
            "pane 1",
            "pane 2",
            NoteKind::Request(3),
            &format!("count\n the  lines {}", "x".repeat(300)),
        );
        assert!(note.text.starts_with("count the lines x"));
        assert_eq!(note.text.chars().count(), NOTE_TEXT);
        let line = serde_json::to_string(&note).unwrap();
        assert!(line.contains(r#""kind":"request","request":3"#), "{line}");
        assert_eq!(serde_json::from_str::<Note>(&line).unwrap(), note);
    }
}
