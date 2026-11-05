//! The screens of a recording: each row as runs of text in one style, kept only where a row changed, and written as
//! the JSON the site's player reads.
//!
//! ```text
//! {"cols":120,"rows":34,"styles":[["#ece6dc","",0], …],
//!  "frames":[{"t":0,"rows":{"0":[["text",styleIndex], …], …}}, …]}
//! ```
//!
//! A colour is `""` (the player's own ink or ground), `#rrggbb`, or `i<n>` for an indexed one; the flags are bold 1,
//! dim 2, italic 4, underline 8, inverse 16.
//!
//! A scene that shows images (`terminal ghostty`) adds `"images":["<base64 PNG>", …]` and, on a frame where what
//! stands changed, `"pics":[[image, x, y, cols, rows], …]` — every picture standing from then on, none for `[]`.

use std::collections::{BTreeMap, HashMap};

use serde_json::{Value, json};

use super::graphics::Picture;

/// A cell's look.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct Style {
    fg: String,
    bg: String,
    flags: u8,
}

/// A row: runs of text in one style.
pub type Row = Vec<(String, usize)>;

/// A kept frame: its time, its changed rows, and every picture standing where what stands changed.
struct Frame {
    millis: u64,
    rows: BTreeMap<usize, Row>,
    pictures: Option<Vec<Picture>>,
}

/// The recording being made.
pub struct Film {
    cols: u16,
    rows: u16,
    styles: Vec<Style>,
    known: HashMap<Style, usize>,
    last: Vec<Row>,
    frames: Vec<Frame>,
    /// The images the terminal was sent, as base64 PNG.
    images: Vec<String>,
    /// The pictures standing now, and as the last kept frame had them.
    pictures: Vec<Picture>,
    last_pictures: Vec<Picture>,
    /// The home folder, shown as `~` wherever it stands.
    home: String,
}

impl Film {
    /// A recording of a `cols` × `rows` terminal, `home` shown as `~`.
    pub fn new(cols: u16, rows: u16, home: &str) -> Self {
        Film {
            cols,
            rows,
            styles: Vec::new(),
            known: HashMap::new(),
            last: Vec::new(),
            frames: Vec::new(),
            images: Vec::new(),
            pictures: Vec::new(),
            last_pictures: Vec::new(),
            home: home.trim_end_matches('/').to_owned(),
        }
    }

    /// Keeps an image the terminal was sent (base64 PNG); returns its place, which [`Picture::image`] names. One sent
    /// again (Glut sends its pictures anew after a clear) is kept once.
    pub fn image(&mut self, data: String) -> usize {
        if let Some(at) = self.images.iter().position(|known| *known == data) {
            return at;
        }
        self.images.push(data);
        self.images.len() - 1
    }

    /// The pictures standing on the screen from now on.
    pub fn stand(&mut self, pictures: Vec<Picture>) {
        self.pictures = pictures;
    }

    /// Keeps `screen` at `millis` from the start, if a row or a picture changed since the last one kept.
    pub fn keep(&mut self, millis: u64, screen: &vt100::Screen) -> bool {
        let rows: Vec<Row> = (0..self.rows).map(|row| self.row(screen, row)).collect();
        let changed: BTreeMap<usize, Row> = rows
            .iter()
            .enumerate()
            .filter(|(at, row)| self.last.get(*at) != Some(row))
            .map(|(at, row)| (at, row.clone()))
            .collect();
        let pictures = (self.pictures != self.last_pictures).then(|| self.pictures.clone());
        if changed.is_empty() && pictures.is_none() {
            return false;
        }
        self.frames.push(Frame {
            millis,
            rows: changed,
            pictures,
        });
        self.last = rows;
        self.last_pictures = self.pictures.clone();
        true
    }

    /// The text on screen now, rows joined by newlines — what a `wait` looks for.
    pub fn text(screen: &vt100::Screen) -> String {
        screen.contents()
    }

    /// The recording as JSON, its last frame held `hold` milliseconds.
    pub fn json(&self, hold: u64) -> Value {
        let styles: Vec<Value> = self
            .styles
            .iter()
            .map(|style| json!([style.fg, style.bg, style.flags]))
            .collect();
        let mut frames: Vec<Value> = self
            .frames
            .iter()
            .map(
                |Frame {
                     millis,
                     rows,
                     pictures,
                 }| {
                    let rows: serde_json::Map<String, Value> = rows
                        .iter()
                        .map(|(at, row)| {
                            let runs: Vec<Value> = row
                                .iter()
                                .map(|(text, style)| json!([text, style]))
                                .collect();
                            (at.to_string(), Value::Array(runs))
                        })
                        .collect();
                    let mut frame = json!({ "t": millis, "rows": rows });
                    if let Some(pictures) = pictures {
                        frame["pics"] = pictures
                            .iter()
                            .map(|p| json!([p.image, p.x, p.y, p.cols, p.rows]))
                            .collect();
                    }
                    frame
                },
            )
            .collect();
        let end = self.frames.last().map_or(0, |frame| frame.millis + hold);
        frames.push(json!({ "t": end, "rows": {} }));
        let mut film =
            json!({ "cols": self.cols, "rows": self.rows, "styles": styles, "frames": frames });
        if !self.images.is_empty() {
            film["images"] = json!(self.images);
        }
        film
    }

    /// One row of the screen as runs, the home folder as `~` and the row filled out to its width again.
    fn row(&mut self, screen: &vt100::Screen, row: u16) -> Row {
        let mut cells: Vec<(String, Style)> = Vec::with_capacity(usize::from(self.cols));
        for col in 0..self.cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.is_wide_continuation() {
                continue;
            }
            // One character per cell, as the terminal drew it: a combining mark or a variation selector after it
            // would make the player's row longer than the screen's. A wide one carries WIDE behind it.
            let mut text: String = cell.contents().chars().next().unwrap_or(' ').to_string();
            if cell.is_wide() {
                text.push(WIDE);
            }
            cells.push((text, style_of(cell)));
        }
        let mut cells = masked(cells, &self.home);
        // A wide character takes two columns: the row is filled to its width by columns, not by characters.
        let width: usize = cells
            .iter()
            .map(|(text, _)| if text.ends_with(WIDE) { 2 } else { 1 })
            .sum();
        let blank = cells
            .last()
            .map(|(_, style)| style.clone())
            .unwrap_or_else(plain);
        for _ in width..usize::from(self.cols) {
            cells.push((" ".to_owned(), blank.clone()));
        }
        let mut runs: Row = Vec::new();
        for (text, style) in cells {
            let index = self.style_index(style);
            match runs.last_mut() {
                Some((run, at)) if *at == index => run.push_str(&text),
                _ => runs.push((text, index)),
            }
        }
        runs
    }

    fn style_index(&mut self, style: Style) -> usize {
        if let Some(index) = self.known.get(&style) {
            return *index;
        }
        let index = self.styles.len();
        self.styles.push(style.clone());
        self.known.insert(style, index);
        index
    }
}

/// A terminal emulator of `rows` × `cols` without scrollback; a size of 0 counts as 1.
pub fn emulator(rows: u16, cols: u16) -> vt100::Parser {
    let size = |n: u16| std::num::NonZeroU16::new(n).unwrap_or(std::num::NonZeroU16::MIN);
    vt100::Parser::new(size(rows), size(cols), 0)
}

fn plain() -> Style {
    Style {
        fg: String::new(),
        bg: String::new(),
        flags: 0,
    }
}

fn style_of(cell: &vt100::Cell) -> Style {
    let colour = |colour: vt100::Color| match colour {
        vt100::Color::Default => String::new(),
        vt100::Color::Idx(index) => format!("i{index}"),
        vt100::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
    };
    let flags = u8::from(cell.bold())
        | u8::from(cell.dim()) << 1
        | u8::from(cell.italic()) << 2
        | u8::from(cell.underline()) << 3
        | u8::from(cell.inverse()) << 4;
    Style {
        fg: colour(cell.fgcolor()),
        bg: colour(cell.bgcolor()),
        flags,
    }
}

/// Written after a character that takes two columns, as the emulator measured it: the player draws that one two
/// cells wide and drops the mark (a character of Unicode's private use, never on a screen of Glut's).
pub const WIDE: char = '\u{E000}';

/// `cells` with every run of cells that spells `home` replaced by one `~` in the first one's style. The columns it
/// saves come back at the next gap of two blanks after it, so what stands right of it (a card's frame, a second pane)
/// keeps its place; with no such gap they come back at the row's end.
fn masked(cells: Vec<(String, Style)>, home: &str) -> Vec<(String, Style)> {
    if home.is_empty() {
        return cells;
    }
    let wanted: Vec<char> = home.chars().collect();
    let blank = |at: usize| cells.get(at).is_some_and(|(text, _)| text == " ");
    let mut out = Vec::with_capacity(cells.len());
    let mut owed = 0;
    let mut at = 0;
    while at < cells.len() {
        let spells = cells.len() - at >= wanted.len()
            && wanted
                .iter()
                .zip(&cells[at..])
                .all(|(c, (text, _))| text.chars().eq(std::iter::once(*c)));
        if spells {
            out.push(("~".to_owned(), cells[at].1.clone()));
            owed += wanted.len() - 1;
            at += wanted.len();
            continue;
        }
        if owed > 0 && blank(at) && blank(at + 1) {
            out.extend(std::iter::repeat_n(cells[at].clone(), owed));
            owed = 0;
        }
        out.push(cells[at].clone());
        at += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(cols: u16, bytes: &[u8]) -> vt100::Parser {
        let mut parser = emulator(2, cols);
        parser.process(bytes);
        parser
    }

    #[test]
    fn a_row_is_runs_of_one_style_and_a_frame_keeps_changed_rows_only() {
        let mut film = Film::new(12, 2, "/Users/me");
        let parser = screen(12, b"\x1b[1;38;2;255;122;26mGlut\x1b[0m ok");
        assert!(film.keep(0, parser.screen()));
        assert!(!film.keep(10, parser.screen()), "nothing changed");
        let json = film.json(500);
        let first = &json["frames"][0]["rows"]["0"];
        assert_eq!(first[0][0], "Glut");
        assert_eq!(
            json["styles"][first[0][1].as_u64().unwrap() as usize],
            json!(["#ff7a1a", "", 1])
        );
        assert_eq!(
            first[1][0], " ok     ",
            "filled to the width: 4 + 8 columns"
        );
        assert_eq!(json["frames"][1]["t"], 500, "the last frame held");
    }

    #[test]
    fn a_picture_standing_or_gone_is_a_frame_of_its_own() {
        let mut film = Film::new(4, 1, "/nowhere");
        let parser = screen(4, b"ab");
        assert!(film.keep(0, parser.screen()));
        let image = film.image("QUJD".to_owned());
        let picture = Picture {
            image,
            x: 1,
            y: 0,
            cols: 2,
            rows: 1,
        };
        film.stand(vec![picture]);
        assert!(film.keep(10, parser.screen()), "the picture came");
        assert!(!film.keep(20, parser.screen()), "nothing changed");
        film.stand(Vec::new());
        assert!(film.keep(30, parser.screen()), "the picture went");
        assert_eq!(
            film.image("QUJD".to_owned()),
            image,
            "sent again, kept once"
        );
        let json = film.json(0);
        assert_eq!(json["images"], json!(["QUJD"]));
        assert!(json["frames"][0].get("pics").is_none());
        assert_eq!(json["frames"][1]["pics"], json!([[0, 1, 0, 2, 1]]));
        assert_eq!(json["frames"][2]["pics"], json!([]));
    }

    #[test]
    fn a_wide_character_is_marked_and_a_combining_mark_dropped() {
        let mut film = Film::new(6, 1, "/nowhere");
        let parser = screen(6, "日e\u{301}x".as_bytes());
        film.keep(0, parser.screen());
        let json = film.json(0);
        let row = json["frames"][0]["rows"]["0"][0][0]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(
            row,
            format!("日{WIDE}ex  "),
            "6 columns: 2 + 1 + 1 + 2 blanks"
        );
    }

    #[test]
    fn the_home_folder_is_shown_as_a_tilde() {
        let mut film = Film::new(30, 1, "/Users/me/");
        let parser = screen(30, b"cd /Users/me/project");
        film.keep(0, parser.screen());
        let json = film.json(0);
        let row = json["frames"][0]["rows"]["0"][0][0].as_str().unwrap();
        assert!(row.starts_with("cd ~/project "), "{row:?}");
        assert_eq!(row.chars().count(), 30);
    }

    #[test]
    fn what_stands_right_of_the_home_folder_keeps_its_column() {
        let mut film = Film::new(30, 1, "/Users/me");
        let parser = screen(30, b"/Users/me/a    |b");
        film.keep(0, parser.screen());
        let json = film.json(0);
        let row = json["frames"][0]["rows"]["0"][0][0].as_str().unwrap();
        assert_eq!(row.find('|'), Some(15), "{row:?}");
        assert_eq!(row.chars().count(), 30);
    }
}
