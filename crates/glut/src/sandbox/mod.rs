//! The sandbox the shell's commands run in (`docs/plans/harness.md`, HS3b): macOS's Seatbelt (`sandbox-exec`), the
//! same mechanism Claude Code and Codex use; on Linux bubblewrap plus seccomp (T59, [`bwrap`], [`linux`]) — the same
//! rules, held by mounts and namespaces. Unlike the rules of HS3a, which judge what a command *says*, the
//! operating system holds whatever the command *does* — a script, `find -delete`, a daemon that left its process
//! group.
//!
//! What a sandboxed command may do:
//! - **write** only in the working folder, the temporary folders, the package caches and the repository's git folder
//!   — never in the protected folders, never git's `config`, `hooks` or `info` (code the owner's git would run);
//! - **read** everything except the owner's secrets (keys and credentials, Axiomata's and the harness's own
//!   configuration, browser and mail data) and `.env` files in the working folder;
//! - **signal** only its own processes, never the owner's others;
//! - **reach the network** only on this machine: localhost, where the run's own proxy decides which domains a
//!   command may reach ([`proxy`]).
//!
//! The profile is built per run from real (resolved) paths: Seatbelt matches what a path resolves to, so `/tmp` must
//! be written `/private/tmp`.

pub mod bwrap;
pub mod choices;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod proxy;
pub mod scan;
pub mod seccomp;
#[cfg(target_os = "linux")]
pub mod watcher;

use std::path::{Path, PathBuf};

/// Where `sandbox-exec` lives on macOS.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Folders below the home folder a command may write: package caches a build fills.
const HOME_WRITABLE: [&str; 6] = [
    ".cargo/registry",
    ".cargo/git",
    ".npm",
    ".cache",
    "Library/Caches",
    ".rustup/tmp",
];

/// Files below the home folder a command may write: cargo's lock on its package cache.
const HOME_WRITABLE_FILES: [&str; 1] = [".cargo/.package-cache"];

/// Folders and files below the home folder no command reads: keys and credentials, Axiomata's and the harness's own
/// configuration (`~/.axiomata/config.toml` holds API keys), other agents' credentials, browser and mail data.
const HOME_SECRETS: [&str; 65] = [
    ".ssh",
    ".git-credentials",
    ".config/git/credentials",
    ".npmrc",
    ".pypirc",
    ".cargo/credentials",
    ".cargo/credentials.toml",
    ".config/gcloud",
    ".azure",
    ".terraform.d",
    ".zsh_history",
    ".bash_history",
    ".local/share/fish/fish_history",
    "Library/Application Support/1Password",
    "Library/Group Containers/2BUA8C4S2C.com.1password",
    "Library/Application Support/Slack",
    "Library/Application Support/discord",
    ".aws",
    ".gnupg",
    ".config/gh",
    ".netrc",
    ".docker/config.json",
    ".kube",
    "Library/Keychains",
    ".axiomata",
    ".glut",
    ".local/share/opencode",
    ".config/opencode",
    ".claude",
    "Library/Application Support/Google",
    "Library/Application Support/Firefox",
    "Library/Application Support/BraveSoftware",
    "Library/Application Support/Microsoft Edge",
    "Library/Safari",
    "Library/Cookies",
    "Library/Mail",
    "Library/Messages",
    "Library/Application Support/AddressBook",
    // Linux's (T59): the desktop's keyrings and password store, browsers and chat as they keep them there.
    ".local/share/keyrings",
    ".password-store",
    ".config/google-chrome",
    ".config/chromium",
    ".config/BraveSoftware",
    ".config/microsoft-edge",
    ".mozilla",
    ".thunderbird",
    ".config/Slack",
    ".config/discord",
    ".config/1Password",
    ".config/Signal",
    // Tokens other tools keep (security audit of T59).
    ".config/rclone",
    ".vault-token",
    ".config/op",
    ".oci",
    ".m2/settings.xml",
    ".gradle/gradle.properties",
    ".config/glab-cli",
    ".config/doctl",
    ".huggingface/token",
    ".cache/huggingface/token",
    ".Xauthority",
    ".local/share/docker",
    ".python_history",
    ".psql_history",
    ".mysql_history",
];

/// Names in the working folder no command writes (the same configuration the file tools protect; the git folder is
/// handled on its own: commits may write it). Public for what else must not bring them in: a pane's worktree merged
/// back (T36a) would put them in the owner's folder.
pub const PROTECTED_IN_ROOT: [&str; 11] = [
    ".glut",
    ".axiomata",
    ".claude",
    ".opencode",
    ".vscode",
    ".cargo",
    ".husky",
    ".envrc",
    ".mcp.json",
    "opencode.json",
    ".hg",
];

/// Where a package manager unpacks packages, and the one protected name a package may ship there (an editor's
/// `.vscode` a package forgot to leave out): written there as anything else. Only that one — a folder of any name can
/// be made and then moved into place whole, which no path rule sees (security audit 2026-11-04), so nothing a build
/// or a later session obeys is let through, whatever folder it lies in.
pub(crate) const PACKAGES: &str = "node_modules";
pub(crate) const PACKAGED: &str = ".vscode";

/// The parts of a git folder that make the owner's git run code: never written by a command. `commondir` and
/// `gitdir` point git at another git folder — with a config of the command's making (security audit, 2026-10-24).
pub(crate) const GIT_RUNS: [&str; 6] = [
    "config",
    "config.worktree",
    "hooks",
    "info",
    "commondir",
    "gitdir",
];

/// [`GIT_RUNS`] as a regex alternation, for git folders found anywhere (submodules, worktrees, a nested repository).
const GIT_RUNS_PATTERN: &str = "(config|config\\.worktree|hooks|info|commondir|gitdir)(/.*)?";

/// The git folders below a git folder (`/modules/a`, `/modules/a/modules/b`, `/worktrees/x`) and the folders holding
/// them, as a regex suffix: none of them may be created, moved or removed — moving a prepared folder into place would
/// bring its own `config` and `hooks` (Seatbelt judges a rename by its target, not by what the folder holds).
const GIT_FOLDERS_PATTERN: &str = "(/modules/[^/]+)*(/modules|/worktrees(/[^/]+)?)?";

/// Programs that drive other applications through Apple Events or Launch Services — `tell app "Terminal" to do
/// script` would run anything outside the sandbox. Seatbelt's `appleevent-send`, `lsopen` and `mach-lookup` rules do
/// not stop them (tried on macOS 2026-10-13); refusing to start them does, also through a link, and a copy of such a
/// system program is killed by macOS itself.
const APPLE_EVENT_TOOLS: [&str; 7] = [
    "/usr/bin/osascript",
    "/usr/bin/osacompile",
    "/usr/bin/open",
    "/usr/bin/automator",
    "/usr/bin/shortcuts",
    // A job handed to launchd runs outside the sandbox, at a time of its own (T60b's plans made it matter).
    "/bin/launchctl",
    // The Keychain's own tool reaches Glut's stored keys and sign-ins past the read-deny list (PV4's audit).
    "/usr/bin/security",
];

/// An arena model's confinement (T61): once set, its commands write their own folder alone — no shared temporary
/// folder or package cache, no git folder but the one below it — with their temporary files and caches in a private
/// home that goes with the folder, and read nothing of the places it hides (the owner's repository). Shared by a
/// toolbox and its shell, set by the screen once, never taken back.
#[derive(Debug, Clone, Default)]
pub struct Confinement(std::sync::Arc<std::sync::Mutex<Option<Confined>>>);

/// What a [`Confinement`] holds: the private home as resolved when it was set — never again, so a link the model puts
/// in its place later widens nothing — and the places its commands may not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confined {
    pub home: PathBuf,
    pub hidden: Vec<PathBuf>,
}

impl Confined {
    /// Whether the home is still the folder it was set to: a real folder at the same place, no link in or at it.
    /// A command runs only while it is.
    pub fn intact(&self) -> bool {
        std::fs::symlink_metadata(&self.home).is_ok_and(|meta| meta.is_dir())
            && self.home.canonicalize().is_ok_and(|real| real == self.home)
    }
}

/// The variables that send a command's temporary files and the package managers' caches below a confined home.
const CONFINED_PLACES: [(&str, &str); 17] = [
    ("TMPDIR", "tmp"),
    ("TMP", "tmp"),
    ("TEMP", "tmp"),
    ("XDG_CACHE_HOME", "cache"),
    ("CARGO_HOME", "cargo"),
    ("npm_config_cache", "npm"),
    ("NPM_CONFIG_CACHE", "npm"),
    ("YARN_CACHE_FOLDER", "yarn"),
    ("PNPM_HOME", "pnpm"),
    ("COREPACK_HOME", "corepack"),
    ("PIP_CACHE_DIR", "pip"),
    ("UV_CACHE_DIR", "uv"),
    ("POETRY_CACHE_DIR", "poetry"),
    ("GOCACHE", "go-build"),
    ("GOMODCACHE", "go-mod"),
    ("GOPATH", "go"),
    ("SCCACHE_DIR", "sccache"),
];

/// Go writes its module cache read-only unless told otherwise: then the clone could not be removed with it.
const CONFINED_FLAGS: [(&str, &str); 1] = [("GOFLAGS", "-modcacherw")];

impl Confinement {
    /// Confines every later command to its folder, with `home` as its private home (made here, resolved once) and
    /// `hidden` kept from its reads.
    pub fn confine(&self, home: PathBuf, hidden: Vec<PathBuf>) {
        for (_, part) in CONFINED_PLACES {
            let _ = std::fs::create_dir_all(home.join(part));
        }
        let home = home.canonicalize().unwrap_or(home);
        let hidden = hidden
            .into_iter()
            .map(|path| path.canonicalize().unwrap_or(path))
            .collect();
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) =
            Some(Confined { home, hidden });
    }

    /// What it holds, once confined.
    pub fn get(&self) -> Option<Confined> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }

    /// The variables a confined command gets on top of its environment; none while not confined.
    pub fn environment(&self) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        self.get().map_or_else(Vec::new, |confined| {
            CONFINED_PLACES
                .iter()
                .map(|(name, part)| (name.into(), confined.home.join(part).into_os_string()))
                .chain(
                    CONFINED_FLAGS
                        .iter()
                        .map(|(name, value)| (name.into(), value.into())),
                )
                .collect()
        })
    }
}

/// Whether commands can run in the sandbox here.
pub fn available() -> bool {
    unavailable().is_none()
}

/// Why commands cannot run in the sandbox here, and what helps; `None` where they can.
pub fn unavailable() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        (!Path::new(SANDBOX_EXEC).is_file()).then(|| format!("{SANDBOX_EXEC} is missing"))
    }
    #[cfg(target_os = "linux")]
    {
        linux::ready().err().map(str::to_owned)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Some(format!("Glut has no sandbox for {}", std::env::consts::OS))
    }
}

/// The sandbox's mechanism on this system, by its own name.
pub fn mechanism() -> &'static str {
    if cfg!(target_os = "linux") {
        "bubblewrap, seccomp"
    } else {
        "Seatbelt"
    }
}

/// Runs the Linux sandbox's helper when this process was started as one and returns its exit code; otherwise
/// remembers that this program serves it (the Linux sandbox needs that) and returns `None`. A program offering
/// Glut's sandbox calls it first thing in `main`, before any thread starts; elsewhere than on Linux it does nothing.
pub fn serve_helper() -> Option<i32> {
    #[cfg(target_os = "linux")]
    {
        linux::serve_helper()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// The places no command reads for the owner whose home is `home`, resolved: Glut's own list of secrets and the
/// owner's `extra` ones from the settings.
pub fn secrets(home: &Path, extra: &[PathBuf]) -> Vec<PathBuf> {
    let real = |path: PathBuf| path.canonicalize().unwrap_or(path);
    HOME_SECRETS
        .iter()
        .map(|rel| real(home.join(rel)))
        .chain(extra.iter().cloned().map(real))
        .collect()
}

/// What a run's sandbox allows, in resolved paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    /// The working folder.
    pub root: PathBuf,
    /// The folders beside it the owner gave the run (T60c), each written to as the working folder is — its
    /// protected names, `.git` and `.env` files kept as there.
    pub beside: Vec<PathBuf>,
    /// Folders a command may write, beside the working folder.
    pub writable: Vec<PathBuf>,
    /// Single files a command may write.
    pub writable_files: Vec<PathBuf>,
    /// Folders or files inside the writable ones a command may not write.
    pub write_denied: Vec<PathBuf>,
    /// Folders or files no command reads.
    pub read_denied: Vec<PathBuf>,
    /// The repository's git folders (a worktree's own and the common one), writable except [`GIT_RUNS`].
    pub git_dirs: Vec<PathBuf>,
    /// The port of the run's network proxy: the one place on localhost a command reaches. `None`: no network at all.
    pub proxy_port: Option<u16>,
    /// Everything on localhost is open (the owner's `allowLocalBinding`).
    pub local_binding: bool,
    /// The Keychain's Mach services are open to the command (T74): the owner's list and the gate's word for this one
    /// command, or the owner's switch for all. Shut by default.
    pub keychain: bool,
    /// Single files a command writes for Glut inside a folder it may not write (the shell's note of where a command
    /// ended, in the scratch folder that also holds the profile snapshot every later command sources): allowed after
    /// every deny, so nothing else of that folder opens with them.
    pub notes: Vec<PathBuf>,
    /// Where what a command made that the sandbox refuses goes (T59, Linux: [`scan`]), below `~/.glut`.
    pub quarantine: PathBuf,
    /// The folder holding what the Linux sandbox covers secrets with ([`bwrap::make_covers`]), below `~/.glut`: no
    /// command reaches it to change them.
    pub covers: PathBuf,
    /// Every git folder read-only to a command, the owner's choice ([`choices::Choices`]): no commit from here.
    pub git_read_only: bool,
    /// The owner's home folder, where the choices are read again ([`Policy::with_choices_now`]).
    pub home: PathBuf,
    /// A confined command's private home ([`Policy::confined`]): written in, never itself moved or replaced.
    pub confined_home: Option<PathBuf>,
}

impl Policy {
    /// The policy for a run in `root` (resolved) by the owner whose home is `home`, with the owner's extra writable
    /// folders and extra secrets from the settings.
    pub fn new(
        root: &Path,
        home: &Path,
        extra_writable: &[PathBuf],
        extra_secrets: &[PathBuf],
    ) -> Self {
        let real = |path: PathBuf| path.canonicalize().unwrap_or(path);
        let mut writable = vec![real(std::env::temp_dir()), PathBuf::from("/private/tmp")];
        // The per-user folder above `$TMPDIR` (`/private/var/folders/xx/yyy/`) also holds the user's cache (`C`).
        if let Some(user_folder) = writable[0]
            .parent()
            .filter(|p| p.starts_with("/private/var/folders"))
        {
            writable.push(user_folder.to_path_buf());
        }
        writable.extend(HOME_WRITABLE.iter().map(|rel| real(home.join(rel))));
        writable.extend(extra_writable.iter().cloned().map(real));
        Policy {
            root: root.to_path_buf(),
            beside: Vec::new(),
            writable,
            writable_files: HOME_WRITABLE_FILES
                .iter()
                .map(|rel| real(home.join(rel)))
                .collect(),
            write_denied: PROTECTED_IN_ROOT
                .iter()
                .map(|name| root.join(name))
                .collect(),
            read_denied: secrets(home, extra_secrets),
            git_dirs: git_dirs(root),
            proxy_port: None,
            local_binding: false,
            keychain: false,
            notes: Vec::new(),
            quarantine: home.join(crate::providers::DIR).join("quarantine"),
            covers: home.join(crate::providers::DIR).join("sandbox"),
            git_read_only: choices::Choices::in_force(home).git_read_only,
            home: home.to_path_buf(),
            confined_home: None,
        }
    }

    /// This policy with `folders` beside the working folder (T60c), their repositories' git folders with them.
    pub fn with_beside(&self, folders: &[crate::folders::Folder]) -> Policy {
        let mut policy = self.clone();
        for folder in folders {
            policy.beside.push(folder.real.clone());
            policy
                .write_denied
                .extend(PROTECTED_IN_ROOT.iter().map(|name| folder.real.join(name)));
            for dir in &folder.git_dirs {
                if !policy.git_dirs.contains(dir) {
                    policy.git_dirs.push(dir.clone());
                }
            }
        }
        policy
    }

    /// This policy for a confined command ([`Confinement`]): nothing written but the working folder, its own git
    /// folder and the private home — not the shared temporary folder, the package caches, the folders beside it nor
    /// the owner's extra writable places —, nothing read of what it hides, and no localhost beside the proxy. The home
    /// is taken as it was resolved when set: never resolved again, where the model may have put a link.
    pub fn confined(mut self, confined: &Confined) -> Policy {
        self.writable = vec![confined.home.clone()];
        self.read_denied.extend(confined.hidden.iter().cloned());
        self.local_binding = false;
        self.confined_home = Some(confined.home.clone());
        self.writable_files.clear();
        self.beside.clear();
        self.git_dirs.retain(|dir| dir.starts_with(&self.root));
        // The owner's `/sandbox git read-only` keeps the owner's tools from running what a command planted in a git
        // folder; a confined command's git folder is its own clone's, which only Glut's git reads (hooks off) and
        // which holds its private home — kept read-only, no temporary file could be made.
        self.git_read_only = false;
        self
    }

    /// This policy with the owner's choices as they stand now ([`choices::Choices`], read before every command): a
    /// switch turned in another window holds from the next command on.
    pub fn with_choices_now(mut self) -> Policy {
        self.git_read_only = choices::Choices::in_force(&self.home).git_read_only;
        self
    }

    /// The working folder and the folders beside it: where a command writes as in the run's own folder.
    fn roots(&self) -> impl Iterator<Item = &PathBuf> {
        std::iter::once(&self.root).chain(&self.beside)
    }

    /// The process running `program` (its arguments the caller adds) in this sandbox, starting in `cwd`. On Linux
    /// `found` is what [`scan::search`] found right before, and `relay` the socket of the bridge to the run's proxy.
    ///
    /// # Errors
    ///
    /// No sandbox here, or (Linux) the helper is a program a command may write — it runs inside the sandbox but
    /// outside its seccomp filter, so a command that changed it would reach what the filter keeps closed — or the
    /// search before the command did not see everything.
    #[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
    pub fn command(
        &self,
        program: &Path,
        cwd: &Path,
        found: &scan::Found,
        relay: Option<&Path>,
    ) -> Result<std::process::Command, String> {
        #[cfg(target_os = "macos")]
        {
            // The profile goes in as an argument: nothing on disk for a command to change between two calls.
            let mut command = std::process::Command::new(SANDBOX_EXEC);
            command.arg("-p").arg(self.profile()).arg(program);
            Ok(command)
        }
        #[cfg(target_os = "linux")]
        {
            let bwrap = linux::ready()?;
            // What the search did not reach is not covered: no command, rather than one that reads a `.env`.
            if !found.complete {
                return Err(format!(
                    "the folders hold more than {} entries: Glut cannot cover every .env file in them, so no command \
                     runs — work in a smaller folder, or take a folder beside this one away",
                    scan::MAX_ENTRIES
                ));
            }
            let helper = linux::helper_program().ok_or("no helper for the sandbox")?;
            let writable = self
                .roots()
                .chain(&self.writable)
                .chain(&self.git_dirs)
                .chain(&found.git_dirs);
            if let Some(place) = writable.into_iter().find(|place| helper.starts_with(place)) {
                return Err(format!(
                    "{} lies in {}, where commands write: install glut elsewhere and start that one",
                    helper.display(),
                    place.display()
                ));
            }
            bwrap::make_covers(self);
            let mut command = std::process::Command::new(bwrap);
            command.args(bwrap::arguments(self, found, helper, relay, cwd, program));
            Ok(command)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(unavailable().unwrap_or_default())
        }
    }

    /// The Seatbelt profile. Later rules win over earlier ones: the broad allows come first, the narrow denies last.
    pub fn profile(&self) -> String {
        let mut out = String::from("(version 1)\n(allow default)\n");
        // Writing: nothing, then the allowed places, then the protected parts of them.
        out.push_str("(deny file-write*)\n(allow file-write*\n");
        for root in self.roots() {
            out.push_str(&format!("  (subpath {})\n", quoted(root)));
        }
        for dir in self.writable.iter().chain(&self.git_dirs) {
            out.push_str(&format!("  (subpath {})\n", quoted(dir)));
        }
        for file in &self.writable_files {
            out.push_str(&format!("  (literal {})\n", quoted(file)));
        }
        out.push_str("  (regex #\"^/dev/(null|tty|zero|fd/[0-9]+|stdout|stderr)$\"))\n");
        // Inside the working folder, a protected name at any depth and in any case: a project below the top keeps its
        // own `.cargo/config.toml` or `.vscode/tasks.json` that the owner's next build or editor there obeys, and a
        // command can build such a path the gate never reads (`d=.cargo; cp x sub/$d/config.toml`, security audit
        // 2026-11-03). Except a package's `.vscode` where packages are unpacked ([`PACKAGED`]).
        let root = regex_escape(&self.root);
        let (packages, packaged) = (any_case(PACKAGES), any_case(PACKAGED));
        let names: Vec<String> = PROTECTED_IN_ROOT
            .iter()
            .map(|name| any_case(name))
            .collect();
        out.push_str(&format!(
            "(deny file-write*\n  (regex #\"^{root}/(.*/)?({})(/.*)?$\"))\n",
            names.join("|")
        ));
        out.push_str(&format!(
            "(allow file-write*\n  (regex #\"^{root}/(.*/)?{packages}/(.*/)?{packaged}(/.*)?$\"))\n"
        ));
        out.push_str("(deny file-write*\n");
        for path in &self.write_denied {
            out.push_str(&format!("  (subpath {})\n", quoted(path)));
        }
        if self.git_read_only {
            // The owner's choice: no git folder written, the repository's own nor any in the roots.
            for dir in &self.git_dirs {
                out.push_str(&format!("  (subpath {})\n", quoted(dir)));
            }
            // In any case: macOS volumes do not tell `.GIT` from `.git`.
            let git = any_case(".git");
            for root in self.roots() {
                let root = regex_escape(root);
                out.push_str(&format!("  (regex #\"^{root}/(.*/)?{git}(/.*)?$\")\n"));
            }
        }
        for dir in &self.git_dirs {
            for part in GIT_RUNS {
                out.push_str(&format!("  (subpath {})\n", quoted(&dir.join(part))));
            }
            // A submodule's or another worktree's own git folder runs code the same way; and the folders themselves
            // stay where they are.
            let dir = regex_escape(dir);
            out.push_str(&format!(
                "  (regex #\"^{dir}/(modules|worktrees)/.+/{GIT_RUNS_PATTERN}$\")\n"
            ));
            out.push_str(&format!("  (regex #\"^{dir}{GIT_FOLDERS_PATTERN}$\")\n"));
        }
        // Inside the working folder, at any depth: every `.git` — folder or file, existing or new — is neither created,
        // moved nor removed (a new one, or a `.git` file pointing at a writable folder, would bring its own config;
        // `git init` there fails, by design), nor any git folder's runnable parts; and direnv's `.envrc`.
        // In a folder beside the working one — often one holding many projects — a protected name at any depth, in
        // any case: a script can build a path the rules never read (`.vscode/tasks.json` of a project in it).
        for root in &self.beside {
            let root = regex_escape(root);
            let names: Vec<String> = PROTECTED_IN_ROOT
                .iter()
                .chain(&[".jj"])
                .map(|name| any_case(name))
                .collect();
            out.push_str(&format!(
                "  (regex #\"^{root}/(.*/)?({})(/.*)?$\")\n",
                names.join("|")
            ));
            // Its git folders' runnable parts in any case too: a new `.GIT/hooks` is git's `.git/hooks` there.
            let git = any_case(".git");
            let runs =
                any_case("config|config.worktree|hooks|info|commondir|gitdir").replace("\\|", "|");
            out.push_str(&format!(
                "  (regex #\"^{root}/(.*/)?{git}/((modules|worktrees)/.+/)?({runs})(/.*)?$\")\n"
            ));
        }
        for root in self.roots() {
            let root = regex_escape(root);
            out.push_str(&format!(
                "  (regex #\"^{root}/(.*/)?\\.git/((modules|worktrees)/.+/)?{GIT_RUNS_PATTERN}$\")\n"
            ));
            out.push_str(&format!(
                "  (regex #\"^{root}/(.*/)?\\.git{GIT_FOLDERS_PATTERN}$\")\n"
            ));
            out.push_str(&format!("  (regex #\"^{root}/(.*/)?\\.envrc$\")\n"));
        }
        out.push_str(")\n");
        // A confined home is written in, never moved, removed or replaced by a link of the command's.
        if let Some(home) = &self.confined_home {
            out.push_str(&format!(
                "(deny file-write-unlink file-write-create (literal {}))\n",
                quoted(home)
            ));
        }
        if !self.notes.is_empty() {
            out.push_str("(allow file-write*\n");
            for note in &self.notes {
                out.push_str(&format!("  (literal {})\n", quoted(note)));
            }
            out.push_str(")\n");
        }
        // Reading: everything but the secrets and the working folder's `.env` files.
        out.push_str("(deny file-read*\n");
        for path in &self.read_denied {
            out.push_str(&format!("  (subpath {})\n", quoted(path)));
        }
        out.push_str(")\n");
        // A working folder inside a hidden one — a Studio session's worktree lives in `~/.axiomata/worktrees` — is read
        // again, and only it: the rest of the hidden folder stays hidden, and a secret inside the working folder too.
        if self
            .read_denied
            .iter()
            .any(|hidden| self.root != *hidden && self.root.starts_with(hidden))
        {
            out.push_str(&format!(
                "(allow file-read*\n  (subpath {}))\n",
                quoted(&self.root)
            ));
            // The folders on the way there, as entries only (no listing, no content): a program that resolves where
            // it is — git, `pwd -P` — looks at each of them.
            let ways: Vec<&Path> = self
                .root
                .ancestors()
                .skip(1)
                .filter(|way| {
                    self.read_denied
                        .iter()
                        .any(|hidden| way.starts_with(hidden))
                })
                .collect();
            out.push_str("(allow file-read-metadata\n");
            for way in ways {
                out.push_str(&format!("  (literal {})\n", quoted(way)));
            }
            out.push_str(")\n");
            let inside: Vec<&PathBuf> = self
                .read_denied
                .iter()
                .filter(|hidden| hidden.starts_with(&self.root))
                .collect();
            if !inside.is_empty() {
                out.push_str("(deny file-read*\n");
                for path in inside {
                    out.push_str(&format!("  (subpath {})\n", quoted(path)));
                }
                out.push_str(")\n");
            }
        }
        out.push_str("(deny file-read*\n");
        for root in self.roots() {
            let root = regex_escape(root);
            out.push_str(&format!(
                "  (regex #\"^{root}/(.*/)?(\\.env(\\..*)?|[^/]*\\.env|\\.envrc(\\..*)?)$\")\n"
            ));
        }
        out.push_str(")\n");
        // Driving other applications: not from here.
        out.push_str("(deny process-exec\n");
        for tool in APPLE_EVENT_TOOLS {
            out.push_str(&format!("  (literal \"{tool}\")\n"));
        }
        out.push_str(")\n");
        // Signals only within the sandbox: a command may stop what it started, never the owner's other programs.
        // (`(target others)` does not do this — tried on macOS 2026-10-13; `same-sandbox` does.) Reading another
        // process's environment needs no rule here: macOS refuses it to non-root processes anyway, sandbox or not, and
        // setuid programs like `ps` do not run in a sandbox at all.
        out.push_str("(deny signal)\n(allow signal (target same-sandbox))\n");
        // The Keychain (T74): a stored key or sign-in is reached through securityd's Mach services, past every file
        // rule above — shut unless this command was given it.
        if !self.keychain {
            out.push_str("(deny mach-lookup\n");
            for service in crate::keychain::MACH_SERVICES {
                out.push_str(&format!("  (global-name \"{service}\")\n"));
            }
            out.push_str(")\n");
        }
        // The network: only the run's proxy — not the other services on this machine (the Opencode service, a
        // database), unless the owner opened localhost.
        out.push_str("(deny network*)\n");
        if self.local_binding {
            out.push_str("(allow network-outbound (remote ip \"localhost:*\"))\n");
            out.push_str("(allow network-bind network-inbound (local ip \"localhost:*\"))\n");
        } else if let Some(port) = self.proxy_port {
            out.push_str(&format!(
                "(allow network-outbound (remote ip \"localhost:{port}\"))\n"
            ));
        }
        out
    }
}

/// The repository's git folders for `root`: its own (`git rev-parse --absolute-git-dir`, for a worktree inside the main
/// repository's `.git/worktrees/`) and the common one (the main repository's `.git`). None outside a repository.
pub(crate) fn git_dirs(root: &Path) -> Vec<PathBuf> {
    let ask = |flag: &str| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", flag])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().to_owned())
            .filter(|text| !text.is_empty())
    };
    let mut dirs = Vec::new();
    for flag in ["--absolute-git-dir", "--git-common-dir"] {
        if let Some(text) = ask(flag) {
            let path = PathBuf::from(&text);
            let path = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            let path = path.canonicalize().unwrap_or(path);
            if !dirs.contains(&path) {
                dirs.push(path);
            }
        }
    }
    dirs
}

/// `name` as an SBPL regex matching it in any case (`.vscode` → `\.[vV][sS]…`): macOS volumes do not tell the cases
/// apart.
fn any_case(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        // APFS folds more than ASCII: the long s and the Kelvin sign open `s` and `k` (`.vſcode` is `.vscode`).
        if c.eq_ignore_ascii_case(&'s') {
            out.push_str("(s|S|\u{17F})");
        } else if c.eq_ignore_ascii_case(&'k') {
            out.push_str("(k|K|\u{212A})");
        } else if c.is_ascii_alphabetic() {
            out.push_str(&format!(
                "[{}{}]",
                c.to_ascii_lowercase(),
                c.to_ascii_uppercase()
            ));
        } else if "\\.^$|?*+()[]{}\"".contains(c) {
            out.push('\\');
            out.push(c);
        } else {
            out.push(c);
        }
    }
    out
}

/// A path as an SBPL string literal.
fn quoted(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A path as a literal inside an SBPL regex.
fn regex_escape(path: &Path) -> String {
    let mut out = String::new();
    for c in path.to_string_lossy().chars() {
        if "\\.^$|?*+()[]{}\"".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for a test, resolved (Seatbelt sees `/private/var/…`, not `/var/…`).
    fn temp(name: &str) -> PathBuf {
        // Numbered too: Linux's clock can give two tests at once the same moment, and two tests share a name.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "glut-sandbox-{name}-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// Runs `script` with `sh` in the sandbox of `policy`, in its working folder, as Glut runs a command there — on
    /// Linux with the search before it and what it made that the sandbox refuses moved away after, the note of that
    /// appended ([`scan::Watch`]); stdout and stderr together.
    fn sandboxed(policy: &Policy, script: &str) -> String {
        sandboxed_relayed(policy, script, None)
    }

    /// [`sandboxed`] with the Linux sandbox's way to the proxy at `relay`.
    fn sandboxed_relayed(policy: &Policy, script: &str, relay: Option<&Path>) -> String {
        let linux = cfg!(target_os = "linux");
        let found = if linux {
            scan::search(&policy.root, &policy.beside, &policy.git_dirs)
        } else {
            scan::Found::default()
        };
        let out = policy
            .command(Path::new("/bin/sh"), &policy.root, &found, relay)
            .unwrap()
            .args(["-c", script])
            .current_dir(&policy.root)
            .output()
            .unwrap();
        let settled = linux
            .then(|| scan::Watch::new(policy, found).settle())
            .flatten()
            .unwrap_or_default();
        format!(
            "{}{}{settled}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    }

    /// Whether `out` tells a write was refused: at once (`REFUSED`, the script's own word), or — Linux, for a path
    /// that was not there — moved away right after.
    fn refused(out: &str) -> bool {
        out.contains("REFUSED") || out.contains("refused by the sandbox")
    }

    /// T61: a confined policy writes its folder, its own git folder and the private home alone — the owner's
    /// read-only switch for git folders aside, since that home lies in the clone's own.
    #[test]
    fn a_confined_policy_keeps_its_folder_and_private_home_alone() {
        let root = PathBuf::from("/w/clone");
        let home = PathBuf::from("/h");
        let mut policy = Policy::new(&root, &home, &[PathBuf::from("/extra")], &[]);
        policy.git_dirs = vec![root.join(".git"), PathBuf::from("/owner/.git")];
        policy.beside = vec![PathBuf::from("/beside")];
        policy.git_read_only = true;
        let private = root.join(".git/glut-home");
        let owner = PathBuf::from("/owner");
        policy.local_binding = true;
        let confined = policy.confined(&Confined {
            home: private.clone(),
            hidden: vec![owner.clone()],
        });
        assert_eq!(confined.writable, std::slice::from_ref(&private));
        assert!(confined.read_denied.contains(&owner) && !confined.local_binding);
        assert!(confined.writable_files.is_empty() && confined.beside.is_empty());
        assert_eq!(confined.git_dirs, [root.join(".git")]);
        assert!(!confined.git_read_only);
        let profile = confined.profile();
        assert!(
            !profile.contains("/owner/.git") && !profile.contains("/extra"),
            "{profile}"
        );
        let environment = Confinement::default();
        assert!(environment.environment().is_empty());
    }

    #[test]
    fn the_profile_names_every_place_quoted_and_escaped() {
        let policy = Policy {
            root: PathBuf::from("/w/a \"b\""),
            writable: vec![PathBuf::from("/tmp/x")],
            ..Policy::default()
        };
        let profile = policy.profile();
        assert!(
            profile.contains("(subpath \"/w/a \\\"b\\\"\")"),
            "{profile}"
        );
        assert!(
            profile.contains(
                "(regex #\"^/w/a \\\"b\\\"/(.*/)?(\\.env(\\..*)?|[^/]*\\.env|\\.envrc(\\..*)?)$\")"
            ),
            "{profile}"
        );
        assert!(profile.contains("(allow signal (target same-sandbox))"));
        assert!(
            profile.contains("(literal \"/usr/bin/osascript\")")
                && profile.contains("(literal \"/bin/launchctl\")"),
            "{profile}"
        );
        assert!(
            !profile.contains("allow network"),
            "no proxy, no network: {profile}"
        );
        assert!(regex_escape(Path::new("/a.b+c")).ends_with("a\\.b\\+c"));
    }

    /// Security audit 2026-11-03: below the working folder's top a protected name is refused as at it — a build or an
    /// editor there obeys it —, but not inside what a package manager fills; a `.git` or an `.envrc` there still is.
    #[test]
    fn a_protected_name_is_refused_at_any_depth_but_in_a_dependency_folder() {
        if !available() {
            return;
        }
        let (home, root) = (temp("home"), temp("root"));
        std::fs::create_dir_all(root.join("crates/foo")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        let policy = Policy::new(&root, &home, &[], &[]);
        let write = |path: &str| {
            sandboxed(
                &policy,
                &format!(
                    "mkdir -p \"$(dirname {path})\" && echo x > {path} && echo wrote || echo REFUSED"
                ),
            )
        };
        for path in [
            "crates/foo/.cargo/config.toml",
            "crates/foo/.VSCODE/tasks.json",
            "crates/foo/opencode.json",
            "node_modules/pkg/.envrc",
            "node_modules/pkg/.cargo/config.toml",
            "target/p/.cargo/config.toml",
            "venv/p/.claude/settings.json",
            "crates/foo/.v\u{17F}code/tasks.json",
        ] {
            let out = write(path);
            assert!(refused(&out), "{path}: {out}");
        }
        for path in [
            "crates/foo/src.rs",
            "node_modules/pkg/.vscode/settings.json",
        ] {
            let out = write(path);
            assert!(out.contains("wrote") && !refused(&out), "{path}: {out}");
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&home);
    }

    /// T60c: a folder beside the working one is written as the working one is — its protected names, its `.git`
    /// runnable parts and its `.env` files kept as there —, and only once the run has it.
    #[test]
    fn a_folder_beside_is_written_as_the_working_one() {
        if !available() {
            return;
        }
        let (home, root) = (temp("home"), temp("root"));
        // Out of the temp folders, which every command writes anyway.
        let vault = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../../target/glut-sandbox-beside-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(vault.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(vault.join(".glut")).unwrap();
        let vault = vault.canonicalize().unwrap();
        std::fs::write(vault.join(".env"), "SECRET=1").unwrap();
        let policy = Policy::new(&root, &home, &[], &[]);
        let write = format!(
            "echo x > {}/note.md && echo wrote || echo REFUSED",
            vault.display()
        );
        assert!(
            sandboxed(&policy, &write).contains("REFUSED"),
            "not before it is given"
        );
        let policy = policy.with_beside(&[crate::folders::Folder {
            real: vault.clone(),
            shown: vault.display().to_string(),
            git_dirs: vec![vault.join(".git")],
        }]);
        assert!(sandboxed(&policy, &write).contains("wrote"));
        assert!(
            sandboxed(
                &policy,
                &format!(
                    "echo y > {}/.git/objects-probe && echo wrote",
                    vault.display()
                )
            )
            .contains("wrote")
        );
        for (script, what) in [
            (format!("echo x > {}/.glut/x", vault.display()), "its .glut"),
            (
                format!("echo x > {}/.git/hooks/pre-commit", vault.display()),
                "its git hooks",
            ),
            (format!("echo x > {}/.envrc", vault.display()), "its .envrc"),
            (format!("cat {}/.env", vault.display()), "its .env"),
            (
                format!(
                    "mkdir -p {0}/app/.vscode; echo x > {0}/app/.vscode/tasks.json",
                    vault.display()
                ),
                "a project's .vscode in it",
            ),
            (
                format!(
                    "mkdir -p {0}/app/.Cargo; echo x > {0}/app/.Cargo/config.toml",
                    vault.display()
                ),
                "a .cargo in another case",
            ),
            (
                format!("echo x > {}/app/.mcp.json", vault.display()),
                "a project's .mcp.json",
            ),
            (
                format!(
                    "mkdir -p {0}/app/.GIT/hooks; echo x > {0}/app/.GIT/hooks/pre-commit",
                    vault.display()
                ),
                "a git hook in another case",
            ),
        ] {
            let out = sandboxed(&policy, &format!("{script} || echo REFUSED"));
            assert!(refused(&out), "{what}: {out}");
        }
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn a_sandboxed_command_writes_and_reads_only_what_it_may() {
        if !available() {
            return;
        }
        let (home, root) = (temp("home"), temp("root"));
        // Outside every writable place: the temp folders are writable, the workspace's `target/` is not.
        let outside =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/glut-sandbox-outside");
        std::fs::create_dir_all(&outside).unwrap();
        let outside = outside.canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/id"), "key").unwrap();
        std::fs::write(outside.join("note"), "n").unwrap();
        std::fs::create_dir_all(root.join(".glut")).unwrap();
        std::fs::write(root.join(".env"), "SECRET=1").unwrap();
        let policy = Policy::new(&root, &home, &[], &[]);

        let out = sandboxed(&policy, "echo a > ok.txt && echo wrote");
        assert!(out.contains("wrote"), "{out}");
        for (script, what) in [
            (
                format!("echo x > {}/x", outside.display()),
                "a write outside",
            ),
            (
                "echo x > .glut/settings.json".to_owned(),
                "a write into .glut",
            ),
            (format!("cat {}/.ssh/id", home.display()), "reading a key"),
            ("cat .env".to_owned(), "reading .env"),
        ] {
            let out = sandboxed(&policy, &format!("{script} || echo REFUSED"));
            assert!(refused(&out), "{what}: {out}");
        }
        let out = sandboxed(&policy, &format!("cat {}/note", outside.display()));
        assert_eq!(out, "n", "reading elsewhere stays allowed");
        let out = sandboxed(
            &policy,
            "echo tmp > \"${TMPDIR:-/tmp}/glut-sandbox-probe\" && echo tmp-ok",
        );
        assert!(out.contains("tmp-ok"), "{out}");
    }

    #[test]
    fn a_working_folder_inside_a_hidden_one_is_read_but_the_rest_of_it_stays_hidden() {
        if !available() {
            return;
        }
        // A Studio session's worktree: `~/.axiomata/worktrees/<project>/<session>`, below the hidden `~/.axiomata`.
        let home = temp("hidden-home").canonicalize().unwrap();
        let root = home.join(".axiomata/worktrees/p/s");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("README.md"), "hello").unwrap();
        std::fs::write(root.join(".env"), "SECRET=1").unwrap();
        std::fs::write(home.join(".axiomata/config.toml"), "api_key").unwrap();
        let git = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .output()
                .unwrap();
            assert!(status.status.success(), "{status:?}");
        };
        git(&["init", "-q"]);
        let policy = Policy::new(&root, &home, &[], &[]);
        assert_eq!(sandboxed(&policy, "cat README.md"), "hello");
        // Git resolves the folder it is in, up through the hidden one.
        let out = sandboxed(&policy, "git status --short && echo GIT-OK");
        assert!(out.contains("GIT-OK"), "{out}");
        let here = sandboxed(&policy, "pwd -P");
        assert!(here.trim_end().ends_with("/s"), "{here}");
        // The folders on the way stay closed to a look inside — on Linux an empty folder holding only the way to the
        // working one, which the command knows already.
        let out = sandboxed(
            &policy,
            &format!("ls {}/.axiomata || echo REFUSED", home.display()),
        );
        if cfg!(target_os = "linux") {
            assert_eq!(out.trim(), "worktrees", "listing the hidden folder: {out}");
        } else {
            assert!(out.contains("REFUSED"), "listing the hidden folder: {out}");
        }
        let out = sandboxed(&policy, "echo more >> README.md && echo wrote");
        assert!(out.contains("wrote"), "{out}");
        for (script, what) in [
            (
                format!("cat {}/.axiomata/config.toml", home.display()),
                "the rest of the hidden folder",
            ),
            ("cat .env".to_owned(), "the working folder's .env"),
        ] {
            let out = sandboxed(&policy, &format!("{script} || echo REFUSED"));
            assert!(refused(&out), "{what}: {out}");
        }
    }

    #[test]
    fn git_may_commit_but_not_touch_its_config_or_hooks() {
        if !available() {
            return;
        }
        let (home, root) = (temp("git-home"), temp("git-root"));
        let git = |args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        let policy = Policy::new(&root, &home, &[], &[]);
        assert_eq!(policy.git_dirs, [root.join(".git")]);
        let out = sandboxed(
            &policy,
            "echo a > f && git add f && git -c user.name=t -c user.email=t@t commit -qm m && echo COMMITTED",
        );
        assert!(out.contains("COMMITTED"), "{out}");
        // `commondir` would point the owner's git at a config of the command's making.
        for script in [
            "echo x >> .git/config",
            "echo x > .git/hooks/pre-commit",
            "echo /tmp/elsewhere > .git/commondir",
        ] {
            let out = sandboxed(&policy, &format!("{script} || echo REFUSED"));
            assert!(refused(&out), "{script}: {out}");
        }
    }

    #[test]
    fn the_git_switch_off_leaves_the_profile_as_it_was() {
        let root = temp("git-switch-root");
        let home = temp("git-switch-home");
        let mut policy = Policy::new(&root, &home, &[], &[]);
        policy.git_dirs = vec![root.join(".git")];
        let off = policy.profile();
        policy.git_read_only = true;
        let on = policy.profile();
        assert!(
            on.len() > off.len()
                && on.starts_with(&off[..off.find("(deny file-write*\n").unwrap()])
        );
        assert!(
            on.contains("[gG][iI][tT](/.*)?$") && !off.contains("[gG][iI][tT](/.*)?$"),
            "{off}"
        );
    }

    #[test]
    fn with_git_read_only_no_git_folder_is_written() {
        if !available() {
            return;
        }
        let (home, root) = (temp("git-ro-home"), temp("git-ro-root"));
        let git = |dir: &Path, args: &[&str]| {
            std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(args)
                .output()
                .unwrap()
        };
        git(&root, &["init", "-q"]);
        std::fs::create_dir_all(root.join("sub")).unwrap();
        git(&root.join("sub"), &["init", "-q"]);
        let mut policy = Policy::new(&root, &home, &[], &[]);
        policy.git_read_only = true;
        let out = sandboxed(
            &policy,
            "echo a > f && echo WROTE; git add f || echo ADD-REFUSED; \
             echo x > .git/commondir || echo COMMONDIR-REFUSED; echo x > sub/.git/description || echo SUB-REFUSED",
        );
        for expected in ["WROTE", "ADD-REFUSED", "COMMONDIR-REFUSED", "SUB-REFUSED"] {
            assert!(out.contains(expected), "{expected}: {out}");
        }
        assert!(!root.join(".git/commondir").exists(), "{out}");
    }

    #[test]
    fn the_keychain_is_shut_to_a_command_unless_it_was_given_it() {
        if !available() {
            return;
        }
        let (home, root) = (temp("kc-home"), temp("kc-root"));
        let mut policy = Policy::new(&root, &home, &[], &[]);
        // git's own Keychain helper, asked for a host nobody keeps a key for: shut, securityd is out of reach (-50);
        // open, it finds nothing and says so with silence.
        let ask = "printf 'protocol=https\\nhost=example.invalid\\n\\n' | git credential-osxkeychain get; echo rc=$?";
        let shut = sandboxed(&policy, ask);
        if shut.contains("is not a git command") {
            return;
        }
        assert!(shut.contains("-50") && !shut.contains("rc=0"), "{shut}");
        policy.keychain = true;
        let open = sandboxed(&policy, ask);
        assert!(open.contains("rc=0") && !open.contains("-50"), "{open}");
        // TLS trust is no Keychain business: it stays where it was.
        assert!(!policy.profile().contains("mach-lookup"));
    }

    #[test]
    fn other_processes_are_not_signalled_but_its_own_are() {
        if !available() {
            return;
        }
        let (home, root) = (temp("ps-home"), temp("ps-root"));
        let policy = Policy::new(&root, &home, &[], &[]);
        let mut other = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let out = sandboxed(
            &policy,
            &format!(
                "kill {} || echo REFUSED; sleep 30 & kill $! && echo OWN-STOPPED",
                other.id()
            ),
        );
        assert!(out.contains("OWN-STOPPED"), "{out}");
        assert!(out.contains("REFUSED"), "{out}");
        let _ = other.kill();
        let _ = other.wait();
    }

    #[test]
    fn the_network_reaches_only_the_proxy_unless_the_owner_opened_localhost() {
        if !available() {
            return;
        }
        let (home, root) = (temp("net-home"), temp("net-root"));
        let mut policy = Policy::new(&root, &home, &[], &[]);
        let proxy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let service = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let (proxy, service) = (
            proxy.local_addr().unwrap().port(),
            service.local_addr().unwrap().port(),
        );
        policy.proxy_port = Some(proxy);
        // Linux: a command has a network of its own, the proxy reached through the relay to a socket in front of it —
        // here straight to the stand-in proxy's port, as a run's bridge hands it on.
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let socket = temp("net-socket").join("proxy.sock");
        let _bridge = cfg!(target_os = "linux").then(|| {
            runtime
                .block_on(async { proxy::Bridge::start(&socket, proxy) })
                .unwrap()
        });
        let relay = cfg!(target_os = "linux").then_some(socket.as_path());
        // A connection's time limit: BSD's netcat (macOS) and OpenBSD's port on Linux spell it apart.
        let wait = if cfg!(target_os = "linux") {
            "-w 2"
        } else {
            "-G 2"
        };
        let probe = format!(
            "nc -z 127.0.0.1 {proxy} && echo PROXY; nc -z 127.0.0.1 {service} || echo SERVICE-REFUSED; \
             nc -z {wait} 1.1.1.1 443 || echo REMOTE-REFUSED"
        );
        let out = sandboxed_relayed(&policy, &probe, relay);
        assert!(
            out.contains("PROXY")
                && out.contains("SERVICE-REFUSED")
                && out.contains("REMOTE-REFUSED"),
            "{out}"
        );
        policy.local_binding = true;
        let out = sandboxed_relayed(&policy, &probe, relay);
        // On Linux the owner's localhost stays out of reach: the command's own is another (T59).
        assert_eq!(
            out.contains("SERVICE-REFUSED"),
            cfg!(target_os = "linux"),
            "{out}"
        );
        assert!(out.contains("REMOTE-REFUSED"), "{out}");
    }

    #[test]
    fn no_git_folder_in_the_working_folder_takes_new_config_and_no_app_is_driven() {
        if !available() {
            return;
        }
        let (home, root) = (temp("git-home"), temp("git-root"));
        std::fs::create_dir_all(root.join("sub/.git/objects")).unwrap();
        std::fs::create_dir_all(root.join("sub/.git/modules/m/hooks")).unwrap();
        let policy = Policy::new(&root, &home, &[], &[]);
        let out = sandboxed(
            &policy,
            "echo x > sub/.git/config || echo CONFIG-REFUSED; \
             echo x > sub/.git/modules/m/hooks/pre-commit || echo HOOK-REFUSED; \
             echo x > sub/.git/objects/o && echo OBJECTS-WRITTEN; \
             mkdir -p deep && (echo x > deep/.envrc || echo ENVRC-REFUSED); \
             /usr/bin/osascript -e 'return 1' || echo OSASCRIPT-REFUSED; \
             /bin/launchctl print-disabled gui/0 >/dev/null 2>&1 || echo LAUNCHCTL-REFUSED; \
             /usr/bin/security list-keychains >/dev/null 2>&1 || echo SECURITY-REFUSED; \
             mkdir -p ev/hooks && echo x > ev/hooks/pre-commit && (mv ev sub2.git; mkdir sub2; mv sub2.git sub2/.git \
               || echo MOVE-IN-REFUSED); \
             mv sub/.git sub/old-git || echo MOVE-OUT-REFUSED; \
             mkdir -p n && (echo 'gitdir: /private/tmp' > n/.git || echo GITFILE-REFUSED); \
             mkdir -p ev2 && (mv ev2 sub/.git/modules/m2 || echo MODULE-REFUSED)",
        );
        // Refused at once on both systems: what is there is mounted read-only, and a git folder cannot be moved.
        for expected in [
            "MOVE-OUT-REFUSED",
            "HOOK-REFUSED",
            "OBJECTS-WRITTEN",
            "OSASCRIPT-REFUSED",
            "LAUNCHCTL-REFUSED",
            "SECURITY-REFUSED",
        ] {
            assert!(out.contains(expected), "{expected}: {out}");
        }
        if cfg!(target_os = "linux") {
            // What was not there is made, then moved away before anything else runs.
            for made in [
                "sub/.git/config",
                "deep/.envrc",
                "sub2/.git",
                "n/.git",
                "sub/.git/modules/m2",
            ] {
                assert!(!root.join(made).exists(), "{made}: {out}");
                assert!(
                    out.contains(&format!("{}/{made}", root.display())),
                    "{made}: {out}"
                );
            }
        } else {
            for expected in [
                "MOVE-IN-REFUSED",
                "GITFILE-REFUSED",
                "MODULE-REFUSED",
                "CONFIG-REFUSED",
                "ENVRC-REFUSED",
            ] {
                assert!(out.contains(expected), "{expected}: {out}");
            }
        }
    }
}
