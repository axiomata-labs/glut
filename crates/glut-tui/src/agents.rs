//! A subagent's own view (T18): its stream over the session's, opened from its tile (a click) or `/agents`; its own
//! subagents open from it by number, Esc goes back one level.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::text::{Line, Span};

use crate::theme;
use crate::transcript::{Agent, Thinking, ToolState, cut};

/// Which subagent is open, and how far its stream is scrolled up from its end.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    /// The tile's block in the session's stream, then in that subagent's stream, and so on.
    pub path: Vec<usize>,
    /// Lines up from the end; 0 follows the end while it grows.
    pub scroll: usize,
}

/// What a key in the view asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    None,
    /// One level up: the parent subagent's view, or the session's stream.
    Back,
    /// The subagent's own subagent with this number (1 the first).
    Open(usize),
}

impl View {
    pub fn new(path: Vec<usize>) -> Self {
        View { path, scroll: 0 }
    }

    /// A key, for a view `height` rows high showing `nested` subagents of its own.
    pub fn key(&mut self, key: KeyEvent, height: usize, nested: usize) -> Step {
        let page = height.saturating_sub(2).max(1);
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return Step::None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return Step::Back,
            KeyCode::Up | KeyCode::Char('k') => self.scroll = self.scroll.saturating_add(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll = self.scroll.saturating_sub(1),
            KeyCode::PageUp => self.scroll = self.scroll.saturating_add(page),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_sub(page),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = usize::MAX,
            KeyCode::End | KeyCode::Char('G') => self.scroll = 0,
            KeyCode::Char(digit @ '1'..='9') => {
                let at = digit as usize - '0' as usize;
                if at <= nested {
                    return Step::Open(at);
                }
            }
            _ => {}
        }
        Step::None
    }
}

/// The view's title: the task, its state, what it spent.
pub fn title(agent: &Agent, depth: usize, width: usize) -> String {
    let state = match agent.state {
        ToolState::Running => "running",
        ToolState::Done => "done",
        ToolState::Failed => "stopped",
        ToolState::Refused => "refused",
    };
    let level = if depth > 1 {
        format!("Subagent (level {depth})")
    } else {
        "Subagent".to_owned()
    };
    cut(
        &format!(
            " {level} · {} · {state} · {} calls · {}k in · {}k out ",
            crate::transcript::clean(&agent.task),
            agent.calls,
            agent.input_tokens / 1000,
            agent.output_tokens / 1000
        ),
        width,
    )
}

/// The rows the view shows at `width` × `height`: its stream from where it is scrolled to, and a last row with its own
/// subagents by number and the keys. The scroll is bounded to the stream here.
pub fn lines(
    agent: &mut Agent,
    view: &mut View,
    width: u16,
    height: usize,
    thinking: Thinking,
    frame: usize,
) -> Vec<Line<'static>> {
    let mut stream: Vec<Line<'static>> = Vec::new();
    for block in &mut agent.stream.blocks {
        stream.extend_from_slice(block.lines(width, thinking, frame));
    }
    if stream.is_empty() && agent.state == ToolState::Running {
        stream.push(Line::from(Span::styled(
            " nothing yet: the subagent is starting",
            theme::quiet(),
        )));
    } else if stream.is_empty() {
        // A session opened again keeps the conversation, not a subagent's steps: what it answered is what is left.
        stream.push(Line::from(Span::styled(
            " its steps were not kept with the session — what it answered:",
            theme::quiet(),
        )));
        stream.extend(crate::markdown::render(&agent.answer, usize::from(width)));
    }
    let room = height.saturating_sub(1).max(1);
    view.scroll = view.scroll.min(stream.len().saturating_sub(room));
    let end = stream.len() - view.scroll;
    let start = end.saturating_sub(room);
    let mut shown: Vec<Line<'static>> = stream[start..end].to_vec();
    shown.resize(room, Line::default());
    shown.push(footer(agent, usize::from(width)));
    shown
}

/// The view's last row: its own subagents by number, then the keys.
fn footer(agent: &Agent, width: usize) -> Line<'static> {
    let nested: Vec<String> = agent
        .stream
        .agent_blocks()
        .iter()
        .take(9)
        .enumerate()
        .filter_map(|(at, block)| match &agent.stream.blocks[*block].kind {
            crate::transcript::Kind::Agent(inner) => Some(format!(
                "{} {}",
                at + 1,
                cut(&crate::transcript::clean(&inner.task), 24)
            )),
            _ => None,
        })
        .collect();
    let keys = if nested.is_empty() {
        "↑↓ PgUp PgDn scroll · Esc back".to_owned()
    } else {
        format!(
            "{} · number opens · ↑↓ scroll · Esc back",
            nested.join(" · ")
        )
    };
    Line::from(Span::styled(cut(&keys, width), theme::quiet()))
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
    fn keys_scroll_open_a_nested_one_and_go_back() {
        let mut view = View::new(vec![3]);
        assert_eq!(view.key(press(KeyCode::Up), 10, 0), Step::None);
        assert_eq!(view.scroll, 1);
        assert_eq!(view.key(press(KeyCode::PageUp), 10, 0), Step::None);
        assert_eq!(view.scroll, 9);
        view.key(press(KeyCode::End), 10, 0);
        assert_eq!(view.scroll, 0);
        assert_eq!(view.key(press(KeyCode::Char('2')), 10, 1), Step::None);
        assert_eq!(view.key(press(KeyCode::Char('1')), 10, 1), Step::Open(1));
        assert_eq!(view.key(press(KeyCode::Esc), 10, 1), Step::Back);
        // Home and further up never overflow.
        view.key(press(KeyCode::Home), 10, 0);
        view.key(press(KeyCode::Up), 10, 0);
        view.key(press(KeyCode::PageUp), 10, 0);
        assert_eq!(view.scroll, usize::MAX);
    }
}
