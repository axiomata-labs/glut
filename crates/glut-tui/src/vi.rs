//! The input's vi mode (T9, TUI3c; `/vim on`): Normal and Insert as the owner's Neovim has them, for one message.
//! Normal mode moves (`h j k l`, `w b e`, `0 ^ $`, `gg G`), changes (`x X r ~ J D C s S`), takes operators with a
//! motion (`d c y` + `w b e 0 ^ $ h l`, doubled for the line), counts (`3w`, `2dd`), puts (`p P`) and undoes
//! (`u`, Ctrl+R); `i a I A o O` go to Insert, Esc comes back. Enter sends from either mode; Esc in Normal mode is
//! the app's again (it stops Glut).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::Input;

/// Which of the two modes the input is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Insert,
}

/// What the app does after a key the vi mode saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The vi mode did it.
    Done,
    /// The key is the app's (typing in Insert mode, Ctrl keys, Esc in Normal mode).
    Pass,
}

/// A key that waits for the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    /// `d`, `c` or `y`: an operator waiting for its motion.
    Operator(char),
    /// `g`: waiting for the second `g`.
    G,
    /// `d`, `c` or `y` then `g`: waiting for the second `g` (`dgg`).
    OperatorG(char),
    /// `r`: waiting for the character to put in.
    Replace,
}

/// A motion's end, and whether an operator over it takes the character at its end (`e`, `$`).
struct Target {
    row: usize,
    col: usize,
    inclusive: bool,
}

/// The vi mode's state between keys.
#[derive(Debug, Clone)]
pub struct Vi {
    /// Normal or Insert.
    pub mode: Mode,
    pending: Pending,
    count: Option<usize>,
    /// What `d`, `c`, `y` and `x` took, and whether it was whole lines.
    register: (String, bool),
}

impl Default for Vi {
    fn default() -> Self {
        Vi {
            mode: Mode::Insert,
            pending: Pending::None,
            count: None,
            register: (String::new(), false),
        }
    }
}

impl Vi {
    /// A key in the input while the vi mode is on.
    pub fn key(&mut self, input: &mut Input, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match self.mode {
            Mode::Insert => {
                if key.code == KeyCode::Esc {
                    self.mode = Mode::Normal;
                    // As vi does: Normal mode stands on the last character typed, not after it.
                    input.col = input.col.saturating_sub(1);
                    return Outcome::Done;
                }
                Outcome::Pass
            }
            Mode::Normal if ctrl => match key.code {
                KeyCode::Char('r') => {
                    for _ in 0..self.take_count() {
                        input.redo();
                    }
                    self.clamp(input);
                    Outcome::Done
                }
                _ => Outcome::Pass,
            },
            Mode::Normal => {
                // The history, an undo or a join can leave the cursor after the line's end: Normal mode stands on it.
                self.clamp(input);
                self.normal(input, key)
            }
        }
    }

    /// The count typed before a command, 1 without one; it is used up.
    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1).max(1)
    }

    /// A Normal-mode key. What has no vi meaning — Tab, PgUp, a key with Alt, Shift+Tab — is the app's.
    fn normal(&mut self, input: &mut Input, key: KeyEvent) -> Outcome {
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let plain = key.modifiers.is_empty();
        let waiting = self.pending != Pending::None;
        let c = match key.code {
            KeyCode::Char(c) if !alt => c,
            // Enter is the app's: it completes a `/` command or sends; a pending key is dropped.
            KeyCode::Enter => {
                self.pending = Pending::None;
                self.count = None;
                return Outcome::Pass;
            }
            KeyCode::Esc if waiting || self.count.is_some() => {
                self.pending = Pending::None;
                self.count = None;
                return Outcome::Done;
            }
            // The arrows and the like move as their letters do — not after `r` or an operator, where they are no
            // character to put in or motion to take.
            KeyCode::Left if plain && !waiting => 'h',
            KeyCode::Right if plain && !waiting => 'l',
            KeyCode::Up if plain && !waiting => 'k',
            KeyCode::Down if plain && !waiting => 'j',
            KeyCode::Home if plain && !waiting => '0',
            KeyCode::End if plain && !waiting => '$',
            KeyCode::Backspace if plain && !waiting => 'h',
            _ => {
                self.pending = Pending::None;
                return Outcome::Pass;
            }
        };
        match self.pending {
            Pending::Replace => {
                self.pending = Pending::None;
                self.replace(input, c);
                return Outcome::Done;
            }
            Pending::G => {
                self.pending = Pending::None;
                if c == 'g' {
                    // `gg` goes to the first line, `3gg` to the third.
                    let line = self.count.take().map_or(0, |count| count.saturating_sub(1));
                    input.row = line.min(input.lines.len() - 1);
                    input.col = first_word(&input.chars(input.row));
                }
                self.count = None;
                return Outcome::Done;
            }
            Pending::OperatorG(op) => {
                self.pending = Pending::None;
                if c == 'g' {
                    self.operate_rows(input, op, 0, input.row);
                }
                self.count = None;
                return Outcome::Done;
            }
            _ => {}
        }
        if c.is_ascii_digit() && (c != '0' || self.count.is_some()) {
            let digit = c.to_digit(10).unwrap_or(0) as usize;
            self.count = Some(
                self.count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(digit)
                    .min(9_999),
            );
            return Outcome::Done;
        }
        if let Pending::Operator(op) = self.pending {
            self.pending = Pending::None;
            if c == 'g' {
                self.pending = Pending::OperatorG(op);
                return Outcome::Done;
            }
            self.operate(input, op, c);
            // An operator with a target it does not know (`ciw`) is dropped whole, its count too.
            self.count = None;
            return Outcome::Done;
        }
        self.command(input, c);
        Outcome::Done
    }

    /// A Normal-mode command that is not an operator's motion.
    fn command(&mut self, input: &mut Input, c: char) {
        match c {
            'd' | 'c' | 'y' => self.pending = Pending::Operator(c),
            'g' => self.pending = Pending::G,
            'r' => self.pending = Pending::Replace,
            'i' => self.insert(input),
            'a' => {
                input.col = (input.col + 1).min(input.chars(input.row).len());
                self.insert(input);
            }
            'I' => {
                input.col = first_word(&input.chars(input.row));
                self.insert(input);
            }
            'A' => {
                input.col = input.chars(input.row).len();
                self.insert(input);
            }
            'o' | 'O' => {
                self.take_count();
                input.snapshot();
                if c == 'o' {
                    input.col = input.chars(input.row).len();
                } else {
                    input.col = 0;
                }
                input.newline();
                if c == 'O' {
                    input.row -= 1;
                }
                self.mode = Mode::Insert;
            }
            'x' | 'X' => {
                let count = self.take_count();
                let chars = input.chars(input.row);
                let (from, to) = if c == 'x' {
                    (input.col, (input.col + count).min(chars.len()))
                } else {
                    (input.col.saturating_sub(count), input.col)
                };
                if from < to {
                    input.snapshot();
                    self.cut(input, input.row, from, to);
                    input.col = from;
                }
            }
            'D' => self.operate(input, 'd', '$'),
            'C' => self.operate(input, 'c', '$'),
            's' => {
                self.operate(input, 'c', 'l');
            }
            'S' => self.operate(input, 'c', 'c'),
            'p' | 'P' => self.put(input, c == 'p'),
            'u' => {
                for _ in 0..self.take_count() {
                    input.undo();
                }
                self.clamp(input);
            }
            '~' => {
                let count = self.take_count();
                let mut chars = input.chars(input.row);
                if !chars.is_empty() {
                    input.snapshot();
                    let end = (input.col + count).min(chars.len());
                    for c in &mut chars[input.col..end] {
                        // A letter whose other case is more than one (ß → SS) stays as it is.
                        let other: Vec<char> = if c.is_uppercase() {
                            c.to_lowercase().collect()
                        } else {
                            c.to_uppercase().collect()
                        };
                        if let [one] = other[..] {
                            *c = one;
                        }
                    }
                    input.set_line(input.row, &chars);
                    input.col = end.min(chars.len().saturating_sub(1));
                }
            }
            'J' => {
                self.take_count();
                if input.row + 1 < input.lines.len() {
                    input.snapshot();
                    let next = input.lines.remove(input.row + 1);
                    let line = &mut input.lines[input.row];
                    let keep = line.trim_end().len();
                    line.truncate(keep);
                    input.col = line.chars().count();
                    if !next.trim().is_empty() {
                        line.push(' ');
                        line.push_str(next.trim_start());
                    }
                    self.clamp(input);
                }
            }
            _ => {
                if let Some(target) = self.motion(input, c, true) {
                    input.row = target.row;
                    input.col = target.col;
                    self.clamp(input);
                }
            }
        }
    }

    /// Insert mode from here, the text before it kept for `u`.
    fn insert(&mut self, input: &mut Input) {
        input.snapshot();
        self.mode = Mode::Insert;
        self.count = None;
    }

    /// Where motion `c` goes from the cursor, `count` times; `None` for no motion. `history`: `j` on the last line
    /// and `k` on the first go through the sent messages, as the arrows do outside the vi mode (a bare motion only;
    /// an operator's never does).
    fn motion(&mut self, input: &mut Input, c: char, history: bool) -> Option<Target> {
        let explicit = self.count.is_some();
        let count = self.take_count();
        if c == 'G' {
            // `G` goes to the last line, `5G` to the fifth.
            let last = input.lines.len() - 1;
            let row = if explicit {
                (count - 1).min(last)
            } else {
                last
            };
            return Some(Target {
                row,
                col: first_word(&input.chars(row)),
                inclusive: false,
            });
        }
        let (mut row, mut col) = (input.row, input.col);
        let mut inclusive = false;
        for _ in 0..count {
            let chars = input.chars(row);
            match c {
                'h' => col = col.saturating_sub(1),
                'l' => col = (col + 1).min(chars.len()),
                '0' => col = 0,
                '^' => col = first_word(&chars),
                '$' => {
                    col = chars.len().saturating_sub(1);
                    inclusive = true;
                }
                'w' => match next_word(&chars, col) {
                    Some(at) => col = at,
                    None if row + 1 < input.lines.len() => {
                        row += 1;
                        col = first_word(&input.chars(row));
                    }
                    None => col = chars.len(),
                },
                'b' => match previous_word(&chars, col) {
                    Some(at) => col = at,
                    None if row > 0 => {
                        row -= 1;
                        let above = input.chars(row);
                        col = previous_word(&above, above.len()).unwrap_or(0);
                    }
                    None => col = 0,
                },
                'e' => {
                    col = word_end(&chars, col);
                    inclusive = true;
                }
                'j' if row + 1 < input.lines.len() => row += 1,
                'k' if row > 0 => row -= 1,
                'j' | 'k' if history && row == input.row => {
                    if c == 'j' {
                        input.down();
                    } else {
                        input.up();
                    }
                    return None;
                }
                'j' | 'k' => break,
                _ => return None,
            }
        }
        if matches!(c, 'j' | 'k') {
            col = col.min(input.chars(row).len().saturating_sub(1));
        }
        Some(Target {
            row,
            col,
            inclusive,
        })
    }

    /// Operator `op` over motion `c`: whole lines for `dd`, `cc`, `yy` and the line motions `j`, `k`, `G`.
    fn operate(&mut self, input: &mut Input, op: char, c: char) {
        if c == op {
            let count = self.take_count();
            let last = (input.row + count - 1).min(input.lines.len() - 1);
            return self.operate_rows(input, op, input.row, last);
        }
        if matches!(c, 'j' | 'k' | 'G') {
            let Some(target) = self.motion(input, c, false) else {
                return;
            };
            let (from, to) = (input.row.min(target.row), input.row.max(target.row));
            return self.operate_rows(input, op, from, to);
        }
        // As vim does: `cw` on a word changes to its end, the space after it stays — on a word's last letter, that
        // letter alone.
        let chars = input.chars(input.row);
        let c = match chars.get(input.col) {
            Some(here) if op == 'c' && c == 'w' && !here.is_whitespace() => {
                let same = chars
                    .get(input.col + 1)
                    .is_some_and(|next| class(*next) == class(*here));
                if same { 'e' } else { 'l' }
            }
            _ => c,
        };
        let start = (input.row, input.col);
        let Some(target) = self.motion(input, c, false) else {
            return;
        };
        // An operator works inside the line: a word motion that went on to the next line stops at this one's end.
        let len = input.chars(start.0).len();
        let end = if target.row == start.0 {
            target.col + usize::from(target.inclusive)
        } else if target.row > start.0 {
            len
        } else {
            0
        };
        let (from, to) = if end >= start.1 {
            (start.1, end.min(len))
        } else {
            (end, start.1)
        };
        if op == 'y' {
            if from < to {
                self.register = (input.chars(start.0)[from..to].iter().collect(), false);
            }
            input.col = from;
            return;
        }
        if from < to {
            input.snapshot();
            self.cut(input, start.0, from, to);
        } else if op == 'c' {
            input.snapshot();
        }
        input.row = start.0;
        input.col = from;
        if op == 'c' {
            self.mode = Mode::Insert;
        } else {
            self.clamp(input);
        }
    }

    /// Operator `op` over lines `from` to `last`, both kept in it.
    fn operate_rows(&mut self, input: &mut Input, op: char, from: usize, last: usize) {
        let to = (last + 1).min(input.lines.len());
        self.register = (input.lines[from..to].join("\n"), true);
        if op == 'y' {
            return;
        }
        input.snapshot();
        if op == 'c' {
            input.lines.drain(from + 1..to);
            input.lines[from].clear();
            input.col = 0;
            self.mode = Mode::Insert;
            return;
        }
        input.lines.drain(from..to);
        if input.lines.is_empty() {
            input.lines.push(String::new());
        }
        input.row = from.min(input.lines.len() - 1);
        input.col = first_word(&input.chars(input.row));
    }

    /// Takes columns `from..to` of line `row` out, into the register.
    fn cut(&mut self, input: &mut Input, row: usize, from: usize, to: usize) {
        let mut chars = input.chars(row);
        let taken: String = chars.drain(from..to).collect();
        self.register = (taken, false);
        input.set_line(row, &chars);
    }

    /// `p` after the cursor, `P` before it: whole lines below or above, else inside the line.
    fn put(&mut self, input: &mut Input, after: bool) {
        let (text, lines) = self.register.clone();
        // An empty line taken (`dd` on one) is something to put; an empty piece of a line is not.
        if text.is_empty() && !lines {
            return;
        }
        let count = self.take_count();
        input.snapshot();
        if lines {
            let at = if after { input.row + 1 } else { input.row };
            for _ in 0..count {
                for (offset, line) in text.split('\n').enumerate() {
                    input.lines.insert(at + offset, line.to_owned());
                }
            }
            input.row = at;
            input.col = first_word(&input.chars(at));
            return;
        }
        let mut chars = input.chars(input.row);
        let at = if after && !chars.is_empty() {
            input.col + 1
        } else {
            input.col
        }
        .min(chars.len());
        let put: Vec<char> = text.repeat(count).chars().collect();
        let end = at + put.len();
        chars.splice(at..at, put);
        input.set_line(input.row, &chars);
        input.col = end.saturating_sub(1);
    }

    /// `r`: the characters under the cursor become `c`.
    fn replace(&mut self, input: &mut Input, c: char) {
        let count = self.take_count();
        let mut chars = input.chars(input.row);
        if input.col + count > chars.len() {
            return;
        }
        input.snapshot();
        chars[input.col..input.col + count].fill(c);
        input.set_line(input.row, &chars);
        input.col += count - 1;
    }

    /// Normal mode stands on a character, never after the line's last.
    fn clamp(&self, input: &mut Input) {
        let len = input.chars(input.row).len();
        input.col = input.col.min(len.saturating_sub(1));
    }
}

/// What kind of character `c` is, for word motions: 0 space, 1 word (letters, digits, `_`), 2 anything else.
fn class(c: char) -> u8 {
    if c.is_whitespace() {
        0
    } else if c.is_alphanumeric() || c == '_' {
        1
    } else {
        2
    }
}

/// The first character that is no space.
fn first_word(chars: &[char]) -> usize {
    chars.iter().position(|c| !c.is_whitespace()).unwrap_or(0)
}

/// The start of the next word after `col`, on this line.
fn next_word(chars: &[char], col: usize) -> Option<usize> {
    let mut at = col;
    if at >= chars.len() {
        return None;
    }
    let start = class(chars[at]);
    while at < chars.len() && class(chars[at]) == start && start != 0 {
        at += 1;
    }
    while at < chars.len() && class(chars[at]) == 0 {
        at += 1;
    }
    (at < chars.len()).then_some(at)
}

/// The start of the word before `col`, on this line.
fn previous_word(chars: &[char], col: usize) -> Option<usize> {
    let mut at = col.min(chars.len());
    while at > 0 && class(chars[at - 1]) == 0 {
        at -= 1;
    }
    if at == 0 {
        return None;
    }
    let kind = class(chars[at - 1]);
    while at > 0 && class(chars[at - 1]) == kind {
        at -= 1;
    }
    Some(at)
}

/// The end of the word at or after `col + 1`, on this line.
fn word_end(chars: &[char], col: usize) -> usize {
    let mut at = col + 1;
    while at < chars.len() && class(chars[at]) == 0 {
        at += 1;
    }
    if at >= chars.len() {
        return chars.len().saturating_sub(1);
    }
    let kind = class(chars[at]);
    while at + 1 < chars.len() && class(chars[at + 1]) == kind {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(vi: &mut Vi, input: &mut Input, keys: &str) {
        for c in keys.chars() {
            let code = match c {
                '⎋' => KeyCode::Esc,
                c => KeyCode::Char(c),
            };
            if vi.key(input, KeyEvent::new(code, KeyModifiers::NONE)) == Outcome::Pass {
                input.insert(c);
            }
        }
    }

    fn typed(text: &str) -> (Vi, Input) {
        let mut input = Input::new();
        input.insert_text(text);
        let mut vi = Vi::default();
        press(&mut vi, &mut input, "⎋");
        (vi, input)
    }

    #[test]
    fn motions_and_operators_work_as_in_vim() {
        let (mut vi, mut input) = typed("fix the parser tests now");
        press(&mut vi, &mut input, "0w");
        assert_eq!(input.col, 4);
        press(&mut vi, &mut input, "dw");
        assert_eq!(input.text(), "fix parser tests now");
        press(&mut vi, &mut input, "cwlexer⎋");
        assert_eq!(input.text(), "fix lexer tests now");
        assert_eq!(vi.mode, Mode::Normal);
        press(&mut vi, &mut input, "$x0");
        assert_eq!(input.text(), "fix lexer tests no");
        press(&mut vi, &mut input, "2dw");
        assert_eq!(input.text(), "tests no");
        press(&mut vi, &mut input, "u");
        assert_eq!(input.text(), "fix lexer tests no", "u brings it back");
        press(&mut vi, &mut input, "D");
        assert_eq!(input.text(), "");
    }

    #[test]
    fn lines_are_cut_put_joined_and_opened() {
        let (mut vi, mut input) = typed("one\ntwo\nthree");
        press(&mut vi, &mut input, "ggyyjp");
        assert_eq!(input.text(), "one\ntwo\none\nthree");
        press(&mut vi, &mut input, "2dd");
        assert_eq!(input.text(), "one\ntwo");
        press(&mut vi, &mut input, "ggJ");
        assert_eq!(input.text(), "one two");
        press(&mut vi, &mut input, "onew⎋");
        assert_eq!(input.text(), "one two\nnew");
        press(&mut vi, &mut input, "ggrO~");
        assert_eq!(
            input.text(),
            "one two\nnew",
            "r puts an O in, ~ turns it back"
        );
    }

    #[test]
    fn the_cursor_never_ends_up_past_a_line_and_nothing_panics_at_the_edges() {
        let mut input = Input::new();
        input.insert_text("hi");
        input.take();
        let mut vi = Vi::default();
        press(&mut vi, &mut input, "⎋ka");
        assert_eq!(input.text(), "hi", "k brought the last message back");
        input.backspace();
        assert_eq!(input.text(), "h");
        let (mut vi, mut input) = typed("");
        press(&mut vi, &mut input, "xXDJp~rz99l99x99dwOab⎋ddddPu");
        let (mut vi, mut input) = typed("one\ntwo\nthree");
        press(&mut vi, &mut input, "ggdj");
        assert_eq!(input.text(), "three", "dj takes both lines");
        press(&mut vi, &mut input, "u2Gdk");
        assert_eq!(input.text(), "three", "dk too");
        press(&mut vi, &mut input, "uGdgg");
        assert_eq!(input.text(), "", "dgg to the top");
        let (mut vi, mut input) = typed("fix the");
        press(&mut vi, &mut input, "0eclZ⎋");
        assert_eq!(
            input.text(),
            "fiZ the",
            "cl on a word's last letter changes that letter"
        );
        let (mut vi, mut input) = typed("fix the");
        press(&mut vi, &mut input, "0ecwZ⎋");
        assert_eq!(input.text(), "fiZ the", "so does cw");
    }

    #[test]
    fn keys_without_a_vi_meaning_are_the_apps() {
        let (mut vi, mut input) = typed("text");
        for key in [
            KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
            KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Char('b'), KeyModifiers::ALT),
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE),
        ] {
            assert_eq!(vi.key(&mut input, key), Outcome::Pass, "{key:?}");
        }
    }

    #[test]
    fn insert_mode_leaves_typing_to_the_input_and_esc_steps_back() {
        let mut input = Input::new();
        let mut vi = Vi::default();
        assert_eq!(
            vi.key(
                &mut input,
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)
            ),
            Outcome::Pass
        );
        input.insert_text("ab");
        press(&mut vi, &mut input, "⎋");
        assert_eq!((vi.mode, input.col), (Mode::Normal, 1));
        assert_eq!(
            vi.key(&mut input, KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Outcome::Pass
        );
        assert_eq!(
            vi.key(
                &mut input,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)
            ),
            Outcome::Pass
        );
    }
}
