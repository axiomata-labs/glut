//! Clickable links (`docs/plans/glut-tui.md`, TUI9d): where the terminal speaks OSC 8, a link's text is the link and
//! the URL after it goes. ratatui knows no links, and an escape inside a cell would break its sense of widths, so a
//! link's cells carry a mark instead — an underline colour standing for the link's number — that the frame's buffer
//! gives up after each draw ([`take_spots`]): the marks are wiped before the frame goes out, and the cells they stood
//! on are written once more inside OSC 8 ([`write`]). An overlay clears the cells it covers (a list, a question), marks
//! and all, so no link is ever written over it. A link never hides where it goes: its host stays after its text unless
//! the text names it ([`after_text`]).

use std::io::{self, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::{cursor, queue, style};
use ratatui::backend::IntoCrossterm as _;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};
use serde::{Deserialize, Serialize};

/// How links are shown (`/links`, kept in `prefs.json`); none kept: clickable where the terminal is known to speak
/// OSC 8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Links {
    /// The text is the link (OSC 8).
    Clickable,
    /// The URL after the text, as before.
    Plain,
}

impl Links {
    pub fn parse(word: &str) -> Option<Links> {
        match word {
            "clickable" | "osc8" => Some(Links::Clickable),
            "plain" => Some(Links::Plain),
            _ => None,
        }
    }
}

/// The most links one session marks: one mark is one of 255 colour numbers.
const MOST: usize = 255;
/// The longest URL made clickable; a longer one stays written out.
const MOST_URL: usize = 2048;

static CLICKABLE: AtomicBool = AtomicBool::new(false);
/// The URLs marked so far, by their number: a link's number stays the same for every frame its lines are drawn in.
static URLS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Links are clickable as `choice` says, or — with no choice kept — where the terminal is known to speak OSC 8.
pub fn set(choice: Option<Links>) {
    let on = match choice {
        Some(choice) => choice == Links::Clickable,
        // Tests do not guess from the terminal they happen to run in.
        None if cfg!(test) => false,
        None => speaks_osc8(
            std::env::var("TERM_PROGRAM").ok().as_deref(),
            std::env::var("TERM").ok().as_deref(),
        ),
    };
    CLICKABLE.store(on, Ordering::Relaxed);
}

/// Whether links are clickable now.
pub fn clickable() -> bool {
    CLICKABLE.load(Ordering::Relaxed)
}

/// The terminals known to open an OSC 8 link on a click: Ghostty, iTerm2, WezTerm and kitty.
fn speaks_osc8(term_program: Option<&str>, term: Option<&str>) -> bool {
    let program = term_program.unwrap_or_default().to_lowercase();
    let term = term.unwrap_or_default().to_lowercase();
    matches!(program.as_str(), "ghostty" | "iterm.app" | "wezterm")
        || term.contains("kitty")
        || term.contains("ghostty")
}

/// Whether `url` may become a link: the web only, nothing that could end the escape around it, and no user part
/// before its host (`https://good.example@evil.example` reads as one host and opens the other).
fn safe(url: &str) -> bool {
    (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() <= MOST_URL
        && url.chars().all(|c| c.is_ascii_graphic())
        && host(url).is_some_and(|host| !host.is_empty() && !host.contains('@'))
}

/// The part of `url` between its scheme and its path: the host (and port) it opens.
fn host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    Some(rest.split(['/', '?', '#']).next().unwrap_or(rest))
}

/// What stays after a clickable link's text: nothing where the text says where it goes (the URL itself, or its host),
/// else the host in brackets — a link's text never hides where a click goes. Text with nothing to see keeps the whole
/// URL. `None`: the link is not clickable, the whole URL follows as before.
pub fn after_text(said: &str, url: &str) -> Option<String> {
    mark(url)?;
    Some(after(said, url))
}

/// [`after_text`] for a link that is clickable.
fn after(said: &str, url: &str) -> String {
    let said = said.trim();
    let host = host(url).unwrap_or(url);
    if said.is_empty() {
        format!(" ({url})")
    } else if said == url || said.to_lowercase().contains(&host.to_lowercase()) {
        String::new()
    } else {
        format!(" ({host})")
    }
}

/// The mark for `url`'s text, while links are clickable: `None` keeps the URL written after the text.
pub fn mark(url: &str) -> Option<Color> {
    if !clickable() || !safe(url) {
        return None;
    }
    let mut urls = URLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let number = match urls.iter().position(|known| known == url) {
        Some(number) => number,
        None if urls.len() < MOST => {
            urls.push(url.to_owned());
            urls.len() - 1
        }
        None => return None,
    };
    u8::try_from(number).ok().map(Color::Indexed)
}

/// A link's cells on the screen: where its run starts, its cells' text and style, and its URL.
#[derive(Debug, Clone, PartialEq)]
pub struct Spot {
    pub x: u16,
    pub y: u16,
    /// The link's number, its OSC 8 id.
    pub number: u8,
    pub cells: Vec<(String, Style)>,
    pub url: String,
}

/// The links of the frame drawn into `buffer`, one spot per run of a link's cells on a row; every mark wiped, so the
/// frame goes out as it looks.
pub fn take_spots(buffer: &mut Buffer) -> Vec<Spot> {
    let urls = URLS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let area = buffer.area;
    let mut spots: Vec<Spot> = Vec::new();
    for y in area.top()..area.bottom() {
        // The link whose run this row is in, by its number.
        let mut open: Option<u8> = None;
        for x in area.left()..area.right() {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            let number = match cell.underline_color {
                Color::Indexed(number) => Some(number),
                _ => None,
            };
            let Some(number) = number else {
                open = None;
                continue;
            };
            cell.underline_color = Color::Reset;
            let Some(url) = urls.get(usize::from(number)) else {
                open = None;
                continue;
            };
            let style = Style::default()
                .fg(cell.fg)
                .bg(cell.bg)
                .add_modifier(cell.modifier);
            let symbol = cell.symbol().to_owned();
            match spots.last_mut() {
                Some(spot) if open == Some(number) => spot.cells.push((symbol, style)),
                _ => spots.push(Spot {
                    x,
                    y,
                    number,
                    cells: vec![(symbol, style)],
                    url: url.clone(),
                }),
            }
            open = Some(number);
        }
    }
    spots
}

/// The `spots` written once more inside OSC 8 — the same text in the same style, so only the link is new; the
/// cursor stays where the frame left it.
///
/// # Errors
///
/// What writing to the terminal fails at.
pub fn write(out: &mut impl Write, spots: &[Spot]) -> io::Result<()> {
    if spots.is_empty() {
        return Ok(());
    }
    queue!(out, cursor::SavePosition)?;
    for spot in spots {
        queue!(
            out,
            cursor::MoveTo(spot.x, spot.y),
            // One id per URL: a link wrapped over two rows lights up whole under the pointer.
            style::Print(format!("\x1b]8;id={};{}\x1b\\", spot.number, spot.url))
        )?;
        for (symbol, look) in &spot.cells {
            // A wide character's second cell holds nothing: the character before it covers it.
            if symbol.is_empty() {
                continue;
            }
            queue!(
                out,
                style::PrintStyledContent(style::StyledContent::new(
                    look.into_crossterm(),
                    symbol.as_str()
                ))
            )?;
        }
        queue!(
            out,
            style::Print("\x1b]8;;\x1b\\"),
            style::SetAttribute(style::Attribute::Reset)
        )?;
    }
    queue!(out, cursor::RestorePosition)?;
    out.flush()
}

/// The URL of the link drawn at `column`, `row` in the last frame, if any.
pub fn under(spots: &[Spot], column: u16, row: u16) -> Option<&str> {
    spots
        .iter()
        .find(|spot| {
            let width: usize = spot
                .cells
                .iter()
                .map(|(symbol, _)| unicode_width::UnicodeWidthStr::width(symbol.as_str()).max(1))
                .sum();
            spot.y == row && column >= spot.x && usize::from(column - spot.x) < width
        })
        .map(|spot| spot.url.as_str())
}

/// Opens `url` in the owner's browser (a click on a link, which the screen's mouse never hands the terminal): the
/// web only, as a link is made.
///
/// # Errors
///
/// A message when it is no link Glut makes or the opener cannot start.
pub fn open(url: &str) -> Result<(), String> {
    if !safe(url) {
        return Err("only a web link opens".into());
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = std::process::Command::new(opener)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|err| format!("{opener}: {err}"))?;
    // Waited for beside the screen: no process left behind, no frame held up.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The host a link opens, for saying so.
pub fn host_of(url: &str) -> &str {
    host(url).unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn only_web_urls_without_anything_that_ends_an_escape_are_links() {
        assert!(safe("https://example.com/a?b=c"));
        assert!(!safe("file:///etc/passwd"));
        assert!(!safe("https://example.com/\x1b\\x"));
        assert!(!safe("https://exa mple.com"));
        assert!(!safe("https://docs.python.org@evil.example/login"));
        assert!(!safe("https://"));
        assert!(speaks_osc8(Some("ghostty"), None));
        assert!(speaks_osc8(Some("iTerm.app"), None));
        assert!(speaks_osc8(None, Some("xterm-kitty")));
        assert!(!speaks_osc8(Some("Apple_Terminal"), Some("xterm-256color")));
    }

    #[test]
    fn a_links_host_stays_after_its_text_unless_the_text_names_it() {
        assert_eq!(
            after("docs.python.org", "https://evil.example/login"),
            " (evil.example)"
        );
        assert_eq!(
            after("the GitHub repo", "https://github.com/o/r"),
            " (github.com)"
        );
        assert_eq!(after("github.com/o/r", "https://github.com/o/r"), "");
        assert_eq!(after("https://x.example", "https://x.example"), "");
        assert_eq!(after(" ", "https://x.example/a"), " (https://x.example/a)");
        assert_eq!(host("https://a.example:8080/p?q"), Some("a.example:8080"));
    }

    #[test]
    fn a_click_finds_the_link_under_it() {
        let spot = Spot {
            x: 4,
            y: 2,
            number: 0,
            cells: "here"
                .chars()
                .map(|c| (c.to_string(), Style::default()))
                .collect(),
            url: "https://glut.test/under".into(),
        };
        let spots = [spot];
        assert_eq!(under(&spots, 4, 2), Some("https://glut.test/under"));
        assert_eq!(under(&spots, 7, 2), Some("https://glut.test/under"));
        assert_eq!(under(&spots, 8, 2), None);
        assert_eq!(under(&spots, 5, 3), None);
        assert!(open("file:///etc/passwd").is_err());
    }

    #[test]
    fn a_marked_run_becomes_a_spot_and_its_mark_is_wiped() {
        let number = {
            let mut urls = URLS.lock().unwrap();
            urls.push("https://glut.test/spot".into());
            u8::try_from(urls.len() - 1).unwrap()
        };
        let mut buffer = Buffer::empty(Rect::new(0, 0, 10, 2));
        buffer.set_string(0, 1, "go here!", Style::default());
        for x in 3..7 {
            buffer[(x, 1)].underline_color = Color::Indexed(number);
        }
        let spots = take_spots(&mut buffer);
        let spot = spots
            .iter()
            .find(|spot| spot.url == "https://glut.test/spot")
            .unwrap();
        assert_eq!((spot.x, spot.y), (3, 1));
        let text: String = spot
            .cells
            .iter()
            .map(|(symbol, _)| symbol.as_str())
            .collect();
        assert_eq!(text, "here");
        assert!(
            (0..10).all(|x| buffer[(x, 1)].underline_color == Color::Reset),
            "the frame goes out unmarked"
        );
        let mut out = Vec::new();
        write(&mut out, &spots).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(
            out.contains(&format!("\x1b]8;id={number};https://glut.test/spot\x1b\\"))
                && out.ends_with("\x1b8")
        );
    }
}
