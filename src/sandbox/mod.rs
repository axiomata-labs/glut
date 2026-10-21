//! The sandbox the shell's commands run in (`docs/plans/harness.md`, HS3b): macOS's Seatbelt (`sandbox-exec`), the
//! same mechanism Claude Code and Codex use. Unlike the rules of HS3a, which judge what a command *says*, the
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

pub mod proxy;

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
const HOME_SECRETS: [&str; 38] = [
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
    ".axh",
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
];

/// Names in the working folder no command writes (the same configuration the file tools protect; the git folder is
/// handled on its own: commits may write it).
const PROTECTED_IN_ROOT: [&str; 11] = [
    ".axh",
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

/// The parts of a git folder that make the owner's git run code: never written by a command.
const GIT_RUNS: [&str; 4] = ["config", "config.worktree", "hooks", "info"];

/// [`GIT_RUNS`] as a regex alternation, for git folders found anywhere (submodules, worktrees, a nested repository).
const GIT_RUNS_PATTERN: &str = "(config|config\\.worktree|hooks|info)(/.*)?";

/// The git folders below a git folder (`/modules/a`, `/modules/a/modules/b`, `/worktrees/x`) and the folders holding
/// them, as a regex suffix: none of them may be created, moved or removed — moving a prepared folder into place would
/// bring its own `config` and `hooks` (Seatbelt judges a rename by its target, not by what the folder holds).
const GIT_FOLDERS_PATTERN: &str = "(/modules/[^/]+)*(/modules|/worktrees(/[^/]+)?)?";

/// Programs that drive other applications through Apple Events or Launch Services — `tell app "Terminal" to do
/// script` would run anything outside the sandbox. Seatbelt's `appleevent-send`, `lsopen` and `mach-lookup` rules do
/// not stop them (tried on macOS 2026-10-13); refusing to start them does, also through a link, and a copy of such a
/// system program is killed by macOS itself.
const APPLE_EVENT_TOOLS: [&str; 5] = [
    "/usr/bin/osascript",
    "/usr/bin/osacompile",
    "/usr/bin/open",
    "/usr/bin/automator",
    "/usr/bin/shortcuts",
];

/// Whether commands can run in the sandbox here.
pub fn available() -> bool {
    cfg!(target_os = "macos") && Path::new(SANDBOX_EXEC).is_file()
}

/// What a run's sandbox allows, in resolved paths.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// The working folder.
    pub root: PathBuf,
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
        let mut read_denied: Vec<PathBuf> = HOME_SECRETS
            .iter()
            .map(|rel| real(home.join(rel)))
            .collect();
        read_denied.extend(extra_secrets.iter().cloned().map(real));
        Policy {
            root: root.to_path_buf(),
            writable,
            writable_files: HOME_WRITABLE_FILES
                .iter()
                .map(|rel| real(home.join(rel)))
                .collect(),
            write_denied: PROTECTED_IN_ROOT
                .iter()
                .map(|name| root.join(name))
                .collect(),
            read_denied,
            git_dirs: git_dirs(root),
            proxy_port: None,
            local_binding: false,
        }
    }

    /// The Seatbelt profile. Later rules win over earlier ones: the broad allows come first, the narrow denies last.
    pub fn profile(&self) -> String {
        let mut out = String::from("(version 1)\n(allow default)\n");
        // Writing: nothing, then the allowed places, then the protected parts of them.
        out.push_str("(deny file-write*)\n(allow file-write*\n");
        out.push_str(&format!("  (subpath {})\n", quoted(&self.root)));
        for dir in self.writable.iter().chain(&self.git_dirs) {
            out.push_str(&format!("  (subpath {})\n", quoted(dir)));
        }
        for file in &self.writable_files {
            out.push_str(&format!("  (literal {})\n", quoted(file)));
        }
        out.push_str("  (regex #\"^/dev/(null|tty|zero|fd/[0-9]+|stdout|stderr)$\"))\n");
        out.push_str("(deny file-write*\n");
        for path in &self.write_denied {
            out.push_str(&format!("  (subpath {})\n", quoted(path)));
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
        let root = regex_escape(&self.root);
        out.push_str(&format!(
            "  (regex #\"^{root}/(.*/)?\\.git/((modules|worktrees)/.+/)?{GIT_RUNS_PATTERN}$\")\n"
        ));
        out.push_str(&format!(
            "  (regex #\"^{root}/(.*/)?\\.git{GIT_FOLDERS_PATTERN}$\")\n"
        ));
        out.push_str(&format!("  (regex #\"^{root}/(.*/)?\\.envrc$\")\n"));
        out.push_str(")\n");
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
        out.push_str(&format!(
            "  (regex #\"^{root}/(.*/)?(\\.env(\\..*)?|[^/]*\\.env|\\.envrc(\\..*)?)$\"))\n"
        ));
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
fn git_dirs(root: &Path) -> Vec<PathBuf> {
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
        let dir = std::env::temp_dir().join(format!(
            "axh-sandbox-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// Runs `script` with zsh in the sandbox of `policy`, in its working folder; stdout and stderr together.
    fn sandboxed(policy: &Policy, script: &str) -> String {
        let out = std::process::Command::new(SANDBOX_EXEC)
            .arg("-p")
            .arg(policy.profile())
            .args(["/bin/sh", "-c", script])
            .current_dir(&policy.root)
            .output()
            .unwrap();
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
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
            profile.contains("(literal \"/usr/bin/osascript\")"),
            "{profile}"
        );
        assert!(
            !profile.contains("allow network"),
            "no proxy, no network: {profile}"
        );
        assert!(regex_escape(Path::new("/a.b+c")).ends_with("a\\.b\\+c"));
    }

    #[test]
    fn a_sandboxed_command_writes_and_reads_only_what_it_may() {
        if !available() {
            return;
        }
        let (home, root) = (temp("home"), temp("root"));
        // Outside every writable place: the temp folders are writable, the workspace's `target/` is not.
        let outside =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/axh-sandbox-outside");
        std::fs::create_dir_all(&outside).unwrap();
        let outside = outside.canonicalize().unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/id"), "key").unwrap();
        std::fs::write(outside.join("note"), "n").unwrap();
        std::fs::create_dir_all(root.join(".axh")).unwrap();
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
                "echo x > .axh/settings.json".to_owned(),
                "a write into .axh",
            ),
            (format!("cat {}/.ssh/id", home.display()), "reading a key"),
            ("cat .env".to_owned(), "reading .env"),
        ] {
            let out = sandboxed(&policy, &format!("{script} || echo REFUSED"));
            assert!(out.contains("REFUSED"), "{what}: {out}");
        }
        let out = sandboxed(&policy, &format!("cat {}/note", outside.display()));
        assert_eq!(out, "n", "reading elsewhere stays allowed");
        let out = sandboxed(
            &policy,
            "echo tmp > \"$TMPDIR/axh-sandbox-probe\" && echo tmp-ok",
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
        // The folders on the way stay closed to a look inside.
        let out = sandboxed(
            &policy,
            &format!("ls {}/.axiomata || echo REFUSED", home.display()),
        );
        assert!(out.contains("REFUSED"), "listing the hidden folder: {out}");
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
            assert!(out.contains("REFUSED"), "{what}: {out}");
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
        for script in ["echo x >> .git/config", "echo x > .git/hooks/pre-commit"] {
            let out = sandboxed(&policy, &format!("{script} || echo REFUSED"));
            assert!(out.contains("REFUSED"), "{script}: {out}");
        }
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
        let probe = format!(
            "nc -z 127.0.0.1 {proxy} && echo PROXY; nc -z 127.0.0.1 {service} || echo SERVICE-REFUSED; \
             nc -z -G 2 1.1.1.1 443 || echo REMOTE-REFUSED"
        );
        let out = sandboxed(&policy, &probe);
        assert!(
            out.contains("PROXY")
                && out.contains("SERVICE-REFUSED")
                && out.contains("REMOTE-REFUSED"),
            "{out}"
        );
        policy.local_binding = true;
        let out = sandboxed(&policy, &probe);
        assert!(
            !out.contains("SERVICE-REFUSED") && out.contains("REMOTE-REFUSED"),
            "{out}"
        );
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
             mkdir -p ev/hooks && echo x > ev/hooks/pre-commit && (mv ev sub2.git; mkdir sub2; mv sub2.git sub2/.git \
               || echo MOVE-IN-REFUSED); \
             mv sub/.git sub/old-git || echo MOVE-OUT-REFUSED; \
             mkdir -p n && (echo 'gitdir: /private/tmp' > n/.git || echo GITFILE-REFUSED); \
             mkdir -p ev2 && (mv ev2 sub/.git/modules/m2 || echo MODULE-REFUSED)",
        );
        for expected in [
            "MOVE-IN-REFUSED",
            "MOVE-OUT-REFUSED",
            "GITFILE-REFUSED",
            "MODULE-REFUSED",
            "CONFIG-REFUSED",
            "HOOK-REFUSED",
            "OBJECTS-WRITTEN",
            "ENVRC-REFUSED",
            "OSASCRIPT-REFUSED",
        ] {
            assert!(out.contains(expected), "{expected}: {out}");
        }
    }
}
