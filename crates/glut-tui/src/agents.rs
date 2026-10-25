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
        ToolState::Running => tr!(agent_running),
        ToolState::Done => tr!(agent_done),
        ToolState::Failed => tr!(agent_stopped),
        ToolState::Refused => tr!(agent_refused),
    };
    let level = if depth > 1 {
        tr!(subagent_level, depth = depth)
    } else {
        tr!(subagent).to_owned()
    };
    cut(
        &format!(
            " {} ",
            tr!(
                subagent_title,
                level = level,
                task = crate::transcript::clean(&agent.task),
                state = state,
                calls = agent.calls,
                input = agent.input_tokens / 1000,
                output = agent.output_tokens / 1000
            )
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
            format!(" {}", tr!(subagent_starting)),
            theme::quiet(),
        )));
    } else if stream.is_empty() {
        // A session opened again keeps the conversation, not a subagent's steps: what it answered is what is left.
        stream.push(Line::from(Span::styled(
            format!(" {}", tr!(subagent_not_kept)),
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
        tr!(subagent_keys).to_owned()
    } else {
        format!("{} · {}", nested.join(" · "), tr!(subagent_keys_nested))
    };
    Line::from(Span::styled(cut(&keys, width), theme::quiet()))
}

/// The Agents tab's list (T34): every subagent of the session, each under the one that started it. The session's own
/// subagents that still run come first, then the others, newest first; a subagent's own stay under it in their order.
pub fn overview(stream: &crate::transcript::Transcript) -> Vec<(Vec<usize>, &Agent)> {
    let found = stream.agents();
    let mut groups: Vec<Vec<(Vec<usize>, &Agent)>> = Vec::new();
    for (path, agent) in found {
        if path.len() == 1 {
            groups.push(Vec::new());
        }
        if let Some(group) = groups.last_mut() {
            group.push((path, agent));
        }
    }
    // Stable: equals keep their order, so the newest of the running or of the rest is reached by reversing first.
    groups.reverse();
    groups.sort_by_key(|group| group[0].1.state != ToolState::Running);
    groups.into_iter().flatten().collect()
}

/// The tab's rows at `width` for at most `room` rows: per subagent its task — glowing while it runs, quiet once it
/// ended — and under it what it does now or how it ended, with its calls and tokens. `chosen` is marked; the list
/// starts late enough for it to be seen.
pub fn overview_lines(
    list: &[(Vec<usize>, &Agent)],
    chosen: Option<usize>,
    width: usize,
    room: usize,
) -> Vec<Line<'static>> {
    let mut entries: Vec<Vec<Line<'static>>> = Vec::new();
    for (at, (path, agent)) in list.iter().enumerate() {
        // Deep nesting stops indenting where the panel would have no room left for the task.
        let indent = "  ".repeat((path.len() - 1).min(width / 4));
        let picked = chosen == Some(at);
        let running = agent.state == ToolState::Running;
        let (mark, end) = match agent.state {
            ToolState::Running => ("⬡", ""),
            ToolState::Done => ("✓", tr!(agent_done)),
            ToolState::Failed => ("✗", tr!(agent_stopped)),
            ToolState::Refused => ("⊘", tr!(agent_refused)),
        };
        let mut task_style = if running {
            theme::glow()
        } else {
            theme::quiet()
        };
        if picked {
            task_style = task_style.add_modifier(ratatui::style::Modifier::BOLD);
        }
        let room_for_text = width.saturating_sub(indent.len() + 4).max(4);
        let mut rows = vec![Line::from(vec![
            Span::styled(if picked { "▸" } else { " " }, theme::glow()),
            Span::raw(indent.clone()),
            Span::styled(format!("{mark} "), task_style),
            Span::styled(
                cut(&crate::transcript::clean(&agent.task), room_for_text),
                task_style,
            ),
        ])];
        let model = agent
            .model
            .as_deref()
            .map(|model| format!(" · {}", crate::transcript::clean(model)))
            .unwrap_or_default();
        let spent = tr!(
            agent_spent,
            calls = agent.calls,
            tokens = crate::meter::tokens(agent.input_tokens + agent.output_tokens),
            model = model
        );
        if running {
            rows.push(Line::from(Span::raw(format!(
                " {indent}  {}",
                cut(&crate::transcript::clean(&agent.live), room_for_text)
            ))));
            rows.push(Line::from(Span::styled(
                format!(" {indent}  {spent}"),
                theme::quiet(),
            )));
        } else {
            rows.push(Line::from(Span::styled(
                cut(&format!(" {indent}  {end} · {spent}"), width),
                theme::quiet(),
            )));
        }
        entries.push(rows);
    }
    // From the chosen one back, as many entries as fit; then on from the first one shown.
    let first = match chosen {
        Some(chosen) if chosen < entries.len() => {
            let mut used = 0;
            let mut first = chosen + 1;
            while first > 0 && used + entries[first - 1].len() <= room {
                first -= 1;
                used += entries[first].len();
            }
            first.min(chosen)
        }
        _ => 0,
    };
    let mut lines = Vec::new();
    for entry in entries.into_iter().skip(first) {
        if lines.len() + entry.len() > room {
            break;
        }
        lines.extend(entry);
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

    fn task(id: &str, label: &str) -> glut::Event {
        glut::Event::ToolCall {
            turn: 1,
            id: id.into(),
            name: "task".into(),
            arguments: format!(r#"{{"description":"{label}","prompt":"go"}}"#),
        }
    }

    fn ended(id: &str) -> glut::Event {
        glut::Event::ToolResult {
            turn: 1,
            id: id.into(),
            name: "task".into(),
            ok: true,
            output: "ok".into(),
            truncated: false,
        }
    }

    #[test]
    fn the_overview_puts_the_running_first_then_the_newest_and_marks_the_chosen() {
        let mut stream = crate::transcript::Transcript::default();
        for event in [
            task("a", "first"),
            ended("a"),
            task("b", "second"),
            ended("b"),
            task("c", "third"),
        ] {
            stream.take(&event);
        }
        let list = overview(&stream);
        let tasks: Vec<&str> = list.iter().map(|(_, agent)| agent.task.as_str()).collect();
        assert_eq!(tasks, ["third", "second", "first"]);
        let rows = overview_lines(&list, Some(2), 40, 100);
        let text: Vec<String> = rows.iter().map(ToString::to_string).collect();
        // The running one has its live line and its spending under it, an ended one a single line.
        assert_eq!(text.len(), 3 + 2 * 2, "{text:?}");
        assert!(text[0].contains("⬡ third"));
        assert!(text[3].contains("✓ second") && text[4].contains("done · 0 calls"));
        assert!(text[5].starts_with('▸') && text[5].contains("first"));
        // Little room: the chosen one is still shown.
        let rows = overview_lines(&list, Some(2), 40, 2);
        assert!(rows[0].to_string().contains("first"), "{rows:?}");
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
