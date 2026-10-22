//! Glut's answers as Markdown (T7): headings, emphasis, lists, quotes, tables, links and code blocks in colour,
//! wrapped to the screen's width. The text is cleaned first (`transcript::clean`): what a model wrote is shown, never
//! obeyed by the terminal. An answer still streaming is parsed whole on every frame it changed — an open code fence
//! reads as code to its end.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::highlight::{self, Run};
use crate::theme;
use crate::transcript::clean;

/// A tab in a code block: four columns, as most editors show it.
const TAB: &str = "    ";
/// The narrowest a table column is squeezed to before its cells wrap.
const MIN_COLUMN: usize = 4;

/// `text` as lines of at most `width` cells.
pub fn render(text: &str, width: usize) -> Vec<Line<'static>> {
    let text = clean(&text.replace('\t', TAB));
    let mut out = Renderer::new(width.max(8));
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    for event in Parser::new_ext(&text, options) {
        out.take(event);
    }
    let width = out.width;
    // The last guard: whatever the layout above got wrong (a grapheme measured short), no row passes the width.
    out.finish()
        .into_iter()
        .map(|line| clamp(line, width))
        .collect()
}

/// `line` cut to `width` cells, measured as the terminal will draw it.
fn clamp(line: Line<'static>, width: usize) -> Line<'static> {
    if line.width() <= width {
        return line;
    }
    let mut spans = Vec::new();
    let mut used = String::new();
    for span in line.spans {
        let mut kept = String::new();
        for c in span.content.chars() {
            used.push(c);
            if used.width() > width {
                break;
            }
            kept.push(c);
        }
        let full = used.width() > width;
        spans.push(Span::styled(kept, span.style));
        if full {
            break;
        }
    }
    Line::from(spans)
}

/// What holds the lines being made: a quote, or a list item with its marker.
enum Container {
    Quote,
    Item {
        marker: String,
        /// Whether the marker was drawn: the item's later lines are indented by its width instead.
        drawn: bool,
    },
}

/// A table being gathered: its rows of cells (each cell its runs), the head marked.
#[derive(Default)]
struct Table {
    rows: Vec<(bool, Vec<Vec<Run>>)>,
    head: bool,
}

struct Renderer {
    width: usize,
    lines: Vec<Line<'static>>,
    /// The paragraph, heading or table cell being gathered.
    inline: Vec<Run>,
    containers: Vec<Container>,
    /// The next number of each open list; `None` for a bulleted one.
    lists: Vec<Option<u64>>,
    bold: usize,
    italic: usize,
    strike: usize,
    heading: Option<HeadingLevel>,
    /// The URLs of the open links, shown after their text when it does not say them.
    links: Vec<(String, usize)>,
    /// A code block being gathered: its language and text.
    code: Option<(String, String)>,
    table: Option<Table>,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Renderer {
            width,
            lines: Vec::new(),
            inline: Vec::new(),
            containers: Vec::new(),
            lists: Vec::new(),
            bold: 0,
            italic: 0,
            strike: 0,
            heading: None,
            links: Vec::new(),
            code: None,
            table: None,
        }
    }

    /// The style of text at this point: emphasis, strength, a strike, a link, a heading.
    fn style(&self) -> Style {
        let mut style = Style::default();
        if self.bold > 0 || self.heading.is_some() {
            style = style.add_modifier(Modifier::BOLD);
        }
        if self.italic > 0 {
            style = style.add_modifier(Modifier::ITALIC);
        }
        if self.strike > 0 {
            style = style.add_modifier(Modifier::CROSSED_OUT);
        }
        if matches!(self.heading, Some(HeadingLevel::H1)) {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        if !self.links.is_empty() {
            style = style.fg(theme::c().link).add_modifier(Modifier::UNDERLINED);
        }
        style
    }

    /// Text as the parser gives it — with its character references decoded (`&#27;`), so it is cleaned here again:
    /// cleaning the source alone would let an escape through.
    fn push_text(&mut self, text: &str, style: Style) {
        let text = clean(&text.replace('\t', TAB));
        match &mut self.code {
            Some((_, code)) => code.push_str(&text),
            None => self.inline.push((text, style)),
        }
    }

    fn take(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.open(tag),
            Event::End(tag) => self.close(tag),
            Event::Text(text) => {
                let style = self.style();
                self.push_text(&text, style);
            }
            Event::Code(text) => {
                let style = self.style().fg(theme::c().code);
                self.push_text(&text, style);
            }
            Event::Html(text) | Event::InlineHtml(text) => self.push_text(&text, theme::quiet()),
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                let style = self.style().fg(theme::c().code);
                self.push_text(&text, style);
            }
            Event::FootnoteReference(name) => self.push_text(&format!("[{name}]"), theme::quiet()),
            Event::SoftBreak => self.push_text(" ", Style::default()),
            Event::HardBreak => self.push_text("\n", Style::default()),
            Event::Rule => {
                self.flush();
                self.gap();
                let room = self.width.saturating_sub(self.prefix_width());
                let prefix = self.prefix(true);
                let mut spans = prefix;
                spans.push(Span::styled("─".repeat(room), theme::quiet()));
                self.lines.push(Line::from(spans));
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "☑ " } else { "☐ " };
                self.push_text(mark, theme::quiet());
            }
        }
    }

    fn open(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                self.flush();
                self.gap();
            }
            Tag::Heading { level, .. } => {
                self.flush();
                self.gap();
                self.heading = Some(level);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.gap();
                self.containers.push(Container::Quote);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.gap();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_owned()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((language, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                if self.lists.is_empty() {
                    self.gap();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(next)) => {
                        let marker = format!("{next}. ");
                        *next += 1;
                        marker
                    }
                    _ => "• ".to_owned(),
                };
                self.containers.push(Container::Item {
                    marker,
                    drawn: false,
                });
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => {
                let at = self.inline.len();
                self.links.push((dest_url.to_string(), at));
            }
            Tag::Image { dest_url, .. } => {
                self.push_text("[image: ", theme::quiet());
                let at = self.inline.len();
                self.links.push((dest_url.to_string(), at));
            }
            Tag::Table(_) => {
                self.flush();
                self.gap();
                self.table = Some(Table::default());
            }
            Tag::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = true;
                    table.rows.push((true, Vec::new()));
                }
            }
            Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.rows.push((false, Vec::new()));
                }
            }
            Tag::TableCell => self.inline.clear(),
            Tag::FootnoteDefinition(name) => {
                self.flush();
                self.push_text(&format!("[{name}] "), theme::quiet());
            }
            _ => {}
        }
    }

    fn close(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::CodeBlock => self.code_block(),
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush();
                // An item with nothing in it still shows its marker.
                if let Some(Container::Item { drawn: false, .. }) = self.containers.last() {
                    let prefix = self.prefix(true);
                    self.lines.push(Line::from(prefix));
                }
                self.containers.pop();
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link | TagEnd::Image => {
                if let Some((url, at)) = self.links.pop() {
                    let said: String = self.inline[at.min(self.inline.len())..]
                        .iter()
                        .map(|(text, _)| text.as_str())
                        .collect();
                    if matches!(tag, TagEnd::Image) {
                        let gap = if said.trim().is_empty() { "" } else { " → " };
                        self.push_text(&format!("{gap}{url}]"), theme::quiet());
                    } else if !url.is_empty() && said.trim() != url && !url.starts_with('#') {
                        self.push_text(&format!(" ({url})"), theme::quiet());
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some((_, cells)) =
                    self.table.as_mut().and_then(|table| table.rows.last_mut())
                {
                    cells.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = false;
                }
            }
            TagEnd::Table => self.table_block(),
            TagEnd::FootnoteDefinition => self.flush(),
            _ => {}
        }
    }

    /// A blank line before a block, unless it opens the answer, follows one, or sits in a tight list.
    fn gap(&mut self) {
        let in_item = self
            .containers
            .iter()
            .any(|c| matches!(c, Container::Item { .. }));
        if in_item || self.lines.is_empty() {
            return;
        }
        let last_blank = self
            .lines
            .last()
            .is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty()));
        if !last_blank {
            let prefix = self.prefix(false);
            self.lines.push(Line::from(prefix));
        }
    }

    /// The prefix of a line: a bar per quote, an item's marker on its first line and its indentation after.
    /// The prefix of a line: a bar per quote, every item's marker on the first line it has (`• • a` for a list
    /// that opens a list) and its indentation after. Deep nesting stops drawing at half the width, so the text
    /// always keeps the other half.
    fn prefix(&mut self, first: bool) -> Vec<Span<'static>> {
        let cap = self.width / 2;
        let mut used = 0;
        let mut spans = Vec::new();
        for container in &mut self.containers {
            let (span, cells) = match container {
                Container::Quote => (Span::styled("│ ", theme::quiet()), 2),
                Container::Item { marker, drawn } => {
                    let cells = marker.width();
                    if first && !*drawn {
                        *drawn = true;
                        (Span::styled(marker.clone(), theme::quiet()), cells)
                    } else {
                        (Span::raw(" ".repeat(cells)), cells)
                    }
                }
            };
            if used + cells <= cap {
                spans.push(span);
                used += cells;
            }
        }
        spans
    }

    /// The cells [`Renderer::prefix`] takes: what fits in half the width.
    fn prefix_width(&self) -> usize {
        let cap = self.width / 2;
        let mut used = 0;
        for container in &self.containers {
            let cells = match container {
                Container::Quote => 2,
                Container::Item { marker, .. } => marker.width(),
            };
            if used + cells <= cap {
                used += cells;
            }
        }
        used
    }

    /// The gathered text as wrapped lines below the prefixes.
    fn flush(&mut self) {
        if self.table.is_some() {
            return;
        }
        let inline = std::mem::take(&mut self.inline);
        if inline.iter().all(|(text, _)| text.trim().is_empty()) {
            return;
        }
        let room = self.width.saturating_sub(self.prefix_width()).max(4);
        for (at, row) in wrap_runs(&inline, room).into_iter().enumerate() {
            let mut spans = self.prefix(at == 0);
            spans.extend(
                row.into_iter()
                    .map(|(text, style)| Span::styled(text, style)),
            );
            self.lines.push(Line::from(spans));
        }
    }

    /// A code block: coloured where Glut knows its language, each line behind a quiet bar, long lines broken.
    fn code_block(&mut self) {
        let Some((language, code)) = self.code.take() else {
            return;
        };
        let code = code.strip_suffix('\n').unwrap_or(&code);
        let rows = highlight::lines(code, &language).unwrap_or_else(|| {
            code.split('\n')
                .map(|line| vec![(line.to_owned(), Style::default())])
                .collect()
        });
        let room = self.width.saturating_sub(self.prefix_width() + 2).max(4);
        for (line, row) in rows.into_iter().enumerate() {
            for (at, part) in break_runs(&row, room).into_iter().enumerate() {
                let mut spans = self.prefix(line == 0 && at == 0);
                spans.push(Span::styled(
                    if at == 0 { "▎ " } else { "┆ " },
                    theme::quiet(),
                ));
                spans.extend(
                    part.into_iter()
                        .map(|(text, style)| Span::styled(text, style)),
                );
                self.lines.push(Line::from(spans));
            }
        }
    }

    /// A table: columns as wide as their cells where the screen has room, else squeezed with the cells wrapped;
    /// the head bold and ruled off.
    fn table_block(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        let columns = table
            .rows
            .iter()
            .map(|(_, cells)| cells.len())
            .max()
            .unwrap_or(0);
        if columns == 0 {
            return;
        }
        let text_of =
            |cell: &Vec<Run>| -> String { cell.iter().map(|(text, _)| text.as_str()).collect() };
        let mut widths = vec![0; columns];
        for (_, cells) in &table.rows {
            for (at, cell) in cells.iter().enumerate() {
                widths[at] = widths[at].max(text_of(cell).trim().width());
            }
        }
        // " │ " between columns; a table with more columns than the screen holds is shown a row per line, its
        // cells apart by " · ".
        let room = self
            .width
            .saturating_sub(self.prefix_width() + 3 * (columns - 1));
        if room < columns * MIN_COLUMN {
            for (head, cells) in table.rows {
                for (at, cell) in cells.into_iter().enumerate() {
                    if at > 0 {
                        self.inline.push((" · ".to_owned(), theme::quiet()));
                    }
                    let bold = |(text, style): Run| {
                        (
                            text,
                            if head {
                                style.add_modifier(Modifier::BOLD)
                            } else {
                                style
                            },
                        )
                    };
                    self.inline.extend(cell.into_iter().map(bold));
                }
                self.flush();
            }
            return;
        }
        while widths.iter().sum::<usize>() > room {
            let Some(widest) = widths.iter_mut().max() else {
                break;
            };
            if *widest <= MIN_COLUMN {
                break;
            }
            *widest -= 1;
        }
        for (head, cells) in &table.rows {
            let wrapped: Vec<Vec<Vec<Run>>> = (0..columns)
                .map(|at| {
                    let cell = cells.get(at).cloned().unwrap_or_default();
                    let cell: Vec<Run> = cell
                        .into_iter()
                        .map(|(text, style)| {
                            let style = if *head {
                                style.add_modifier(Modifier::BOLD)
                            } else {
                                style
                            };
                            (text, style)
                        })
                        .collect();
                    trim_runs(wrap_runs(&cell, widths[at].max(1)))
                })
                .collect();
            let height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for line in 0..height {
                let first = line == 0 && *head;
                let mut spans = self.prefix(first);
                for (at, cell) in wrapped.iter().enumerate() {
                    if at > 0 {
                        spans.push(Span::styled(" │ ", theme::quiet()));
                    }
                    let part = cell.get(line).cloned().unwrap_or_default();
                    let used: usize = part.iter().map(|(text, _)| text.width()).sum();
                    spans.extend(
                        part.into_iter()
                            .map(|(text, style)| Span::styled(text, style)),
                    );
                    if at + 1 < wrapped.len() {
                        spans.push(Span::raw(" ".repeat(widths[at].saturating_sub(used))));
                    }
                }
                self.lines.push(Line::from(spans));
            }
            if *head {
                let mut spans = self.prefix(false);
                let rule: Vec<String> = widths.iter().map(|width| "─".repeat(*width)).collect();
                spans.push(Span::styled(rule.join("─┼─"), theme::quiet()));
                self.lines.push(Line::from(spans));
            }
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        if self.code.is_some() {
            self.code_block();
        }
        if self.table.is_some() {
            self.table_block();
        }
        self.flush();
        self.lines
    }
}

/// Runs broken into rows of at most `width` cells: at spaces where it can (the spaces at a break dropped), inside a
/// word where it must; a `\n` run breaks the row.
pub fn wrap_runs(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Run>> = Vec::new();
    let mut row: Vec<Run> = Vec::new();
    let mut used = 0;
    let mut broke = false;
    for (word, space) in words(runs) {
        if word.len() == 1 && word[0].0 == "\n" {
            rows.push(trim_end(std::mem::take(&mut row)));
            used = 0;
            broke = false;
            continue;
        }
        let word_width: usize = word.iter().map(|(text, _)| text.width()).sum();
        if space {
            if used == 0 && (broke || !rows.is_empty()) {
                continue;
            }
            if used + word_width > width {
                rows.push(std::mem::take(&mut row));
                used = 0;
                broke = true;
                continue;
            }
            push_runs(&mut row, word);
            used += word_width;
            continue;
        }
        if used + word_width > width && used > 0 {
            rows.push(trim_end(std::mem::take(&mut row)));
            used = 0;
            broke = true;
        }
        if word_width <= width {
            push_runs(&mut row, word);
            used += word_width;
            continue;
        }
        // A word wider than the row: broken where the row ends.
        for (text, style) in word {
            for c in text.chars() {
                let cells = c.width().unwrap_or(0);
                if used + cells > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                    broke = true;
                }
                push_char(&mut row, c, style);
                used += cells;
            }
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(trim_end(row));
    }
    rows
}

/// A code line broken every `width` cells, with no regard for words: code is read as it is.
fn break_runs(runs: &[Run], width: usize) -> Vec<Vec<Run>> {
    let mut rows = vec![Vec::new()];
    let mut used = 0;
    for (text, style) in runs {
        for c in text.chars() {
            let cells = c.width().unwrap_or(0);
            if used + cells > width && used > 0 {
                rows.push(Vec::new());
                used = 0;
            }
            if let Some(row) = rows.last_mut() {
                push_char(row, c, *style);
            }
            used += cells;
        }
    }
    rows
}

/// The runs as words and runs of spaces, in order, each a run list of its own; a `\n` is a word of its own.
fn words(runs: &[Run]) -> Vec<(Vec<Run>, bool)> {
    let mut words: Vec<(Vec<Run>, bool)> = Vec::new();
    for (text, style) in runs {
        for c in text.chars() {
            if c == '\n' {
                words.push((vec![("\n".to_owned(), *style)], false));
                continue;
            }
            let space = c == ' ';
            match words.last_mut() {
                Some((word, was)) if *was == space && !(word.len() == 1 && word[0].0 == "\n") => {
                    push_char(word, c, *style)
                }
                _ => words.push((vec![(c.to_string(), *style)], space)),
            }
        }
    }
    words
}

fn push_char(row: &mut Vec<Run>, c: char, style: Style) {
    match row.last_mut() {
        Some((text, was)) if *was == style => text.push(c),
        _ => row.push((c.to_string(), style)),
    }
}

fn push_runs(row: &mut Vec<Run>, runs: Vec<Run>) {
    for (text, style) in runs {
        match row.last_mut() {
            Some((kept, was)) if *was == style => kept.push_str(&text),
            _ => row.push((text, style)),
        }
    }
}

/// A row without the spaces it ends with.
fn trim_end(mut row: Vec<Run>) -> Vec<Run> {
    while let Some((text, _)) = row.last_mut() {
        let trimmed = text.trim_end_matches(' ').len();
        if trimmed == 0 {
            row.pop();
        } else {
            text.truncate(trimmed);
            break;
        }
    }
    row
}

fn trim_runs(rows: Vec<Vec<Run>>) -> Vec<Vec<Run>> {
    rows.into_iter().map(trim_end).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn markdown_reads_as_text_with_its_structure() {
        let lines = render(
            "# Title\n\nSome **bold** and `code` and [a link](https://example.com).\n\n- one\n- two\n  1. inner\n\n> quoted\n\n---",
            40,
        );
        assert_eq!(
            text(&lines),
            [
                "Title",
                "",
                "Some bold and code and a link",
                "(https://example.com).",
                "",
                "• one",
                "• two",
                "  1. inner",
                "",
                "│ quoted",
                "",
                "────────────────────────────────────────",
            ]
        );
        let bold = &lines[2].spans[1];
        assert_eq!(bold.content, "bold");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(lines[2].spans[3].style.fg, Some(theme::c().code));
    }

    #[test]
    fn code_is_coloured_kept_as_written_and_never_wider_than_the_screen() {
        let lines = render(
            "```rust\nfn main() {\n\tlet answer = \"a very long string that will not fit\";\n}\n```",
            30,
        );
        let rows = text(&lines);
        assert_eq!(rows[0], "▎ fn main() {");
        assert!(
            rows[1].starts_with("▎     let"),
            "a tab is four columns: {:?}",
            rows[1]
        );
        assert!(
            rows.iter().any(|row| row.starts_with("┆ ")),
            "a long line goes on"
        );
        for row in &rows {
            assert!(row.width() <= 30, "{row:?}");
        }
        assert!(
            lines[0].spans.iter().any(|span| span.style.fg.is_some()),
            "coloured"
        );
        let open = render("```python\nprint(1)", 30);
        assert_eq!(
            text(&open),
            ["▎ print(1)"],
            "a fence still streaming reads as code"
        );
    }

    #[test]
    fn a_table_fits_its_columns_and_squeezes_on_a_narrow_screen() {
        let table = "| Name | Kind |\n|---|---|\n| glut | harness |\n| axh | cli |";
        assert_eq!(
            text(&render(table, 40)),
            [
                "Name │ Kind",
                "─────┼────────",
                "glut │ harness",
                "axh  │ cli"
            ]
        );
        let wide = "| a | b |\n|---|---|\n| one two three four five six | x |";
        for row in text(&render(wide, 20)) {
            assert!(row.width() <= 20, "{row:?}");
        }
    }

    #[test]
    fn a_character_reference_is_cleaned_like_the_text_it_stands_in() {
        let lines = render(
            "a&#27;[2J &#x202e;x `&#x9b;` [l](http://x/&#27;)\n\n| &#27;]0;x&#7; |\n|---|\n| b |\n\n```\n&#27;\n```",
            60,
        );
        for line in &lines {
            for span in &line.spans {
                assert!(
                    !span
                        .content
                        .chars()
                        .any(|c| c.is_control() || ('\u{202a}'..='\u{202e}').contains(&c)),
                    "{:?}",
                    span.content
                );
            }
        }
    }

    #[test]
    fn no_row_is_ever_wider_than_the_screen() {
        let deep_quote = format!("{}deep", "> ".repeat(30));
        let deep_list: String = (0..17)
            .map(|level| format!("{}- l{level}\n", "  ".repeat(level)))
            .collect();
        let many_columns = format!(
            "|{}\n|{}\n|{}",
            " a |".repeat(30),
            "---|".repeat(30),
            " 1 |".repeat(30)
        );
        let emoji = "❤️".repeat(40);
        let samples = [
            deep_quote,
            deep_list,
            many_columns,
            emoji.clone(),
            format!("```\n{emoji}\n```"),
            "> - ```rust\n>   fn main() { let x = 1; }\n>   ```".to_owned(),
            "| a | b |\n|---|---|\n| 日本語テキスト日本語 | x |".to_owned(),
        ];
        for sample in &samples {
            for width in [8, 12, 20, 40, 80] {
                for line in render(sample, width) {
                    assert!(line.width() <= width, "{width}: {line:?}");
                }
            }
        }
    }

    #[test]
    fn every_item_shows_its_marker_once_whatever_it_opens_with() {
        assert_eq!(
            text(&render("- - a\n  - b\n- c", 40)),
            ["• • a", "  • b", "• c"]
        );
        let code = text(&render("- ```rust\n  fn x() {}\n  ```\n- y", 40));
        assert_eq!(code, ["• ▎ fn x() {}", "• y"]);
        let image = text(&render("![a cat](http://x/c.png)", 60));
        assert_eq!(image, ["[image: a cat → http://x/c.png]"]);
        for open in ["| a | b |\n|---|", "1. one\n2.", "```", "> - [ ] "] {
            let _ = render(open, 30);
        }
    }

    #[test]
    fn what_a_model_writes_is_shown_and_never_sent_to_the_terminal() {
        let rows = text(&render("a\u{1b}[2Jb \u{202e}evil", 40));
        assert_eq!(rows, ["a [2Jb evil"]);
        assert!(text(&render("- [x] done\n- [ ] open", 40)).contains(&"• ☐ open".to_owned()));
    }
}
