//! The owner's images (`docs/plans/glut-tui.md`, T45): taken from the clipboard (`⌃V` — the terminal pastes text
//! alone), taken from a pasted or dropped path, and opened for a look. What a model is sent of them is `glut::images`'
//! part.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

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

/// The number of the first image a message of the owner's names (`[image #2 · …]`, `[image #2]`).
pub fn first_number(text: &str) -> Option<u32> {
    let at = text.find("[image #")? + "[image #".len();
    let digits: String = text[at..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(first_number("see [image #12 · 4×3 · 1 kB]"), Some(12));
        assert_eq!(first_number("no image"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
