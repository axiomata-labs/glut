//! The input (T9, first part): several lines, a cursor, word motions and a history of what was sent. Soft-wrapped to
//! the box's width; the cursor's place on the screen is worked out from the same wrap.

use unicode_width::UnicodeWidthChar;

/// The most messages the history keeps.
const HISTORY: usize = 200;

#[derive(Debug, Default)]
pub struct Input {
    /// The text, one string per line; never empty.
    lines: Vec<String>,
    /// The cursor: line, and character within it.
    row: usize,
    col: usize,
    history: Vec<String>,
    /// Where in the history Up has gone: `None` while editing a new message.
    browsing: Option<usize>,
    /// The new message kept while the history is browsed.
    draft: Option<String>,
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
    }

    /// Takes the message out to send it, and keeps it in the history.
    pub fn take(&mut self) -> String {
        let text = self.text();
        if !text.trim().is_empty() && self.history.last() != Some(&text) {
            self.history.push(text.clone());
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

    pub fn newline(&mut self) {
        self.browsing = None;
        let at = self.byte(self.row, self.col);
        let rest = self.lines[self.row].split_off(at);
        self.row += 1;
        self.lines.insert(self.row, rest);
        self.col = 0;
    }

    pub fn backspace(&mut self) {
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
                self.draft = Some(self.text());
                self.history.len() - 1
            }
            Some(0) => return,
            Some(at) => at - 1,
        };
        self.browsing = Some(next);
        let text = self.history[next].clone();
        self.set(&text);
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
                let text = self.history[at + 1].clone();
                self.set(&text);
                self.browsing = Some(at + 1);
            }
            Some(_) => {
                let draft = self.draft.take().unwrap_or_default();
                self.set(&draft);
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
