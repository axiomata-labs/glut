//! The harness's own settings files (`docs/plans/harness.md`, P4, P5): `~/.axh/settings.json` (the owner's),
//! `<project>/.axh/settings.json` and `<project>/.axh/settings.local.json`.
//!
//! A project may always make a run stricter: its `deny` and `ask` rules count in every folder. What would loosen it —
//! its `allow` rules — counts only in a folder the owner trusted once (`~/.axh/trusted.json`), and its mode never
//! lets through more than the owner's own (security review of HS3a, 2026-10-13).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Mode, Rule, RuleSet};
use crate::error::HarnessError;

/// The folder of the owner's settings, below the home folder.
pub const USER_DIR: &str = ".axh";
/// The folder of a project's settings, below the project.
pub const PROJECT_DIR: &str = ".axh";
const SETTINGS_FILE: &str = "settings.json";
const LOCAL_SETTINGS_FILE: &str = "settings.local.json";
const TRUST_FILE: &str = "trusted.json";
/// A settings file larger than this is no settings file.
const MAX_SETTINGS_BYTES: u64 = 256 * 1024;
/// Rules one file may hold: enough for any real list, a stop for a file that would make every call slow to judge.
const MAX_RULES: usize = 1_000;

// Unknown fields are refused: a misspelled `"denny"` would otherwise drop a deny list without a word.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    permissions: Permissions,
    #[serde(default)]
    sandbox: Option<SandboxSection>,
}

/// The `sandbox` section of one settings file (HS3b), as written: paths are still text, `~/…` or relative.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxSection {
    /// `enabled`: whether commands run in the sandbox.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// `allowWrite`: extra places a command may write.
    #[serde(default)]
    pub allow_write: Vec<String>,
    /// `denyRead`: extra places no command reads.
    #[serde(default)]
    pub deny_read: Vec<String>,
    /// `allowLocalBinding`: commands may listen on and connect to anything on localhost (a dev server, a test's own
    /// socket) — which also opens every local service to them.
    #[serde(default)]
    pub allow_local_binding: Option<bool>,
}

/// What the settings say about the sandbox. Writable places and switching it off are the owner's alone (their own
/// settings file); a project may only add secrets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxSettings {
    /// Commands run in the sandbox (where the system has one).
    pub enabled: bool,
    /// Extra places a command may write, resolved.
    pub allow_write: Vec<PathBuf>,
    /// Extra places no command reads, resolved.
    pub deny_read: Vec<PathBuf>,
    /// Localhost is open to commands beyond the proxy (see [`SandboxSection::allow_local_binding`]).
    pub allow_local_binding: bool,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        SandboxSettings {
            enabled: true,
            allow_write: Vec::new(),
            deny_read: Vec::new(),
            allow_local_binding: false,
        }
    }
}

/// A path from a settings file: `~/…` below the home folder, a relative one below `base`.
fn settings_path(text: &str, home: &Path, base: &Path) -> PathBuf {
    match text.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if text == "~" => home.to_path_buf(),
        None => base.join(text),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Permissions {
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    ask: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    default_mode: Option<String>,
}

/// What one settings file says.
#[derive(Debug, Default)]
pub struct Settings {
    pub rules: RuleSet,
    pub mode: Option<Mode>,
    /// The `sandbox` section, as written.
    pub sandbox: SandboxSection,
}

/// Reads one settings file; a missing file says nothing.
///
/// # Errors
///
/// [`HarnessError::Settings`] for a file that cannot be read, is too large, is no JSON of the expected shape, or holds
/// a rule or mode that does not parse — a broken deny list must stop the run, not be skipped (P4).
pub fn read(path: &Path) -> Result<Settings, HarnessError> {
    let fail = |reason: String| HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    };
    use std::io::Read as _;
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(err) => return Err(fail(err.to_string())),
    };
    // Opened once and checked on the open file: a FIFO or a file swapped in between is not read.
    if !file
        .metadata()
        .map_err(|err| fail(err.to_string()))?
        .is_file()
    {
        return Err(fail("not a regular file".into()));
    }
    let mut text = String::new();
    file.take(MAX_SETTINGS_BYTES + 1)
        .read_to_string(&mut text)
        .map_err(|err| fail(err.to_string()))?;
    if text.len() as u64 > MAX_SETTINGS_BYTES {
        return Err(fail(format!("larger than {MAX_SETTINGS_BYTES} bytes")));
    }
    let file: File = serde_json::from_str(&text).map_err(|err| fail(err.to_string()))?;
    let count =
        file.permissions.allow.len() + file.permissions.ask.len() + file.permissions.deny.len();
    if count > MAX_RULES {
        return Err(fail(format!("{count} rules, more than {MAX_RULES}")));
    }
    let parse = |list: Vec<String>| -> Result<Vec<Rule>, HarnessError> {
        list.iter()
            .map(|text| Rule::parse(text).map_err(fail))
            .collect()
    };
    let mode = match file.permissions.default_mode {
        None => None,
        Some(text) => {
            Some(Mode::parse(&text).ok_or_else(|| fail(format!("unknown defaultMode {text:?}")))?)
        }
    };
    Ok(Settings {
        rules: RuleSet {
            allow: parse(file.permissions.allow)?,
            ask: parse(file.permissions.ask)?,
            deny: parse(file.permissions.deny)?,
        },
        mode,
        sandbox: file.sandbox.unwrap_or_default(),
    })
}

/// Everything the settings files of a run say.
#[derive(Debug, Default)]
pub struct Loaded {
    pub rules: RuleSet,
    /// The mode the files name: the owner's, or a project's where it is no looser.
    pub mode: Option<Mode>,
    /// The project has allow rules or a mode of its own that were left out because the folder is not trusted.
    pub untrusted_project: bool,
    pub sandbox: SandboxSettings,
}

/// The project's settings files.
pub fn project_files(project: &Path) -> [PathBuf; 2] {
    let dir = project.join(PROJECT_DIR);
    [dir.join(SETTINGS_FILE), dir.join(LOCAL_SETTINGS_FILE)]
}

/// Whether the project has settings files of its own.
pub fn project_has_settings(project: &Path) -> bool {
    project_files(project).iter().any(|file| file.is_file())
}

/// What the project's files would add if the folder were trusted: their allow rules and mode, for the trust prompt.
///
/// # Errors
///
/// Whatever [`read`] refuses.
pub fn project_summary(project: &Path) -> Result<(Vec<String>, Option<Mode>), HarnessError> {
    let mut allow = Vec::new();
    let mut mode = None;
    for file in project_files(project) {
        let settings = read(&file)?;
        allow.extend(
            settings
                .rules
                .allow
                .iter()
                .map(|rule| rule.text().to_owned()),
        );
        mode = settings.mode.or(mode);
    }
    Ok((allow, mode))
}

/// Reads the owner's settings below `home` and the project's: its deny and ask rules always, its allow rules and mode
/// only if `trusted`, and its mode only where it is no looser than the owner's (or the default).
///
/// # Errors
///
/// Whatever [`read`] refuses.
pub fn load(home: &Path, project: &Path, trusted: bool) -> Result<Loaded, HarnessError> {
    let user = read(&home.join(USER_DIR).join(SETTINGS_FILE))?;
    let ceiling = user.mode.unwrap_or_default().looseness();
    let owner = &user.sandbox;
    let mut sandbox = SandboxSettings {
        enabled: owner.enabled.unwrap_or(true),
        allow_write: owner
            .allow_write
            .iter()
            .map(|path| settings_path(path, home, home))
            .collect(),
        deny_read: owner
            .deny_read
            .iter()
            .map(|path| settings_path(path, home, home))
            .collect(),
        allow_local_binding: owner.allow_local_binding.unwrap_or(false),
    };
    let mut loaded = Loaded {
        mode: user.mode,
        rules: user.rules,
        untrusted_project: false,
        sandbox: SandboxSettings::default(),
    };
    for file in project_files(project) {
        let settings = read(&file)?;
        // A project may keep more secret; it may not open places to write, localhost, or switch the sandbox off.
        sandbox.deny_read.extend(
            settings
                .sandbox
                .deny_read
                .iter()
                .map(|path| settings_path(path, home, project)),
        );
        loaded.rules.deny.extend(settings.rules.deny);
        loaded.rules.ask.extend(settings.rules.ask);
        if !trusted {
            loaded.untrusted_project |= !settings.rules.allow.is_empty() || settings.mode.is_some();
            continue;
        }
        loaded.rules.allow.extend(settings.rules.allow);
        if let Some(mode) = settings.mode.filter(|mode| mode.looseness() <= ceiling) {
            loaded.mode = Some(mode);
        }
    }
    loaded.sandbox = sandbox;
    Ok(loaded)
}

/// The folders the owner trusted (P5), kept in `~/.axh/trusted.json`.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Trust {
    #[serde(default)]
    folders: Vec<PathBuf>,
}

impl Trust {
    fn path(home: &Path) -> PathBuf {
        home.join(USER_DIR).join(TRUST_FILE)
    }

    /// The trusted folders; none when the file is missing.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] for a file that exists but cannot be read: adding to it would drop the folders it
    /// already holds.
    pub fn load(home: &Path) -> Result<Self, HarnessError> {
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|err| fail(err.to_string())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Trust::default()),
            Err(err) => Err(fail(err.to_string())),
        }
    }

    /// Whether `folder` (canonical) is trusted.
    pub fn contains(&self, folder: &Path) -> bool {
        self.folders.iter().any(|known| known == folder)
    }

    /// Trusts `folder` (canonical) from now on.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the file cannot be written.
    pub fn add(&mut self, home: &Path, folder: &Path) -> Result<(), HarnessError> {
        use std::os::unix::fs::DirBuilderExt;
        if self.contains(folder) {
            return Ok(());
        }
        self.folders.push(folder.to_path_buf());
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        if let Some(dir) = path.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)
                .map_err(|err| fail(err.to_string()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|err| fail(err.to_string()))?;
        // Written beside and renamed over: a crash halfway leaves the old list, never half a list. Readable by the
        // owner only.
        let temp = path.with_extension("json.tmp");
        {
            use std::io::Write as _;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temp)
                .map_err(|err| fail(err.to_string()))?;
            file.write_all(text.as_bytes())
                .map_err(|err| fail(err.to_string()))?;
        }
        std::fs::rename(&temp, &path).map_err(|err| fail(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-settings-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_project_may_tighten_always_but_loosen_only_when_trusted_and_never_past_the_owner() {
        let (home, project) = (temp("home"), temp("project"));
        write(
            &home.join(".axh/settings.json"),
            r#"{"permissions": {"allow": ["Bash(git status)"], "defaultMode": "default"}}"#,
        );
        write(
            &project.join(".axh/settings.json"),
            r#"{"permissions": {"allow": ["Bash(make:*)"], "deny": ["Bash(rm:*)"], "defaultMode": "plan"}}"#,
        );
        // Looser than the owner's own mode: ignored even in a trusted folder.
        write(
            &project.join(".axh/settings.local.json"),
            r#"{"permissions": {"defaultMode": "auto"}}"#,
        );

        let trusted = load(&home, &project, true).unwrap();
        assert_eq!(trusted.rules.allow.len(), 2);
        assert_eq!(trusted.rules.deny.len(), 1);
        assert_eq!(
            trusted.mode,
            Some(Mode::Plan),
            "the stricter project mode, not the looser local one"
        );
        assert!(!trusted.untrusted_project);

        let untrusted = load(&home, &project, false).unwrap();
        assert_eq!(
            untrusted.rules.deny.len(),
            1,
            "a project's deny counts in any folder"
        );
        assert_eq!(untrusted.rules.allow.len(), 1, "only the owner's allow");
        assert_eq!(untrusted.mode, Some(Mode::Default));
        assert!(untrusted.untrusted_project);
        let (allow, mode) = project_summary(&project).unwrap();
        assert_eq!(
            (allow, mode),
            (vec!["Bash(make:*)".to_owned()], Some(Mode::Auto))
        );
    }

    #[test]
    fn a_misspelled_field_and_too_many_rules_stop_the_run() {
        let dir = temp("strict");
        write(
            &dir.join("s.json"),
            r#"{"permissions": {"denny": ["Bash(rm:*)"]}}"#,
        );
        assert!(read(&dir.join("s.json")).is_err());
        let many: Vec<String> = (0..=MAX_RULES)
            .map(|i| format!("\"Bash(cmd{i})\""))
            .collect();
        write(
            &dir.join("s.json"),
            &format!(r#"{{"permissions": {{"allow": [{}]}}}}"#, many.join(",")),
        );
        assert!(read(&dir.join("s.json")).is_err());
    }

    #[test]
    fn a_broken_trust_file_is_not_overwritten() {
        let home = temp("trust-broken");
        write(&home.join(".axh/trusted.json"), "{ not json");
        assert!(Trust::load(&home).is_err());
    }

    #[test]
    fn a_broken_file_stops_the_run_and_a_missing_one_says_nothing() {
        let dir = temp("broken");
        assert!(read(&dir.join("none.json")).unwrap().rules.allow.is_empty());
        for text in [
            "{",
            r#"{"permissions": {"deny": ["Bash(rm"]}}"#,
            r#"{"permissions": {"defaultMode": "bypass"}}"#,
        ] {
            write(&dir.join("s.json"), text);
            assert!(
                matches!(
                    read(&dir.join("s.json")),
                    Err(HarnessError::Settings { .. })
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn trust_is_kept_per_folder() {
        let (home, project) = (temp("trust-home"), temp("trust-project"));
        assert!(!Trust::load(&home).unwrap().contains(&project));
        let mut trust = Trust::load(&home).unwrap();
        trust.add(&home, &project).unwrap();
        assert!(Trust::load(&home).unwrap().contains(&project));
        assert!(!Trust::load(&home).unwrap().contains(&home));
    }

    #[test]
    fn the_sandbox_section_is_the_owners_and_a_project_may_only_keep_more_secret() {
        let (home, project) = (temp("sbx-home"), temp("sbx-project"));
        write(
            &home.join(".axh/settings.json"),
            r#"{"sandbox": {"enabled": true, "allowWrite": ["~/.gradle"], "denyRead": ["~/notes/private"]}}"#,
        );
        write(
            &project.join(".axh/settings.json"),
            r#"{"sandbox": {"enabled": false, "allowWrite": ["/"], "denyRead": ["fixtures/keys"], "allowLocalBinding": true}}"#,
        );
        let loaded = load(&home, &project, true).unwrap().sandbox;
        assert!(loaded.enabled, "a project cannot switch it off");
        assert!(!loaded.allow_local_binding, "nor open localhost");
        assert_eq!(
            loaded.allow_write,
            [home.join(".gradle")],
            "nor open places to write"
        );
        assert_eq!(
            loaded.deny_read,
            [home.join("notes/private"), project.join("fixtures/keys")]
        );
        write(
            &home.join(".axh/settings.json"),
            r#"{"sandbox": {"enabled": false, "allowLocalBinding": true}}"#,
        );
        let owner = load(&home, &project, false).unwrap().sandbox;
        assert!(!owner.enabled && owner.allow_local_binding, "the owner can");
    }
}
