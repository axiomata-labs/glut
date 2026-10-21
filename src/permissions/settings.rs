//! The harness's own settings files (`docs/plans/harness.md`, P4, P5): `~/.axh/settings.json` (the owner's),
//! `<project>/.axh/settings.json` and `<project>/.axh/settings.local.json`.
//!
//! A project may always make a run stricter: its `deny` and `ask` rules count in every folder. What would loosen it —
//! its `allow` rules — counts only in a folder the owner trusted once (`~/.axh/trusted.json`), and its mode never
//! lets through more than the owner's own (security review of HS3a, 2026-10-13).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Mode, Rule, RuleSet};
use crate::config::{Compaction, Effort, MIN_CONTEXT_WINDOW, RefusalLimits};
use crate::error::HarnessError;
use crate::hooks::{Hooks, HooksSection};

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
/// Overrides the harness's home folder — the `AXIOMATA_HOME` pattern of the repo (AGENTS.md), so a test or an
/// embedder runs against its own `~/.axh` and never reads the owner's settings or starts the owner's MCP servers.
const HOME_ENV: &str = "AXH_HOME";

/// The harness's home folder: `$AXH_HOME` when set, else `$HOME`.
///
/// # Errors
///
/// [`HarnessError::Settings`] when neither variable holds an absolute path — everything below this folder
/// (`settings.json`, `trusted.json`, `mcp.json`, `skills/`) would otherwise silently resolve somewhere else.
pub fn home_dir() -> Result<PathBuf, HarnessError> {
    home_from_env(std::env::var_os(HOME_ENV), std::env::var_os("HOME"))
}

/// [`home_dir`] with the two environment values handed in, so the resolution is testable without touching the
/// environment. An empty variable (`AXH_HOME=`) counts as unset — a script that exports an empty override must
/// not point every run at `""` below the drive root.
fn home_from_env(
    axh_home: Option<OsString>,
    home: Option<OsString>,
) -> Result<PathBuf, HarnessError> {
    let folder = |value: Option<OsString>| -> Option<PathBuf> {
        let path = PathBuf::from(value?);
        (!path.as_os_str().is_empty()).then_some(path)
    };
    let (name, found) = match folder(axh_home) {
        Some(path) => (HOME_ENV, Some(path)),
        None => ("HOME", folder(home)),
    };
    match found {
        Some(path) if path.is_absolute() => Ok(path),
        _ => Err(HarnessError::Settings {
            path: name.to_owned(),
            reason: "is not set to an absolute folder".to_owned(),
        }),
    }
}

// Unknown fields are refused: a misspelled `"denny"` would otherwise drop a deny list without a word.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    permissions: Permissions,
    #[serde(default)]
    sandbox: Option<SandboxSection>,
    /// Hooks in Claude's shape (HS7): a project's are parsed here too — a settings file holding hooks must
    /// not fail the load — but only the owner's ever run (B1, `load`).
    #[serde(default)]
    hooks: HooksSection,
    /// How hard the model reasons (Glut effort, E1); the owner's file only — a project's costs no more than the
    /// owner chose.
    #[serde(default)]
    effort: Option<String>,
    /// When refusals end a run (Glut robust, #112); the owner's file only — a project could otherwise lift them.
    #[serde(default)]
    refusals: Option<RefusalsSection>,
    /// Whether a run compacts itself near the limit (#111, K6): `{"auto": false}` turns it off. The owner's only.
    #[serde(default)]
    compact: Option<CompactSection>,
    /// The models' context windows in tokens (#111, K2), for a provider whose model list says none:
    /// `{"gemma4:12b": 32768}`. The owner's only.
    #[serde(default, rename = "contextWindow")]
    context_window: HashMap<String, u32>,
}

/// The `compact` section (#111, K6).
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactSection {
    #[serde(default)]
    pub auto: Option<bool>,
}

/// The `refusals` section (#112, R6): `{"repeat": 3, "total": 15}`, either may be left out.
#[derive(Debug, Default, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefusalsSection {
    #[serde(default)]
    pub repeat: Option<u32>,
    #[serde(default)]
    pub total: Option<u32>,
}

impl RefusalsSection {
    /// The limits this section sets over the defaults.
    ///
    /// # Errors
    ///
    /// A message when `repeat` is below 2 (the warning would never come) or `total` below `repeat`.
    pub fn limits(self) -> Result<RefusalLimits, String> {
        let limits = RefusalLimits {
            repeat: self.repeat.unwrap_or(RefusalLimits::DEFAULT.repeat),
            total: self.total.unwrap_or(RefusalLimits::DEFAULT.total),
        };
        if limits.repeat < 2 {
            return Err(format!("refusals.repeat is {}, at least 2", limits.repeat));
        }
        if limits.total < limits.repeat {
            return Err(format!(
                "refusals.total ({}) is below refusals.repeat ({})",
                limits.total, limits.repeat
            ));
        }
        Ok(limits)
    }
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
    /// The compiled hooks (HS7); empty when the file has none.
    pub hooks: Hooks,
    /// The `effort` as written: [`load`] reads the owner's and leaves a project's unread, so a typo there stops nothing.
    pub effort: Option<String>,
    /// The `refusals` section as written; like `effort`, only the owner's is read.
    pub refusals: Option<RefusalsSection>,
    /// The `compact` section as written; only the owner's is read.
    pub compact: Option<CompactSection>,
    /// `contextWindow` as written; only the owner's is read.
    pub context_window: HashMap<String, u32>,
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
    let hooks = Hooks::compile(&file.hooks).map_err(|reason| fail(format!("hooks: {reason}")))?;
    Ok(Settings {
        rules: RuleSet {
            allow: parse(file.permissions.allow)?,
            ask: parse(file.permissions.ask)?,
            deny: parse(file.permissions.deny)?,
        },
        mode,
        sandbox: file.sandbox.unwrap_or_default(),
        hooks,
        effort: file.effort,
        refusals: file.refusals,
        compact: file.compact,
        context_window: file.context_window,
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
    /// The owner's hooks (HS7, B1) — the only ones that ever run.
    pub hooks: Hooks,
    /// The project has hooks of its own; they were parsed but not taken over (B1).
    pub project_hooks_skipped: bool,
    /// The owner's `effort`; a project's is left out unread — it would spend the owner's money.
    pub effort: Option<Effort>,
    /// The owner's refusal limits, else the defaults (#112).
    pub refusals: RefusalLimits,
    /// Whether runs compact themselves (#111, K6): the owner's `compact.auto`, else on.
    pub compact_auto: bool,
    /// The owner's context windows by model id (#111, K2).
    pub context_windows: HashMap<String, u32>,
}

impl Loaded {
    /// The run's compaction for `model`: the owner's switch and the window the settings name for it.
    pub fn compaction(&self, model: &str) -> Compaction {
        Compaction {
            auto: self.compact_auto,
            context_window: self.context_windows.get(model).copied(),
        }
    }
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
    let owners = home.join(USER_DIR).join(SETTINGS_FILE);
    let user = read(&owners)?;
    let effort = user
        .effort
        .as_deref()
        .map(Effort::parse_or_explain)
        .transpose()
        .map_err(|reason| HarnessError::Settings {
            path: owners.display().to_string(),
            reason,
        })?;
    let refusals = user
        .refusals
        .unwrap_or_default()
        .limits()
        .map_err(|reason| HarnessError::Settings {
            path: owners.display().to_string(),
            reason,
        })?;
    if let Some((model, tokens)) = user
        .context_window
        .iter()
        .find(|(_, tokens)| **tokens < MIN_CONTEXT_WINDOW)
    {
        return Err(HarnessError::Settings {
            path: owners.display().to_string(),
            reason: format!("contextWindow of {model} is {tokens}, at least {MIN_CONTEXT_WINDOW}"),
        });
    }
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
        hooks: user.hooks,
        project_hooks_skipped: false,
        effort,
        refusals,
        compact_auto: user
            .compact
            .and_then(|section| section.auto)
            .unwrap_or(true),
        context_windows: user.context_window,
    };
    for file in project_files(project) {
        let settings = read(&file)?;
        // HS7, B1: a project's hooks are parsed (above) but never run — nothing a stored file says may start
        // a process. The caller is told and puts a note where a person will see it.
        loaded.project_hooks_skipped |= !settings.hooks.is_empty();
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
    fn axh_home_overrides_home_and_both_must_be_absolute() {
        let own = home_from_env(Some("/scratch/axh".into()), Some("/Users/owner".into())).unwrap();
        assert_eq!(own, PathBuf::from("/scratch/axh"));
        // An empty override (an unset-looking variable) counts as absent, not as "" below the drive root.
        let fallback = home_from_env(Some(OsString::new()), Some("/Users/owner".into())).unwrap();
        assert_eq!(fallback, PathBuf::from("/Users/owner"));
        assert!(home_from_env(None, Some("/Users/owner".into())).is_ok());
        for (axh, home) in [
            (Some("relative/axh".into()), Some("/Users/owner".into())),
            (None, Some("relative".into())),
            (None, None),
        ] {
            assert!(
                home_from_env(axh, home).is_err(),
                "a relative or missing folder is an error"
            );
        }
        // A valid AXH_HOME never consults HOME, whatever HOME says.
        assert!(home_from_env(Some("/scratch/axh".into()), Some("relative".into())).is_ok());
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

    #[test]
    fn the_owners_hooks_are_loaded_and_a_projects_are_parsed_but_left_out() {
        let (home, project) = (temp("hooks-home"), temp("hooks-project"));
        write(
            &home.join(".axh/settings.json"),
            r#"{"hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "true"}]}]}}"#,
        );
        write(
            &project.join(".axh/settings.json"),
            r#"{"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "echo project"}]}]}}"#,
        );
        let loaded = load(&home, &project, false).unwrap();
        assert!(!loaded.hooks.is_empty(), "the owner's hooks run (B1)");
        assert!(
            loaded.project_hooks_skipped,
            "and the project's are said to be left out"
        );
        // Even where the project is trusted its hooks stay out — B1 has no trust step.
        assert!(load(&home, &project, true).unwrap().project_hooks_skipped);

        // A project whose hooks are broken still stops the run: parsed, not skipped (D1).
        write(
            &project.join(".axh/settings.json"),
            r#"{"hooks": {"PreToolUse": [{"hooks": [{"type": "prompt", "command": "true"}]}]}}"#,
        );
        assert!(matches!(
            load(&home, &project, false),
            Err(HarnessError::Settings { .. })
        ));
    }

    #[test]
    fn the_owners_refusal_limits_count_a_projects_are_left_out_and_odd_ones_stop_the_run() {
        let (home, project) = (temp("refusals-home"), temp("refusals-project"));
        assert_eq!(
            load(&home, &project, true).unwrap().refusals,
            RefusalLimits::DEFAULT
        );
        write(
            &home.join(".axh/settings.json"),
            r#"{"refusals": {"total": 30}}"#,
        );
        write(
            &project.join(".axh/settings.json"),
            r#"{"refusals": {"repeat": 99, "total": 999}}"#,
        );
        assert_eq!(
            load(&home, &project, true).unwrap().refusals,
            RefusalLimits {
                repeat: 3,
                total: 30
            },
            "a project could otherwise lift the limits"
        );
        for odd in [
            r#"{"refusals": {"repeat": 1}}"#,
            r#"{"refusals": {"repeat": 5, "total": 4}}"#,
        ] {
            write(&home.join(".axh/settings.json"), odd);
            let err = load(&home, &project, true).unwrap_err().to_string();
            assert!(err.contains("refusals."), "{err}");
        }
        write(
            &home.join(".axh/settings.json"),
            r#"{"refusals": {"repat": 3}}"#,
        );
        assert!(load(&home, &project, true).is_err(), "a misspelled field");
    }

    #[test]
    fn the_owners_compaction_counts_a_projects_is_left_out_and_a_tiny_window_stops_the_run() {
        let (home, project) = (temp("compact-home"), temp("compact-project"));
        let loaded = load(&home, &project, true).unwrap();
        assert_eq!(loaded.compaction("m"), Compaction::default());
        write(
            &home.join(".axh/settings.json"),
            r#"{"compact": {"auto": false}, "contextWindow": {"gemma4:12b": 32768}}"#,
        );
        write(
            &project.join(".axh/settings.json"),
            r#"{"compact": {"auto": true}, "contextWindow": {"other": 999999}}"#,
        );
        let loaded = load(&home, &project, true).unwrap();
        assert_eq!(
            loaded.compaction("gemma4:12b"),
            Compaction {
                auto: false,
                context_window: Some(32768)
            }
        );
        assert_eq!(
            loaded.compaction("other").context_window,
            None,
            "a project's is left out"
        );
        write(
            &home.join(".axh/settings.json"),
            r#"{"contextWindow": {"m": 100}}"#,
        );
        let err = load(&home, &project, true).unwrap_err().to_string();
        assert!(err.contains("contextWindow of m is 100"), "{err}");
    }

    #[test]
    fn the_owners_effort_counts_a_projects_is_left_out_and_an_unknown_one_stops_the_run() {
        let (home, project) = (temp("effort-home"), temp("effort-project"));
        write(&home.join(".axh/settings.json"), r#"{"effort": "high"}"#);
        write(&project.join(".axh/settings.json"), r#"{"effort": "max"}"#);
        assert_eq!(
            load(&home, &project, true).unwrap().effort,
            Some(Effort::High)
        );
        write(&home.join(".axh/settings.json"), "{}");
        assert_eq!(
            load(&home, &project, true).unwrap().effort,
            None,
            "a project's effort would spend the owner's money"
        );
        // A project's level is never used, so not even a typo in it stops a run; the owner's does.
        write(
            &project.join(".axh/settings.json"),
            r#"{"effort": "extreme"}"#,
        );
        assert_eq!(load(&home, &project, true).unwrap().effort, None);
        write(&home.join(".axh/settings.json"), r#"{"effort": "extreme"}"#);
        let err = load(&home, &project, false).unwrap_err().to_string();
        assert!(err.contains("unknown effort \"extreme\""), "{err}");
    }

    #[test]
    fn a_settings_file_with_hooks_is_no_misspelled_field() {
        // The `hooks` key parses (it used to be a deny_unknown_fields error before HS7) and an unknown event
        // name in it is ignored rather than fatal (D1).
        let dir = temp("hooks-parse");
        write(
            &dir.join("s.json"),
            r#"{"hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "true"}]}],
                          "SomethingNew": [{"hooks": [{"type": "command", "command": "true"}]}]}}"#,
        );
        let settings = read(&dir.join("s.json")).unwrap();
        assert!(!settings.hooks.is_empty());
    }
}
