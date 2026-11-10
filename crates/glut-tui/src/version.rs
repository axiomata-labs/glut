//! Which Glut the owner runs, which is newest, and what was shown of it (#158, #159).
//!
//! `~/.glut/version.json` keeps three values: the version whose news the owner saw, the newest one Glut heard of and
//! when it last asked. Glut asks `glut-ai.com/latest.txt` — a file the site's deploy writes only once GitHub holds
//! that release whole — at most once a day, in the background, and only where the owner's terminal UI runs:
//! never in a plan, a loop's round, `glut run`, `glut acp` or the arena, and never where the owner said no
//! (`updateCheck: false`, `/update off`). A failure says nothing. Glut only tells; it never updates itself.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The version of this build.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// Where the newest version is read: a file of one line, the site's own.
const LATEST_URL: &str = "https://glut-ai.com/latest.txt";
/// The file the three values are kept in, in `~/.glut`.
pub const FILE: &str = "version.json";
/// How often Glut asks at most.
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// How long one question may take: a slow answer is no answer.
const TIMEOUT: Duration = Duration::from_secs(5);
/// The most bytes of the answer read: a version is short.
const MOST_BYTES: usize = 64;

/// What `~/.glut/version.json` holds.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Kept {
    /// The version whose news the owner saw (#158); `None` before this file existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seen: Option<String>,
    /// The newest version Glut heard of (#159).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// When Glut last asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<DateTime<Utc>>,
}

impl Kept {
    fn path(home: &Path) -> PathBuf {
        home.join(".glut").join(FILE)
    }

    /// The file's values; none where it is missing or unreadable.
    pub fn load(home: &Path) -> Kept {
        std::fs::read_to_string(Self::path(home))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the file whole, through a new file renamed into place.
    ///
    /// # Errors
    ///
    /// A message when it cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), String> {
        let path = Self::path(home);
        let dir = home.join(".glut");
        let mut text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        text.push('\n');
        glut::providers::make_private_dir(&dir)
            .and_then(|()| glut::providers::write_replacing(&dir, &path, text.as_bytes(), 0o644))
            .map_err(|err| format!("{}: {err}", path.display()))
    }
}

/// A release version as numbers (`0.1.2`); `None` for anything else — a pre-release (`0.2.0-rc1`) is never offered.
fn numbers(version: &str) -> Option<Vec<u64>> {
    let parts: Option<Vec<u64>> = version
        .trim()
        .split('.')
        .map(|part| part.parse().ok())
        .collect();
    parts.filter(|parts| parts.len() == 3)
}

/// Whether `version` is a release later than `than`.
pub fn newer(version: &str, than: &str) -> bool {
    match (numbers(version), numbers(than)) {
        (Some(version), Some(than)) => version > than,
        _ => false,
    }
}

/// Whether this process is Glut on its own (`glut`), not a host embedding it: only then it looks for a newer Glut.
static ON_ITS_OWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Says this process is Glut on its own: [`auto`] may look.
pub fn on_its_own() {
    ON_ITS_OWN.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The newer version this window heard of, for the footer.
static AVAILABLE: Mutex<Option<String>> = Mutex::new(None);

/// The newer version there is, if Glut heard of one.
pub fn available() -> Option<String> {
    AVAILABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

fn make_available(latest: Option<&str>) {
    let newer = latest
        .filter(|latest| newer(latest, CURRENT))
        .map(str::to_owned);
    *AVAILABLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = newer;
}

/// Forgets the newer version heard of: the owner turned the look off.
pub fn forget_available() {
    make_available(None);
}

/// This build's highlights as `/news` shows them (#158): the title, a line each; a note where there are none.
pub fn news_text() -> String {
    match crate::news::NEWS
        .iter()
        .find(|entry| entry.version == CURRENT)
    {
        Some(entry) => {
            let mut text = tr!(news_title, version = CURRENT);
            for line in crate::news::lines(entry, glut::lang::current()) {
                text.push_str("\n• ");
                text.push_str(line);
            }
            text
        }
        None => tr!(news_none).to_owned(),
    }
}

/// Whether the owner lets Glut ask: `/update` kept, else the settings' `updateCheck`.
pub fn allowed(home: &Path, setting: bool) -> bool {
    crate::prefs::Prefs::load(home)
        .update_check
        .unwrap_or(setting)
}

/// At a window's start (the owner's terminal UI only): what is known shows at once, and once a day Glut asks
/// again in the background. Nothing where the owner said no.
pub fn auto(home: &Path, setting: bool) {
    if !ON_ITS_OWN.load(std::sync::atomic::Ordering::Relaxed) || !allowed(home, setting) {
        return;
    }
    let mut kept = Kept::load(home);
    make_available(kept.latest.as_deref());
    // A time ahead of the clock (it was wrong once) counts as long ago.
    let due = kept.checked.is_none_or(|checked| {
        (Utc::now() - checked)
            .to_std()
            .map_or(true, |since| since >= EVERY)
    });
    if !due {
        return;
    }
    // The attempt is kept before it is made: offline, or with several windows starting at once, Glut still asks
    // once a day at most.
    kept.checked = Some(Utc::now());
    if kept.save(home).is_err() {
        return;
    }
    let home = home.to_owned();
    tokio::spawn(async move {
        let Some(latest) = fetch().await else {
            return;
        };
        make_available(Some(&latest));
        // Read again: another window may have kept something meanwhile.
        let mut kept = Kept::load(&home);
        kept.latest = Some(latest);
        let _ = kept.save(&home);
    });
}

/// The newest version the site names; `None` on any failure.
async fn fetch() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(concat!("glut/", env!("CARGO_PKG_VERSION")))
        .build()
        .ok()?;
    let mut response = client
        .get(LATEST_URL)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MOST_BYTES {
            return None;
        }
    }
    let text = String::from_utf8(body).ok()?;
    let version = text.trim();
    numbers(version).map(|_| version.to_owned())
}

/// How this Glut was installed, as far as its own path tells: the command that updates it, or every one when the
/// path says nothing.
pub fn update_commands(exe: Option<&Path>) -> Vec<&'static str> {
    const BREW: &str = "brew upgrade glut";
    const CARGO: &str = "cargo install glut-tui";
    const SCRIPT: &str = "curl -fsSL https://glut-ai.com/install.sh | sh";
    let Some(exe) = exe else {
        return vec![BREW, CARGO, SCRIPT];
    };
    let path = exe.to_string_lossy();
    if path.contains("/Cellar/") || path.contains("/homebrew/") || path.contains("/linuxbrew/") {
        vec![BREW]
    } else if path.contains("/.cargo/bin/") {
        vec![CARGO]
    } else if path.contains("/.local/bin/") || path.contains("/usr/local/bin/") {
        vec![SCRIPT]
    } else {
        vec![BREW, CARGO, SCRIPT]
    }
}

/// The running program's real path: a link (`/usr/local/bin/glut` → the Cellar) followed.
pub fn running_exe() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .map(|exe| exe.canonicalize().unwrap_or(exe))
}

/// The news the owner has not seen (#158): every version's highlights after the one seen, up to this build's,
/// newest first. A first start sees none (the welcome is there), a start from before this file counts as having
/// seen 0.1.0.
pub fn unseen(seen: Option<&str>, first_start: bool) -> Vec<&'static crate::news::Entry> {
    if first_start {
        return Vec::new();
    }
    let seen = seen.unwrap_or("0.1.0");
    let mut news: Vec<_> = crate::news::NEWS
        .iter()
        .filter(|entry| newer(entry.version, seen) && !newer(entry.version, CURRENT))
        .collect();
    news.sort_by_key(|entry| std::cmp::Reverse(numbers(entry.version)));
    news
}

/// Keeps that the owner saw this build's news.
///
/// # Errors
///
/// A message when the file cannot be written.
pub fn mark_seen(home: &Path) -> Result<(), String> {
    let mut kept = Kept::load(home);
    if kept.seen.as_deref() == Some(CURRENT) {
        return Ok(());
    }
    kept.seen = Some(CURRENT.to_owned());
    kept.save(home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_later_release_is_newer_and_a_pre_release_never() {
        assert!(newer("0.1.2", "0.1.1"));
        assert!(newer("0.10.0", "0.9.9"), "numbers, not words");
        assert!(!newer("0.1.1", "0.1.1"));
        assert!(!newer("0.1.0", "0.1.1"));
        assert!(!newer("0.2.0-rc1", "0.1.1"));
        assert!(!newer("garbage", "0.1.1"));
        assert!(!newer("1.2", "0.1.1"));
    }

    #[test]
    fn the_path_tells_the_update_command_or_every_one_is_shown() {
        let of = |path: &str| update_commands(Some(Path::new(path)));
        assert_eq!(
            of("/opt/homebrew/Cellar/glut/0.1.0/bin/glut"),
            ["brew upgrade glut"]
        );
        assert_eq!(of("/Users/o/.cargo/bin/glut"), ["cargo install glut-tui"]);
        assert_eq!(
            of("/Users/o/.local/bin/glut")[0],
            "curl -fsSL https://glut-ai.com/install.sh | sh"
        );
        assert_eq!(of("/srv/tools/glut").len(), 3);
        assert_eq!(update_commands(None).len(), 3);
    }

    #[test]
    fn the_kept_values_read_back_and_a_broken_file_counts_as_none() {
        let home = std::env::temp_dir().join(format!("glut-version-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(Kept::load(&home), Kept::default());
        mark_seen(&home).unwrap();
        assert_eq!(Kept::load(&home).seen.as_deref(), Some(CURRENT));
        std::fs::write(home.join(".glut").join(FILE), "{ broken").unwrap();
        assert_eq!(Kept::load(&home), Kept::default());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn news_start_after_the_version_seen_and_never_at_the_first_start() {
        assert!(unseen(None, true).is_empty(), "the welcome is there");
        assert!(unseen(Some(CURRENT), false).is_empty(), "seen already");
        let from_before = unseen(None, false);
        assert!(
            from_before
                .iter()
                .all(|entry| newer(entry.version, "0.1.0") && !newer(entry.version, CURRENT)),
            "{}",
            from_before.len()
        );
    }
}
