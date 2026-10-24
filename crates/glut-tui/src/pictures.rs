//! The owner's images (`docs/plans/glut-tui.md`, T45): taken from the clipboard (`⌃V` — the terminal pastes text
//! alone), taken from a pasted or dropped path, and opened for a look. What a model is sent of them is `glut::images`'
//! part.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc::UnboundedSender;

/// The image in the clipboard, as the system gives it: macOS through `osascript` (its PNG), Linux through
/// `wl-paste` under Wayland, else `xclip`.
///
/// # Errors
///
/// A message when the clipboard holds no image, or the program that reads it is missing.
pub fn from_clipboard() -> Result<Vec<u8>, String> {
    if cfg!(target_os = "macos") {
        let out = Command::new("osascript")
            .args(["-e", "the clipboard as «class PNGf»"])
            .stdin(Stdio::null())
            .output()
            .map_err(|err| format!("osascript: {err}"))?;
        if !out.status.success() {
            return Err("no image in the clipboard".into());
        }
        return hex_data(&String::from_utf8_lossy(&out.stdout))
            .ok_or_else(|| "no image in the clipboard".into());
    }
    let (program, args): (&str, &[&str]) = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ("wl-paste", &["--type", "image/png"])
    } else {
        (
            "xclip",
            &["-selection", "clipboard", "-t", "image/png", "-o"],
        )
    };
    let out = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|_| format!("{program} reads the clipboard's images — it is not installed"))?;
    if !out.status.success() || out.stdout.is_empty() {
        return Err("no image in the clipboard".into());
    }
    Ok(out.stdout)
}

/// The bytes of AppleScript's `«data PNGf89504E47…»`.
fn hex_data(said: &str) -> Option<Vec<u8>> {
    let hex = said.trim().strip_prefix("«data PNGf")?.strip_suffix('»')?;
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok())
        .collect()
}

/// The image files a paste names, when it names nothing else: a terminal's drop of files is their paths, spaces
/// escaped or quoted. `None`: the paste is text.
pub fn pasted_paths(text: &str, home: Option<&Path>) -> Option<Vec<PathBuf>> {
    let words = shell_words(text.trim())?;
    if words.is_empty() {
        return None;
    }
    words
        .into_iter()
        .map(|word| {
            let path = match (word.strip_prefix("~/"), home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ => PathBuf::from(word),
            };
            let image = path.is_absolute()
                && path.is_file()
                && path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        matches!(
                            extension.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "gif" | "webp"
                        )
                    });
            image.then_some(path)
        })
        .collect()
}

/// `text` split as a shell would split a line of paths: spaces part them, a backslash escapes the next character,
/// quotes hold spaces. `None` for a line that is no such list (an open quote, a line break).
fn shell_words(text: &str) -> Option<Vec<String>> {
    if text.contains('\n') {
        return None;
    }
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\\') => word.push(chars.next()?),
            (None, '\'' | '"') => quote = Some(c),
            (None, ' ') => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, c) => word.push(c),
        }
    }
    if quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

/// Opens a kept image for a look (Preview on macOS).
///
/// # Errors
///
/// A message when the file is gone or the opener cannot start.
pub fn open(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err(format!("{} is gone", path.display()));
    }
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = Command::new(opener)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|err| format!("{opener}: {err}"))?;
    // Waited for beside the screen: no process left behind, no frame held up.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// How images are drawn (T46, `/images`, kept in `prefs.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Drawing {
    /// The real image, where the terminal speaks a graphics protocol; half blocks elsewhere.
    Inline,
    /// True-colour half blocks, everywhere.
    Blocks,
    /// The image's line alone.
    Off,
}

impl Drawing {
    pub fn parse(word: &str) -> Option<Drawing> {
        match word {
            "inline" => Some(Drawing::Inline),
            "blocks" => Some(Drawing::Blocks),
            "off" => Some(Drawing::Off),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Drawing::Inline => "inline",
            Drawing::Blocks => "blocks",
            Drawing::Off => "off",
        }
    }
}

/// The way chosen, as its number: 0 inline, 1 blocks, 2 off.
static DRAWING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(1);

/// Images are drawn as `choice` says, or — with none kept — inline where the terminal is known to speak a graphics
/// protocol (Ghostty, kitty, WezTerm, iTerm2), in half blocks elsewhere.
pub fn set_drawing(choice: Option<Drawing>) {
    let drawing = choice.unwrap_or_else(|| {
        let program = std::env::var("TERM_PROGRAM")
            .unwrap_or_default()
            .to_lowercase();
        let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
        let speaks = matches!(program.as_str(), "ghostty" | "wezterm" | "iterm.app")
            || term.contains("kitty")
            || term.contains("ghostty");
        // Tests draw the same in every terminal they run in.
        if speaks && !cfg!(test) {
            Drawing::Inline
        } else {
            Drawing::Blocks
        }
    });
    let number = match drawing {
        Drawing::Inline => 0,
        Drawing::Blocks => 1,
        Drawing::Off => 2,
    };
    DRAWING.store(number, std::sync::atomic::Ordering::Relaxed);
}

/// How images are drawn now: never at all in the reader mode.
pub fn drawing() -> Drawing {
    if crate::motion::reader() {
        return Drawing::Off;
    }
    match DRAWING.load(std::sync::atomic::Ordering::Relaxed) {
        0 => Drawing::Inline,
        2 => Drawing::Off,
        _ => Drawing::Blocks,
    }
}

/// The most rows a picture in the stream takes.
pub const MOST_ROWS: u16 = 10;

/// The cells a picture of `width` × `height` pixels takes within `columns` × `rows` cells, its proportions kept — a
/// cell twice as high as it is wide, so two of the picture's pixels to a cell from top to bottom.
pub fn fitted(width: u32, height: u32, columns: u16, rows: u16) -> (u16, u16) {
    let (width, height) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let columns_f = f64::from(columns.max(1));
    let rows_f = f64::from(rows.max(1));
    // A cell is one pixel wide and two high: the picture's height in cells is its height over two, scaled.
    let scale = (columns_f / width).min(rows_f * 2.0 / height);
    let fit_columns = (width * scale).round().clamp(1.0, columns_f) as u16;
    let fit_rows = (height * scale / 2.0).round().clamp(1.0, rows_f) as u16;
    (fit_columns, fit_rows)
}

/// `image` in `columns` × `rows` cells as drawing goes now: blank cells marked for the terminal to draw it in where it
/// draws images itself (T46b), half blocks elsewhere; `None` where it cannot be read.
pub fn lines(
    image: &glut::images::Image,
    columns: u16,
    rows: u16,
) -> Option<Vec<ratatui::text::Line<'static>>> {
    // An image that cannot be read is no picture: its line stands alone.
    if !image.path.is_file() {
        return None;
    }
    if drawing() == Drawing::Inline
        && crate::graphics::protocol().is_some()
        && let Some(mark) = crate::graphics::mark(image, columns, rows)
    {
        let blank = " ".repeat(usize::from(columns));
        return Some(
            (0..rows)
                .map(|_| {
                    Line::from(Span::styled(
                        blank.clone(),
                        Style::default().underline_color(mark),
                    ))
                })
                .collect(),
        );
    }
    half_blocks(image, columns, rows)
}

/// Where the screen hears that a picture made beside it is ready (T47); unset — the tests, the scrollback — a picture
/// is made at once.
static READY: Mutex<Option<UnboundedSender<()>>> = Mutex::new(None);

/// While it lives, pictures are made beside the screen: decoding an image takes longer than a frame may wait. Each
/// one ready is said on the screen's channel, for it to draw anew; once it is dropped (the screen's loop ended),
/// pictures are made at once again.
pub struct Beside;

impl Beside {
    /// Pictures made beside the screen, each one ready said on `ready`.
    pub fn begin(ready: UnboundedSender<()>) -> Beside {
        *READY.lock().unwrap_or_else(PoisonError::into_inner) = Some(ready);
        Beside
    }
}

impl Drop for Beside {
    fn drop(&mut self) {
        *READY.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

/// A picture's work for the worker, and whom to tell once it is done.
type Job = (Box<dyn FnOnce() + Send>, UnboundedSender<()>);

/// The one thread pictures are made on, started with the first: one at a time, however many a resumed session holds.
static WORKER: Mutex<Option<std::sync::mpsc::Sender<Job>>> = Mutex::new(None);

/// `work` on the pictures' thread, the screen told once it is done; given back where pictures are made at once.
///
/// # Errors
///
/// `work` itself, unrun, where nothing listens for a picture made beside the screen.
pub fn beside<F: FnOnce() + Send + 'static>(work: F) -> Result<(), F> {
    beside_on(ready(), work)
}

/// Where the screen hears of a picture made beside it, if it listens.
fn ready() -> Option<UnboundedSender<()>> {
    READY.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// [`beside`], told on `ready`.
fn beside_on<F: FnOnce() + Send + 'static>(
    ready: Option<UnboundedSender<()>>,
    work: F,
) -> Result<(), F> {
    let Some(ready) = ready else {
        return Err(work);
    };
    let mut worker = WORKER.lock().unwrap_or_else(PoisonError::into_inner);
    let jobs = worker.get_or_insert_with(|| {
        let (jobs, taken) = std::sync::mpsc::channel::<Job>();
        std::thread::spawn(move || {
            for (work, ready) in taken {
                // A decoder that panics fails its picture alone, never the thread — and the screen is told anyway.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work));
                let _ = ready.send(());
            }
        });
        jobs
    });
    // The worker never ends (it catches what panics), so its channel stays open.
    let _ = jobs.send((Box::new(work), ready));
    Ok(())
}

/// A picture laid out in half blocks: its file and its size in cells.
type DrawnKey = (PathBuf, u16, u16);

/// A picture in half blocks: still made beside the screen, or made — `None` where it cannot be read.
enum Laid {
    Making,
    Made(Option<Vec<Line<'static>>>),
}

/// The pictures laid out, the newest last: the overlay draws every frame, the stream at every new width.
static DRAWN: Mutex<Vec<(DrawnKey, Laid)>> = Mutex::new(Vec::new());
/// The most pictures kept laid out (at most some 50 kB each): more than a stream shows at once, so the blocks laid out
/// anew when one is ready find theirs.
const KEPT: usize = 128;
/// The files of the pictures made beside the screen since the screen last asked.
static FRESH: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// `laid` kept for `key`, in place of what was.
fn keep_laid(key: DrawnKey, laid: Laid) {
    let mut drawn = DRAWN.lock().unwrap_or_else(PoisonError::into_inner);
    drawn.retain(|(kept, _)| *kept != key);
    // The oldest made one goes — never one still making, whose block waits for it.
    if drawn.len() >= KEPT
        && let Some(at) = drawn
            .iter()
            .position(|(_, laid)| matches!(laid, Laid::Made(_)))
    {
        drawn.remove(at);
    }
    drawn.push((key, laid));
}

/// The files of the pictures made beside the screen since the last call: their blocks are laid out anew.
pub fn fresh() -> Vec<PathBuf> {
    std::mem::take(&mut *FRESH.lock().unwrap_or_else(PoisonError::into_inner))
}

/// `image` as `columns` × `rows` cells of half blocks (`▀`, the upper pixel its colour, the lower its background);
/// `None` where it cannot be read. Made beside the screen where it listens: blank cells of the same size until then.
pub fn half_blocks(
    image: &glut::images::Image,
    columns: u16,
    rows: u16,
) -> Option<Vec<Line<'static>>> {
    half_blocks_on(image, columns, rows, ready())
}

/// [`half_blocks`], made beside the screen where `ready` listens.
fn half_blocks_on(
    image: &glut::images::Image,
    columns: u16,
    rows: u16,
    ready: Option<UnboundedSender<()>>,
) -> Option<Vec<Line<'static>>> {
    let key = (image.path.clone(), columns, rows);
    let blank = || {
        Some(vec![
            Line::from(" ".repeat(usize::from(columns)));
            usize::from(rows)
        ])
    };
    if let Some((_, laid)) = DRAWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find(|(kept, _)| *kept == key)
    {
        return match laid {
            Laid::Making => blank(),
            Laid::Made(lines) => lines.clone(),
        };
    }
    keep_laid(key.clone(), Laid::Making);
    let made = {
        let (image, key) = (image.clone(), key.clone());
        move || {
            // A decoder that panics leaves a picture that cannot be read, never one making for good.
            let lines = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                laid_out(&image, columns, rows)
            }))
            .unwrap_or(None);
            keep_laid(key, Laid::Made(lines));
        }
    };
    let path = image.path.clone();
    let told = move || {
        made();
        FRESH
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(path);
    };
    match beside_on(ready, told) {
        Ok(()) => blank(),
        Err(_) => {
            keep_laid(key.clone(), Laid::Made(laid_out(image, columns, rows)));
            laid_out_kept(&key)
        }
    }
}

/// What is kept laid out for `key`.
fn laid_out_kept(key: &DrawnKey) -> Option<Vec<Line<'static>>> {
    DRAWN
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find_map(|(kept, laid)| match laid {
            Laid::Made(lines) if kept == key => lines.clone(),
            _ => None,
        })
}

/// `image` decoded and laid out in half blocks.
fn laid_out(image: &glut::images::Image, columns: u16, rows: u16) -> Option<Vec<Line<'static>>> {
    let pixels = glut::images::thumbnail(image, u32::from(columns), u32::from(rows) * 2).ok()?;
    Some(
        pixels
            .chunks(2)
            .map(|pair| {
                let upper = &pair[0];
                let lower = pair.get(1).unwrap_or(upper);
                Line::from(
                    upper
                        .iter()
                        .zip(lower)
                        .map(|(up, down)| {
                            Span::styled(
                                "▀",
                                Style::default()
                                    .fg(Color::Rgb(up[0], up[1], up[2]))
                                    .bg(Color::Rgb(down[0], down[1], down[2])),
                            )
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_picture_is_made_beside_the_screen_blank_until_it_is_ready() {
        let home = std::env::temp_dir().join(format!("glut-beside-{}", std::process::id()));
        let image = glut::images::keep(&home, &crate::app::tests::TINY_PNG, 1).unwrap();
        let (ready, mut told) = tokio::sync::mpsc::unbounded_channel();
        let blank = half_blocks_on(&image, 2, 1, Some(ready.clone())).unwrap();
        assert_eq!(blank.len(), 1);
        assert_eq!(blank[0].to_string(), "  ");
        told.recv().await.unwrap();
        assert!(fresh().contains(&image.path));
        // Made once: no second thread, the half blocks at once.
        let made = half_blocks_on(&image, 2, 1, Some(ready)).unwrap();
        assert_eq!(made[0].to_string(), "▀▀");
        assert!(told.try_recv().is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn applescripts_data_is_read_as_bytes() {
        assert_eq!(
            hex_data("«data PNGf89504E47»\n"),
            Some(vec![0x89, 0x50, 0x4e, 0x47])
        );
        assert_eq!(hex_data("plain text"), None);
        assert_eq!(hex_data("«data PNGf895»"), None);
    }

    #[test]
    fn a_paste_of_image_paths_is_images_and_anything_else_text() {
        let dir = std::env::temp_dir().join(format!("glut-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let one = dir.join("a shot.png");
        let two = dir.join("b.JPG");
        std::fs::write(&one, b"x").unwrap();
        std::fs::write(&two, b"x").unwrap();
        let escaped = format!(
            "{} {}",
            one.display().to_string().replace(' ', "\\ "),
            two.display()
        );
        assert_eq!(
            pasted_paths(&escaped, None),
            Some(vec![one.clone(), two.clone()])
        );
        assert_eq!(
            pasted_paths(&format!("'{}'", one.display()), None),
            Some(vec![one.clone()])
        );
        assert_eq!(
            pasted_paths(&format!("look at {}", two.display()), None),
            None
        );
        assert_eq!(
            pasted_paths(&dir.join("gone.png").display().to_string(), None),
            None
        );
        assert_eq!(pasted_paths("relative.png", None), None);
        assert_eq!(fitted(1600, 800, 40, 10), (40, 10));
        assert_eq!(
            fitted(800, 1600, 40, 10),
            (10, 10),
            "a tall one by its height"
        );
        assert_eq!(fitted(100, 50, 40, 10), (40, 10));
        assert_eq!(fitted(2000, 100, 40, 10), (40, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
