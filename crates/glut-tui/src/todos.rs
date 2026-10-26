//! The project's to-dos (`docs/plans/glut-tui.md`, T41): its `TODO.md`, a GitHub checklist the owner and the model
//! both keep, read for the To-dos tab and changed one line at a time — a box ticked, an item added at the end, the
//! done ones dropped. The rest of the file is never touched, a change is made to the file as it is at that moment
//! (found by its text, not its line number), and nothing is read or written through a link.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use ratatui::text::{Line, Span};

use crate::theme;
use crate::transcript::wrap;

/// The most of a to-do list that is read: a longer one is no checklist to tick by hand.
const MAX_BYTES: u64 = 256 * 1024;
/// The columns a tab indents by, as Markdown counts them.
const TAB_COLUMNS: usize = 4;
/// The columns of a file's indentation that make one step in the tab.
const STEP_COLUMNS: usize = 2;
/// The deepest step the tab shows: deeper items stand at this one.
const MAX_STEPS: usize = 4;
/// How long a folder without a list is not searched again: the panel is drawn many times a second.
const SEARCH_EVERY: Duration = Duration::from_secs(2);
/// The mode of a `TODO.md` Glut creates.
const NEW_MODE: u32 = 0o644;

/// What one line of the list is, as Glut understands it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A checklist item: its indentation in columns, whether it is ticked, its text.
    Item {
        indent: usize,
        done: bool,
        text: String,
    },
    /// A heading, without its `#`.
    Heading(String),
    /// Any other text: shown, never changed.
    Text(String),
    Blank,
}

/// Why a change was not written.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The line is no longer there as it was shown: the file changed in the meantime.
    Changed,
    /// The file could not be read: a link, no plain file, too large, no text.
    Unreadable(String),
    Unwritable(String),
}

impl Failure {
    /// The failure as a note for the owner.
    pub fn message(&self, name: &str) -> String {
        match self {
            Self::Changed => tr!(todos_changed, name = name),
            Self::Unreadable(reason) => tr!(todos_unreadable, name = name, fault = reason),
            Self::Unwritable(reason) => tr!(todos_unwritable, name = name, fault = reason),
        }
    }
}

/// A row of the tab: what is shown, and whether the owner can pick it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Heading(String),
    Text(String),
    /// A checklist item, by its line in the file.
    Item(usize),
    /// The done items, folded into one row (or that row's twin that folds them again).
    Done {
        count: usize,
        unfolded: bool,
    },
}

/// The To-dos tab's state: the file as last read, and the owner's place in it.
#[derive(Debug, Default)]
pub struct Todos {
    /// The list found in the working folder; `None`: there is none yet.
    path: Option<PathBuf>,
    /// When the file read was changed last: a different time reads it again.
    modified: Option<SystemTime>,
    /// When the folder was searched last and held no list.
    searched: Option<Instant>,
    text: String,
    lines: Vec<Kind>,
    /// The chosen row, counting only the rows that can be picked.
    pub chosen: usize,
    /// Whether the done items are shown in their place.
    pub unfolded: bool,
    /// Whether the tab's list has the keys (Ctrl+Y); else the input has them.
    pub keys: bool,
    /// Why the file could not be read at the last look.
    pub fault: Option<String>,
}

impl Todos {
    /// The file's name as the folder has it, or the one the first item creates.
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_name())
            .map_or_else(
                || glut::project::TODO_FILE.to_owned(),
                |name| name.to_string_lossy().into_owned(),
            )
    }

    /// The list's path, when there is one.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Reads the list again if it changed since the last look (or appeared, or went): one look at its time while it
    /// stays where it was, the folder searched only when it is not there.
    pub fn refresh(&mut self, cwd: &Path) {
        let still = self
            .path
            .as_ref()
            .and_then(|path| path.symlink_metadata().ok())
            .filter(|meta| meta.is_file());
        let (path, modified) = match still {
            Some(meta) => (self.path.clone(), meta.modified().ok()),
            None => {
                if self.path.is_none()
                    && self.searched.is_some_and(|at| at.elapsed() < SEARCH_EVERY)
                {
                    return;
                }
                let path = find(cwd);
                self.searched = path.is_none().then(Instant::now);
                let modified = path
                    .as_ref()
                    .and_then(|path| path.symlink_metadata().ok())
                    .and_then(|meta| meta.modified().ok());
                (path, modified)
            }
        };
        if path == self.path && modified == self.modified && path.is_some() {
            return;
        }
        self.path = path;
        self.modified = modified;
        let read = match &self.path {
            Some(path) => read(path).map(Option::unwrap_or_default),
            None => Ok(String::new()),
        };
        match read {
            Ok(text) => {
                self.fault = None;
                self.take(text);
            }
            Err(reason) => {
                self.fault = Some(reason);
                self.take(String::new());
            }
        }
    }

    /// The next [`Self::refresh`] searches the folder even if it held no list a moment ago.
    pub fn search_again(&mut self) {
        self.searched = None;
    }

    /// The items not ticked yet.
    pub fn open(&self) -> usize {
        self.lines
            .iter()
            .filter(|kind| matches!(kind, Kind::Item { done: false, .. }))
            .count()
    }

    /// The items ticked.
    pub fn done(&self) -> usize {
        self.lines
            .iter()
            .filter(|kind| matches!(kind, Kind::Item { done: true, .. }))
            .count()
    }

    /// ↑↓: the chosen row moves by `by`, within the rows that can be picked.
    pub fn choose(&mut self, by: isize) {
        let picks = self.picks().len();
        self.chosen = self
            .chosen
            .saturating_add_signed(by)
            .min(picks.saturating_sub(1));
    }

    /// Space or Enter on the chosen row: an item ticked or unticked in the file, the done row folded or unfolded.
    ///
    /// # Errors
    ///
    /// A [`Failure`]: the line gone or changed, the file unreadable or unwritable.
    pub fn press(&mut self, cwd: &Path) -> Result<(), Failure> {
        match self.picks().get(self.chosen) {
            Some(Row::Done { .. }) => {
                self.unfolded = !self.unfolded;
                Ok(())
            }
            Some(Row::Item(line)) => {
                let (line, shown) = (*line, self.raw_line(*line));
                self.change(cwd, |text| toggle(text, line, &shown))
            }
            _ => Ok(()),
        }
    }

    /// `/todo <text>` and the tab's **a**: the item goes at the end of the list; the first one creates `TODO.md`.
    ///
    /// # Errors
    ///
    /// A [`Failure`]: the file unreadable or unwritable.
    pub fn add(&mut self, cwd: &Path, item: &str) -> Result<(), Failure> {
        if self.path.is_none() {
            self.path = Some(cwd.join(glut::project::TODO_FILE));
        }
        self.change(cwd, |text| Some(append(text, item)))
    }

    /// `/todo clean`: the done items go, each with what stands under it when all of that is done too.
    ///
    /// # Errors
    ///
    /// A [`Failure`]: the file unreadable or unwritable.
    pub fn clean(&mut self, cwd: &Path) -> Result<usize, Failure> {
        let mut dropped = 0;
        self.change(cwd, |text| {
            let (kept, count) = drop_done(text);
            dropped = count;
            Some(kept)
        })?;
        Ok(dropped)
    }

    /// The tab's rows in `width` columns, at most `room` of them: the chosen one in view, marked while the list has the
    /// keys. Each line comes with the pick it shows (what ↑↓ would choose), `None` for a heading, a text or the scroll
    /// note — for a click to find (T71).
    pub fn lines(&self, width: usize, room: usize) -> Vec<(Option<usize>, Line<'static>)> {
        let mut blocks: Vec<(bool, Option<usize>, Vec<Line<'static>>)> = Vec::new();
        let mut pick = 0;
        for row in self.rows() {
            let picked = matches!(row, Row::Item(_) | Row::Done { .. });
            let chosen = picked && self.keys && pick == self.chosen;
            let owner = picked.then_some(pick);
            if picked {
                pick += 1;
            }
            blocks.push((chosen, owner, self.row_lines(&row, chosen, width)));
        }
        let total: usize = blocks.iter().map(|(_, _, lines)| lines.len()).sum();
        let mut skip = 0;
        // A list longer than the room says so on its last row.
        let room = if total > room {
            room.saturating_sub(1)
        } else {
            room
        };
        if total > room {
            // The chosen row's last line ends the view when it would fall below it.
            let mut end = 0;
            for (chosen, _, lines) in &blocks {
                end += lines.len();
                if *chosen {
                    break;
                }
            }
            skip = end.saturating_sub(room).min(total - room);
            if !self.keys {
                skip = 0;
            }
        }
        let mut shown: Vec<(Option<usize>, Line<'static>)> = blocks
            .into_iter()
            .flat_map(|(_, owner, lines)| lines.into_iter().map(move |line| (owner, line)))
            .skip(skip)
            .take(room)
            .collect();
        if total > room {
            let below = total - skip - shown.len();
            let text = match (skip, below) {
                (0, _) => format!(" {}", tr!(rows_below, below = below)),
                (_, 0) => format!(" {}", tr!(rows_above, above = skip)),
                _ => format!(" {}", tr!(rows_around, above = skip, below = below)),
            };
            shown.push((None, Line::from(Span::styled(text, theme::quiet()))));
        }
        shown
    }

    /// What the tab shows of one row.
    fn row_lines(&self, row: &Row, chosen: bool, width: usize) -> Vec<Line<'static>> {
        let lead = if chosen { "›" } else { " " };
        let styled = |text: String, style| {
            wrap(&text, width.saturating_sub(1))
                .into_iter()
                .map(|row| Line::from(Span::styled(format!(" {row}"), style)))
                .collect()
        };
        match row {
            Row::Heading(text) => styled(text.clone(), theme::strong()),
            Row::Text(text) => styled(text.clone(), theme::quiet()),
            Row::Done { count, unfolded } => {
                let action = if *unfolded {
                    tr!(todos_fold)
                } else {
                    tr!(todos_show)
                };
                let style = if chosen {
                    theme::glow()
                } else {
                    theme::quiet()
                };
                vec![Line::from(Span::styled(
                    format!(
                        "{lead}✓ {}",
                        tr!(todos_done, count = count, action = action)
                    ),
                    style,
                ))]
            }
            Row::Item(line) => {
                let Some(Kind::Item { indent, done, text }) = self.lines.get(*line) else {
                    return Vec::new();
                };
                let steps = (indent / STEP_COLUMNS).min(MAX_STEPS);
                let pad = " ".repeat(steps * STEP_COLUMNS);
                let (mark, style) = match (done, chosen) {
                    (_, true) => (if *done { "✓" } else { "○" }, theme::glow()),
                    (true, false) => ("✓", theme::quiet()),
                    (false, false) => ("○", ratatui::style::Style::default()),
                };
                let room = width.saturating_sub(pad.len() + 3).max(4);
                wrap(text, room)
                    .into_iter()
                    .enumerate()
                    .map(|(at, row)| {
                        let head = if at == 0 {
                            format!("{lead}{pad}{mark} ")
                        } else {
                            format!(" {pad}  ")
                        };
                        Line::from(vec![Span::styled(head, style), Span::styled(row, style)])
                    })
                    .collect()
            }
        }
    }

    /// The rows in order: the done items left out while folded, one row for them at the end.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (line, kind) in self.lines.iter().enumerate() {
            match kind {
                Kind::Item { done: true, .. } if !self.unfolded => {}
                Kind::Item { .. } => rows.push(Row::Item(line)),
                Kind::Heading(text) => rows.push(Row::Heading(text.clone())),
                Kind::Text(text) => rows.push(Row::Text(text.clone())),
                Kind::Blank => {}
            }
        }
        let count = self.done();
        if count > 0 {
            rows.push(Row::Done {
                count,
                unfolded: self.unfolded,
            });
        }
        rows
    }

    /// The rows the owner can pick.
    fn picks(&self) -> Vec<Row> {
        self.rows()
            .into_iter()
            .filter(|row| matches!(row, Row::Item(_) | Row::Done { .. }))
            .collect()
    }

    /// Line `line` of the file as read, without its line break.
    fn raw_line(&self, line: usize) -> String {
        self.text
            .split_inclusive('\n')
            .nth(line)
            .map(|raw| raw.trim_end_matches(['\n', '\r']).to_owned())
            .unwrap_or_default()
    }

    /// One change: the file read as it is now, changed by `edit` (`None`: the line it looked for is gone), written
    /// whole beside it and renamed into place, then taken as the list.
    fn change(
        &mut self,
        cwd: &Path,
        edit: impl FnOnce(&str) -> Option<String>,
    ) -> Result<(), Failure> {
        let path = self
            .path
            .clone()
            .unwrap_or_else(|| cwd.join(glut::project::TODO_FILE));
        let now = read(&path)
            .map_err(Failure::Unreadable)?
            .unwrap_or_default();
        let changed = edit(&now).ok_or(Failure::Changed)?;
        // Nothing changed writes nothing: no empty file made, no time touched.
        if changed != now {
            write(&path, &changed).map_err(Failure::Unwritable)?;
        }
        self.modified = path
            .symlink_metadata()
            .ok()
            .and_then(|meta| meta.modified().ok());
        self.path = Some(path);
        self.fault = None;
        self.take(changed);
        Ok(())
    }

    /// The text read becomes the list; the chosen item stays chosen where it moved to (found by its text), else the
    /// chosen row stays within the list.
    fn take(&mut self, text: String) {
        let was = match self.picks().get(self.chosen) {
            Some(Row::Item(line)) => Some((*line, self.raw_line(*line))),
            _ => None,
        };
        self.lines = parse(&text);
        self.text = text;
        if let Some((line, raw)) = was {
            let found = self
                .picks()
                .iter()
                .enumerate()
                .filter_map(|(at, row)| match row {
                    Row::Item(now) if self.raw_line(*now) == raw => Some((at, now.abs_diff(line))),
                    _ => None,
                })
                .min_by_key(|(_, distance)| *distance)
                .map(|(at, _)| at);
            if let Some(at) = found {
                self.chosen = at;
            }
        }
        self.choose(0);
    }
}

/// The to-do list of `cwd`, under the first of its names the folder has.
fn find(cwd: &Path) -> Option<PathBuf> {
    glut::project::to_do_list(cwd).map(|name| cwd.join(name))
}

/// The list's text, read without following a link; `None`: there is no file yet.
fn read(path: &Path) -> Result<Option<String>, String> {
    let file = match glut::providers::open_no_follow(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let meta = file.metadata().map_err(|error| error.to_string())?;
    if !meta.is_file() {
        return Err(tr!(no_plain_file).to_owned());
    }
    let too_large = || tr!(larger_than_kb, kb = MAX_BYTES / 1024);
    if meta.len() > MAX_BYTES {
        return Err(too_large());
    }
    // One byte more than allowed: a file that grew since its size was looked at is refused, never cut.
    let mut text = String::new();
    file.take(MAX_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|error| error.to_string())?;
    if text.len() as u64 > MAX_BYTES {
        return Err(too_large());
    }
    Ok(Some(text))
}

/// `text` written as the list: whole, beside it, then renamed into place — with the mode the file had.
fn write(path: &Path, text: &str) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = match path.symlink_metadata() {
        Ok(meta) if meta.is_file() => meta.permissions().mode() & 0o777,
        Ok(_) => return Err(tr!(no_plain_file).to_owned()),
        Err(_) => NEW_MODE,
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    glut::providers::write_replacing(dir, path, text.as_bytes(), mode)
        .map_err(|error| error.to_string())
}

/// Every line of `text`, understood.
pub fn parse(text: &str) -> Vec<Kind> {
    text.split_inclusive('\n')
        .map(|raw| {
            let line = raw.trim_end_matches(['\n', '\r']);
            if let Some((indent, done, text)) = item(line) {
                return Kind::Item {
                    indent,
                    done,
                    text: text.to_owned(),
                };
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                Kind::Blank
            } else if trimmed.starts_with('#') {
                Kind::Heading(trimmed.trim_start_matches('#').trim().to_owned())
            } else {
                Kind::Text(trimmed.to_owned())
            }
        })
        .collect()
}

/// A checklist item's indentation, box and text: `- [ ] text`, `* [x] text`, `+ [X] text`.
fn item(line: &str) -> Option<(usize, bool, &str)> {
    // A byte order mark before the first line is no indentation and no text.
    let line = line.strip_prefix('\u{feff}').unwrap_or(line);
    let rest = line.trim_start_matches([' ', '\t']);
    let indent = indent(line);
    let rest = rest.strip_prefix(['-', '*', '+'])?.strip_prefix(' ')?;
    let (done, rest) = if let Some(rest) = rest.strip_prefix("[ ]") {
        (false, rest)
    } else {
        (
            true,
            rest.strip_prefix("[x]")
                .or_else(|| rest.strip_prefix("[X]"))?,
        )
    };
    if !rest.is_empty() && !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((indent, done, rest.trim()))
}

/// The columns a line is indented by.
fn indent(line: &str) -> usize {
    line.chars()
        .take_while(|c| matches!(c, ' ' | '\t'))
        .map(|c| if c == '\t' { TAB_COLUMNS } else { 1 })
        .sum()
}

/// The item that reads `shown` ticked or unticked — of several such, the one nearest line `near`; `None` when no
/// item reads so any more.
pub fn toggle(text: &str, near: usize, shown: &str) -> Option<String> {
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at = lines
        .iter()
        .enumerate()
        .filter(|(_, raw)| raw.trim_end_matches(['\n', '\r']) == shown)
        .min_by_key(|(at, _)| at.abs_diff(near))
        .map(|(at, _)| at)?;
    let (_, done, _) = item(shown)?;
    let (from, to) = if done {
        (["[x]", "[X]"], "[ ]")
    } else {
        (["[ ]", "[ ]"], "[x]")
    };
    let raw = lines[at];
    // The box is the first one on the line: the text after it may hold one of its own.
    let at_box = from.iter().filter_map(|mark| raw.find(mark)).min()?;
    let changed = format!("{}{to}{}", &raw[..at_box], &raw[at_box + 3..]);
    lines[at] = &changed;
    Some(lines.concat())
}

/// `item` as a new open item at the end of `text`, on a line of its own.
pub fn append(text: &str, item: &str) -> String {
    let item: String = item
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    // A file that ends its lines with CR LF gets one more such line.
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = text.to_owned();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(newline);
    }
    out.push_str(&format!("- [ ] {}{newline}", item.trim()));
    out
}

/// `text` without its done items, and how many went: an item goes with the lines under it (deeper, up to a blank
/// line) when every item among them is done too; one with an open item under it stays.
pub fn drop_done(text: &str) -> (String, usize) {
    let raw: Vec<&str> = text.split_inclusive('\n').collect();
    let kinds = parse(text);
    let mut kept = String::with_capacity(text.len());
    let mut dropped = 0;
    let mut at = 0;
    while at < raw.len() {
        if let Kind::Item {
            indent: own,
            done: true,
            ..
        } = kinds[at]
        {
            let end = (at + 1..raw.len())
                .find(|&next| kinds[next] == Kind::Blank || indent(raw[next]) <= own)
                .unwrap_or(raw.len());
            let all_done = kinds[at..end]
                .iter()
                .all(|kind| !matches!(kind, Kind::Item { done: false, .. }));
            if all_done {
                dropped += kinds[at..end]
                    .iter()
                    .filter(|kind| matches!(kind, Kind::Item { .. }))
                    .count();
                at = end;
                continue;
            }
        }
        kept.push_str(raw[at]);
        at += 1;
    }
    (kept, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str =
        "# To-dos\n\nSome words.\n- [ ] first\n  - [x] its part\n* [X] second\n- [ ]third\n";

    #[test]
    fn a_checklist_is_understood_and_what_is_not_stays_text() {
        let kinds = parse(LIST);
        assert_eq!(kinds[0], Kind::Heading("To-dos".to_owned()));
        assert_eq!(kinds[1], Kind::Blank);
        assert_eq!(kinds[2], Kind::Text("Some words.".to_owned()));
        assert!(
            matches!(&kinds[3], Kind::Item { indent: 0, done: false, text } if text == "first")
        );
        assert!(matches!(
            &kinds[4],
            Kind::Item {
                indent: 2,
                done: true,
                ..
            }
        ));
        assert!(matches!(&kinds[5], Kind::Item { done: true, .. }));
        // No space after the box: no item.
        assert_eq!(kinds[6], Kind::Text("- [ ]third".to_owned()));
        assert!(matches!(
            &parse("\u{feff}- [ ] marked")[0],
            Kind::Item { indent: 0, .. }
        ));
    }

    #[test]
    fn a_tick_changes_its_line_alone_found_by_its_text() {
        let ticked = toggle(LIST, 3, "- [ ] first").unwrap();
        assert_eq!(ticked, LIST.replace("- [ ] first", "- [x] first"));
        let back = toggle(&ticked, 3, "- [x] first").unwrap();
        assert_eq!(back, LIST);
        // Moved by a line above it: still found by its text.
        let moved = format!("- [ ] new\n{LIST}");
        assert_eq!(
            toggle(&moved, 3, "- [ ] first").unwrap(),
            moved.replace("- [ ] first", "- [x] first")
        );
        assert!(toggle(LIST, 3, "- [ ] gone").is_none());
    }

    #[test]
    fn of_two_alike_the_nearer_one_is_ticked_and_a_box_in_the_text_stays() {
        let text = "- [ ] same\n- [ ] other [ ] box\n- [ ] same\n";
        assert_eq!(
            toggle(text, 2, "- [ ] same").unwrap(),
            "- [ ] same\n- [ ] other [ ] box\n- [x] same\n"
        );
        assert_eq!(
            toggle(text, 1, "- [ ] other [ ] box").unwrap(),
            "- [ ] same\n- [x] other [ ] box\n- [ ] same\n"
        );
    }

    #[test]
    fn an_item_goes_at_the_end_on_a_line_of_its_own() {
        assert_eq!(append("", "idea"), "- [ ] idea\n");
        assert_eq!(append("- [ ] a", " b\tc "), "- [ ] a\n- [ ] b c\n");
        assert_eq!(append("x\n", "y\nz"), "x\n- [ ] y z\n");
        assert_eq!(append("- [ ] a\r\n", "b"), "- [ ] a\r\n- [ ] b\r\n");
    }

    #[test]
    fn clean_drops_done_items_with_their_done_parts_but_keeps_one_with_open_parts() {
        let text = "# Head\n- [x] done\n  - [x] done part\n  note\n- [x] parent\n  - [ ] open part\n- [ ] open\n\n\
                    - [X] last\n";
        let (kept, dropped) = drop_done(text);
        assert_eq!(
            kept,
            "# Head\n- [x] parent\n  - [ ] open part\n- [ ] open\n\n"
        );
        assert_eq!(dropped, 3);
    }

    #[test]
    fn the_tab_folds_the_done_ones_and_reads_the_file_anew_when_it_changes() {
        let dir = std::env::temp_dir().join(format!("glut-tui-todos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut todos = Todos::default();
        todos.refresh(&dir);
        assert!(todos.path().is_none());
        assert_eq!(todos.name(), "TODO.md");
        todos.add(&dir, "first idea").unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("TODO.md")).unwrap(),
            "- [ ] first idea\n"
        );
        todos.add(&dir, "second").unwrap();
        assert_eq!(todos.open(), 2);
        todos.press(&dir).unwrap();
        assert_eq!((todos.open(), todos.done()), (1, 1));
        // Folded, the done one is a row at the end: one item and that row can be picked.
        assert_eq!(todos.picks().len(), 2);
        todos.choose(1);
        todos.press(&dir).unwrap();
        assert!(todos.unfolded);
        assert_eq!(todos.picks().len(), 3);
        // Someone else's change: the tick looks for a line no longer there.
        std::fs::write(dir.join("TODO.md"), "- [ ] rewritten\n").unwrap();
        todos.chosen = 1;
        assert_eq!(todos.press(&dir), Err(Failure::Changed));
        std::fs::write(dir.join("TODO.md"), "- [ ] rewritten\n- [x] gone\n").unwrap();
        todos.modified = None;
        todos.refresh(&dir);
        assert_eq!((todos.open(), todos.done()), (1, 1));
        assert_eq!(todos.clean(&dir).unwrap(), 1);
        assert_eq!(
            std::fs::read_to_string(dir.join("TODO.md")).unwrap(),
            "- [ ] rewritten\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_chosen_item_stays_chosen_when_lines_come_before_it() {
        let mut todos = Todos {
            keys: true,
            ..Todos::default()
        };
        todos.take("- [ ] a\n- [ ] b\n".to_owned());
        todos.choose(1);
        todos.take("- [ ] new\n- [ ] a\n- [ ] b\n".to_owned());
        assert_eq!(todos.picks()[todos.chosen], Row::Item(2));
    }

    #[test]
    fn a_linked_list_is_neither_read_nor_written() {
        let dir = std::env::temp_dir().join(format!("glut-tui-todos-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("elsewhere.md"), "- [ ] secret\n").unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere.md"), dir.join("TODO.md")).unwrap();
        let mut todos = Todos::default();
        todos.refresh(&dir);
        assert_eq!(todos.open(), 0);
        assert!(todos.add(&dir, "x").is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("elsewhere.md")).unwrap(),
            "- [ ] secret\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
