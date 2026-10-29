//! How long Glut keeps what it stores in `~/.glut` (T82, `glut clean`): the owner's terms in `~/.glut/clean.json`,
//! written only by `glut clean keep` and `/clean keep` — never `settings.json`, which a command never writes — and
//! what one pass over a kind of data found ([`Tally`]). The library sweeps what it alone writes (snapshots,
//! journals, `lean.jsonl`); the program `glut` adds the sessions and what hangs off them.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::error::HarnessError;

/// The file's name in `~/.glut`.
pub const FILE: &str = "clean.json";
/// The largest file read: a handful of numbers.
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// The longest term the owner can set: ten years, past which "keep" means "forever" anyway.
pub const MOST_DAYS: u32 = 3650;
const DAY: u64 = 24 * 60 * 60;

/// The owner's terms, in days, each kind of data on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Keep {
    /// A run's snapshots (G11); a store whose every run is older goes whole, its folder there or not.
    pub snapshots: u32,
    /// An unfinished run's journal (D8).
    pub journals: u32,
    /// A kept session, since it was last written.
    pub sessions: u32,
    /// The long results of a session that is gone, or of none.
    pub results: u32,
    /// The lines of `lean.jsonl`.
    pub lean: u32,
}

impl Default for Keep {
    fn default() -> Self {
        Self {
            snapshots: 7,
            journals: 7,
            sessions: 90,
            results: 30,
            lean: 90,
        }
    }
}

/// The kinds of data the owner sets a term for, by the name `glut clean keep` takes.
pub const KINDS: [&str; 5] = ["snapshots", "journals", "sessions", "results", "lean"];

impl Keep {
    /// The file under `home` (`~`).
    pub fn path(home: &Path) -> PathBuf {
        Self::path_in(&home.join(crate::providers::DIR))
    }

    fn path_in(glut_dir: &Path) -> PathBuf {
        glut_dir.join(FILE)
    }

    /// What the owner set; the defaults when the file is missing.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the file cannot be read, is too large, does not parse or holds a term outside
    /// one day to [`MOST_DAYS`].
    pub fn load(home: &Path) -> Result<Self, HarnessError> {
        Self::load_in(&home.join(crate::providers::DIR))
    }

    /// [`Keep::load`] from the `.glut` folder itself (the snapshot store's parent).
    ///
    /// # Errors
    ///
    /// As [`Keep::load`].
    pub fn load_in(glut_dir: &Path) -> Result<Self, HarnessError> {
        use std::io::Read as _;
        let path = Self::path_in(glut_dir);
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
        let keep: Self = serde_json::from_str(&text).map_err(|err| fail(err.to_string()))?;
        for kind in KINDS {
            let days = keep.days(kind).unwrap_or_default();
            if !(1..=MOST_DAYS).contains(&days) {
                return Err(fail(format!("{kind}: {days} days, not 1 to {MOST_DAYS}")));
            }
        }
        Ok(keep)
    }

    /// Writes the file under `home`, only the owner able to read it.
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

    /// The term of `kind`, one of [`KINDS`].
    pub fn days(&self, kind: &str) -> Option<u32> {
        match kind {
            "snapshots" => Some(self.snapshots),
            "journals" => Some(self.journals),
            "sessions" => Some(self.sessions),
            "results" => Some(self.results),
            "lean" => Some(self.lean),
            _ => None,
        }
    }

    /// Sets the term of `kind` from the owner's words (`kind`, `30d`).
    ///
    /// # Errors
    ///
    /// A message naming the kinds, or the form of a term, when either is not one.
    pub fn set(&mut self, kind: &str, term: &str) -> Result<(), String> {
        let days = parse_days(term)?;
        let slot = match kind {
            "snapshots" => &mut self.snapshots,
            "journals" => &mut self.journals,
            "sessions" => &mut self.sessions,
            "results" => &mut self.results,
            "lean" => &mut self.lean,
            _ => return Err(format!("{kind:?}: one of {}", KINDS.join(", "))),
        };
        *slot = days;
        Ok(())
    }
}

/// A term as the owner writes it: a number of days and `d` (`30d`), one day to [`MOST_DAYS`].
///
/// # Errors
///
/// A message saying the form when `term` is not one.
pub fn parse_days(term: &str) -> Result<u32, String> {
    term.strip_suffix('d')
        .and_then(|days| days.parse::<u32>().ok())
        .filter(|days| (1..=MOST_DAYS).contains(days))
        .ok_or_else(|| format!("{term:?}: days as 30d, 1d to {MOST_DAYS}d"))
}

/// A term in days as a span of time.
pub fn span(days: u32) -> Duration {
    Duration::from_secs(u64::from(days) * DAY)
}

/// A term in days as the owner writes it.
pub fn term(days: u32) -> String {
    format!("{days}d")
}

/// What one pass over a kind of data found: how much there is, and how much went — or would go, in a dry run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tally {
    /// The kind, a fixed word (`snapshots`, `sessions`, …): the caller names it in its own language.
    pub name: String,
    pub bytes: u64,
    /// What went, or would go. For snapshots a lower bound in a dry run: what a run's ref alone holds is only known
    /// once git has collected it.
    pub freeable_bytes: u64,
    pub count: u64,
    pub freeable_count: u64,
    /// The term, as the owner writes it; absent for what is never removed by age.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep: Option<String>,
}

impl Tally {
    /// An empty tally of `name`, kept for `days` (none: never removed by age).
    pub fn new(name: &str, days: Option<u32>) -> Self {
        Self {
            name: name.to_owned(),
            keep: days.map(term),
            ..Self::default()
        }
    }
}

/// The bytes a file or a folder takes — the path itself followed if it is a link (`~/.glut` linked elsewhere), no link
/// inside it; what cannot be read counts nothing.
pub fn size_of(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_dir() => entries_size(path),
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

fn entries_size(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.metadata() {
            // `DirEntry::metadata` does not follow a link.
            Ok(meta) if meta.is_dir() => entries_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

/// How many entries a folder holds; none when it cannot be read.
pub fn entries_in(path: &Path) -> u64 {
    std::fs::read_dir(path)
        .map(|entries| entries.flatten().count() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_term_is_days_with_a_d_within_ten_years() {
        assert_eq!(parse_days("30d"), Ok(30));
        assert_eq!(parse_days("1d"), Ok(1));
        assert!(parse_days("0d").is_err());
        assert!(parse_days("30").is_err());
        assert!(parse_days("2w").is_err());
        assert!(parse_days("3651d").is_err());
        assert_eq!(term(7), "7d");
    }

    #[test]
    fn the_terms_are_kept_in_their_own_file_and_a_bad_one_refuses_the_file() {
        let home = std::env::temp_dir().join(format!("glut-clean-keep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(Keep::load(&home).unwrap(), Keep::default());
        let mut keep = Keep::default();
        keep.set("sessions", "30d").unwrap();
        assert!(keep.set("images", "30d").is_err());
        keep.save(&home).unwrap();
        assert_eq!(Keep::load(&home).unwrap().sessions, 30);
        assert_eq!(
            Keep::load_in(&home.join(".glut")).unwrap().snapshots,
            Keep::default().snapshots
        );
        std::fs::write(Keep::path(&home), r#"{"sessions": 0}"#).unwrap();
        assert!(Keep::load(&home).is_err());
        std::fs::write(Keep::path(&home), r#"{"sessions": 12}"#).unwrap();
        assert_eq!(
            Keep::load(&home).unwrap().snapshots,
            7,
            "an absent term is the default"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
