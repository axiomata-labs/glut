//! The stream (T2, T6): the session as blocks — the owner's messages, Glut's answers as they stream, one line per tool
//! call that unfolds, notes. Each block is wrapped to the screen's width once and kept until it changes, so a long
//! session costs only the blocks that are on the screen.

use glut::Event;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme;

/// How many lines of a tool's result an unfolded tool line shows.
const RESULT_LINES: usize = 40;

/// How a tool call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
    Refused,
}

/// One block of the stream.
#[derive(Debug, Clone)]
pub enum Kind {
    Owner(String),
    /// Glut's text; `streaming` while it still grows.
    Answer {
        text: String,
        streaming: bool,
    },
    Tool {
        id: String,
        name: String,
        target: String,
        state: ToolState,
        /// The gate's word when it was not a plain allow: "asked", a refusal's reason.
        gate: Option<String>,
        output: String,
        unfolded: bool,
        /// How deep in subagents the call is (HS8): 0 for the session's own.
        depth: u32,
    },
    Note {
        text: String,
        tone: Tone,
    },
}

/// A note's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Quiet,
    Fault,
    Fine,
}

/// A block and its wrapped lines for the width they were made for.
#[derive(Debug, Clone)]
pub struct Block {
    pub kind: Kind,
    wrapped: Option<(u16, Vec<Line<'static>>)>,
}

impl Block {
    fn new(kind: Kind) -> Self {
        Block {
            kind,
            wrapped: None,
        }
    }

    /// The block changed: its lines are made again when next drawn.
    fn touch(&mut self) {
        self.wrapped = None;
    }

    /// The block's lines at `width`, made once per change and width.
    pub fn lines(&mut self, width: u16) -> &[Line<'static>] {
        if self.wrapped.as_ref().is_none_or(|(at, _)| *at != width) {
            let lines = render(&self.kind, width);
            self.wrapped = Some((width, lines));
        }
        self.wrapped
            .as_ref()
            .map(|(_, lines)| lines.as_slice())
            .unwrap_or(&[])
    }
}

/// The whole stream.
#[derive(Debug, Default)]
pub struct Transcript {
    pub blocks: Vec<Block>,
}

impl Transcript {
    pub fn push(&mut self, kind: Kind) {
        self.blocks.push(Block::new(kind));
    }

    pub fn note(&mut self, text: impl Into<String>, tone: Tone) {
        self.push(Kind::Note {
            text: text.into(),
            tone,
        });
    }

    /// Takes in one step of a run.
    pub fn take(&mut self, event: &Event) {
        self.take_at(event, 0);
    }

    fn take_at(&mut self, event: &Event, depth: u32) {
        match event {
            Event::TextDelta { text, .. } if depth == 0 => self.stream(text),
            Event::AssistantText { text, .. } if depth == 0 => self.answered(text),
            Event::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                // A streamed answer that stopped for a tool call is complete: the call comes after it.
                self.close_answer();
                self.push(Kind::Tool {
                    id: id.clone(),
                    name: name.clone(),
                    target: target_of(arguments),
                    state: ToolState::Running,
                    gate: None,
                    output: String::new(),
                    unfolded: false,
                    depth,
                });
            }
            Event::Permission {
                id,
                allowed,
                reason,
                ..
            } => {
                if let Some(block) = self.tool(id) {
                    if let Kind::Tool { gate, state, .. } = &mut block.kind {
                        if !allowed {
                            *state = ToolState::Refused;
                            *gate = Some(reason.clone());
                        } else if reason.contains("owner") || reason.contains("asked") {
                            *gate = Some(reason.clone());
                        }
                    }
                    block.touch();
                }
            }
            Event::ToolResult { id, ok, output, .. } => {
                if let Some(block) = self.tool(id) {
                    if let Kind::Tool {
                        state,
                        output: kept,
                        ..
                    } = &mut block.kind
                    {
                        if *state != ToolState::Refused {
                            *state = if *ok {
                                ToolState::Done
                            } else {
                                ToolState::Failed
                            };
                        }
                        kept.clone_from(output);
                    }
                    block.touch();
                }
            }
            Event::Compacted {
                before_tokens,
                after_tokens,
                ..
            } => self.note(
                format!(
                    "compacted the conversation: {}k → {}k tokens",
                    before_tokens / 1000,
                    after_tokens / 1000
                ),
                Tone::Quiet,
            ),
            Event::Error { message } if depth == 0 => self.note(message.clone(), Tone::Fault),
            Event::ModeChanged { mode, .. } => self.note(format!("now in {mode} mode"), Tone::Fine),
            Event::Sub {
                depth: inner,
                event,
            } => self.take_at(event, *inner),
            _ => {}
        }
    }

    /// A piece of Glut's answer as it streams.
    fn stream(&mut self, piece: &str) {
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Answer {
                text,
                streaming: true,
            } = &mut block.kind
        {
            text.push_str(piece);
            block.touch();
            return;
        }
        self.push(Kind::Answer {
            text: piece.to_owned(),
            streaming: true,
        });
    }

    /// The answer's whole text: it replaces what streamed, or stands alone when nothing did.
    fn answered(&mut self, whole: &str) {
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Answer { text, streaming } = &mut block.kind
            && *streaming
        {
            whole.clone_into(text);
            *streaming = false;
            block.touch();
            return;
        }
        if !whole.trim().is_empty() {
            self.push(Kind::Answer {
                text: whole.to_owned(),
                streaming: false,
            });
        }
    }

    fn close_answer(&mut self) {
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Answer { streaming, .. } = &mut block.kind
            && *streaming
        {
            *streaming = false;
            block.touch();
        }
    }

    /// The turn ended: whatever still runs or streams is settled.
    pub fn settle(&mut self) {
        self.close_answer();
        for block in &mut self.blocks {
            if let Kind::Tool { state, .. } = &mut block.kind
                && *state == ToolState::Running
            {
                *state = ToolState::Failed;
                block.touch();
            }
        }
    }

    fn tool(&mut self, wanted: &str) -> Option<&mut Block> {
        self.blocks
            .iter_mut()
            .rev()
            .find(|block| matches!(&block.kind, Kind::Tool { id, .. } if id == wanted))
    }

    /// Unfolds every tool line, or — when all are unfolded already — folds them all.
    pub fn unfold_all(&mut self) {
        let open = self.blocks.iter().any(|block| {
            matches!(
                block.kind,
                Kind::Tool {
                    unfolded: false,
                    ..
                }
            )
        });
        for block in &mut self.blocks {
            if let Kind::Tool { unfolded, .. } = &mut block.kind {
                *unfolded = open;
                block.touch();
            }
        }
    }

    /// Folds or unfolds block `at`, when it is a tool line.
    pub fn toggle(&mut self, at: usize) {
        if let Some(block) = self.blocks.get_mut(at)
            && let Kind::Tool { unfolded, .. } = &mut block.kind
        {
            *unfolded = !*unfolded;
            block.touch();
        }
    }
}

/// What a tool call works on, from its arguments: the command, the path, the pattern — else the arguments cut short.
fn target_of(arguments: &str) -> String {
    const KEYS: [&str; 8] = [
        "command",
        "path",
        "file_path",
        "pattern",
        "url",
        "query",
        "description",
        "code",
    ];
    let found = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| {
            KEYS.iter().find_map(|key| {
                value
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
        });
    let text = found.unwrap_or_else(|| arguments.to_owned());
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    cut(&line, 160)
}

/// `text` cut to `max` cells, with an ellipsis when it was longer.
fn cut(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let mut short = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cells = cell_width(c);
        if used + cells + 1 > max {
            break;
        }
        short.push(c);
        used += cells;
    }
    short.push('…');
    short
}

/// A block as lines of the screen's width.
fn render(kind: &Kind, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width.max(8));
    let mut lines = Vec::new();
    match kind {
        Kind::Owner(text) => {
            lines.push(Line::default());
            let style = Style::default().fg(theme::OWNER);
            for (at, row) in wrap(text, width - 2).into_iter().enumerate() {
                let mark = if at == 0 { "› " } else { "  " };
                lines.push(Line::from(vec![
                    Span::styled(mark, theme::quiet()),
                    Span::styled(row, style),
                ]));
            }
        }
        Kind::Answer { text, streaming } => {
            lines.push(Line::default());
            for row in wrap(text, width) {
                lines.push(Line::from(row));
            }
            if *streaming {
                lines.push(Line::from(Span::styled("▍", theme::glow())));
            }
        }
        Kind::Tool {
            name,
            target,
            state,
            gate,
            output,
            unfolded,
            depth,
            ..
        } => {
            let indent = "  ".repeat(*depth as usize);
            let (mark, mark_style) = match state {
                ToolState::Running => ("◆", theme::glow()),
                ToolState::Done => ("◇", theme::quiet()),
                ToolState::Failed => ("✕", Style::default().fg(theme::FAULT)),
                ToolState::Refused => ("⊘", Style::default().fg(theme::FAULT)),
            };
            // The head is one line: the name and the gate's word cut first to what fits, the target gets the rest.
            let name = cut(&clean(name), 32);
            let gate = gate
                .as_deref()
                .map(|gate| format!("  ({})", cut(&clean(gate), 48)));
            let used = indent.len()
                + 2
                + name.width()
                + 1
                + gate.as_deref().map_or(0, UnicodeWidthStr::width);
            let room = width.saturating_sub(used).max(8);
            let mut head = vec![
                Span::raw(indent.clone()),
                Span::styled(format!("{mark} "), mark_style),
                Span::styled(name, theme::strong()),
                Span::raw(" "),
                Span::styled(cut(&clean(target), room), theme::quiet()),
            ];
            if let Some(gate) = gate {
                head.push(Span::styled(gate, theme::quiet()));
            }
            lines.push(Line::from(head));
            if *unfolded && !output.is_empty() {
                let shown: Vec<&str> = output.lines().collect();
                for row in shown.iter().take(RESULT_LINES) {
                    for part in wrap(row, width.saturating_sub(indent.len() + 4)) {
                        lines.push(Line::from(vec![
                            Span::raw(format!("{indent}  │ ")),
                            Span::styled(part, theme::quiet()),
                        ]));
                    }
                }
                if shown.len() > RESULT_LINES {
                    lines.push(Line::from(Span::styled(
                        format!("{indent}  │ … {} more lines", shown.len() - RESULT_LINES),
                        theme::quiet().add_modifier(Modifier::ITALIC),
                    )));
                }
            }
        }
        Kind::Note { text, tone } => {
            let style = match tone {
                Tone::Quiet => theme::quiet(),
                Tone::Fault => Style::default().fg(theme::FAULT),
                Tone::Fine => Style::default().fg(theme::FINE),
            };
            for row in wrap(text, width - 2) {
                lines.push(Line::from(vec![
                    Span::styled("· ", theme::quiet()),
                    Span::styled(row, style),
                ]));
            }
        }
    }
    lines
}

/// `text` broken into rows of at most `width` cells: at spaces where it can (the spaces at a break dropped), inside a
/// word where it must. What a model or a tool sent is shown, never obeyed: control characters become spaces, and
/// characters that turn or hide text (bidi overrides, zero-width marks) are left out — an asked command must read as
/// it would run.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in clean(text).split('\n') {
        let mut row = String::new();
        let mut used = 0;
        // After a break the spaces that caused it are dropped; a line's own indentation is kept.
        let mut broke = false;
        for part in parts(line) {
            let part_width: usize = part.chars().map(cell_width).sum();
            if part.starts_with(' ') {
                if broke && used == 0 {
                    continue;
                }
                if used + part_width > width {
                    rows.push(std::mem::take(&mut row).trim_end().to_owned());
                    used = 0;
                    broke = true;
                    continue;
                }
                row.push_str(part);
                used += part_width;
                continue;
            }
            if used + part_width > width && used > 0 {
                rows.push(std::mem::take(&mut row).trim_end().to_owned());
                used = 0;
                broke = true;
            }
            for c in part.chars() {
                let cells = cell_width(c);
                if used + cells > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                    broke = true;
                }
                row.push(c);
                used += cells;
            }
        }
        rows.push(row);
    }
    rows
}

/// `text` as it may reach the terminal: control characters (but the line break) as spaces, characters that reorder or
/// hide text left out.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !hides(*c))
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

/// Bidi controls and zero-width characters: they make what is shown differ from what is there.
fn hides(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}'
    )
}

/// A character's cells.
fn cell_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// `line` as runs of spaces and runs of everything else, in order.
fn parts(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    for (at, c) in line.char_indices() {
        let space = c == ' ';
        if in_space.is_some_and(|was| was != space) {
            parts.push(&line[start..at]);
            start = at;
        }
        in_space = Some(space);
    }
    if start < line.len() {
        parts.push(&line[start..]);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapping_breaks_at_spaces_counts_cells_and_never_passes_the_width() {
        assert_eq!(wrap("the quick brown fox", 10), ["the quick", "brown fox"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("日本語テキスト", 6), ["日本語", "テキス", "ト"]);
        assert_eq!(wrap("one\n\ntwo", 10), ["one", "", "two"]);
        assert_eq!(
            wrap("a\u{1b}[2Jb", 10),
            ["a [2Jb"],
            "an escape is shown, not sent"
        );
        for row in wrap("lorem ipsum dolor sit amet, consectetur 日本 adipiscing", 7) {
            assert!(row.width() <= 7, "{row:?}");
        }
    }

    #[test]
    fn a_turn_streams_into_one_answer_and_tools_unfold() {
        let mut stream = Transcript::default();
        stream.take(&Event::TextDelta {
            turn: 1,
            text: "Let me ".into(),
        });
        stream.take(&Event::TextDelta {
            turn: 1,
            text: "look.".into(),
        });
        stream.take(&Event::ToolCall {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
        });
        stream.take(&Event::ToolResult {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            ok: true,
            output: "fn main() {}".into(),
            truncated: false,
        });
        stream.take(&Event::AssistantText {
            turn: 2,
            text: "Done.".into(),
        });
        assert_eq!(stream.blocks.len(), 3);
        assert!(
            matches!(&stream.blocks[0].kind, Kind::Answer { text, streaming: false } if text == "Let me look.")
        );
        assert!(matches!(
            &stream.blocks[1].kind,
            Kind::Tool { target, state: ToolState::Done, .. } if target == "src/main.rs"
        ));
        assert_eq!(stream.blocks[1].lines(80).len(), 1, "folded: one line");
        stream.toggle(1);
        assert_eq!(
            stream.blocks[1].lines(80).len(),
            2,
            "unfolded: the result below"
        );
        assert!(matches!(&stream.blocks[2].kind, Kind::Answer { text, .. } if text == "Done."));
    }

    #[test]
    fn a_refused_call_stays_refused_and_a_cut_turn_settles() {
        let mut stream = Transcript::default();
        for (id, name) in [("c1", "run_command"), ("c2", "edit_file")] {
            stream.take(&Event::ToolCall {
                turn: 1,
                id: id.into(),
                name: name.into(),
                arguments: "{}".into(),
            });
        }
        stream.take(&Event::Permission {
            turn: 1,
            id: "c1".into(),
            tool: "run_command".into(),
            target: "rm -rf /".into(),
            allowed: false,
            reason: "rule Bash(rm:*)".into(),
        });
        stream.take(&Event::ToolResult {
            turn: 1,
            id: "c1".into(),
            name: "run_command".into(),
            ok: false,
            output: "refused".into(),
            truncated: false,
        });
        stream.settle();
        assert!(matches!(
            &stream.blocks[0].kind,
            Kind::Tool {
                state: ToolState::Refused,
                gate: Some(_),
                ..
            }
        ));
        assert!(matches!(
            &stream.blocks[1].kind,
            Kind::Tool {
                state: ToolState::Failed,
                ..
            }
        ));
    }
}
