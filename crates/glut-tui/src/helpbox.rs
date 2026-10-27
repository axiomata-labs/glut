//! `/help` as a box of its own (the owner's Mac test of T60, 2026-10-27): every command with what it does, filtered as
//! the owner types, ↑↓ to choose; Enter opens the command's whole page (its forms, defaults, keys), Esc goes back to
//! the list and closes it from there. `/help <command>` opens a page at once. The last entry holds the keys and what
//! is neither, as `/help` said them before.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::commands::{COMMANDS, Command, KEYS};
use crate::theme;
use crate::transcript::{cut, wrap};

/// One entry of the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Command(&'static Command),
    /// The keys, and what is neither a command nor a key.
    More,
}

/// The box: what was typed, the entry chosen, and the page open, scrolled.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HelpBox {
    pub query: String,
    pub chosen: usize,
    /// The page open, and how far it is scrolled.
    pub opened: Option<(Entry, usize)>,
}

/// What a key in the box asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Stay,
    Close,
}

impl HelpBox {
    /// The box at the page of command `name` (`/help loop`, `/help /loop`); `None` for no command of that name.
    pub fn at(name: &str) -> Option<HelpBox> {
        let name = name.trim().trim_start_matches('/');
        let command = COMMANDS.iter().find(|command| command.name == name)?;
        Some(HelpBox {
            opened: Some((Entry::Command(command), 0)),
            ..HelpBox::default()
        })
    }

    /// The entries the typed words fit, the keys' entry last while nothing is typed.
    pub fn entries(&self) -> Vec<Entry> {
        let mut entries: Vec<Entry> = crate::commands::fitting(self.query.trim())
            .into_iter()
            .map(Entry::Command)
            .collect();
        if self.query.trim().is_empty() {
            entries.push(Entry::More);
        }
        entries
    }

    /// A key, for a box `rows` rows high.
    pub fn key(&mut self, key: KeyEvent, rows: usize) -> Step {
        let page = rows.saturating_sub(1).max(1);
        if let Some((_, scroll)) = &mut self.opened {
            match key.code {
                KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => self.opened = None,
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                KeyCode::PageUp => *scroll = scroll.saturating_sub(page),
                KeyCode::PageDown | KeyCode::Char(' ') => *scroll = scroll.saturating_add(page),
                KeyCode::Char('q') => return Step::Close,
                _ => {}
            }
            return Step::Stay;
        }
        let entries = self.entries();
        match key.code {
            KeyCode::Esc => return Step::Close,
            KeyCode::Up => self.chosen = self.chosen.saturating_sub(1),
            KeyCode::Down => self.chosen = (self.chosen + 1).min(entries.len().saturating_sub(1)),
            KeyCode::PageUp => self.chosen = self.chosen.saturating_sub(page),
            KeyCode::PageDown => {
                self.chosen = (self.chosen + page).min(entries.len().saturating_sub(1))
            }
            KeyCode::Enter | KeyCode::Right => {
                if let Some(entry) = entries.get(self.chosen) {
                    self.opened = Some((*entry, 0));
                }
            }
            KeyCode::Backspace => {
                self.query.pop();
                self.chosen = 0;
            }
            // A leading `/` is how the owner thinks of a command: it filters nothing.
            KeyCode::Char('/') if self.query.is_empty() => {}
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.query.push(c);
                self.chosen = 0;
            }
            _ => {}
        }
        Step::Stay
    }

    /// The mouse's wheel: the page scrolled, or the choice in the list moved — `lines` at a time.
    pub fn wheel(&mut self, up: bool, lines: usize) {
        match &mut self.opened {
            Some((_, scroll)) if up => *scroll = scroll.saturating_sub(lines),
            Some((_, scroll)) => *scroll = scroll.saturating_add(lines),
            None if up => self.chosen = self.chosen.saturating_sub(lines),
            None => self.chosen = (self.chosen + lines).min(self.entries().len().saturating_sub(1)),
        }
    }

    /// The box's title.
    pub fn title(&self) -> String {
        match self.opened {
            Some((Entry::Command(command), _)) => {
                format!("{} · /{}", tr!(help_title), command.name)
            }
            Some((Entry::More, _)) => format!("{} · {}", tr!(help_title), tr!(help_more)),
            None => tr!(help_title).to_owned(),
        }
    }

    /// The box's rows at `width`, at most `rows` of them: the list, or the page open — its scroll kept within it.
    pub fn lines(&mut self, width: usize, rows: usize) -> Vec<Line<'static>> {
        if let Some((entry, scroll)) = &mut self.opened {
            let page = page_lines(*entry, width);
            *scroll = (*scroll).min(page.len().saturating_sub(rows));
            return page.into_iter().skip(*scroll).take(rows).collect();
        }
        let entries = self.entries();
        self.chosen = self.chosen.min(entries.len().saturating_sub(1));
        let mut lines = vec![Line::from(vec![
            Span::styled("› ", theme::glow()),
            Span::raw(self.query.clone()),
        ])];
        let room = rows.saturating_sub(1).max(1);
        let first = self.chosen.saturating_sub(room.saturating_sub(1));
        // The names in a column as wide as the longest shown, what each does beside it.
        let name_width = entries
            .iter()
            .map(|entry| entry_name(*entry).width())
            .max()
            .unwrap_or(0)
            .min(width / 2);
        for (at, entry) in entries.iter().enumerate().skip(first).take(room) {
            let chosen = at == self.chosen;
            let mark = if chosen { "▸ " } else { "  " };
            let name = cut(&entry_name(*entry), name_width);
            let pad = name_width.saturating_sub(name.width());
            let what = match entry {
                Entry::Command(command) => command.what(),
                Entry::More => tr!(help_more_what),
            };
            let what = cut(what, width.saturating_sub(name_width + 4));
            let style = if chosen {
                theme::glow()
            } else {
                theme::strong()
            };
            lines.push(Line::from(vec![
                Span::styled(mark, theme::glow()),
                Span::styled(name, style),
                Span::raw(" ".repeat(pad + 2)),
                Span::styled(what, theme::quiet()),
            ]));
        }
        if entries.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("  {}", tr!(nothing_matches)),
                theme::quiet(),
            )));
        }
        lines
    }

    /// The box's last row: its keys.
    pub fn hint(&self) -> &'static str {
        if self.opened.is_some() {
            tr!(help_page_keys)
        } else {
            tr!(help_list_keys)
        }
    }
}

/// An entry's name as the list shows it: `/loop`.
fn entry_name(entry: Entry) -> String {
    match entry {
        Entry::Command(command) => format!("/{}", command.name),
        Entry::More => tr!(help_more).to_owned(),
    }
}

/// An entry's page at `width`: a command's usage, its manual page and the keys that do it; or the keys and the rest.
pub fn page_lines(entry: Entry, width: usize) -> Vec<Line<'static>> {
    let width = width.max(16);
    let mut lines = Vec::new();
    let Entry::Command(command) = entry else {
        for row in crate::commands::help_more().lines() {
            for (at, part) in wrap(row, width.saturating_sub(4)).into_iter().enumerate() {
                let indent = if at == 0 { "" } else { "    " };
                lines.push(Line::from(Span::raw(format!("{indent}{part}"))));
            }
        }
        return lines;
    };
    let usage = match command.args {
        "" => format!("/{}", command.name),
        args => format!("/{} {args}", command.name),
    };
    for row in wrap(&usage, width) {
        lines.push(Line::from(Span::styled(
            row,
            theme::glow().add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::default());
    for row in rendered(crate::manual::page(command.name), width) {
        lines.push(row);
    }
    let keys: Vec<String> = KEYS
        .iter()
        .filter(|key| key.command == format!("/{}", command.name))
        .map(|key| format!("{} {}", crate::motion::spoken(key.key), key.what()))
        .collect();
    if !keys.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            tr!(help_page_keys_heading).to_owned(),
            theme::strong(),
        )));
        for key in keys {
            for row in wrap(&key, width) {
                lines.push(Line::from(Span::styled(row, theme::quiet())));
            }
        }
    }
    lines
}

/// A manual page's text at `width`: paragraphs apart by a blank row, a usage line (`/…`) or a list item (`- …`) on a
/// row of its own, a long one going on indented.
fn rendered(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (at, paragraph) in text.split("\n\n").enumerate() {
        if at > 0 {
            lines.push(Line::default());
        }
        // The paragraph's rows: a usage line or list item starts one, other lines join the row before.
        let mut rows: Vec<String> = Vec::new();
        for line in paragraph.lines().map(str::trim) {
            let starts = line.starts_with('/') || line.starts_with("- ");
            match rows.last_mut() {
                Some(row) if !starts => crate::agents::join_words(row, line),
                _ => rows.push(line.to_owned()),
            }
        }
        for row in rows {
            let usage = row.starts_with('/');
            let indent = if usage || row.starts_with("- ") {
                "  "
            } else {
                ""
            };
            for (at, part) in wrap(&row, width.saturating_sub(indent.len()))
                .into_iter()
                .enumerate()
            {
                let lead = if at == 0 { "" } else { indent };
                let style = if usage && at == 0 {
                    theme::strong()
                } else {
                    ratatui::style::Style::default()
                };
                lines.push(Line::from(Span::styled(format!("{lead}{part}"), style)));
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use crossterm::event::KeyEventKind;

    use super::*;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        }
    }

    fn text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn typing_filters_enter_opens_a_page_and_esc_goes_back_then_closes() {
        let mut help = HelpBox::default();
        assert_eq!(
            help.entries().len(),
            COMMANDS.len() + 1,
            "every command and the keys"
        );
        for c in "/loo".chars() {
            help.key(press(KeyCode::Char(c)), 20);
        }
        assert_eq!(help.query, "loo", "the leading / filters nothing");
        assert_eq!(
            help.entries()[0],
            Entry::Command(COMMANDS.iter().find(|c| c.name == "loop").unwrap())
        );
        assert_eq!(help.key(press(KeyCode::Enter), 20), Step::Stay);
        assert!(
            matches!(help.opened, Some((Entry::Command(command), 0)) if command.name == "loop")
        );
        let page = text(&help.lines(60, 200));
        assert!(page.starts_with("/loop <interval>"), "{page}");
        help.key(press(KeyCode::Down), 20);
        assert_eq!(help.key(press(KeyCode::Esc), 20), Step::Stay);
        assert!(help.opened.is_none(), "Esc on a page goes back to the list");
        assert_eq!(help.key(press(KeyCode::Esc), 20), Step::Close);
    }

    #[test]
    fn help_with_a_name_opens_its_page_and_the_last_entry_holds_the_keys() {
        assert!(HelpBox::at("/schedule").is_some());
        assert!(HelpBox::at("loop").is_some());
        assert!(HelpBox::at("nonesuch").is_none());
        let mut help = HelpBox::default();
        help.chosen = help.entries().len() - 1;
        help.key(press(KeyCode::Enter), 20);
        let page = text(&help.lines(80, 400));
        assert!(page.contains("⌃X R"), "the keys are there: {page}");
    }

    #[test]
    fn a_page_keeps_usage_lines_and_items_on_rows_of_their_own() {
        let page = text(&rendered(
            "What it is\nfor.\n\n/a b — one\n/c — two\n- an item\ngoes on\n\nLast.",
            40,
        ));
        assert_eq!(
            page,
            "What it is for.\n\n/a b — one\n/c — two\n- an item goes on\n\nLast."
        );
        // Chinese and Japanese go on without a space where a line was broken.
        assert_eq!(text(&rendered("循环会\n继续。", 40)), "循环会继续。");
        let mut help = HelpBox::default();
        help.wheel(false, 3);
        assert_eq!(help.chosen, 3);
        help.wheel(true, 5);
        assert_eq!(help.chosen, 0);
    }
}
