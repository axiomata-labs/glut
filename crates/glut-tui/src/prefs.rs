//! What the owner sets in Glut itself (T29): `~/.axh/prefs.json`, written by Glut's commands (`/editor`) and readable
//! by hand. Never the owner's hand-written `settings.json`, which a command does not rewrite.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file below the home folder's `.axh`.
const FILE: &str = "prefs.json";
/// A prefs file larger than this is none.
const MAX_BYTES: u64 = 64 * 1024;
/// The editors `/editor` offers when they are on the `PATH`, with how each opens a file at a line.
pub const EDITORS: [(&str, &str); 10] = [
    ("nvim", "nvim +{line} {file}"),
    ("vim", "vim +{line} {file}"),
    ("hx", "hx {file}:{line}"),
    ("micro", "micro {file}:{line}"),
    ("nano", "nano +{line} {file}"),
    ("emacs", "emacs +{line} {file}"),
    ("zed", "zed {file}:{line}"),
    ("code", "code -g {file}:{line}"),
    ("cursor", "cursor -g {file}:{line}"),
    ("subl", "subl {file}:{line}"),
];

/// The prefs, with whatever else a later Glut or the owner put in the file kept as it was.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Prefs {
    /// How a file opens at a line: `nvim +{line} {file}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor: Option<String>,
    #[serde(flatten)]
    other: serde_json::Map<String, serde_json::Value>,
}

impl Prefs {
    fn dir(home: &Path) -> PathBuf {
        home.join(glut::providers::DIR)
    }

    /// The prefs; a missing file has none.
    ///
    /// # Errors
    ///
    /// A message for a file that cannot be read or is no prefs file — it is never written over then.
    pub fn read(home: &Path) -> Result<Self, String> {
        let path = Self::dir(home).join(FILE);
        let file = match glut::providers::open_no_follow(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(format!("{}: {err}", path.display())),
        };
        let mut text = String::new();
        file.take(MAX_BYTES)
            .read_to_string(&mut text)
            .map_err(|err| format!("{}: {err}", path.display()))?;
        serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The prefs, or none when the file cannot be read (a preference never stops a session).
    pub fn load(home: &Path) -> Self {
        Self::read(home).unwrap_or_default()
    }

    /// Writes the file whole, through a new file renamed into place.
    ///
    /// # Errors
    ///
    /// A message when it cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), String> {
        let dir = Self::dir(home);
        let path = dir.join(FILE);
        let mut text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        text.push('\n');
        glut::providers::make_private_dir(&dir)
            .and_then(|()| glut::providers::write_replacing(&dir, &path, text.as_bytes(), 0o644))
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The editor in use: the one set here, else `$VISUAL`, else `$EDITOR`, else none.
    pub fn editor(&self) -> Option<String> {
        self.editor.clone().or_else(|| {
            ["VISUAL", "EDITOR"].iter().find_map(|name| {
                std::env::var(name)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
            })
        })
    }
}

/// Sets `command` as the editor and keeps it: a command without `{file}` gets the file at its end.
///
/// # Errors
///
/// A message when the file cannot be read (it is not written over) or written, or the command is empty.
pub fn set_editor(home: &Path, command: &str) -> Result<String, String> {
    let command = command.trim();
    if command.is_empty() {
        return Err("/editor needs a command".into());
    }
    let command = if command.contains("{file}") {
        command.to_owned()
    } else {
        format!("{command} {{file}}")
    };
    let mut prefs = Prefs::read(home)?;
    prefs.editor = Some(command.clone());
    prefs.save(home)?;
    Ok(command)
}

/// The editors of [`EDITORS`] that are on the `PATH`.
pub fn installed_editors() -> Vec<(&'static str, &'static str)> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
    EDITORS
        .into_iter()
        .filter(|(program, _)| folders.iter().any(|folder| folder.join(program).is_file()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(tag: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "glut-prefs-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|at| at.as_nanos())
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[test]
    fn the_editor_is_kept_with_what_else_the_file_holds_and_a_broken_file_is_never_written_over() {
        let home = home("keep");
        assert_eq!(
            set_editor(&home, "nvim").unwrap(),
            "nvim {file}",
            "the file goes at the end"
        );
        assert_eq!(Prefs::load(&home).editor.as_deref(), Some("nvim {file}"));
        let path = home.join(".axh").join(FILE);
        std::fs::write(&path, r#"{"editor": "vi {file}", "theme": "forest"}"#).unwrap();
        set_editor(&home, "zed {file}:{line}").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("forest") && text.contains("zed {file}:{line}"),
            "{text}"
        );
        std::fs::write(&path, "{ broken").unwrap();
        assert!(set_editor(&home, "hx").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{ broken");
        assert!(set_editor(&home, "  ").is_err());
    }
}
