//! A subagent's own view (T18): its stream over the session's, opened from its tile (a click) or `/agents`; its own
//! subagents open from it by number, Esc goes back one level.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style;
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

/// Why a subagent stopped, from its result: the reason after Glut's own words, one short line.
fn stopped_why(answer: &str) -> Option<String> {
    let line = answer.lines().next()?.trim();
    let line = line.strip_prefix("error: ").unwrap_or(line);
    // The loop's own words before the reason say nothing the mark does not; a reason with a colon stays whole.
    let why = line
        .strip_prefix("the subagent stopped without answering: ")
        .unwrap_or(line)
        .trim();
    (!why.is_empty()).then(|| cut(&crate::transcript::clean(why), 40))
}

/// The tab's rows at `width` for at most `room` rows (T69): a subagent that runs as a card — its task and state on
/// the frame, glowing; what it does now inside; its model, calls, tokens and time below —, one that ended as a quiet
/// line of how it ended and what it spent. `chosen` is marked; the list starts late enough for it to be seen. Each
/// line comes with its entry's place in `list`, for a click to find (T71).
pub fn overview_lines(
    list: &[(Vec<usize>, &Agent)],
    chosen: Option<usize>,
    width: usize,
    room: usize,
) -> Vec<(usize, Line<'static>)> {
    let mut entries: Vec<Vec<Line<'static>>> = Vec::new();
    for (at, (path, agent)) in list.iter().enumerate() {
        // Deep nesting stops indenting where the panel would have no room left for the task.
        let indent = "  ".repeat((path.len() - 1).min(width / 4));
        let picked = chosen == Some(at);
        let model = agent
            .model
            .as_deref()
            .map(|model| format!(" · {}", crate::transcript::clean(model)))
            .unwrap_or_default();
        let ran = agent
            .ran()
            .map(|ran| format!(" · {}", took(ran)))
            .unwrap_or_default();
        let spent = tr!(
            agent_spent,
            calls = agent.calls,
            tokens = crate::meter::tokens(agent.input_tokens + agent.output_tokens),
            model = model
        );
        if agent.state == ToolState::Running {
            let live = if agent.live.is_empty() {
                tr!(agent_starting).to_owned()
            } else {
                agent.live.clone()
            };
            let card = Card {
                title: &agent.task,
                state: tr!(agent_running),
                live: &live,
                live_style: theme::quiet(),
                stats: &format!("{spent}{ran}"),
                style: theme::glow(),
                picked,
            };
            let rows = card.lines(width.saturating_sub(indent.len()));
            entries.push(
                rows.into_iter()
                    .map(|mut row| {
                        row.spans.insert(0, Span::raw(indent.clone()));
                        row
                    })
                    .collect(),
            );
            continue;
        }
        // The mark in the theme's colour of how it ended, the line in plain ink: a quiet one went unseen beside the
        // cards (the owner's Mac test).
        let fine = Style::default().fg(theme::c().fine);
        let fault = Style::default().fg(theme::c().fault);
        let (mark, mark_style, end) = match agent.state {
            ToolState::Done => ("✓", fine, tr!(agent_done).to_owned()),
            // Why it stopped, as its result says it ("the subagent stopped without answering: Timeout").
            ToolState::Failed => match stopped_why(&agent.answer) {
                Some(why) => ("✗", fault, format!("{} ({why})", tr!(agent_stopped))),
                None => ("✗", fault, tr!(agent_stopped).to_owned()),
            },
            ToolState::Refused | ToolState::Running => ("⊘", fault, tr!(agent_refused).to_owned()),
        };
        let mut style = Style::default();
        if picked {
            style = style.add_modifier(ratatui::style::Modifier::BOLD);
        }
        let task = crate::transcript::clean(&agent.task).replace('\n', " ");
        let room = width.saturating_sub(1 + indent.len() + 2);
        entries.push(vec![Line::from(vec![
            Span::styled(if picked { "▸" } else { " " }, theme::glow()),
            Span::raw(indent.clone()),
            Span::styled(format!("{mark} "), mark_style),
            Span::styled(cut(&format!("{task} · {end} · {spent}{ran}"), room), style),
        ])]);
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
    for (at, entry) in entries.into_iter().enumerate().skip(first) {
        if lines.len() + entry.len() > room {
            // Too little room for a whole card: the chosen one is still seen, as its frame's top row.
            if lines.is_empty() && room > 0 {
                lines.extend(entry.into_iter().take(room).map(|line| (at, line)));
            }
            break;
        }
        lines.extend(entry.into_iter().map(|line| (at, line)));
    }
    lines
}

/// An agent as a card (T69), the stream's tile (T18) in the panel: the title and the state on the frame, what it does
/// now inside, its numbers below. Too narrow for a frame: one line.
pub struct Card<'a> {
    pub title: &'a str,
    pub state: &'a str,
    pub live: &'a str,
    pub live_style: ratatui::style::Style,
    pub stats: &'a str,
    /// The frame's: the glow while it works or asks.
    pub style: ratatui::style::Style,
    /// Chosen in the list: its title stands out.
    pub picked: bool,
}

impl Card<'_> {
    /// Its rows at `width`; text from a model or an agent's name is cleaned before it is drawn.
    pub fn lines(&self, width: usize) -> Vec<Line<'static>> {
        use unicode_width::UnicodeWidthStr;
        let one_line = |text: &str| crate::transcript::clean(text).replace('\n', " ");
        let title_style = if self.picked {
            theme::strong().add_modifier(ratatui::style::Modifier::REVERSED)
        } else {
            theme::strong()
        };
        let state = one_line(self.state);
        if width < 24 {
            return vec![Line::from(vec![
                Span::styled(if self.picked { "▸" } else { " " }, theme::glow()),
                Span::styled(
                    cut(
                        &format!("⬡ {} · {state}", one_line(self.title)),
                        width.saturating_sub(1),
                    ),
                    self.style,
                ),
            ])];
        }
        // Inside the side rows' "│ " and " │".
        let inner = width.saturating_sub(4).max(4);
        let state = cut(&state, inner / 2);
        // The top row: "╭─ ⬡ ", the title, " ─… ", the state, " ─╮" — ten cells of frame and at least one of line.
        let title = cut(
            &one_line(self.title),
            width.saturating_sub(state.width() + 11).max(1),
        );
        let fill = width.saturating_sub(title.width() + state.width() + 10);
        // Room above and below what it says, its last words on up to two rows, its numbers whole on their own rows.
        let padded = |text: String, style: ratatui::style::Style| {
            let pad = inner.saturating_sub(text.width());
            Line::from(vec![
                Span::styled("│ ", self.style),
                Span::styled(text, style),
                Span::raw(" ".repeat(pad)),
                Span::styled(" │", self.style),
            ])
        };
        let mut lines = vec![
            Line::from(vec![
                Span::styled("╭─ ⬡ ", self.style),
                Span::styled(title, title_style),
                Span::styled(format!(" {} ", "─".repeat(fill)), self.style),
                Span::styled(state, self.style),
                Span::styled(" ─╮", self.style),
            ]),
            padded(String::new(), self.style),
        ];
        lines.extend(
            folded(&one_line(self.live), inner, CARD_LIVE_ROWS)
                .into_iter()
                .map(|text| padded(text, self.live_style)),
        );
        lines.push(padded(String::new(), self.style));
        lines.extend(
            folded(&one_line(self.stats), inner, CARD_STATS_ROWS)
                .into_iter()
                .map(|text| padded(text, theme::quiet())),
        );
        lines.push(Line::from(Span::styled(
            format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
            self.style,
        )));
        lines
    }
}

/// How many rows a card gives an agent's last words, and its numbers (the owner's Mac test of T60a: more room).
const CARD_LIVE_ROWS: usize = 2;
const CARD_STATS_ROWS: usize = 2;

/// `next` after `text` as words go on: a space between, but none where Chinese or Japanese goes on — its characters
/// stand without one.
pub(crate) fn join_words(text: &mut String, next: &str) {
    let wide =
        |c: Option<char>| c.is_some_and(|c| unicode_width::UnicodeWidthChar::width(c) == Some(2));
    if !text.is_empty() && !wide(text.chars().next_back()) && !wide(next.chars().next()) {
        text.push(' ');
    }
    text.push_str(next);
}

/// `text` wrapped at `width` on at most `rows` rows, the last one cut where more was left; one empty row for none.
fn folded(text: &str, width: usize, rows: usize) -> Vec<String> {
    let mut wrapped = crate::transcript::wrap(text, width);
    if wrapped.len() > rows {
        let mut rest = String::new();
        for part in &wrapped[rows - 1..] {
            join_words(&mut rest, part);
        }
        wrapped.truncate(rows - 1);
        wrapped.push(cut(&rest, width));
    }
    if wrapped.is_empty() {
        wrapped.push(String::new());
    }
    wrapped
}

/// A background agent as the Agents tab shows it (T69), told by the screen once per frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Behind {
    pub name: String,
    pub role: String,
    pub model: String,
    pub doing: Doing,
    /// What it does now, what it asks, or the first line of its last answer.
    pub live: String,
    pub cost: Option<f64>,
    /// Its cache: `Some(true)` kept warm now, `Some(false)` seen but cold, `None` never seen.
    pub cache: Option<bool>,
    /// How long it has worked without a rest.
    pub working: Option<std::time::Duration>,
    /// The loop it runs (T60a): its interval and its rounds, `↻ 5m 3/50`.
    pub looping: Option<String>,
    /// The loop it ran, ended: `↻ ended 2/2`.
    pub looped: Option<String>,
}

/// A row of the Agents tab its keys may stand on (T34, T69): a background agent's card, by its name, or a subagent,
/// by its path — never a row number, so the choice stays on its agent as the tab reorders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Behind(String),
    Sub(Vec<usize>),
}

/// How a background agent stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Doing {
    Asks,
    Holds,
    Works,
    Rests,
}

impl Behind {
    /// One quiet line of it, where the tab is short of room: its name, how it stands, what it said last.
    pub fn line(&self, width: usize, picked: bool) -> Vec<Line<'static>> {
        let live = crate::transcript::clean(&self.live).replace('\n', " ");
        let mut style = theme::quiet();
        if picked {
            style = style.add_modifier(ratatui::style::Modifier::BOLD);
        }
        vec![Line::from(Span::styled(
            cut(
                &format!(
                    "{}⬡ {} · {} · {live}",
                    if picked { "▸" } else { " " },
                    crate::transcript::clean(&self.name),
                    tr!(agent_rests_state)
                ),
                width,
            ),
            style,
        ))]
    }

    /// Its card at `width`: glowing while it works or asks, what it asks in the glow; `picked` while the tab's keys
    /// stand on it.
    pub fn lines(&self, width: usize, picked: bool) -> Vec<Line<'static>> {
        let (state, style) = match self.doing {
            Doing::Asks => (tr!(agent_asks_state), theme::glow()),
            Doing::Holds => (tr!(agent_holds_state), theme::glow()),
            Doing::Works => (tr!(agent_works_state), theme::glow()),
            Doing::Rests => (tr!(agent_rests_state), theme::quiet()),
        };
        let mut stats = format!("{} · {}", self.role, self.model);
        if let Some(looping) = self.looping.as_ref().or(self.looped.as_ref()) {
            stats.push_str(&format!(" · {looping}"));
        }
        if let Some(cost) = self.cost {
            stats.push_str(&format!(" · {}", glut::lang::usd(cost, 4)));
        }
        match self.cache {
            Some(true) => stats.push_str(" · cache ●"),
            Some(false) => stats.push_str(" · cache ○"),
            None => {}
        }
        if let Some(working) = self.working.filter(|working| working.as_secs() >= 1) {
            stats.push_str(&format!(" · {}", took(working)));
        }
        let live_style = match self.doing {
            Doing::Asks => theme::glow(),
            Doing::Works => theme::strong(),
            Doing::Holds | Doing::Rests => theme::quiet(),
        };
        Card {
            title: &self.name,
            state,
            live: &self.live,
            live_style,
            stats: &stats,
            style,
            picked,
        }
        .lines(width)
    }
}

/// A run's time as a card says it: `45s`, `2:14`, `1:02:03`.
pub fn took(ran: std::time::Duration) -> String {
    let secs = ran.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}:{:02}", secs / 60, secs % 60),
        _ => format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn a_stopped_subagent_says_why_in_its_own_last_words() {
        assert_eq!(
            stopped_why("error: the subagent stopped without answering: Timeout").as_deref(),
            Some("Timeout")
        );
        assert_eq!(
            stopped_why("refused by the owner").as_deref(),
            Some("refused by the owner")
        );
        assert_eq!(stopped_why(""), None);
    }

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
        let rows = overview_lines(&list, Some(2), 40, 100)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        let text: Vec<String> = rows.iter().map(ToString::to_string).collect();
        // The running one is a card of six rows (room above and below its words), an ended one a single line.
        assert_eq!(text.len(), 6 + 1 + 1, "{text:?}");
        assert!(
            text[0].starts_with("╭─ ⬡ third") && text[0].ends_with("running ─╮"),
            "{text:?}"
        );
        assert!(text[2].starts_with("│ starting…"), "{text:?}");
        assert!(text[4].starts_with("│ 0 calls"), "{text:?}");
        assert!(text[5].starts_with("╰──"), "{text:?}");
        assert!(text[6].contains("✓ second · done · 0 calls"), "{text:?}");
        assert!(text[7].starts_with('▸') && text[7].contains("first"));
        assert!(
            text.iter().take(6).all(|row| row.width() == 40),
            "the card fills the width: {text:?}"
        );
        // Little room: the chosen one is still shown.
        let rows = overview_lines(&list, Some(2), 40, 1)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        assert!(rows[0].to_string().contains("first"), "{rows:?}");
        // A running card chosen with less room than it needs shows its top row.
        let rows = overview_lines(&list, Some(0), 40, 1)
            .into_iter()
            .map(|(_, line)| line)
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].to_string().starts_with("╭─ ⬡ third"), "{rows:?}");
    }

    #[test]
    fn a_background_agent_is_a_card_with_what_it_does_and_its_numbers() {
        let behind = Behind {
            name: "reviewer".into(),
            role: "reviewer".into(),
            model: "m/x".into(),
            doing: Doing::Works,
            live: "bash cargo test\u{202e}".into(),
            cost: Some(0.0123),
            cache: Some(true),
            working: Some(std::time::Duration::from_secs(134)),
            looping: None,
            looped: None,
        };
        let text: Vec<String> = behind
            .lines(48, false)
            .iter()
            .map(ToString::to_string)
            .collect();
        // Room above and below its words; its numbers whole, wrapped rather than cut.
        assert_eq!(text.len(), 6, "{text:?}");
        assert!(
            text[0].starts_with("╭─ ⬡ reviewer") && text[0].ends_with("works ─╮"),
            "{text:?}"
        );
        assert!(text[1].trim_matches(|c| c == '│' || c == ' ').is_empty());
        assert!(
            text[2].starts_with("│ bash cargo test ") && !text[2].contains('\u{202e}'),
            "{text:?}"
        );
        assert!(text[3].trim_matches(|c| c == '│' || c == ' ').is_empty());
        assert!(
            text[4].contains("reviewer · m/x · $0.0123 · cache ● · 2:14"),
            "{text:?}"
        );
        let long = Behind {
            live: "word ".repeat(40),
            looped: Some("↻ ended 2/2".into()),
            ..behind.clone()
        };
        let text: Vec<String> = long
            .lines(48, false)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            text.len(),
            8,
            "two rows of words at the most, the numbers on two: {text:?}"
        );
        assert!(text[3].contains('…'), "{text:?}");
        assert!(
            text[5].contains("↻ ended 2/2"),
            "an ended loop says so: {text:?}"
        );
        assert!(text.iter().all(|row| row.width() == 48), "{text:?}");
        let narrow = behind.lines(20, false);
        assert_eq!(narrow.len(), 1, "too narrow for a frame");
        assert_eq!(took(std::time::Duration::from_secs(3723)), "1:02:03");
        assert_eq!(took(std::time::Duration::from_secs(45)), "45s");
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
