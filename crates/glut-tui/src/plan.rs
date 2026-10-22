//! Plan mode with a real plan (T19): the plan `exit_plan` sent, kept for the Plan tab and `/plan`, and the dialog
//! its ask opens — approve, into which mode, or send it back with a note.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use glut::planning::{TodoItem, TodoStatus};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use unicode_width::UnicodeWidthStr;

use crate::theme;
use crate::transcript::{clean, cut, wrap};

/// Longest note the owner may send a plan back with, in characters.
const MAX_NOTE_CHARS: usize = 2_000;

/// Where the plan stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// Glut waits for the owner's word.
    Waiting,
    /// Approved; Glut works in this mode now (its label: build, auto, ask).
    Approved(String),
    /// Sent back, with the owner's note when there was one.
    SentBack(Option<String>),
    /// The turn ended before the owner decided.
    Dropped,
}

/// The plan as the model wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub text: String,
    pub state: State,
}

impl Plan {
    /// The plan from an `exit_plan` call's arguments; `None` when they hold none.
    pub fn of_call(arguments: &str) -> Option<Self> {
        let value: serde_json::Value = serde_json::from_str(arguments).ok()?;
        let text = value.get("plan")?.as_str()?.trim();
        (!text.is_empty()).then(|| Plan {
            text: text.to_owned(),
            state: State::Waiting,
        })
    }

    /// The state as one line, for the Plan tab and the dialog.
    pub fn state_line(&self) -> (String, Style) {
        match &self.state {
            State::Waiting => ("waiting for you".to_owned(), theme::glow()),
            State::Approved(mode) => (
                format!("approved · {mode}"),
                Style::default().fg(theme::c().fine),
            ),
            State::SentBack(Some(note)) => (format!("sent back: {}", clean(note)), theme::quiet()),
            State::SentBack(None) => ("sent back".to_owned(), theme::quiet()),
            State::Dropped => ("not decided: the turn ended".to_owned(), theme::quiet()),
        }
    }
}

/// The plan's view: the dialog while Glut asks, else only to read (`/plan`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dialog {
    /// Glut waits for an answer.
    pub asking: bool,
    /// Lines scrolled down from the plan's top.
    pub scroll: usize,
    /// The note being typed to send the plan back with.
    pub note: Option<String>,
}

/// What a key in the dialog decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    None,
    /// Approve into the offered mode with this index (0 the run's build mode).
    Approve(usize),
    /// Send the plan back, with a note when one was typed.
    SendBack(Option<String>),
    /// Close the view (only to read).
    Close,
}

impl Dialog {
    pub fn asking() -> Self {
        Dialog {
            asking: true,
            ..Dialog::default()
        }
    }

    pub fn reading() -> Self {
        Dialog::default()
    }

    /// A key: `modes` is how many modes the ask offers, `page` the rows the plan has on the screen.
    pub fn key(&mut self, key: KeyEvent, modes: usize, page: usize) -> Choice {
        if let Some(note) = &mut self.note {
            match key.code {
                KeyCode::Esc => self.note = None,
                KeyCode::Enter => {
                    let note = note.trim().to_owned();
                    self.note = None;
                    return Choice::SendBack((!note.is_empty()).then_some(note));
                }
                KeyCode::Backspace => {
                    note.pop();
                }
                KeyCode::Char(c)
                    if !key.modifiers.contains(KeyModifiers::CONTROL)
                        && note.chars().count() < MAX_NOTE_CHARS =>
                {
                    note.push(c)
                }
                _ => {}
            }
            return Choice::None;
        }
        let page = page.max(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::PageDown | KeyCode::Char(' ') => {
                self.scroll = self.scroll.saturating_add(page)
            }
            KeyCode::Esc | KeyCode::Char('q') if !self.asking => return Choice::Close,
            KeyCode::Enter | KeyCode::Char('y') if self.asking => return Choice::Approve(0),
            KeyCode::Char(digit @ '1'..='9') if self.asking => {
                let at = digit as usize - '1' as usize;
                if at < modes {
                    return Choice::Approve(at);
                }
            }
            KeyCode::Char('n') if self.asking => self.note = Some(String::new()),
            // Esc answers as on every ask: no, without a note.
            KeyCode::Esc if self.asking => return Choice::SendBack(None),
            _ => {}
        }
        Choice::None
    }
}

/// What the dialog shows at `width` × `height`: the plan from where it is scrolled to, the task list, and — while Glut
/// asks — the choices: the modes by number (the run's build mode first), and sending it back. The scroll is bounded
/// here. `modes` are the labels (build, auto, ask); `auto_unsandboxed` warns that auto would run without a sandbox.
pub fn lines(
    plan: &Plan,
    todos: &[TodoItem],
    dialog: &mut Dialog,
    modes: &[String],
    auto_unsandboxed: bool,
    width: usize,
    height: usize,
) -> Vec<Line<'static>> {
    let mut body = crate::markdown::render(&plan.text, width.max(8));
    if !todos.is_empty() {
        body.push(Line::default());
        body.push(Line::from(Span::styled("Task list", theme::strong())));
        body.extend(todo_lines(todos, width));
    }
    let mut foot: Vec<Line<'static>> = Vec::new();
    if dialog.asking {
        foot.push(Line::default());
        if let Some(note) = &dialog.note {
            for row in wrap(
                "Why it goes back — Glut reads it with the plan's answer (Enter sends, Esc returns):",
                width,
            ) {
                foot.push(Line::from(Span::styled(row, theme::quiet())));
            }
            let shown = cut(&clean(note), width.saturating_sub(4));
            foot.push(Line::from(vec![
                Span::styled("› ", theme::glow()),
                Span::raw(shown),
                Span::styled("▍", theme::glow()),
            ]));
        } else {
            let mut choices = vec![("Enter".to_owned(), "approve".to_owned())];
            for (at, mode) in modes.iter().enumerate().take(9) {
                let label = if mode == "auto" && auto_unsandboxed {
                    format!("{mode} (no sandbox!)")
                } else {
                    clean(mode)
                };
                choices.push(((at + 1).to_string(), label));
            }
            choices.push(("n".to_owned(), "send back with a note".to_owned()));
            choices.push(("Esc".to_owned(), "send back".to_owned()));
            foot.extend(flow(&choices, width));
            if let Some(first) = modes.first() {
                foot.push(Line::from(Span::styled(
                    cut(
                        &format!(
                            "approved, Glut carries it out in {} mode; a number picks another",
                            clean(first)
                        ),
                        width,
                    ),
                    theme::quiet(),
                )));
            }
        }
    } else {
        let (state, style) = plan.state_line();
        foot.push(Line::default());
        foot.push(Line::from(vec![
            Span::styled(cut(&state, width.saturating_sub(24)), style),
            Span::styled(" · ↑↓ scroll · Esc closes", theme::quiet()),
        ]));
    }
    let room = height.saturating_sub(foot.len()).max(1);
    dialog.scroll = dialog.scroll.min(body.len().saturating_sub(room));
    let mut shown: Vec<Line<'static>> = body.into_iter().skip(dialog.scroll).take(room).collect();
    shown.resize(room, Line::default());
    shown.extend(foot);
    shown
}

/// Key and label pairs as rows of at most `width` cells, the keys glowing, a pair never split over two rows.
fn flow(pairs: &[(String, String)], width: usize) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let mut row: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for (key, label) in pairs {
        let label = cut(label, width.saturating_sub(key.width() + 1).max(1));
        let size = key.width() + 1 + label.width();
        let gap = if row.is_empty() { 0 } else { 3 };
        if !row.is_empty() && used + gap + size > width {
            rows.push(Line::from(std::mem::take(&mut row)));
            used = 0;
        }
        if !row.is_empty() {
            row.push(Span::styled(" · ", theme::quiet()));
            used += 3;
        }
        row.push(Span::styled(key.clone(), theme::glow()));
        row.push(Span::raw(format!(" {label}")));
        used += size;
    }
    if !row.is_empty() {
        rows.push(Line::from(row));
    }
    rows
}

/// The task list as rows: done quiet, under way glowing, open plain.
pub fn todo_lines(todos: &[TodoItem], width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for item in todos {
        let (mark, style) = match item.status {
            TodoStatus::Done => ("✓", theme::quiet()),
            TodoStatus::InProgress => ("◆", theme::glow()),
            TodoStatus::Pending => ("○", Style::default()),
        };
        for (at, row) in wrap(&item.text, width.saturating_sub(3))
            .into_iter()
            .enumerate()
        {
            let lead = if at == 0 {
                format!(" {mark} ")
            } else {
                "   ".to_owned()
            };
            lines.push(Line::from(vec![
                Span::styled(lead, style),
                Span::styled(row, style),
            ]));
        }
    }
    lines
}

/// The plan's head for the Plan tab: its state, then at most `most` rows of it, and how many more `/plan` shows.
pub fn head(plan: &Plan, width: usize, most: usize) -> Vec<Line<'static>> {
    let (state, style) = plan.state_line();
    let mut lines = vec![Line::from(vec![
        Span::styled(" Plan · ", theme::strong()),
        Span::styled(cut(&state, width.saturating_sub(8)), style),
    ])];
    let body = crate::markdown::render(&plan.text, width.saturating_sub(1).max(8));
    let total = body.len();
    for mut row in body.into_iter().take(most) {
        row.spans.insert(0, Span::raw(" "));
        lines.push(row);
    }
    if total > most {
        lines.push(Line::from(Span::styled(
            format!(" … {} more lines — /plan shows it whole", total - most),
            theme::quiet().add_modifier(Modifier::ITALIC),
        )));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    #[test]
    fn a_plan_is_read_from_its_call() {
        let plan = Plan::of_call(r#"{"plan":"  1. change a.rs  "}"#).unwrap();
        assert_eq!(plan.text, "1. change a.rs");
        assert_eq!(plan.state, State::Waiting);
        assert!(Plan::of_call(r#"{"plan":" "}"#).is_none());
        assert!(Plan::of_call("{\"plan\":\"cut").is_none());
    }

    #[test]
    fn the_dialog_approves_into_a_mode_or_sends_back_with_a_note() {
        let mut dialog = Dialog::asking();
        assert_eq!(dialog.key(press(KeyCode::Enter), 3, 10), Choice::Approve(0));
        assert_eq!(
            dialog.key(press(KeyCode::Char('2')), 3, 10),
            Choice::Approve(1)
        );
        assert_eq!(dialog.key(press(KeyCode::Char('4')), 3, 10), Choice::None);
        assert_eq!(dialog.key(press(KeyCode::Char('n')), 3, 10), Choice::None);
        for c in "no tests?".chars() {
            dialog.key(press(KeyCode::Char(c)), 3, 10);
        }
        // Typing a note, the keys are the note's: 1 is a character, not a mode.
        dialog.key(press(KeyCode::Char('1')), 3, 10);
        dialog.key(press(KeyCode::Backspace), 3, 10);
        assert_eq!(
            dialog.key(press(KeyCode::Enter), 3, 10),
            Choice::SendBack(Some("no tests?".into()))
        );
        assert_eq!(
            dialog.key(press(KeyCode::Esc), 3, 10),
            Choice::SendBack(None)
        );
    }

    #[test]
    fn reading_only_closes_and_never_answers() {
        let mut dialog = Dialog::reading();
        assert_eq!(dialog.key(press(KeyCode::Enter), 3, 10), Choice::None);
        assert_eq!(dialog.key(press(KeyCode::Char('1')), 3, 10), Choice::None);
        assert_eq!(dialog.key(press(KeyCode::Esc), 3, 10), Choice::Close);
    }

    #[test]
    fn the_dialog_bounds_its_scroll_and_keeps_its_choices_on_screen() {
        let plan = Plan {
            text: (1..=40).map(|n| format!("step {n}\n\n")).collect(),
            state: State::Waiting,
        };
        let mut dialog = Dialog::asking();
        dialog.scroll = 10_000;
        let modes = ["build".to_owned(), "auto".to_owned()];
        let shown = lines(&plan, &[], &mut dialog, &modes, true, 60, 12);
        assert_eq!(shown.len(), 12);
        let text: Vec<String> = shown.iter().map(|line| line.to_string()).collect();
        assert!(text.iter().any(|row| row.contains("step 40")));
        assert!(text.iter().any(|row| row.contains("auto (no sandbox!)")));
        assert!(text.last().unwrap().contains("build mode"));
        // On a narrow screen every choice stays whole and no row passes the width.
        let modes = ["build".to_owned(), "auto".to_owned(), "ask".to_owned()];
        let shown = lines(&plan, &[], &mut dialog, &modes, true, 50, 14);
        let text: Vec<String> = shown.iter().map(|line| line.to_string()).collect();
        assert!(text.iter().all(|row| row.width() <= 50), "{text:#?}");
        assert!(text.iter().any(|row| row.contains("Esc send back")));
        assert!(
            text.iter()
                .any(|row| row.contains("n send back with a note"))
        );
    }
}
