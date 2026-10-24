//! How much the screen moves, and the reader mode (`docs/plans/glut-tui.md`, TUI9b). Motion follows the app's
//! `core/motion.ts`: *full* — Glimm breathes, sparks rise, the flame flickers; *calm* and *off* — everything holds
//! still, a frame is drawn only when something changed, and "working" counts its seconds once a second instead. macOS
//! "Reduce motion" counts as off whatever the setting says. The reader mode is for a screen reader: words for the key
//! symbols, plain footer, no Glimm, ASCII frames and a line of text at each change of state — and motion off.

use std::borrow::Cow;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use ratatui::buffer::Buffer;
use serde::{Deserialize, Serialize};

/// How much the screen moves (`/motion`, kept in `prefs.json`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Motion {
    #[default]
    Full,
    Calm,
    Off,
}

impl Motion {
    pub fn parse(word: &str) -> Option<Motion> {
        match word {
            "full" => Some(Motion::Full),
            "calm" => Some(Motion::Calm),
            "off" => Some(Motion::Off),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Motion::Full => "full",
            Motion::Calm => "calm",
            Motion::Off => "off",
        }
    }
}

/// Nothing moves on its own: the motion chosen, the reader mode or the system's "Reduce motion".
static STILL: AtomicBool = AtomicBool::new(false);
/// The reader mode is on.
static READER: AtomicBool = AtomicBool::new(false);
/// `glut --reader`: on for this run, whatever the prefs say.
static FORCED_READER: AtomicBool = AtomicBool::new(false);
/// How often the look changed (`/motion`, `/reader`): every pane lays its blocks out anew when it did.
static LOOK: AtomicUsize = AtomicUsize::new(0);

/// The screen moves as `motion` and the reader mode `reader` say, the system's wish on top.
pub fn set(motion: Motion, reader: bool) {
    LOOK.fetch_add(1, Ordering::Relaxed);
    let reader = reader || FORCED_READER.load(Ordering::Relaxed);
    READER.store(reader, Ordering::Relaxed);
    STILL.store(
        reader || motion != Motion::Full || system_reduced(),
        Ordering::Relaxed,
    );
}

/// `glut --reader`: the reader mode for this run.
pub fn force_reader() {
    FORCED_READER.store(true, Ordering::Relaxed);
    READER.store(true, Ordering::Relaxed);
    STILL.store(true, Ordering::Relaxed);
}

/// Whether `glut --reader` holds the reader mode on for this run.
pub fn reader_forced() -> bool {
    FORCED_READER.load(Ordering::Relaxed)
}

/// Whether the screen holds still.
pub fn still() -> bool {
    STILL.load(Ordering::Relaxed)
}

/// Whether the reader mode is on.
pub fn reader() -> bool {
    READER.load(Ordering::Relaxed)
}

/// The look changed in another way (`/links`): every pane lays its blocks out anew.
pub fn relook() {
    LOOK.fetch_add(1, Ordering::Relaxed);
}

/// The look's count of changes so far.
pub fn look() -> usize {
    LOOK.load(Ordering::Relaxed)
}

/// Whether the system asks for reduced motion: macOS "Reduce motion", read once — the owner of the machine asked for
/// it there. Current macOS keeps it in `com.apple.Accessibility`, older ones in `com.apple.universalaccess`.
fn system_reduced() -> bool {
    static REDUCED: OnceLock<bool> = OnceLock::new();
    *REDUCED.get_or_init(|| {
        if cfg!(test) || !cfg!(target_os = "macos") {
            return false;
        }
        let read = |domain: &str, key: &str| {
            std::process::Command::new("defaults")
                .args(["read", domain, key])
                .stderr(std::process::Stdio::null())
                .output()
                .is_ok_and(|out| {
                    out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "1"
                })
        };
        read("com.apple.Accessibility", "ReduceMotionEnabled")
            || read("com.apple.universalaccess", "reduceMotion")
    })
}

/// The keys' symbols and their words, as a screen reader should say them. The modifiers end in `+` and join the key
/// after them; any other two symbols side by side are parted by a space (`↑↓` is "Up Down", `⎋⎋` "Esc Esc").
fn word(symbol: char, arrows: bool) -> Option<&'static str> {
    Some(match symbol {
        '⌃' => "Ctrl+",
        '⇧' => "Shift+",
        '⌥' => "Alt+",
        '⏎' => "Enter",
        '⎋' => "Esc",
        '⇥' => "Tab",
        '⌫' => "Backspace",
        '↑' if arrows => "Up",
        '↓' if arrows => "Down",
        '←' if arrows => "Left",
        '→' if arrows => "Right",
        _ => return None,
    })
}

/// A key hint as the reader mode says it: the key symbols as words (`⌃X ⇥` is "Ctrl+X Tab", `↑↓` "Up Down");
/// unchanged otherwise. Only for text that is all keys and what they do — prose keeps its arrows.
pub fn spoken(text: &str) -> Cow<'_, str> {
    if !reader() {
        return Cow::Borrowed(text);
    }
    words(text, true)
}

/// Running text as the reader mode says it (a note): the modifier and key symbols as words, the arrows left as they
/// are — in "20k → 5k" an arrow is no key.
pub fn spoken_keys(text: &str) -> Cow<'_, str> {
    if !reader() {
        return Cow::Borrowed(text);
    }
    words(text, false)
}

fn words(text: &str, arrows: bool) -> Cow<'_, str> {
    if !text.chars().any(|c| word(c, arrows).is_some())
        && !text.contains("PgUp")
        && !text.contains("PgDn")
    {
        return Cow::Borrowed(text);
    }
    let mut said = String::with_capacity(text.len() + 16);
    // Whether the last thing written was a key's word that wants a space before the next one.
    let mut after_key = false;
    for c in text.chars() {
        match word(c, arrows) {
            Some(spoken) => {
                if after_key {
                    said.push(' ');
                }
                said.push_str(spoken);
                after_key = !spoken.ends_with('+');
            }
            None => {
                said.push(c);
                after_key = false;
            }
        }
    }
    Cow::Owned(said.replace("PgUp", "Page Up").replace("PgDn", "Page Down"))
}

/// The cell `symbol` drawn as plain ASCII, if it is a frame's, one of Glut's marks or one of Powerline's: one cell for
/// one, so the layout stays as it was. What a model or a file wrote (`…`, `•`, `→`) is left alone.
fn plain(symbol: &str) -> Option<&'static str> {
    let mut chars = symbol.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    Some(match c {
        '─' | '━' | '┄' | '╌' => "-",
        '│' | '┃' | '┆' | '╎' | '║' => "|",
        '═' => "=",
        '╭' | '╮' | '╰' | '╯' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' => {
            "+"
        }
        // Glut's own marks: the reading mode's bar, a fold's arrow, the owner's message, the cache's dot.
        '▐' | '▌' | '▍' | '▎' | '▏' => "|",
        '▸' | '›' => ">",
        '▾' => "v",
        '▴' | '▵' | '▲' => "^",
        '●' => "*",
        '○' => "o",
        // The meter's bar: what is used, what is free.
        '█' => "#",
        '░' => ".",
        '\u{e0a0}'..='\u{e0d4}' => " ",
        _ => return None,
    })
}

/// The frame drawn in plain ASCII while the reader mode is on: frames, marks and Powerline's glyphs cell by cell.
pub fn plain_cells(buffer: &mut Buffer) {
    if !reader() {
        return;
    }
    for cell in &mut buffer.content {
        if let Some(ascii) = plain(cell.symbol()) {
            cell.set_symbol(ascii);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_levels_read_as_typed_and_as_kept() {
        assert_eq!(Motion::parse("calm"), Some(Motion::Calm));
        assert_eq!(Motion::parse("still"), None);
        assert_eq!(serde_json::to_string(&Motion::Off).unwrap(), "\"off\"");
    }

    #[test]
    fn key_symbols_become_words_and_frames_ascii_one_cell_for_one() {
        assert_eq!(
            words("⌃X ⇥ the panel · ⇧⏎ a line", true),
            "Ctrl+X Tab the panel · Shift+Enter a line"
        );
        assert_eq!(
            words("↑↓ choose · ←→ tabs · ⎋⎋ rewind", true),
            "Up Down choose · Left Right tabs · Esc Esc rewind"
        );
        assert_eq!(
            words("⌃⌥9 · PgUp PgDn", true),
            "Ctrl+Alt+9 · Page Up Page Down"
        );
        // In running text an arrow is no key.
        assert_eq!(
            words("20k → 5k tokens · ⌃C again", false),
            "20k → 5k tokens · Ctrl+C again"
        );
        assert_eq!(words("plain text", true), "plain text");
        assert_eq!(plain("╭"), Some("+"));
        assert_eq!(plain("─"), Some("-"));
        assert_eq!(plain("█"), Some("#"));
        assert_eq!(plain("\u{e0b4}"), Some(" "));
        assert_eq!(plain("…"), None);
        assert_eq!(plain("a"), None);
        assert_eq!(plain("ab"), None);
    }
}
