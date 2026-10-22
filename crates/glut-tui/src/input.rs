//! The input (T9, first part): several lines, a cursor, word motions and a history of what was sent. Soft-wrapped to
//! the box's width; the cursor's place on the screen is worked out from the same wrap.

use unicode_width::UnicodeWidthChar;

/// The most messages the history keeps.
const HISTORY: usize = 200;
/// A paste of more lines than this, or more characters, stands in the input as one short line (T9).
const PASTE_LINES: usize = 10;
const PASTE_CHARS: usize = 2_000;

/// A message of the history, or the draft: its text with its marks, and the pastes they stand for.
#[derive(Debug, Clone, Default, PartialEq)]
struct Entry {
    text: String,
    pastes: Vec<(String, String)>,
}

#[derive(Debug, Default)]
pub struct Input {
    /// The text, one string per line; never empty.
    lines: Vec<String>,
    /// The cursor: line, and character within it.
    row: usize,
    col: usize,
    /// What was sent, as it stood in the input — its paste marks and what they stand for kept beside it.
    history: Vec<Entry>,
    /// Where in the history Up has gone: `None` while editing a new message.
    browsing: Option<usize>,
    /// The new message kept while the history is browsed.
    draft: Option<Entry>,
    /// The large pastes standing in the input as `[pasted 240 lines #1]`: each mark and the text it stands for.
    pastes: Vec<(String, String)>,
}

impl Input {
    pub fn new() -> Self {
        Input {
            lines: vec![String::new()],
            ..Input::default()
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    #[cfg(test)]
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    fn set(&mut self, text: &str) {
        self.lines = text.split('\n').map(str::to_owned).collect();
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].chars().count();
    }

    pub fn clear(&mut self) {
        self.set("");
        self.browsing = None;
        self.draft = None;
        self.pastes.clear();
    }

    /// The text with every paste mark in it given back as what was pasted — each mark once, found in the text as
    /// typed, never inside a paste already given back.
    pub fn expanded(&self) -> String {
        let text = self.text();
        let mut out = String::new();
        let mut rest = text.as_str();
        let mut used = vec![false; self.pastes.len()];
        loop {
            let next = self
                .pastes
                .iter()
                .enumerate()
                .filter(|(at, _)| !used[*at])
                .filter_map(|(at, (mark, _))| rest.find(mark.as_str()).map(|found| (found, at)))
                .min();
            let Some((found, at)) = next else {
                out.push_str(rest);
                return out;
            };
            used[at] = true;
            let (mark, pasted) = &self.pastes[at];
            out.push_str(&rest[..found]);
            out.push_str(pasted);
            rest = &rest[found + mark.len()..];
        }
    }

    /// How many pastes lost their mark to an edit: they would not be sent.
    pub fn broken_pastes(&self) -> usize {
        let text = self.text();
        self.pastes
            .iter()
            .filter(|(mark, _)| !text.contains(mark.as_str()))
            .count()
    }

    fn entry(&self) -> Entry {
        Entry {
            text: self.text(),
            pastes: self.pastes.clone(),
        }
    }

    fn restore(&mut self, entry: &Entry) {
        self.set(&entry.text);
        self.pastes.clone_from(&entry.pastes);
    }

    /// A paste: a large one stands in the input as one mark, the text kept until it is sent; a small one is typed.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let lines = text.lines().count();
        if lines <= PASTE_LINES && text.chars().count() <= PASTE_CHARS {
            return self.insert_text(&text);
        }
        let lines = if lines == 1 {
            "1 line".to_owned()
        } else {
            format!("{lines} lines")
        };
        let mark = format!("[pasted {lines} #{}]", self.pastes.len() + 1);
        for c in mark.chars() {
            self.insert(c);
        }
        let kept: String = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        self.pastes.push((mark, kept));
    }

    /// Takes the message out to send it — the pastes given back in it — and keeps it in the history.
    pub fn take(&mut self) -> String {
        let text = self.expanded();
        let entry = self.entry();
        if !text.trim().is_empty() && self.history.last() != Some(&entry) {
            self.history.push(entry);
            if self.history.len() > HISTORY {
                self.history.remove(0);
            }
        }
        self.clear();
        text
    }

    fn byte(&self, row: usize, col: usize) -> usize {
        self.lines[row]
            .char_indices()
            .nth(col)
            .map_or(self.lines[row].len(), |(at, _)| at)
    }

    fn len(&self, row: usize) -> usize {
        self.lines[row].chars().count()
    }

    pub fn insert(&mut self, c: char) {
        self.browsing = None;
        let at = self.byte(self.row, self.col);
        self.lines[self.row].insert(at, c);
        self.col += 1;
    }

    /// A paste: its line breaks become lines; other control characters are dropped.
    pub fn insert_text(&mut self, text: &str) {
        for c in text.replace("\r\n", "\n").replace('\r', "\n").chars() {
            match c {
                '\n' => self.newline(),
                '\t' => self.insert(' '),
                c if c.is_control() => {}
                c => self.insert(c),
            }
        }
    }

    /// Whether the cursor stands where a word begins: at a line's start or after a space.
    pub fn at_word_start(&self) -> bool {
        self.col == 0
            || self.lines[self.row]
                .chars()
                .nth(self.col - 1)
                .is_some_and(char::is_whitespace)
    }

    pub fn newline(&mut self) {
        self.browsing = None;
        let at = self.byte(self.row, self.col);
        let rest = self.lines[self.row].split_off(at);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        // A paste's mark goes as a whole, and the paste with it: half a mark would send neither.
        let before: String = self.lines[self.row].chars().take(self.col).collect();
        if let Some(at) = self
            .pastes
            .iter()
            .position(|(mark, _)| before.ends_with(mark.as_str()))
        {
            let (mark, _) = self.pastes.remove(at);
            for _ in 0..mark.chars().count() {
                self.backspace();
            }
            return;
        }
        if self.col > 0 {
            let at = self.byte(self.row, self.col - 1);
            self.lines[self.row].remove(at);
            self.col -= 1;
        } else if self.row > 0 {
            let line = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.len(self.row);
            self.lines[self.row].push_str(&line);
        }
    }

    pub fn delete(&mut self) {
        if self.col < self.len(self.row) {
            let at = self.byte(self.row, self.col);
            self.lines[self.row].remove(at);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    /// Deletes the word before the cursor (Ctrl+W, Alt+Backspace).
    pub fn delete_word(&mut self) {
        let start = self.word_start();
        while self.col > start {
            self.backspace();
        }
    }

    /// Deletes from the start of the line to the cursor (Ctrl+U).
    pub fn delete_to_start(&mut self) {
        while self.col > 0 {
            self.backspace();
        }
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.len(self.row);
        }
    }

    pub fn right(&mut self) {
        if self.col < self.len(self.row) {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    fn word_start(&self) -> usize {
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let mut at = self.col;
        while at > 0 && chars[at - 1].is_whitespace() {
            at -= 1;
        }
        while at > 0 && !chars[at - 1].is_whitespace() {
            at -= 1;
        }
        at
    }

    pub fn word_left(&mut self) {
        if self.col == 0 {
            self.left();
        } else {
            self.col = self.word_start();
        }
    }

    pub fn word_right(&mut self) {
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        if self.col == chars.len() {
            self.right();
            return;
        }
        let mut at = self.col;
        while at < chars.len() && chars[at].is_whitespace() {
            at += 1;
        }
        while at < chars.len() && !chars[at].is_whitespace() {
            at += 1;
        }
        self.col = at;
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.len(self.row);
    }

    /// Up: the line above, or — on the first line — the message sent before.
    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.len(self.row));
            return;
        }
        let next = match self.browsing {
            None if self.history.is_empty() => return,
            None => {
                self.draft = Some(self.entry());
                self.history.len() - 1
            }
            Some(0) => return,
            Some(at) => at - 1,
        };
        let entry = self.history[next].clone();
        self.restore(&entry);
        self.browsing = Some(next);
    }

    /// Down: the line below, or — on the last line while browsing — the message sent after.
    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.len(self.row));
            return;
        }
        match self.browsing {
            Some(at) if at + 1 < self.history.len() => {
                let entry = self.history[at + 1].clone();
                self.restore(&entry);
                self.browsing = Some(at + 1);
            }
            Some(_) => {
                let draft = self.draft.take().unwrap_or_default();
                self.restore(&draft);
                self.browsing = None;
            }
            None => {}
        }
    }

    /// The text as rows of at most `width` cells, and the cursor's row and column among them.
    pub fn layout(&self, width: usize) -> (Vec<String>, (usize, usize)) {
        let width = width.max(2);
        let mut rows = Vec::new();
        let mut cursor = (0, 0);
        for (line_at, line) in self.lines.iter().enumerate() {
            let mut row = String::new();
            let mut used = 0;
            for (char_at, c) in line.chars().enumerate() {
                let cells = c.width().unwrap_or(0);
                if used + cells > width {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                if line_at == self.row && char_at == self.col {
                    cursor = (rows.len(), used);
                }
                row.push(c);
                used += cells;
            }
            if line_at == self.row && self.col >= line.chars().count() {
                if used >= width {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                cursor = (rows.len(), used);
            }
            rows.push(row);
        }
        (rows, cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(text: &str) -> Input {
        let mut input = Input::new();
        input.insert_text(text);
        input
    }

    #[test]
    fn typing_newlines_and_deleting_edit_the_lines() {
        let mut input = typed("hello wörld\nsecond");
        assert_eq!(input.line_count(), 2);
        input.home();
        input.backspace();
        assert_eq!(
            input.text(),
            "hello wörldsecond",
            "backspace at a line's start joins it to the one above"
        );
        input.delete_word();
        assert_eq!(input.text(), "hello second");
        input.end();
        input.word_left();
        input.delete_to_start();
        assert_eq!(input.text(), "second");
        let mut pasted = typed("a\r\nb\tc\u{1b}");
        assert_eq!(pasted.take(), "a\nb c");
        assert!(pasted.is_empty());
    }

    #[test]
    fn a_large_paste_stands_as_one_mark_and_comes_back_whole_when_sent() {
        let mut input = typed("look: ");
        let long: String = (1..=40).map(|n| format!("line {n}\n")).collect();
        input.paste(&long);
        input.insert_text(" thanks");
        assert_eq!(input.text(), "look: [pasted 40 lines #1] thanks");
        assert_eq!(input.line_count(), 1);
        let sent = input.take();
        assert!(sent.starts_with("look: line 1\nline 2"), "{sent:?}");
        assert!(sent.ends_with("line 40\n thanks"), "{sent:?}");
        let mut small = Input::new();
        small.paste("a\nb");
        assert_eq!(small.text(), "a\nb", "a small paste is typed");
    }

    #[test]
    fn a_mark_goes_as_a_whole_and_history_keeps_the_mark_not_the_text() {
        let long: String = (1..=20).map(|n| format!("{n}\n")).collect();
        let mut input = Input::new();
        input.paste(&long);
        input.paste(&format!("[pasted 20 lines #1]\n{long}"));
        assert_eq!(input.text(), "[pasted 20 lines #1][pasted 21 lines #2]");
        let sent = input.expanded();
        assert!(
            sent.starts_with("1\n2\n") && sent.contains("20\n[pasted 20 lines #1]\n1\n"),
            "{sent:?}"
        );
        input.backspace();
        assert_eq!(
            input.text(),
            "[pasted 20 lines #1]",
            "the second mark went whole"
        );
        assert_eq!(input.broken_pastes(), 0);
        input.take();
        input.up();
        assert_eq!(
            input.text(),
            "[pasted 20 lines #1]",
            "the history shows the mark"
        );
        assert!(
            input.expanded().starts_with("1\n2\n"),
            "and still sends the paste"
        );
    }

    #[test]
    fn up_and_down_walk_the_lines_then_the_history_and_bring_the_draft_back() {
        let mut input = Input::new();
        input.insert_text("first");
        input.take();
        input.insert_text("second");
        input.take();
        input.insert_text("draft");
        input.up();
        assert_eq!(input.text(), "second");
        input.up();
        assert_eq!(input.text(), "first");
        input.up();
        assert_eq!(input.text(), "first", "the oldest stays");
        input.down();
        input.down();
        assert_eq!(input.text(), "draft");
        let mut two = typed("one\ntwo");
        two.up();
        assert_eq!(
            two.text(),
            "one\ntwo",
            "Up on the second line moves to the first, no history"
        );
    }

    #[test]
    fn the_layout_wraps_by_cells_and_finds_the_cursor() {
        let input = typed("abcdef");
        assert_eq!(
            input.layout(4),
            (vec!["abcd".to_owned(), "ef".to_owned()], (1, 2))
        );
        let full = typed("abcd");
        assert_eq!(
            full.layout(4).1,
            (1, 0),
            "a full row puts the cursor at the start of the next"
        );
        let mut wide = typed("日本語");
        wide.home();
        wide.right();
        assert_eq!(
            wide.layout(4),
            (vec!["日本".to_owned(), "語".to_owned()], (0, 2))
        );
        assert_eq!(
            typed("a\nb").layout(10),
            (vec!["a".to_owned(), "b".to_owned()], (1, 1))
        );
    }
}
