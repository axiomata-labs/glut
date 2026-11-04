//! The owner's choices about the sandbox (T59, 2026-11-01): `~/.glut/sandbox.json`, written only by `/sandbox` and
//! `glut sandbox` — never `settings.json`, a project's `.glut/` or a model — and read again before every command.
//!
//! One choice so far: every git folder read-only to a command. Then no command commits, and none makes a git
//! folder's runnable part (`commondir`, `config.worktree`) either — the gap Linux's sandbox leaves while a command
//! runs, closed for every repository the search finds.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::HarnessError;

/// The file's name in `~/.glut`.
pub const FILE: &str = "sandbox.json";
/// The most the file may hold: a switch or two.
const MAX_FILE_BYTES: u64 = 4 * 1024;

/// What the owner chose.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Choices {
    /// Every git folder read-only to a command: no commit from the sandbox.
    pub git_read_only: bool,
    /// Without a sandbox here, no command runs at all (T94): rather none than one only the rules hold.
    pub required: bool,
}

impl Choices {
    /// The file under `home` (`~`).
    pub fn path(home: &Path) -> PathBuf {
        home.join(crate::providers::DIR).join(FILE)
    }

    /// The owner's choices; the defaults when the file is missing.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the file cannot be read, is too large or does not parse.
    pub fn load(home: &Path) -> Result<Self, HarnessError> {
        use std::io::Read as _;
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(fail(err.to_string())),
        };
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| fail(err.to_string()))?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(fail(format!("larger than {MAX_FILE_BYTES} bytes")));
        }
        serde_json::from_str(&text).map_err(|err| fail(err.to_string()))
    }

    /// The choices a command runs under: a file that cannot be read keeps both switches on — one the owner turned on
    /// is not lost to a broken file. A switch set then starts from these, and the file is whole again.
    pub fn in_force(home: &Path) -> Self {
        let here = Self::load(home).unwrap_or(Self::STRICT);
        // Where Glut keeps its own folder elsewhere (`GLUT_HOME`), `/sandbox` writes its switches there while a
        // command's policy is built from the user's home: each switch holds as the stricter of the two files.
        match crate::permissions::settings::home_dir() {
            Ok(glut) if glut.as_path() != home => {
                here.stricter(Self::load(&glut).unwrap_or(Self::STRICT))
            }
            _ => here,
        }
    }

    /// What a file that cannot be read counts as: every switch on.
    const STRICT: Choices = Choices {
        git_read_only: true,
        required: true,
    };

    /// Each switch on where either is.
    fn stricter(self, other: Choices) -> Choices {
        Choices {
            git_read_only: self.git_read_only || other.git_read_only,
            required: self.required || other.required,
        }
    }

    /// Writes the choices under `home`, only the owner able to read them.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the folder or the file cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), HarnessError> {
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        let dir = path.parent().ok_or_else(|| fail("no folder".into()))?;
        crate::providers::make_private_dir(dir).map_err(|err| fail(err.to_string()))?;
        let text = serde_json::to_string_pretty(self).map_err(|err| fail(err.to_string()))?;
        crate::providers::write_replacing(dir, &path, text.as_bytes(), 0o600)
            .map_err(|err| fail(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_switch_holds_as_the_stricter_of_two_homes() {
        let off = Choices {
            git_read_only: false,
            required: false,
        };
        let git = Choices {
            git_read_only: true,
            required: false,
        };
        let required = Choices {
            git_read_only: false,
            required: true,
        };
        assert_eq!(off.stricter(off), off);
        assert_eq!(git.stricter(required), Choices::STRICT);
        assert_eq!(off.stricter(git), git);
    }

    #[test]
    fn the_choice_is_kept_and_a_broken_file_keeps_git_read_only() {
        let home =
            std::env::temp_dir().join(format!("glut-sandbox-choices-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(Choices::load(&home).unwrap(), Choices::default());
        Choices {
            git_read_only: true,
            required: false,
        }
        .save(&home)
        .unwrap();
        assert!(Choices::load(&home).unwrap().git_read_only);
        std::fs::write(Choices::path(&home), "{ not json").unwrap();
        assert!(Choices::load(&home).is_err());
        assert!(Choices::in_force(&home).git_read_only);
        std::fs::remove_dir_all(&home).unwrap();
    }
}
