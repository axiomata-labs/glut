//! Selection, copy and search in the stream (T3, TUI2b). A place in the stream is a line of it — counted from the
//! stream's first line, at the width it was laid out for — and a column of cells, so a selection stays on its text
//! while the stream scrolls; a new width lays the stream out anew and lets the selection go.

use std::io::Write as _;
use std::process::{Command, Stdio};

use ratatui::text::Line;
use unicode_width::UnicodeWidthChar;

/// A place in the stream: its line and a column of cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Point {
    pub line: usize,
    pub col: usize,
}

/// A selection: where the drag began and where it is now, at the width the stream had.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: Point,
    pub head: Point,
    pub width: u16,
}

impl Selection {
    /// Its two ends in order.
    pub fn ends(&self) -> (Point, Point) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    /// The columns of `line` it covers, from and to (end excluded); `None` when it does not touch the line.
    pub fn columns(&self, line: usize, line_width: usize) -> Option<(usize, usize)> {
        let (from, to) = self.ends();
        if line < from.line || line > to.line {
            return None;
        }
        let start = if line == from.line { from.col } else { 0 };
        // The cell under the pointer is part of it, at either end.
        let end = if line == to.line {
            to.col.saturating_add(1)
        } else {
            line_width
        };
        (start < end).then_some((start, end))
    }
}

/// A line's text as the screen shows it.
pub fn text_of(line: &Line) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

/// The part of `text` from column `start` to column `end` (cells): a wide character is taken when any of its cells
/// is inside, a combining mark goes with the character before it.
pub fn columns_of(text: &str, start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut col = 0;
    let mut took = false;
    for c in text.chars() {
        let cells = c.width().unwrap_or(0);
        if cells == 0 {
            if took {
                out.push(c);
            }
            continue;
        }
        if col >= end {
            break;
        }
        took = col + cells > start;
        if took {
            out.push(c);
        }
        col += cells;
    }
    out
}

/// What a selection holds of `lines` (the stream's, from its first line): each line's part, its trailing spaces
/// left out, the lines joined by line breaks.
pub fn selected_text(selection: &Selection, lines: &[String]) -> String {
    let (from, to) = selection.ends();
    let mut parts = Vec::new();
    let taken = lines
        .iter()
        .enumerate()
        .skip(from.line)
        .take(to.line + 1 - from.line);
    for (at, text) in taken {
        let Some((start, end)) = selection.columns(at, usize::MAX) else {
            parts.push(String::new());
            continue;
        };
        parts.push(columns_of(text, start, end).trim_end().to_owned());
    }
    parts.join("\n")
}

/// Where `query` stands in `lines`, in any case: each match's line and columns (start, end).
pub fn matches(query: &str, lines: &[String]) -> Vec<(usize, usize, usize)> {
    // Lowered one to one, as the text is below: a letter whose lower case is longer (`İ`) still meets itself.
    let wanted: Vec<char> = query.chars().map(lower).collect();
    if wanted.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (at, text) in lines.iter().enumerate() {
        // Each character with the column it starts at, lowered one to one so the columns stay where they are.
        let chars: Vec<(char, usize, usize)> = {
            let mut col = 0;
            text.chars()
                .map(|c| {
                    let cells = c.width().unwrap_or(0);
                    let lowered = lower(c);
                    let place = (lowered, col, cells);
                    col += cells;
                    place
                })
                .collect()
        };
        let mut start = 0;
        while start + wanted.len() <= chars.len() {
            let hit = chars[start..start + wanted.len()]
                .iter()
                .zip(&wanted)
                .all(|((c, _, _), w)| c == w);
            if hit {
                let (_, from, _) = chars[start];
                let (_, last, cells) = chars[start + wanted.len() - 1];
                found.push((at, from, last + cells));
                start += wanted.len();
            } else {
                start += 1;
            }
        }
    }
    found
}

/// A character in lower case, one for one.
fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// How the text reached the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Copied {
    /// macOS's own clipboard, through `pbcopy`.
    Pbcopy,
    /// The terminal's, through OSC 52 (over SSH, in tmux, where there is no `pbcopy`).
    Terminal,
}

/// Puts `text` on the clipboard: `pbcopy` where it is, else the terminal's (OSC 52).
///
/// # Errors
///
/// A message when neither took it.
pub fn copy(text: &str) -> Result<Copied, String> {
    if pbcopy(text).is_ok() {
        return Ok(Copied::Pbcopy);
    }
    crossterm::execute!(
        std::io::stdout(),
        crossterm::clipboard::CopyToClipboard::to_clipboard_from(text)
    )
    .map(|()| Copied::Terminal)
    .map_err(|err| format!("no clipboard took it: {err}"))
}

fn pbcopy(text: &str) -> std::io::Result<()> {
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // The child is waited for whatever the write did: never left behind as a zombie.
    let written = child
        .stdin
        .take()
        .map_or(Ok(()), |mut stdin| stdin.write_all(text.as_bytes()));
    let status = child.wait()?;
    written?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("pbcopy failed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: usize, col: usize) -> Point {
        Point { line, col }
    }

    #[test]
    fn a_selection_takes_its_columns_on_its_first_and_last_line_and_all_between() {
        let lines = vec![
            "first line".to_owned(),
            "middle".to_owned(),
            "last line here".to_owned(),
        ];
        let selection = Selection {
            anchor: at(2, 4),
            head: at(0, 6),
            width: 80,
        };
        assert_eq!(selected_text(&selection, &lines), "line\nmiddle\nlast");
        assert_eq!(selection.columns(1, 6), Some((0, 6)));
        assert_eq!(selection.columns(3, 6), None);
        assert_eq!(
            columns_of("日本語", 2, 4),
            "本",
            "a wide character counts two cells"
        );
        assert_eq!(
            columns_of("日本語", 3, 4),
            "本",
            "taken when the selection starts on its right half"
        );
        assert_eq!(
            columns_of("e\u{301}x", 0, 1),
            "e\u{301}",
            "a combining mark goes with its letter"
        );
    }

    #[test]
    fn search_finds_every_match_in_any_case_with_its_columns() {
        let lines = vec![
            "Glut and glut".to_owned(),
            "日本 GLUT".to_owned(),
            "none".to_owned(),
        ];
        assert_eq!(matches("glut", &lines), [(0, 0, 4), (0, 9, 13), (1, 5, 9)]);
        assert_eq!(matches("İ", &["İ".to_owned()]).len(), 1);
        assert!(matches("", &lines).is_empty());
    }
}
