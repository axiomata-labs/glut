//! The shell tools (HS2, S1–S6): `bash` runs a command in the owner's shell, in the
//! foreground or as a background job; `bash_output` and `kill_shell` look after the jobs.
//!
//! One [`Session`] per run holds what the commands share: the shell, the profile snapshot (S2), the folder the last
//! command ended in (S3), the cleaned environment (S5) and the jobs (S6). The tools hold it through an `Arc`; when the
//! run's toolbox is dropped, the session kills every job still running and removes its scratch folder.
//!
//! Every command runs in a process group of its own, so a timeout, a stop or the end of the run kills it with all its
//! children — a server it started included. A child that leaves the group on purpose (`setsid`, a daemon) is out of
//! that reach, and a command can read what the owner's user can read, the harness's own environment included (`ps`):
//! the shell is no fence until the sandbox of HS3.

use std::ffi::OsString;
use std::hash::BuildHasher;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustix::process::{Pid, Signal, kill_process_group};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, Command};
use tokio::sync::OnceCell;

use super::{CallContext, StopFlag, Tool, ToolFuture};
use crate::error::HarnessError;
use crate::model::ToolSpec;
use crate::permissions::{Access, Gate};
use crate::sandbox::proxy::Proxy;

/// Set in every command Glut runs: the commands that keep a key or a sign-in (`glut providers key|add|login|…`,
/// `glut mcp login|…`) refuse under it — the Keychain is reached past the sandbox, so a model must not drive them.
/// A speed bump, not a barrier: a command can unset it; the owner's word stays the gate's.
pub const SANDBOXED_ENV: &str = "GLUT_SANDBOXED";

/// What a command given the Keychain runs with (T74b): git finds no bare repository by itself — one a model made in the
/// project (`x.git`, its config and hooks its own) would run them with the Keychain open; the project's own `.git` is
/// the sandbox's to keep.
const KEYCHAIN_GIT: [(&str, &str); 3] = [
    ("GIT_CONFIG_COUNT", "1"),
    ("GIT_CONFIG_KEY_0", "safe.bareRepository"),
    ("GIT_CONFIG_VALUE_0", "explicit"),
];
/// A command's time limit when the model names none.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
/// The longest time limit the model may ask for.
const MAX_TIMEOUT: Duration = Duration::from_secs(600);
/// How long writing the profile snapshot may take; a profile that hangs (a prompt waiting for input) is given up.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(15);
/// The bytes of a foreground command's output that are kept. The tool result is cut further for the model; this only
/// keeps a command that prints without end from filling the memory.
const MAX_FOREGROUND_BYTES: usize = 256 * 1024;
/// The bytes of a background job's output that are kept: its latest part.
const MAX_JOB_BYTES: usize = 1024 * 1024;
/// Jobs that may run at the same time.
const MAX_JOBS: usize = 8;
/// Jobs kept in all, running or ended: an ended job stays for a last look until newer ones push it out.
const MAX_KEPT_JOBS: usize = 32;
/// How often a waiting foreground command checks whether the run was stopped.
const STOP_POLL: Duration = Duration::from_millis(100);
/// After a foreground command ended, how long its output pipe may still be drained. A process it left behind holds
/// the pipe open; the group is killed after this.
const DRAIN_GRACE: Duration = Duration::from_millis(200);
/// The shell when `$SHELL` names none that can be used.
const FALLBACK_SHELL: &str = "/bin/sh";

/// Which shell runs the commands, and how the session should start.
#[derive(Debug, Clone)]
pub struct ShellOptions {
    pub shell: PathBuf,
    /// Write and source a snapshot of the shell's profile (S2).
    pub profile: bool,
    /// The environment the cleaned one is made from (S5) — the process's own, unless a test or an embedder says
    /// otherwise.
    pub ambient: Vec<(OsString, OsString)>,
    /// Run commands in the sandbox (HS3b) with these settings; `None`, or a system without one, runs them as they
    /// are, judged by the rules alone.
    pub sandbox: Option<crate::permissions::settings::SandboxSettings>,
    /// The caller's own programs a declared command may start directly — see [`DirectProgram`].
    pub direct: Vec<DirectProgram>,
    /// The folders beside the working one (T60c): a sandboxed command writes there as in the working folder.
    pub beside: crate::folders::Folders,
    /// The Keychain's list and switch (T74), shared with the gate: a sandboxed command gets the Keychain only when
    /// it is open to all, or the command runs a listed program the gate let through.
    pub keychain: crate::keychain::Keychain,
    /// An arena model's confinement (T61), shared with the toolbox: set, every command writes its own folder alone.
    pub confinement: crate::sandbox::Confinement,
}

/// A program of the caller's own that a run starts directly — no shell, no sandbox, its words as its arguments — when
/// the run's declaration names the exact command (`Bash(axiomata-cli board add:*)`, glut-round W8). The app's CLI
/// needs its database under `~/.axiomata`, which the sandbox hides from every command because the owner's keys live
/// there too; the declaration is what makes the one command trusted, never the program alone. A command with anything
/// besides words — an assignment, a redirection, a second command, an expansion — runs in the shell as usual.
#[derive(Debug, Clone)]
pub struct DirectProgram {
    /// The command word that names it, exactly (`axiomata-cli`).
    pub name: String,
    /// The program itself, an absolute path; never looked up on the `PATH`.
    pub path: PathBuf,
    /// What it needs beside the cleaned environment (`AXIOMATA_HOME` of a run under a scratch home).
    pub env: Vec<(OsString, OsString)>,
    /// The only commands of it that may start this way, as their leading words (`["board", "add"]`): a declaration
    /// as wide as `Bash(axiomata-cli:*)` still reaches no other one outside the sandbox (security audit).
    pub allowed: Vec<Vec<String>>,
}

impl ShellOptions {
    /// The owner's shell from `$SHELL` (an absolute path to a file), else `/bin/sh`, with the profile snapshot. A shell
    /// other than zsh or bash runs as `/bin/sh` (see [`shell_tools`]).
    pub fn from_environment() -> Self {
        let shell = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute() && path.is_file())
            .unwrap_or_else(|| PathBuf::from(FALLBACK_SHELL));
        ShellOptions {
            shell,
            profile: true,
            ambient: std::env::vars_os().collect(),
            sandbox: Some(Default::default()),
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
        }
    }
}

/// The shell tools for a run in `cwd`.
///
/// # Errors
///
/// [`HarnessError::Workspace`] if `cwd` cannot be resolved.
pub fn shell_tools(cwd: &Path, options: ShellOptions) -> Result<Vec<Box<dyn Tool>>, HarnessError> {
    let session = Arc::new(Session::new(cwd, options)?);
    Ok(vec![
        Box::new(Bash(Arc::clone(&session))),
        Box::new(BashOutput(Arc::clone(&session))),
        Box::new(KillShell(session)),
    ])
}

/// The shell families the harness knows how to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Zsh,
    Bash,
    /// `/bin/sh`: no snapshot, plain POSIX.
    Posix,
}

impl Family {
    fn of(shell: &Path) -> Self {
        match shell.file_name().and_then(|name| name.to_str()) {
            Some("zsh") => Family::Zsh,
            Some("bash") => Family::Bash,
            _ => Family::Posix,
        }
    }

    /// The script a login shell runs to write the snapshot to `$AXH_SNAPSHOT`. Not an interactive shell (`-i`): in a
    /// process group of its own it would fight the terminal for control and stop. The interactive profile is read by
    /// hand instead — `~/.zshrc` for zsh; bash's login profile usually reads `~/.bashrc` itself. The snapshot is the
    /// owner's own, readable only by them (`umask 077`).
    fn snapshot_script(self) -> Option<&'static str> {
        match self {
            Family::Zsh => Some(concat!(
                "[ -r \"${ZDOTDIR:-$HOME}/.zshrc\" ] && . \"${ZDOTDIR:-$HOME}/.zshrc\" >/dev/null 2>&1; umask 077; ",
                "{ typeset -f; alias -L; printf 'export PATH=%q\\n' \"$PATH\"; } > \"$AXH_SNAPSHOT\" 2>/dev/null"
            )),
            Family::Bash => Some(concat!(
                "umask 077; ",
                "{ declare -f; alias -p; printf 'export PATH=%q\\n' \"$PATH\"; } > \"$AXH_SNAPSHOT\" 2>/dev/null"
            )),
            Family::Posix => None,
        }
    }

    /// What goes before the wrapper's own `cd`, `eval` and `pwd`: a function or alias of the same name from the
    /// owner's profile must not take over the wrapper. `/bin/sh` sources no profile and may not know `builtin`.
    fn builtin(self) -> &'static str {
        match self {
            Family::Zsh | Family::Bash => "builtin ",
            Family::Posix => "",
        }
    }
}

/// The script every command runs in: stderr into stdout, the snapshot, the folder, the command (from the environment,
/// so no quoting can break it), and where it ended.
fn command_script(family: Family) -> String {
    // bash expands aliases only in interactive shells unless told to.
    let aliases = if family == Family::Bash {
        "shopt -s expand_aliases\n"
    } else {
        ""
    };
    // The gate reads a glob as the shell's default does: `*` never matches a leading dot. A profile (`~/.zshenv`
    // runs for every zsh) must not change that; a command that does is judged by its own words.
    let globs = match family {
        Family::Zsh => "builtin unsetopt glob_dots 2>/dev/null\n",
        Family::Bash => {
            "builtin shopt -u dotglob 2>/dev/null; builtin unset GLOBIGNORE 2>/dev/null\n"
        }
        Family::Posix => "unset GLOBIGNORE 2>/dev/null\n",
    };
    let b = family.builtin();
    format!(
        "exec 2>&1\n{aliases}\
         if [ -n \"$AXH_SNAPSHOT\" ]; then . \"$AXH_SNAPSHOT\" >/dev/null 2>&1; fi\n\
         {globs}\
         {b}cd \"$AXH_CWD\" || exit 1\n\
         {b}eval \"$AXH_COMMAND\"\n\
         __glut_status=$?\n\
         if [ -n \"$AXH_CWD_FILE\" ]; then {b}pwd -P > \"$AXH_CWD_FILE\" 2>/dev/null; fi\n\
         exit $__glut_status\n"
    )
}

/// The script a hook runs in (HS7, D5): the snapshot and the folder, then the command — stderr kept apart from
/// stdout (a hook's two channels mean different things), and no folder note: a hook does not move the run's folder.
fn hook_script(family: Family) -> String {
    let b = family.builtin();
    format!(
        "if [ -n \"$AXH_SNAPSHOT\" ]; then . \"$AXH_SNAPSHOT\" >/dev/null 2>&1; fi\n\
         {b}cd \"$AXH_CWD\" || exit 1\n\
         {b}eval \"$AXH_COMMAND\"\n"
    )
}

/// Whether a command runs in the sandbox, and then with the gate its proxy asks about hosts and whether it was given
/// the Keychain (T74: a listed program the gate let through).
enum Sandboxing {
    On(Option<Arc<Gate>>, bool),
    /// The escape (`dangerously_disable_sandbox`), already asked for by the gate.
    Off,
}

/// A run's sandbox once its first command needs it.
struct Sandbox {
    proxy: Option<Proxy>,
    /// What the sandbox allows, with the proxy's port as the one way to the network; the profile is made from it for
    /// each command, with the folders beside the working one as they stand then (T60c).
    policy: crate::sandbox::Policy,
    /// The way from the Linux sandbox's own network to the proxy (T59); `None` elsewhere or without a proxy.
    bridge: Option<crate::sandbox::proxy::Bridge>,
}

/// What the shell tools of one run share — also the hooks of a run (HS7, D5): they get what a command gets.
pub(crate) struct Session {
    root: PathBuf,
    shell: PathBuf,
    family: Family,
    profile: bool,
    /// The cleaned environment (S5).
    env: Vec<(OsString, OsString)>,
    /// A private folder for the snapshot and the folder notes; removed with the session.
    scratch: PathBuf,
    snapshot: OnceCell<Option<PathBuf>>,
    cwd: Mutex<PathBuf>,
    /// What the sandbox allows (HS3b); `None` without one.
    policy: Option<crate::sandbox::Policy>,
    /// The proxy and the profile naming its port, made with the first sandboxed command.
    sandbox: OnceCell<Sandbox>,
    jobs: Mutex<Vec<Job>>,
    next_job: AtomicU32,
    next_note: AtomicU64,
    direct: Vec<DirectProgram>,
    /// The folders beside the working one (T60c).
    beside: crate::folders::Folders,
    /// The Keychain's list and switch (T74).
    keychain: crate::keychain::Keychain,
    /// An arena model's confinement (T61): read before every command.
    confinement: crate::sandbox::Confinement,
}

/// Settles a command's watch ([`crate::sandbox::scan::Watch`]) off the async threads — it walks the folders — and
/// returns its note. Begun, it finishes though the caller is dropped meanwhile.
async fn settled(watch: Option<crate::sandbox::scan::Watch>) -> Option<String> {
    let watch = watch?;
    tokio::task::spawn_blocking(move || watch.settle())
        .await
        .ok()
        .flatten()
}

/// What `reader` writes, kept up to [`MAX_FOREGROUND_BYTES`]; the rest is read and dropped, so the program never
/// waits on a full pipe.
async fn read_capped(reader: Option<impl tokio::io::AsyncRead + Unpin>) -> Vec<u8> {
    let mut kept = Vec::new();
    let Some(mut reader) = reader else {
        return kept;
    };
    let mut chunk = [0u8; 8192];
    while let Ok(read) = reader.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        let room = MAX_FOREGROUND_BYTES.saturating_sub(kept.len());
        kept.extend_from_slice(&chunk[..read.min(room)]);
    }
    kept
}

/// One background job.
struct Job {
    id: String,
    command: String,
    group: Group,
    output: Arc<Mutex<Capture>>,
    /// How many bytes of the output the model has seen.
    read: u64,
    /// How it ended, once it has.
    ended: Arc<Mutex<Option<String>>>,
}

/// How a hook ended (HS7): its exit code (`None` when a signal took it), what it wrote on either channel, and
/// whether it ran into its time limit.
pub(crate) struct HookOutput {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) timed_out: bool,
}

impl Session {
    pub(crate) fn new(cwd: &Path, options: ShellOptions) -> Result<Self, HarnessError> {
        let root = cwd.canonicalize().map_err(|err| HarnessError::Workspace {
            path: cwd.display().to_string(),
            reason: err.to_string(),
        })?;
        let search: Vec<PathBuf> = options
            .ambient
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| std::env::split_paths(value).collect())
            .unwrap_or_default();
        let env = axiomata_files::toolenv::tool_env_from(options.ambient.iter().cloned(), &search);
        let scratch = scratch_dir().map_err(|err| HarnessError::Workspace {
            path: std::env::temp_dir().display().to_string(),
            reason: format!("no scratch folder for the shell: {err}"),
        })?;
        // The wrapper is POSIX shell: a fish, nu or csh would not run it, so anything but zsh and bash is `/bin/sh`.
        let family = Family::of(&options.shell);
        let shell = if family == Family::Posix {
            PathBuf::from(FALLBACK_SHELL)
        } else {
            options.shell
        };
        let home = options
            .ambient
            .iter()
            .find(|(key, _)| key == "HOME")
            .map(|(_, value)| PathBuf::from(value))
            .filter(|home| home.is_absolute());
        let policy = match (&options.sandbox, home) {
            (Some(settings), Some(home)) if settings.enabled && crate::sandbox::available() => {
                let mut policy = crate::sandbox::Policy::new(
                    &root,
                    &home,
                    &settings.allow_write,
                    &settings.deny_read,
                );
                policy.local_binding = settings.allow_local_binding;
                Some(policy)
            }
            // A sandbox asked for and possible here, but no home to keep the secrets of: refused, never run without.
            (Some(settings), None) if settings.enabled && crate::sandbox::available() => {
                return Err(HarnessError::Workspace {
                    path: root.display().to_string(),
                    reason: "the sandbox needs an absolute HOME in the environment".into(),
                });
            }
            _ => None,
        };
        Ok(Session {
            policy,
            family,
            shell,
            profile: options.profile,
            env,
            scratch,
            snapshot: OnceCell::new(),
            sandbox: OnceCell::new(),
            cwd: Mutex::new(root.clone()),
            root,
            jobs: Mutex::new(Vec::new()),
            next_job: AtomicU32::new(1),
            next_note: AtomicU64::new(1),
            direct: options.direct,
            beside: options.beside,
            keychain: options.keychain,
            confinement: options.confinement,
        })
    }

    /// The profile snapshot, written on first use; `None` without a profile, for `/bin/sh`, or when writing it failed
    /// (commands then run without it).
    async fn snapshot(&self) -> Option<PathBuf> {
        self.snapshot
            .get_or_init(|| async {
                let script = self.family.snapshot_script().filter(|_| self.profile)?;
                let file = self.scratch.join("snapshot.sh");
                let mut command = Command::new(&self.shell);
                command
                    .args(["-l", "-c", script])
                    .env_clear()
                    .envs(self.env.iter().cloned())
                    .env("AXH_SNAPSHOT", &file)
                    .current_dir(&self.root)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .process_group(0)
                    .kill_on_drop(true);
                let mut child = command.spawn().ok()?;
                // Whatever the profile started (an agent, a helper) goes with the snapshot shell, on every path out.
                let group = GroupGuard(Group::of(&child));
                let status = tokio::time::timeout(SNAPSHOT_TIMEOUT, child.wait())
                    .await
                    .ok()?
                    .ok()?;
                drop(group);
                let written = std::fs::metadata(&file).is_ok_and(|meta| meta.len() > 0);
                (status.success() && written).then_some(file)
            })
            .await
            .clone()
    }

    /// The run's proxy and profile, made on first use with the run's gate (one gate per run, so the first caller's
    /// is everyone's). `None` without a sandbox.
    async fn sandbox(&self, gate: Option<Arc<Gate>>) -> Option<&Sandbox> {
        let policy = self.policy.as_ref()?;
        let socket = self.scratch.join("proxy.sock");
        let sandbox = self
            .sandbox
            .get_or_init(|| async move {
                // Without a proxy a command still runs, it just reaches no network: the profile then opens no port.
                let proxy = Proxy::start(gate).await.ok();
                let mut policy = policy.clone();
                policy.proxy_port = proxy.as_ref().map(Proxy::port);
                let bridge = proxy
                    .as_ref()
                    .filter(|_| cfg!(target_os = "linux"))
                    .and_then(|proxy| {
                        crate::sandbox::proxy::Bridge::start(&socket, proxy.port()).ok()
                    });
                Sandbox {
                    policy,
                    proxy,
                    bridge,
                }
            })
            .await;
        Some(sandbox)
    }

    /// A command ready to spawn: the shell running `script` for `command` in `cwd`; with the Linux sandbox, the watch
    /// that tells what the command made which the sandbox refuses ([`crate::sandbox::scan`]), settled once it ends.
    async fn command(
        &self,
        command: &str,
        cwd: &Path,
        cwd_file: Option<&Path>,
        sandbox: Sandboxing,
        script: &str,
    ) -> Result<(Command, Option<crate::sandbox::scan::Watch>), String> {
        let mut proxy_env = Vec::new();
        // T36c: a pane's role without a `Network` rule keeps its commands offline, as a subagent's narrowing does.
        let role_online = match &sandbox {
            Sandboxing::On(gate, _) => gate.as_ref().is_none_or(|gate| gate.role_reaches_network()),
            Sandboxing::Off => true,
        };
        let mut keychain_open = false;
        let mut watch = None;
        let (sandbox, given_keychain) = match sandbox {
            Sandboxing::On(gate, keychain) => (self.sandbox(gate).await, keychain),
            Sandboxing::Off => (None, false),
        };
        // The owner's word (T94): without a sandbox, no command — the escape past it neither.
        // A home not to be found counts as the switch on: the switch fails closed.
        if sandbox.is_none()
            && self
                .home()
                .is_none_or(|home| crate::sandbox::choices::Choices::in_force(&home).required)
        {
            let why = crate::sandbox::unavailable().unwrap_or_else(|| {
                "it is off in ~/.glut/settings.json, or this command was to run past it".into()
            });
            return Err(format!(
                "no command runs without a sandbox here (the owner's /sandbox required is on): {why}"
            ));
        }
        // An arena's model runs nothing outside the sandbox, whatever was asked — nor a command that must not read
        // places it hides (#142: a helper's copy), which only the sandbox keeps from it.
        if sandbox.is_none() && self.confinement.get().is_some() {
            return Err("an arena's model runs no command outside the sandbox".into());
        }
        if sandbox.is_none() && !self.confinement.hidden().is_empty() {
            return Err("a helper's copy runs no command outside the sandbox: it would read what is hidden from it".into());
        }
        // The owner's profile runs for the snapshot only once a command is going to run.
        let snapshot = self.snapshot().await;
        let mut process = match sandbox {
            Some(sandbox) => {
                let online = crate::permissions::agent_reaches_network() && role_online;
                if online && let Some(proxy) = &sandbox.proxy {
                    proxy_env = proxy.environment();
                }
                // Without the network for a named agent whose `tools` name none (T37a) — the proxy judges on a task of
                // its own, where the agent's narrowing is not seen.
                let mut policy = sandbox
                    .policy
                    .with_beside(&self.beside.list())
                    .with_choices_now();
                if let Some(confined) = self.confinement.get() {
                    if !confined.intact() {
                        return Err(
                            "the arena's private home is no longer the folder it was: no command runs".into(),
                        );
                    }
                    policy = policy.confined(&confined);
                }
                policy.read_denied.extend(self.confinement.hidden());
                if !online {
                    policy.proxy_port = None;
                }
                policy.keychain = given_keychain || self.keychain.is_open();
                keychain_open = policy.keychain;
                // The scratch folder holds the profile snapshot every later command sources: a command that wrote
                // there would run its code inside the next one — one given the Keychain too. Only its own note of
                // where it ended stays writable.
                // Every session's scratch folder, not only this one's ([`scratch_home`]).
                policy
                    .write_denied
                    .push(self.scratch.parent().unwrap_or(&self.scratch).to_path_buf());
                policy.notes.extend(cwd_file.map(Path::to_path_buf));
                let linux = cfg!(target_os = "linux");
                // macOS searches too: Seatbelt judges a rename by where it lands, so a folder made elsewhere and moved
                // in whole brings a `.cargo` or a `.git` no rule saw — the check after the command moves it away.
                let searched = linux || cfg!(target_os = "macos");
                let found = if searched {
                    // Linux binds only what is there: the note too, before the command writes it.
                    if linux && let Some(file) = cwd_file {
                        std::fs::File::create(file)
                            .map_err(|err| format!("no note for the command's folder: {err}"))?;
                    }
                    // It walks the folders: off the async threads.
                    let (root, beside, git) = (
                        policy.root.clone(),
                        policy.beside.clone(),
                        policy.git_dirs.clone(),
                    );
                    tokio::task::spawn_blocking(move || {
                        crate::sandbox::scan::search(&root, &beside, &git)
                    })
                    .await
                    .map_err(|err| format!("the search before the command failed: {err}"))?
                } else {
                    crate::sandbox::scan::Found::default()
                };
                let relay = sandbox
                    .bridge
                    .as_ref()
                    .filter(|_| policy.proxy_port.is_some())
                    .map(crate::sandbox::proxy::Bridge::path);
                let process =
                    Command::from(policy.command(&self.shell, &self.root, &found, relay)?);
                // On macOS a search cut short checks nothing after: the profile, not the search, holds there.
                if linux || searched && found.complete {
                    // It follows every folder the search went through: off the async threads too.
                    let watched = policy.clone();
                    watch = Some(
                        tokio::task::spawn_blocking(move || {
                            crate::sandbox::scan::Watch::new(&watched, found)
                        })
                        .await
                        .map_err(|err| format!("the watch over the command failed: {err}"))?,
                    );
                }
                process
            }
            None => Command::new(&self.shell),
        };
        process
            .arg("-c")
            .arg(script)
            .env_clear()
            .envs(self.env.iter().cloned())
            .envs(self.confinement.environment())
            .envs(proxy_env)
            .env("AXH_COMMAND", command)
            .env(SANDBOXED_ENV, "1")
            .env("AXH_CWD", cwd)
            .env("AXH_SNAPSHOT", snapshot.as_deref().unwrap_or(Path::new("")))
            .env("AXH_CWD_FILE", cwd_file.unwrap_or(Path::new("")))
            .envs(
                if keychain_open {
                    KEYCHAIN_GIT.as_slice()
                } else {
                    &[]
                }
                .iter()
                .copied(),
            )
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        Ok((process, watch))
    }

    /// The owner's home folder, as the cleaned environment names it, else as Glut's own does.
    fn home(&self) -> Option<PathBuf> {
        self.env
            .iter()
            .find(|(key, _)| key == "HOME")
            .map(|(_, value)| PathBuf::from(value))
            .filter(|home| home.is_absolute())
            .or_else(|| crate::permissions::settings::home_dir().ok())
    }

    /// A mark for one chain's parts (N7): random for each call, so no file a command prints can hold one. It is no
    /// secret from the command itself (the marked text is in its environment); a part that prints a mark only
    /// misplaces its own chain's parts for compaction — the raw output is kept either way.
    fn mark(&self) -> String {
        let seed = self.next_note.fetch_add(1, Ordering::Relaxed);
        format!(
            "glut-mark-{:016x}",
            std::hash::RandomState::new().hash_one(seed)
        )
    }

    /// The folder the next command starts in, and a note when it had to go back to the run's folder because the one it
    /// was in has gone (deleted by a command or a job).
    fn start_dir(&self) -> (PathBuf, Option<String>) {
        let mut cwd = self.cwd.lock().unwrap_or_else(|poison| poison.into_inner());
        if cwd.is_dir() {
            return (cwd.clone(), None);
        }
        let gone = std::mem::replace(&mut *cwd, self.root.clone());
        let note = format!(
            "[the folder {} is gone; this command runs in {}]\n",
            gone.display(),
            self.root.display()
        );
        (cwd.clone(), Some(note))
    }

    /// Runs `command` to its end or its time limit, and carries over the folder it ended in (S3).
    async fn run(
        &self,
        command: &str,
        limit: Duration,
        stop: &StopFlag,
        sandbox: Sandboxing,
    ) -> Result<String, String> {
        let (cwd, moved_back) = self.start_dir();
        let note = self.scratch.join(format!(
            "cwd-{}",
            self.next_note.fetch_add(1, Ordering::Relaxed)
        ));
        let (mut process, watch) = self
            .command(
                command,
                &cwd,
                Some(&note),
                sandbox,
                &command_script(self.family),
            )
            .await?;
        let mut child = process
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", self.shell.display()))?;
        // Dropped with this future (the run timed out or was stopped while waiting here): the group goes with it.
        let group = GroupGuard(Group::of(&child));
        let output = Arc::new(Mutex::new(Capture::keep_ends(MAX_FOREGROUND_BYTES)));
        let mut reader = collect(&mut child, Arc::clone(&output));
        let waited = tokio::time::timeout(limit, async {
            loop {
                tokio::select! {
                    status = child.wait() => break status,
                    () = tokio::time::sleep(STOP_POLL) => {
                        if stop.load(Ordering::SeqCst) {
                            group.0.kill();
                        }
                    }
                }
            }
        })
        .await;
        // Whatever ended the wait, the group goes — after a normal end that is a process the command left running
        // (`server &`), which would hold the pipe open. Then the pipe gets a moment to drain what was already written.
        let ending = match waited {
            Ok(Ok(status)) => Ok(status),
            Ok(Err(err)) => Err(format!("[the command could not be waited for: {err}]")),
            Err(_) => Err(format!(
                "[timed out after {} s; the command and its process group were stopped]",
                limit.as_secs()
            )),
        };
        if ending.is_err() {
            group.0.kill();
            let _ = child.wait().await;
        }
        drop(group);
        if tokio::time::timeout(DRAIN_GRACE, &mut reader)
            .await
            .is_err()
        {
            reader.abort();
        }
        let mut text = moved_back.unwrap_or_default();
        text.push_str(
            &output
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .text(),
        );
        if let Some(moved) = self.carry_folder(&note) {
            text.push_str(&moved);
        }
        if let Some(refused) = settled(watch).await {
            text.push_str(&refused);
        }
        let _ = std::fs::remove_file(&note);
        match ending {
            Ok(status) if status.success() => Ok(if text.trim().is_empty() {
                "(no output)".into()
            } else {
                text
            }),
            Ok(status) => Err(format!("{}\n[{}]", text.trim_end(), describe_exit(status))),
            Err(reason) => Err(format!("{}\n{reason}", text.trim_end())),
        }
    }

    /// The direct program `command` starts and its arguments, when it is one: a single simple command, every bit of it
    /// read, without assignments or redirections, whose command word names a [`DirectProgram`], and which the run's
    /// declaration names exactly ([`Gate::declares_command`]).
    fn direct_call(
        &self,
        command: &str,
        gate: Option<&Gate>,
    ) -> Option<(&DirectProgram, Vec<String>)> {
        let analysis = crate::permissions::command::analyze(command);
        let [part] = analysis.parts.as_slice() else {
            return None;
        };
        let plain = !analysis.is_opaque()
            && analysis.writes.is_empty()
            && analysis.reads.is_empty()
            && analysis.nested.is_empty()
            && !analysis.may_name_protected()
            && part.assignments.is_empty();
        let (name, args) = part.words.split_first()?;
        let program = self.direct.iter().find(|program| program.name == *name)?;
        let allowed = program
            .allowed
            .iter()
            .any(|prefix| args.len() >= prefix.len() && args[..prefix.len()] == prefix[..]);
        (plain && allowed && gate?.declares_command(&part.words)).then(|| (program, args.to_vec()))
    }

    /// Runs a direct program (see [`DirectProgram`]) in the shell's folder, its output and its errors kept together
    /// as a command's are; stopped when the run stops (the future is dropped) or at `limit`.
    async fn run_direct(
        &self,
        program: &DirectProgram,
        args: &[String],
        limit: Duration,
        stop: &StopFlag,
    ) -> Result<String, String> {
        let (cwd, _) = self.start_dir();
        let mut child = Command::new(&program.path)
            .args(args)
            .env_clear()
            .envs(self.env.iter().cloned())
            .envs(program.env.iter().cloned())
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", program.name))?;
        let (stdout, stderr) = (child.stdout.take(), child.stderr.take());
        let run = async {
            let (out, err, status) =
                tokio::join!(read_capped(stdout), read_capped(stderr), async {
                    // A stopped run stops the program too, as it stops a shell command.
                    loop {
                        tokio::select! {
                            status = child.wait() => break status,
                            () = tokio::time::sleep(STOP_POLL) => {
                                if stop.load(Ordering::SeqCst) {
                                    let _ = child.start_kill();
                                }
                            }
                        }
                    }
                });
            status.map(|status| (out, err, status))
        };
        let (out, err, status) = tokio::time::timeout(limit, run)
            .await
            .map_err(|_| {
                format!(
                    "[timed out after {} s; {} was stopped]",
                    limit.as_secs(),
                    program.name
                )
            })?
            .map_err(|err| format!("[{} could not be waited for: {err}]", program.name))?;
        let output = std::process::Output {
            status,
            stdout: out,
            stderr: err,
        };
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if text.len() > MAX_FOREGROUND_BYTES {
            // Head and end, as a shell command's output: a CLI reports its outcome last.
            let mut head = MAX_FOREGROUND_BYTES / 2;
            while !text.is_char_boundary(head) {
                head -= 1;
            }
            let mut tail = text.len() - MAX_FOREGROUND_BYTES / 2;
            while !text.is_char_boundary(tail) {
                tail += 1;
            }
            let dropped = tail - head;
            text = format!(
                "{}\n[… {dropped} bytes of output in the middle were not kept …]\n{}",
                &text[..head],
                &text[tail..]
            );
        }
        if output.status.success() {
            Ok(if text.trim().is_empty() {
                "(no output)".into()
            } else {
                text
            })
        } else {
            Err(format!(
                "{}\n[{}]",
                text.trim_end(),
                describe_exit(output.status)
            ))
        }
    }

    /// Runs a hook command (HS7, D5): `payload` on stdin, stdout and stderr kept apart — a hook's two channels
    /// mean different things (context vs. block reason). Hooks start in the run's folder but do not carry a
    /// folder over, and they run in the sandbox like any command, with the run's proxy for the network. Killed
    /// with its process group on a time limit or when the run stops.
    pub(crate) async fn run_hook(
        &self,
        command: &str,
        payload: &str,
        limit: Duration,
        stop: &StopFlag,
        gate: Option<Arc<Gate>>,
    ) -> Result<HookOutput, String> {
        let (cwd, _) = self.start_dir();
        let (mut process, watch) = self
            .command(
                command,
                &cwd,
                None,
                // A hook gets the Keychain only through the owner's switch, never for a listed program: nobody
                // was asked for it.
                Sandboxing::On(gate, false),
                &hook_script(self.family),
            )
            .await
            .map_err(|err| format!("the hook could not start: {err}"))?;
        process.stdin(Stdio::piped()).stderr(Stdio::piped());
        let mut child = process
            .spawn()
            .map_err(|err| format!("the hook could not start: {err}"))?;
        // The group goes whichever way this future ends (B3: a dropped hook is a note, never a stop).
        let group = GroupGuard(Group::of(&child));
        // Written from a task of its own: a payload larger than the pipe's buffer must not deadlock against a
        // hook that reads its stdin late; killing the group on any way out closes the pipe under it.
        let writer = child.stdin.take().map(|mut stdin| {
            let text = payload.to_owned();
            tokio::spawn(async move {
                let _ = stdin.write_all(text.as_bytes()).await;
            })
        });
        let out = Arc::new(Mutex::new(Capture::keep_head(MAX_FOREGROUND_BYTES)));
        let err = Arc::new(Mutex::new(Capture::keep_head(MAX_FOREGROUND_BYTES)));
        let mut out_reader = collect_from(child.stdout.take(), Arc::clone(&out));
        let mut err_reader = collect_from(child.stderr.take(), Arc::clone(&err));
        let waited = tokio::time::timeout(limit, async {
            loop {
                tokio::select! {
                    status = child.wait() => break status,
                    () = tokio::time::sleep(STOP_POLL) => {
                        if stop.load(Ordering::SeqCst) {
                            group.0.kill();
                        }
                    }
                }
            }
        })
        .await;
        let (status, timed_out) = match waited {
            Ok(Ok(status)) => (Some(status), false),
            Ok(Err(_)) => (None, false),
            Err(_) => (None, true),
        };
        if status.is_none() {
            group.0.kill();
            let _ = child.wait().await;
        }
        drop(group);
        for reader in [&mut out_reader, &mut err_reader] {
            if tokio::time::timeout(DRAIN_GRACE, &mut *reader)
                .await
                .is_err()
            {
                reader.abort();
            }
        }
        if let Some(writer) = writer {
            let _ = writer.await;
        }
        let mut stderr = err
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .text();
        if let Some(refused) = settled(watch).await {
            stderr.push_str(&refused);
        }
        Ok(HookOutput {
            code: status.and_then(|status| status.code()),
            stdout: out
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .text(),
            stderr,
            timed_out,
        })
    }

    /// Takes the folder a command ended in as the next command's, if it lies inside the run's folder; otherwise
    /// returns to the run's folder and says so. No note (the command ended with `exit`) keeps the folder as it was.
    fn carry_folder(&self, note: &Path) -> Option<String> {
        // Empty: made for the Linux sandbox's mount, never written (the command ended with `exit`).
        let ended = std::fs::read(note).ok().filter(|ended| !ended.is_empty())?;
        let ended = PathBuf::from(std::ffi::OsStr::from_bytes(
            ended.strip_suffix(b"\n").unwrap_or(&ended),
        ));
        let mut cwd = self.cwd.lock().unwrap_or_else(|poison| poison.into_inner());
        match ended.canonicalize() {
            Ok(path) if path.starts_with(&self.root) => {
                *cwd = path;
                None
            }
            _ => {
                *cwd = self.root.clone();
                Some(format!(
                    "\n[the folder {} is outside the working folder; the next command starts in {} again]",
                    ended.display(),
                    self.root.display()
                ))
            }
        }
    }

    /// Starts `command` as a background job (S6) and returns its id.
    async fn start_job(&self, command: &str, sandbox: Sandboxing) -> Result<String, String> {
        let full = || format!("{MAX_JOBS} jobs are running already; end one with kill_shell first");
        if self.running_jobs() >= MAX_JOBS {
            return Err(full());
        }
        let (cwd, _) = self.start_dir();
        let (mut process, watch) = self
            .command(command, &cwd, None, sandbox, &command_script(self.family))
            .await?;
        let mut child = process
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", self.shell.display()))?;
        let group = Group::of(&child);
        let output = Arc::new(Mutex::new(Capture::keep_tail(MAX_JOB_BYTES)));
        let ended = Arc::new(Mutex::new(None));
        let mut reader = collect(&mut child, Arc::clone(&output));
        let (waiter_group, done) = (group.clone(), Arc::clone(&ended));
        tokio::spawn(async move {
            let status = child.wait().await;
            // What the job left behind goes now, and the group id is forgotten once its leader is reaped.
            waiter_group.finish();
            if tokio::time::timeout(DRAIN_GRACE, &mut reader)
                .await
                .is_err()
            {
                reader.abort();
            }
            let mut text = match status {
                Ok(status) => describe_exit(status),
                Err(err) => format!("could not be waited for: {err}"),
            };
            if let Some(refused) = settled(watch).await {
                text.push('\n');
                text.push_str(refused.trim_end());
            }
            *done.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(text);
        });
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        // Checked again under the lock: two calls at once both passed the first look.
        if jobs.iter().filter(|job| job.is_running()).count() >= MAX_JOBS {
            group.kill();
            return Err(full());
        }
        // Ended jobs are kept for a last look, but not without end.
        while jobs.len() >= MAX_KEPT_JOBS {
            match jobs.iter().position(|job| !job.is_running()) {
                Some(oldest) => {
                    jobs.remove(oldest);
                }
                None => break,
            }
        }
        let id = format!("bg{}", self.next_job.fetch_add(1, Ordering::Relaxed));
        jobs.push(Job {
            id: id.clone(),
            command: command.to_owned(),
            group,
            output,
            read: 0,
            ended,
        });
        Ok(id)
    }

    fn running_jobs(&self) -> usize {
        let jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        jobs.iter().filter(|job| job.is_running()).count()
    }

    /// What job `id` wrote since the last look, and whether it still runs.
    fn job_output(&self, id: &str) -> Result<String, String> {
        let mut jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let job = jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("there is no job {id:?}"))?;
        let ended = job
            .ended
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        let (fresh, upto) = job
            .output
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .since(job.read, ended.is_some());
        job.read = upto;
        let state = match ended {
            Some(ending) => format!("[{id} ended: {ending}]"),
            None => format!("[{id} is still running: {}]", job.command),
        };
        Ok(if fresh.is_empty() {
            format!("(no new output)\n{state}")
        } else {
            format!("{fresh}\n{state}")
        })
    }

    /// Ends job `id` with its children.
    fn kill_job(&self, id: &str) -> Result<String, String> {
        let jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let job = jobs
            .iter()
            .find(|job| job.id == id)
            .ok_or_else(|| format!("there is no job {id:?}"))?;
        if !job.is_running() {
            return Ok(format!("{id} had ended already"));
        }
        job.group.kill();
        Ok(format!("{id} was stopped"))
    }
}

impl Job {
    fn is_running(&self) -> bool {
        self.ended
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_none()
    }
}

impl Drop for Session {
    /// Every job ends with the run (S6), and the scratch folder goes. A job that ended has no group id any more, so
    /// nothing else is hit.
    fn drop(&mut self) {
        let jobs = self
            .jobs
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        for job in jobs.iter() {
            job.group.kill();
        }
        let _ = std::fs::remove_dir_all(&self.scratch);
    }
}

/// A command's process group, killed whole so its children go with it.
///
/// Shared by whoever may stop the command and the task that reaps its leader. Once the leader is reaped the id is
/// forgotten ([`Group::finish`]): an id kept after that could later name another process group of the owner's.
/// Not a fence: a child that leaves the group (`setsid`, a daemon) is out of reach — the sandbox of HS3 is the place
/// for that.
#[derive(Clone)]
struct Group(Arc<Mutex<Option<Pid>>>);

impl Group {
    fn of(child: &Child) -> Self {
        // The child leads its own group (`process_group(0)`), so its pid is the group's id.
        let pid = child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(Pid::from_raw);
        Group(Arc::new(Mutex::new(pid)))
    }

    /// Kills the group, if it is still known.
    fn kill(&self) {
        if let Some(pid) = *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) {
            // Already gone is fine: the group ended by itself.
            let _ = kill_process_group(pid, Signal::KILL);
        }
    }

    /// Kills what is left of the group and forgets its id — right after its leader was reaped. While a member of the
    /// group lives, the id cannot be given to another process, so this kill still hits only the command's own.
    fn finish(&self) {
        if let Some(pid) = self
            .0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .take()
        {
            let _ = kill_process_group(pid, Signal::KILL);
        }
    }
}

/// Finishes its group when dropped, whichever way the code holding it ends.
struct GroupGuard(Group);

impl Drop for GroupGuard {
    fn drop(&mut self) {
        self.0.finish();
    }
}

/// Reads a child's output into `output` until the pipe closes.
fn collect(child: &mut Child, output: Arc<Mutex<Capture>>) -> tokio::task::JoinHandle<()> {
    collect_from(child.stdout.take(), output)
}

/// The same for either pipe: a hook's stdout and stderr are drained beside each other, kept apart.
fn collect_from<R>(pipe: Option<R>, output: Arc<Mutex<Capture>>) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let Some(mut pipe) = pipe else { return };
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk).await {
            if n == 0 {
                break;
            }
            output
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push(&chunk[..n]);
        }
    })
}

/// A capped output buffer: the first bytes (a hook), the first and the latest ones (a foreground command — its end is
/// where a build or a test run reports), or the latest ones (a job), and how many were written in all.
struct Capture {
    data: Vec<u8>,
    cap: usize,
    keep_tail: bool,
    /// With [`Capture::keep_ends`]: the latest bytes once `data` holds its half of the cap.
    tail: Option<Vec<u8>>,
    total: u64,
}

impl Capture {
    fn keep_head(cap: usize) -> Self {
        Capture {
            data: Vec::new(),
            cap,
            keep_tail: false,
            tail: None,
            total: 0,
        }
    }

    /// Keeps the first half of `cap` and, after it, the latest bytes (between half of `cap` and `cap` of them).
    fn keep_ends(cap: usize) -> Self {
        Capture {
            data: Vec::new(),
            cap: cap / 2,
            keep_tail: false,
            tail: Some(Vec::new()),
            total: 0,
        }
    }

    /// Keeps between `cap` and twice `cap` of the latest bytes: trimming in steps of `cap` rather than on every chunk
    /// keeps a chatty job from moving a megabyte per read.
    fn keep_tail(cap: usize) -> Self {
        Capture {
            data: Vec::new(),
            cap,
            keep_tail: true,
            tail: None,
            total: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        if self.keep_tail {
            self.data.extend_from_slice(bytes);
            trim_front(&mut self.data, self.cap);
        } else {
            let room = self.cap.saturating_sub(self.data.len());
            let mut taken = bytes.len().min(room);
            // The head ends between characters; a character it has no room for goes to the tail whole.
            while taken > 0 && taken < bytes.len() && is_continuation(bytes[taken]) {
                taken -= 1;
            }
            self.data.extend_from_slice(&bytes[..taken]);
            if let Some(tail) = &mut self.tail {
                tail.extend_from_slice(&bytes[taken..]);
                trim_front(tail, self.cap);
            }
        }
    }

    /// Everything kept, with a note where something was dropped.
    fn text(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.data).into_owned();
        let tail = self.tail.as_deref().unwrap_or_default();
        let dropped = self.total - (self.data.len() + tail.len()) as u64;
        if self.tail.is_some() {
            if dropped > 0 {
                text.push_str(&format!(
                    "\n[… {dropped} {} …]\n",
                    super::output::DROPPED_MIDDLE
                ));
            }
            let mut from = 0;
            while from < tail.len() && is_continuation(tail[from]) {
                from += 1;
            }
            text.push_str(&String::from_utf8_lossy(&tail[from..]));
        } else if dropped > 0 {
            text.push_str(&format!("\n[{dropped} more bytes of output were not kept]"));
        }
        text
    }

    /// What was written after the first `seen` bytes (as far as it is still kept), and how far that reaches. Unless
    /// `complete`, a character whose bytes are not all there yet is left for the next look rather than shown broken.
    fn since(&self, seen: u64, complete: bool) -> (String, u64) {
        let first_kept = self.total - self.data.len() as u64;
        let mut text = String::new();
        let mut from = if seen < first_kept {
            text.push_str(&format!(
                "[{} bytes of output were not kept]\n",
                first_kept - seen
            ));
            0
        } else {
            usize::try_from(seen - first_kept)
                .unwrap_or(self.data.len())
                .min(self.data.len())
        };
        while from < self.data.len() && is_continuation(self.data[from]) {
            from += 1;
        }
        let end = if complete {
            self.data.len()
        } else {
            self.data.len() - incomplete_tail(&self.data[from..])
        };
        text.push_str(&String::from_utf8_lossy(&self.data[from..end]));
        (text, first_kept + end as u64)
    }
}

/// Keeps between `cap` and twice `cap` of the latest bytes: trimming in steps of `cap` rather than on every chunk keeps
/// a chatty command from moving a megabyte per read.
fn trim_front(data: &mut Vec<u8>, cap: usize) {
    if data.len() > 2 * cap {
        let mut cut = data.len() - cap;
        // Never into the middle of a character.
        while cut < data.len() && is_continuation(data[cut]) {
            cut += 1;
        }
        data.drain(..cut);
    }
}

/// Whether `byte` continues a UTF-8 character rather than starting one.
fn is_continuation(byte: u8) -> bool {
    byte & 0b1100_0000 == 0b1000_0000
}

/// How many bytes at the end of `bytes` begin a UTF-8 character whose other bytes have not arrived yet.
fn incomplete_tail(bytes: &[u8]) -> usize {
    /// The longest UTF-8 character, in bytes.
    const MAX_CHAR_BYTES: usize = 4;
    for back in 1..=MAX_CHAR_BYTES.min(bytes.len()) {
        let byte = bytes[bytes.len() - back];
        if is_continuation(byte) {
            continue;
        }
        let needed = match byte {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        };
        return if needed > back { back } else { 0 };
    }
    0
}

fn describe_exit(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exit code {code}"),
        (None, Some(signal)) => format!("killed by signal {signal}"),
        (None, None) => "ended".into(),
    }
}

/// The folder every session's scratch folder lies in, resolved: one for all, so the sandbox keeps a command out of
/// every session's — one that wrote another session's profile snapshot would run its code in that session's next
/// command, which may run without the sandbox or with other rights. Made by Glut before any command runs; one that
/// is a link, another user's or open to others is not taken.
fn scratch_home() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let uid = rustix::process::geteuid().as_raw();
    let home = std::env::temp_dir().join(format!("glut-shells-{uid}"));
    match std::fs::DirBuilder::new().mode(0o700).create(&home) {
        Err(err) if err.kind() != std::io::ErrorKind::AlreadyExists => return Err(err),
        _ => {}
    }
    let meta = std::fs::symlink_metadata(&home)?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err(std::io::Error::other(format!(
            "{} is not Glut's own folder (a link, another user's, or open to others)",
            home.display()
        )));
    }
    home.canonicalize()
}

/// A new private folder for the session's files, in [`scratch_home`]. Its name carries the time as well as the pid
/// and a counter, so a folder a crashed run left behind under the same pid does not stand in the way.
fn scratch_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = scratch_home()?.join(format!(
        "glut-shell-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    // Resolved, as Seatbelt sees it (`/private/var/…`): the sandbox keeps commands from writing it.
    dir.canonicalize()
}

fn parse<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|err| format!("bad arguments: {err}"))
}

#[derive(Deserialize)]
struct BashArgs {
    command: String,
    timeout_secs: Option<u64>,
    #[serde(default)]
    run_in_background: bool,
    /// Run outside the sandbox (HS3b) — always asked.
    #[serde(default)]
    dangerously_disable_sandbox: bool,
}

#[derive(Deserialize)]
struct JobArgs {
    id: String,
}

struct Bash(Arc<Session>);

impl Tool for Bash {
    fn spec(&self) -> ToolSpec {
        let shell = self.0.shell.display().to_string();
        let sandbox = if self.0.policy.is_some() {
            " Commands run in a sandbox: they write only in the working folder, the temporary folders and the package \
             caches, read no secrets (keys, credentials, .env files) and reach the network only through the run's \
             proxy, which allows a domain only by the owner's rules. If a command fails because of the sandbox and \
             truly needs more, run it again with dangerously_disable_sandbox — the owner is asked every time."
        } else {
            ""
        };
        ToolSpec {
            name: "bash".into(),
            description: format!(
                "Runs a command in {shell} and answers its output (stdout and stderr) and exit code. The folder a \
                 command ends in is where the next one starts (inside the working folder); variables do not carry \
                 over. Time limit {} s unless timeout_secs says otherwise (at most {} s). Whatever a command leaves \
                 running (`server &`) is stopped when it ends: for a server or a watcher, set run_in_background and \
                 read it later with bash_output.{sandbox}",
                DEFAULT_TIMEOUT.as_secs(),
                MAX_TIMEOUT.as_secs()
            ),
            parameters: json!({"type": "object", "properties": {
                "command": {"type": "string"},
                "timeout_secs": {"type": "integer", "minimum": 1},
                "run_in_background": {"type": "boolean"},
                "dangerously_disable_sandbox": {"type": "boolean"}
            }, "required": ["command"]}),
        }
    }

    fn access(&self, arguments: &Value) -> Access {
        let (cwd, _) = self.0.start_dir();
        let folder = cwd
            .strip_prefix(&self.0.root)
            .map(|rel| rel.to_string_lossy().into_owned())
            .unwrap_or_default();
        let escape = arguments
            .get("dangerously_disable_sandbox")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Access::Bash {
            command: arguments
                .get("command")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            folder,
            // Without a sandbox there is nothing to leave: the rules alone judge, as before HS3b.
            unsandboxed: escape && self.0.policy.is_some(),
            sandboxed: self.0.policy.is_some() && !escape,
        }
    }

    fn here(&self) -> Option<PathBuf> {
        let cwd = self
            .0
            .cwd
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone();
        // A folder gone meanwhile: the command starts at the top (`Session::start_dir`).
        Some(if cwd.is_dir() {
            cwd
        } else {
            self.0.root.clone()
        })
    }

    fn back_to_top(&self) {
        *self
            .0
            .cwd
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()) = self.0.root.clone();
    }

    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_> {
        let stop = context.stop;
        Box::pin(async move {
            let args: BashArgs = parse(arguments)?;
            if args.command.trim().is_empty() {
                return Err("the command is empty".into());
            }
            // T74: the gate asked for a listed program (or a rule of the owner's allowed it) before this call ran —
            // by its own list, so what opens is what it judged. Without a gate nobody judged: it stays shut.
            let keychain = context
                .gate
                .as_ref()
                .and_then(|gate| gate.keychain_given(&args.command));
            let shut_keychain = keychain.is_none() && !self.0.keychain.is_open();
            // T74b: a listed program in a chain with other commands runs with it shut; a failure says why.
            let chained = keychain
                .is_none()
                .then(|| self.0.keychain.listed_in(&args.command))
                .flatten();
            let sandbox = if args.dangerously_disable_sandbox {
                Sandboxing::Off
            } else {
                Sandboxing::On(context.gate, keychain.is_some())
            };
            let sandboxed = matches!(sandbox, Sandboxing::On(..)) && self.0.policy.is_some();
            if args.run_in_background {
                let id = self.0.start_job(&args.command, sandbox).await?;
                return Ok(format!(
                    "started background job {id}; read it with bash_output, end it with kill_shell"
                ));
            }
            let limit = args
                .timeout_secs
                .map_or(DEFAULT_TIMEOUT, Duration::from_secs)
                .clamp(Duration::from_secs(1), MAX_TIMEOUT);
            if let Sandboxing::On(gate, _) = &sandbox
                && let Some((program, words)) = self.0.direct_call(&args.command, gate.as_deref())
            {
                return self.0.run_direct(program, &words, limit, &stop).await;
            }
            // Glut lean, N7: a chain runs with a mark before each part, so its output can be compacted part by part.
            // The gate judged the command as written; the marks change nothing it ran.
            let ran = if let Some(chain) = super::output::chain(&args.command) {
                let mark = self.0.mark();
                let marked = super::output::marked(&chain, &mark, self.0.family.builtin());
                let ran = self.0.run(&marked, limit, &stop, sandbox).await;
                unmarked(ran, &mark, &chain.parts, &context.report)
            } else {
                self.0.run(&args.command, limit, &stop, sandbox).await
            };
            // T74: a failure that reads like the shut Keychain gets Glut's note of how the owner opens it.
            match ran {
                Err(text) if sandboxed && shut_keychain => {
                    match crate::keychain::blocked_program(&args.command, &text) {
                        Some(program) => {
                            let note = match &chained {
                                Some(listed) => crate::keychain::chain_note(listed),
                                None => crate::keychain::blocked_note(&program),
                            };
                            Err(format!("{text}\n{note}"))
                        }
                        None => Err(text),
                    }
                }
                ran => ran,
            }
        })
    }
}

/// A marked chain's answer without its marks (N7), and the places of the parts that ran on `report` — none when the
/// marks cannot be trusted. An answer that is only marks is the empty answer a command without output gives.
fn unmarked(
    ran: Result<String, String>,
    mark: &str,
    parts: &[&str],
    report: &crate::tools::Report,
) -> Result<String, String> {
    let ok = ran.is_ok();
    let (text, ranges) = super::output::unmark(&ran.unwrap_or_else(|text| text), mark);
    if ok && text.trim().is_empty() {
        return Ok("(no output)".into());
    }
    let found: Option<Vec<crate::tools::Part>> = ranges.and_then(|ranges| {
        ranges
            .into_iter()
            .map(|(k, range)| {
                parts.get(k).map(|command| crate::tools::Part {
                    command: (*command).to_owned(),
                    range,
                })
            })
            .collect()
    });
    if let Some(found) = found {
        report.parts(found);
    }
    if ok { Ok(text) } else { Err(text) }
}

struct BashOutput(Arc<Session>);

impl Tool for BashOutput {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "bash_output".into(),
            description:
                "Reads what a background job wrote since the last look, and whether it still runs."
                    .into(),
            parameters: json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        }
    }

    fn access(&self, _arguments: &Value) -> Access {
        // Only the run's own jobs.
        Access::Own
    }

    fn offered_as(&self) -> Access {
        // A job is started by `bash`: offered with it, or not at all.
        Access::Bash {
            command: String::new(),
            folder: String::new(),
            unsandboxed: false,
            sandboxed: false,
        }
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        Box::pin(async move { self.0.job_output(&parse::<JobArgs>(arguments)?.id) })
    }
}

struct KillShell(Arc<Session>);

impl Tool for KillShell {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "kill_shell".into(),
            description: "Stops a background job and everything it started.".into(),
            parameters: json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
        }
    }

    fn access(&self, _arguments: &Value) -> Access {
        // Only the run's own jobs.
        Access::Own
    }

    fn offered_as(&self) -> Access {
        // A job is started by `bash`: offered with it, or not at all.
        Access::Bash {
            command: String::new(),
            folder: String::new(),
            unsandboxed: false,
            sandboxed: false,
        }
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        Box::pin(async move { self.0.kill_job(&parse::<JobArgs>(arguments)?.id) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Toolbox;

    /// A fresh folder with the shell tools of `shell` on it, without a profile, in an environment of `extra` plus
    /// the test process's `PATH` and `HOME`.
    fn setup(shell: &str, extra: &[(&str, &str)]) -> (PathBuf, Toolbox) {
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let mut ambient: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(key, _)| key == "PATH" || key == "HOME")
            .collect();
        ambient.extend(
            extra
                .iter()
                .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        );
        let options = ShellOptions {
            shell: PathBuf::from(shell),
            profile: false,
            ambient,
            sandbox: None,
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir, options).unwrap());
        (dir, tools)
    }

    async fn bash(tools: &Toolbox, command: &str) -> (bool, String) {
        tools
            .call("bash", &json!({"command": command}).to_string())
            .await
    }

    #[tokio::test]
    async fn a_sandboxed_command_finds_the_keychain_shut_with_a_note_and_open_when_the_owner_opened_it()
     {
        if !crate::sandbox::available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-keychain-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let keychain = crate::keychain::Keychain::default();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/bash"),
            profile: false,
            ambient: std::env::vars_os()
                .filter(|(key, _)| key == "PATH" || key == "HOME" || key == "TMPDIR")
                .collect(),
            sandbox: Some(Default::default()),
            direct: Vec::new(),
            beside: Default::default(),
            keychain: keychain.clone(),
            confinement: Default::default(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir, options).unwrap());
        // git's own Keychain helper, asked for a host nobody keeps a key for.
        let ask =
            "printf 'protocol=https\\nhost=example.invalid\\n\\n' | git credential-osxkeychain get";
        let (ok, text) = bash(&tools, ask).await;
        if text.contains("is not a git command") {
            return;
        }
        assert!(!ok && text.contains("-50"), "{text}");
        assert!(
            text.contains("/keychain add git credential-osxkeychain"),
            "Glut's note names how the owner opens it: {text}"
        );
        let (_, plain) = bash(&tools, "printf '[%s]' \"$GIT_CONFIG_KEY_0\"").await;
        assert!(plain.contains("[]"), "{plain}");
        keychain.set_session_open(true);
        let (ok, text) = bash(&tools, ask).await;
        assert!(ok && !text.contains("-50"), "{text}");
        // With the Keychain open git finds no bare repository by itself (T74b).
        let (_, guarded) = bash(
            &tools,
            "printf '[%s]' \"$GIT_CONFIG_KEY_0=$GIT_CONFIG_VALUE_0\"",
        )
        .await;
        assert!(
            guarded.contains("[safe.bareRepository=explicit]"),
            "{guarded}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// T61: a confined command writes its own folder and private home alone — not the shared temporary folder nor
    /// anything beside it — and finds its temporary files and caches sent there.
    #[tokio::test]
    async fn a_confined_command_writes_its_folder_and_private_home_alone() {
        if !crate::sandbox::available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-confined-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let root = dir.join("clone");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("beside")).unwrap();
        let confinement = crate::sandbox::Confinement::default();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/bash"),
            profile: false,
            ambient: std::env::vars_os()
                .filter(|(key, _)| key == "PATH" || key == "HOME" || key == "TMPDIR")
                .collect(),
            sandbox: Some(Default::default()),
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: confinement.clone(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&root, options).unwrap());
        let beside = dir.join("beside/x").display().to_string();
        let (_, open) = bash(&tools, &format!("echo x > '{beside}' && echo WROTE")).await;
        assert!(
            open.contains("WROTE"),
            "unconfined, the temporary folder is open: {open}"
        );
        confinement.confine(root.join(".git/glut-home"), vec![dir.join("beside")]);
        let (_, text) = bash(
            &tools,
            &format!(
                "echo x > '{beside}2' 2>/dev/null || echo BESIDE-REFUSED; \
                 echo x > own.txt && echo OWN; \
                 echo x > \"$TMPDIR/t\" && echo TMP; \
                 echo x > \"$CARGO_HOME/c\" && echo CACHE; \
                 rm -rf sub && mkdir -p sub/deep && rm -rf sub && echo REMOVED; \
                 case \"$TMPDIR\" in */.git/glut-home/tmp) echo PRIVATE;; esac"
            ),
        )
        .await;
        for said in [
            "BESIDE-REFUSED",
            "OWN",
            "TMP",
            "CACHE",
            "REMOVED",
            "PRIVATE",
        ] {
            assert!(text.contains(said), "{said}: {text}");
        }
        assert!(!dir.join("beside/x2").exists());
        // What it hides is not read; its home is neither moved nor replaced, and a home replaced runs nothing more.
        let (_, hidden) = bash(&tools, &format!("cat '{beside}' || echo READ-REFUSED")).await;
        assert!(hidden.contains("READ-REFUSED"), "{hidden}");
        let (_, moved) = bash(
            &tools,
            "mv .git/glut-home .git/elsewhere 2>/dev/null || echo MOVE-REFUSED",
        )
        .await;
        assert!(moved.contains("MOVE-REFUSED"), "{moved}");
        let home = root.join(".git/glut-home");
        std::fs::rename(&home, root.join(".git/kept")).unwrap();
        std::os::unix::fs::symlink(&dir, &home).unwrap();
        let (ok, swapped) = bash(&tools, "echo x > \"$TMPDIR/../../../escaped\"").await;
        assert!(!ok && swapped.contains("no longer the folder"), "{swapped}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_sandboxed_command_cannot_plant_code_in_the_scratch_folder_but_its_folder_carries_over()
     {
        if !crate::sandbox::available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-scratch-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/bash"),
            profile: false,
            ambient: std::env::vars_os()
                .filter(|(key, _)| key == "PATH" || key == "HOME" || key == "TMPDIR")
                .collect(),
            sandbox: Some(Default::default()),
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir, options).unwrap());
        // The snapshot every later command sources lies beside the note: neither it nor the folder may change.
        let (_, text) = bash(
            &tools,
            "d=$(dirname \"$AXH_CWD_FILE\"); echo 'gh() { :; }' > \"$d/snapshot.sh\" || echo WRITE-REFUSED; \
             mv \"$d\" \"$d.x\" || echo MOVE-REFUSED; rm -rf \"$d\" || echo REMOVE-REFUSED",
        )
        .await;
        for refused in ["WRITE-REFUSED", "MOVE-REFUSED", "REMOVE-REFUSED"] {
            assert!(text.contains(refused), "{refused}: {text}");
        }
        bash(&tools, "cd sub").await;
        let (ok, text) = bash(&tools, "pwd").await;
        assert!(ok && text.trim_end().ends_with("/sub"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn back_to_top_starts_the_next_command_in_the_runs_folder() {
        let (dir, tools) = setup("/bin/bash", &[]);
        bash(&tools, "cd sub").await;
        tools.back_to_top();
        let (ok, text) = bash(&tools, "pwd").await;
        assert!(ok && !text.trim_end().ends_with("/sub"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_command_answers_its_output_and_a_failure_its_exit_code() {
        let (_dir, tools) = setup("/bin/bash", &[]);
        assert_eq!(
            bash(&tools, "echo hi; echo oops >&2").await,
            (true, "hi\noops\n".into())
        );
        let (ok, text) = bash(&tools, "echo half; exit 3").await;
        assert!(!ok);
        assert_eq!(text, "error: half\n[exit code 3]");
        assert_eq!(bash(&tools, "true").await, (true, "(no output)".into()));
        let (ok, text) = bash(&tools, "  ").await;
        assert!(!ok && text.contains("empty"));
    }

    #[tokio::test]
    async fn a_chain_runs_as_written_and_reports_where_each_part_printed() {
        // The shells this system has: Linux often comes without zsh.
        for shell in ["/bin/bash", "/bin/zsh", "/bin/sh"]
            .into_iter()
            .filter(|shell| Path::new(shell).exists())
        {
            let (dir, tools) = setup(shell, &[]);
            std::fs::write(dir.join("sub").join("a.txt"), "a").unwrap();
            std::fs::create_dir(dir.join("sub").join("empty")).unwrap();
            let chain = |command: &'static str| {
                let tools = &tools;
                async move {
                    tools
                        .call_reporting(
                            "bash",
                            &json!({ "command": command }).to_string(),
                            &StopFlag::default(),
                        )
                        .await
                }
            };
            // `cd` in a part moves the folder for the parts after it, and for the next command.
            let called = chain("cd sub && ls && echo two").await;
            assert!(called.ok, "{shell}: {}", called.text);
            assert_eq!(called.text, "a.txt\nempty\ntwo\n", "{shell}");
            let parts = called.parts.expect("the parts are reported");
            let commands: Vec<_> = parts.iter().map(|p| p.command.as_str()).collect();
            assert_eq!(commands, ["cd sub", "ls", "echo two"], "{shell}");
            let pieces: Vec<_> = parts
                .iter()
                .map(|p| &called.text[p.range.clone()])
                .collect();
            assert_eq!(pieces, ["", "a.txt\nempty\n", "two\n"], "{shell}");
            assert!(
                bash(&tools, "pwd -P").await.1.ends_with("/sub\n"),
                "{shell}"
            );
            // `||` and `;` keep their meaning; a failed part's exit status decides as before.
            let called = chain("ls missing || echo fallback; echo after").await;
            assert!(called.ok, "{shell}: {}", called.text);
            assert!(
                called.text.ends_with("fallback\nafter\n"),
                "{shell}: {}",
                called.text
            );
            assert_eq!(called.parts.map(|p| p.len()), Some(3), "{shell}");
            // A part `||` passes over has no place; the parts after it keep theirs.
            let called = chain("ls || echo never; echo after").await;
            assert!(!called.text.contains("never"), "{shell}: {}", called.text);
            let commands: Vec<String> = called
                .parts
                .unwrap()
                .into_iter()
                .map(|p| p.command)
                .collect();
            assert_eq!(commands, ["ls", "echo after"], "{shell}");
            // A part that prints the command's environment shows it whole — the marked text holds no mark.
            let called = chain("ls && env").await;
            assert!(
                called.text.contains("AXH_COMMAND="),
                "{shell}: {}",
                called.text
            );
            assert!(
                called.text.contains("printf '%s-%s"),
                "{shell}: {}",
                called.text
            );
            assert_eq!(called.parts.map(|p| p.len()), Some(2), "{shell}");
            // A failed chain: the parts lie behind the error's prefix, the parts that never ran have none.
            let called = chain("ls missing && echo never").await;
            assert!(!called.ok, "{shell}");
            assert!(called.text.starts_with("error: "), "{}", called.text);
            assert!(!called.text.contains("never"), "{shell}: {}", called.text);
            let parts = called.parts.expect("the part that ran is reported");
            assert_eq!(parts.len(), 1, "{shell}");
            assert_eq!(
                parts[0].range,
                "error: ".len()..called.text.len(),
                "{shell}"
            );
            // A chain without output answers as a command without output does, and no mark is ever seen.
            let called = chain("cd . && ls -d sub/missing 2>/dev/null; true").await;
            assert!(
                !called.text.contains("glut-mark"),
                "{shell}: {}",
                called.text
            );
            let quiet = chain("cd empty && ls").await;
            assert_eq!(quiet.text, "(no output)", "{shell}");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[tokio::test]
    async fn the_folder_carries_over_but_never_outside_the_working_folder() {
        let (dir, tools) = setup("/bin/bash", &[]);
        let root = dir.canonicalize().unwrap();
        bash(&tools, "cd sub").await;
        assert_eq!(
            bash(&tools, "pwd -P").await.1,
            format!("{}\n", root.join("sub").display())
        );
        let (_, text) = bash(&tools, "cd /").await;
        assert!(text.contains("outside the working folder"), "{text}");
        assert_eq!(
            bash(&tools, "pwd -P").await.1,
            format!("{}\n", root.display())
        );
        // Variables do not carry over.
        bash(&tools, "export AXH_X=1").await;
        assert_eq!(bash(&tools, "echo \"[$AXH_X]\"").await.1, "[]\n");
    }

    #[tokio::test]
    async fn the_environment_is_cleaned_of_stray_variables() {
        let (_dir, tools) = setup("/bin/bash", &[("GLUT_API_KEY", "sk-secret"), ("LANG", "C")]);
        let (_, env) = bash(&tools, "env").await;
        assert!(!env.contains("sk-secret"), "{env}");
        assert!(env.contains("LANG=C"));
    }

    #[tokio::test]
    async fn a_timeout_kills_the_command_and_its_children() {
        let (dir, tools) = setup("/bin/bash", &[]);
        let pid_file = dir.join("child.pid");
        let command = format!("sleep 300 & echo $! > {}; wait", pid_file.display());
        let started = std::time::Instant::now();
        let (ok, text) = tools
            .call(
                "bash",
                &json!({"command": command, "timeout_secs": 1}).to_string(),
            )
            .await;
        assert!(!ok && text.contains("timed out after 1 s"), "{text}");
        assert!(started.elapsed() < Duration::from_secs(10));
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            rustix::process::test_kill_process(Pid::from_raw(pid).unwrap()).is_err(),
            "the child is gone"
        );
    }

    #[tokio::test]
    async fn a_background_job_is_read_and_stopped() {
        let (_dir, tools) = setup("/bin/bash", &[]);
        let (ok, text) = tools
            .call(
                "bash",
                &json!({"command": "echo one; sleep 0.2; echo two", "run_in_background": true})
                    .to_string(),
            )
            .await;
        assert!(ok && text.contains("bg1"), "{text}");
        tokio::time::sleep(Duration::from_millis(800)).await;
        let (_, text) = tools.call("bash_output", r#"{"id":"bg1"}"#).await;
        assert!(
            text.starts_with("one\ntwo\n") && text.contains("[bg1 ended: exit code 0]"),
            "{text}"
        );
        let (_, text) = tools.call("bash_output", r#"{"id":"bg1"}"#).await;
        assert!(text.starts_with("(no new output)"), "{text}");

        tools
            .call(
                "bash",
                &json!({"command": "sleep 300", "run_in_background": true}).to_string(),
            )
            .await;
        let (_, text) = tools.call("kill_shell", r#"{"id":"bg2"}"#).await;
        assert_eq!(text, "bg2 was stopped");
        tokio::time::sleep(Duration::from_millis(300)).await;
        let (_, text) = tools.call("bash_output", r#"{"id":"bg2"}"#).await;
        assert!(text.contains("bg2 ended: killed by signal 9"), "{text}");
        let (ok, _) = tools.call("bash_output", r#"{"id":"bg9"}"#).await;
        assert!(!ok);
    }

    #[tokio::test]
    async fn jobs_end_with_the_run() {
        let (dir, tools) = setup("/bin/bash", &[]);
        let pid_file = dir.join("job.pid");
        let command = format!("echo $$ > {}; sleep 300", pid_file.display());
        tools
            .call(
                "bash",
                &json!({"command": command, "run_in_background": true}).to_string(),
            )
            .await;
        tokio::time::sleep(Duration::from_millis(300)).await;
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        drop(tools);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(rustix::process::test_kill_process(Pid::from_raw(pid).unwrap()).is_err());
    }

    #[tokio::test]
    async fn the_profile_snapshot_brings_aliases_functions_and_path() {
        let shell = "/bin/zsh";
        if !Path::new(shell).is_file() {
            return;
        }
        let home = std::env::temp_dir().join(format!(
            "glut-home-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(
            home.join(".zshrc"),
            format!(
                "alias greet='echo hello'\nshout() {{ echo \"$1!\"; }}\nexport PATH=\"{}:$PATH\"\n",
                home.join("bin").display()
            ),
        )
        .unwrap();
        let dir = home.join("work");
        std::fs::create_dir_all(&dir).unwrap();
        let options = ShellOptions {
            shell: PathBuf::from(shell),
            profile: true,
            ambient: vec![
                ("HOME".into(), home.clone().into_os_string()),
                ("PATH".into(), "/usr/bin:/bin".into()),
            ],
            sandbox: None,
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir, options).unwrap());
        assert_eq!(bash(&tools, "greet").await.1, "hello\n");
        assert_eq!(bash(&tools, "shout hey").await.1, "hey!\n");
        let (_, path) = bash(&tools, "echo $PATH").await;
        assert!(
            path.starts_with(&home.join("bin").display().to_string()),
            "{path}"
        );
    }

    #[test]
    fn a_capture_keeps_the_latest_bytes_and_says_what_was_dropped() {
        // Up to twice the cap is kept; past it, the buffer is trimmed back to the cap.
        let mut tail = Capture::keep_tail(4);
        tail.push(b"abcdef");
        assert_eq!(tail.since(0, false), ("abcdef".into(), 6));
        tail.push(b"ghij");
        assert_eq!(
            tail.since(0, false),
            ("[6 bytes of output were not kept]\nghij".into(), 10)
        );
        tail.push(b"kl");
        assert_eq!(tail.since(10, false), ("kl".into(), 12));
        let mut head = Capture::keep_head(3);
        head.push(b"abcdef");
        assert_eq!(head.text(), "abc\n[3 more bytes of output were not kept]");
    }

    #[test]
    fn a_character_at_the_seam_of_head_and_tail_stays_whole() {
        let mut ends = Capture::keep_ends(8);
        ends.push("abcä".as_bytes());
        assert_eq!(ends.text(), "abcä");
        // Once the head is full, a character split over two chunks still arrives whole in the tail.
        let mut ends = Capture::keep_ends(8);
        ends.push(b"abcd");
        ends.push(&"ö".as_bytes()[..1]);
        ends.push(&"ö".as_bytes()[1..]);
        assert_eq!(ends.text(), "abcdö");
    }

    #[test]
    fn a_foreground_capture_keeps_the_start_and_the_end_where_a_build_reports() {
        let mut ends = Capture::keep_ends(8);
        ends.push(b"abcd");
        assert_eq!(ends.text(), "abcd");
        // Past the head's half, the latest bytes are kept (up to twice the other half), the middle is said.
        for chunk in [&b"efgh"[..], b"ijkl", b"mnop", b"qrst", b"E1"] {
            ends.push(chunk);
        }
        let text = ends.text();
        assert!(text.starts_with("abcd\n[… "), "{text}");
        assert!(text.ends_with("stE1"), "{text}");
        assert!(text.contains("bytes of output in the middle were not kept"));
    }

    #[test]
    fn a_split_character_waits_for_its_last_byte() {
        let mut capture = Capture::keep_tail(64);
        let umlaut = "ä".as_bytes();
        capture.push(b"a");
        capture.push(&umlaut[..1]);
        let (text, upto) = capture.since(0, false);
        assert_eq!(
            (text.as_str(), upto),
            ("a", 1),
            "the half character is held back"
        );
        capture.push(&umlaut[1..]);
        assert_eq!(capture.since(upto, false), ("ä".into(), 3));
        // A look that starts inside a character skips to the next whole one.
        assert_eq!(capture.since(2, true).0, "");
    }

    #[test]
    fn a_full_tail_trims_at_a_character_boundary() {
        // Nine bytes over a cap of three: the cut would land inside "ö" and moves on to "ü".
        let mut capture = Capture::keep_tail(3);
        capture.push("abcäöü".as_bytes());
        let (text, _) = capture.since(0, true);
        assert!(!text.contains('\u{fffd}'), "{text}");
        assert!(text.ends_with("\nü"), "{text}");
    }

    #[tokio::test]
    async fn a_dropped_call_takes_the_command_and_its_children_with_it() {
        let (dir, tools) = setup("/bin/bash", &[]);
        let pid_file = dir.join("dropped.pid");
        let command = format!("sleep 300 & echo $! > {}; wait", pid_file.display());
        let arguments = json!({"command": command}).to_string();
        let call = tools.call("bash", &arguments);
        assert!(
            tokio::time::timeout(Duration::from_millis(700), call)
                .await
                .is_err()
        );
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(rustix::process::test_kill_process(Pid::from_raw(pid).unwrap()).is_err());
    }

    #[tokio::test]
    async fn what_a_command_leaves_running_is_stopped_and_does_not_hold_the_call() {
        let (dir, tools) = setup("/bin/bash", &[]);
        let pid_file = dir.join("left.pid");
        let started = std::time::Instant::now();
        let (ok, text) = bash(
            &tools,
            &format!("sleep 300 & echo $! > {}; echo hi", pid_file.display()),
        )
        .await;
        assert!(ok && text == "hi\n", "{text}");
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: i32 = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(rustix::process::test_kill_process(Pid::from_raw(pid).unwrap()).is_err());
    }

    #[tokio::test]
    async fn a_stopped_run_stops_a_waiting_command_and_a_timeout_keeps_the_output_so_far() {
        let (_dir, tools) = setup("/bin/bash", &[]);
        let stop = StopFlag::default();
        let later = Arc::clone(&stop);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            later.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let (ok, text) = tools
            .call_until("bash", r#"{"command":"sleep 30"}"#, &stop)
            .await;
        assert!(!ok && text.contains("killed by signal 9"), "{text}");
        assert!(started.elapsed() < Duration::from_secs(5));

        let (_, text) = tools
            .call(
                "bash",
                &json!({"command": "echo before; sleep 30", "timeout_secs": 1}).to_string(),
            )
            .await;
        assert!(
            text.starts_with("error: before\n") && text.contains("timed out"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn a_ninth_job_is_refused() {
        let (_dir, tools) = setup("/bin/bash", &[]);
        for _ in 0..MAX_JOBS {
            let (ok, _) = tools
                .call(
                    "bash",
                    &json!({"command": "sleep 300", "run_in_background": true}).to_string(),
                )
                .await;
            assert!(ok);
        }
        let (ok, text) = tools
            .call(
                "bash",
                &json!({"command": "sleep 300", "run_in_background": true}).to_string(),
            )
            .await;
        assert!(!ok && text.contains("running already"), "{text}");
    }

    #[tokio::test]
    async fn a_folder_that_was_deleted_sends_the_next_command_home() {
        let (dir, tools) = setup("/bin/bash", &[]);
        bash(&tools, "cd sub").await;
        std::fs::remove_dir(dir.join("sub")).unwrap();
        let (ok, text) = bash(&tools, "pwd -P").await;
        assert!(ok, "{text}");
        assert!(
            text.contains("is gone")
                && text.ends_with(&format!("{}\n", dir.canonicalize().unwrap().display()))
        );
    }

    #[tokio::test]
    async fn a_shell_other_than_zsh_or_bash_runs_as_sh() {
        let (_dir, tools) = setup("/opt/homebrew/bin/fish", &[]);
        assert_eq!(bash(&tools, "echo ok").await, (true, "ok\n".into()));
        let (_dir, zsh) = setup("/bin/zsh", &[]);
        if Path::new("/bin/zsh").is_file() {
            assert_eq!(
                bash(&zsh, "echo ${ZSH_VERSION:+zsh}").await,
                (true, "zsh\n".into())
            );
        }
    }

    /// W8: a command the declaration names exactly starts the caller's program directly — outside the sandbox, its
    /// words as arguments, with the program's own environment; anything more than words, or a command no pattern
    /// names, stays a shell command in the sandbox.
    #[tokio::test]
    async fn a_declared_command_of_a_direct_program_runs_outside_the_sandbox() {
        use crate::permissions::{Mode, Nobody, Rule, RuleSet};
        if !crate::sandbox::available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-direct-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let outside =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/glut-shell-direct-outside");
        std::fs::create_dir_all(&outside).unwrap();
        let probe = outside
            .canonicalize()
            .unwrap()
            .join(format!("probe-{}", std::process::id()));
        let program = dir.join("fake-cli");
        std::fs::write(
            &program,
            "#!/bin/sh\necho \"args:$*|home:$AXIOMATA_HOME\"\necho x > \"$PROBE\"\n",
        )
        .unwrap();
        std::fs::set_permissions(
            &program,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let ambient: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(key, _)| key == "PATH" || key == "TMPDIR")
            .chain(std::iter::once((
                "HOME".into(),
                dir.clone().into_os_string(),
            )))
            .collect();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/bash"),
            profile: false,
            ambient,
            sandbox: Some(Default::default()),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
            direct: vec![DirectProgram {
                name: "axiomata-cli".into(),
                path: program,
                env: vec![
                    ("AXIOMATA_HOME".into(), "/scratch-home".into()),
                    ("PROBE".into(), probe.clone().into_os_string()),
                ],
                allowed: vec![vec!["board".into(), "list".into()]],
            }],
        };
        let declared = vec![
            Rule::parse("Bash(axiomata-cli board list:*)").unwrap(),
            Rule::parse("Bash(echo:*)").unwrap(),
        ];
        let mut gate = Gate::new(
            RuleSet {
                allow: declared.clone(),
                ..RuleSet::default()
            },
            Mode::Default,
            Arc::new(Nobody),
        );
        gate.narrow("the skill board", declared);
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir.join("work"), options).unwrap());
        tools.set_gate(Arc::new(gate));

        let (ok, text) = bash(&tools, "axiomata-cli board list --board '1'").await;
        assert!(ok, "{text}");
        assert_eq!(text, "args:board list --board 1|home:/scratch-home\n");
        assert!(probe.exists(), "it ran outside the sandbox");
        std::fs::remove_file(&probe).unwrap();

        let session = Session::new(
            &dir.join("work"),
            ShellOptions {
                shell: PathBuf::from("/bin/bash"),
                profile: false,
                ambient: Vec::new(),
                sandbox: None,
                beside: Default::default(),
                keychain: Default::default(),
                confinement: Default::default(),
                direct: vec![DirectProgram {
                    name: "axiomata-cli".into(),
                    path: PathBuf::from("/bin/echo"),
                    env: Vec::new(),
                    allowed: vec![vec!["board".into(), "list".into()]],
                }],
            },
        )
        .unwrap();
        let gate = tools.gate().cloned();
        assert!(
            session
                .direct_call("axiomata-cli board list", gate.as_deref())
                .is_some()
        );
        for shell_command in [
            "axiomata-cli board list > f",
            "axiomata-cli board list; echo x",
            "X=1 axiomata-cli board list",
            "axiomata-cli board list $(id)",
            "axiomata-cli board add --column 1 x",
            "/bin/axiomata-cli board list",
        ] {
            assert!(
                session
                    .direct_call(shell_command, gate.as_deref())
                    .is_none(),
                "{shell_command}"
            );
        }
        // A declaration as wide as the whole program reaches only the commands the program allows directly.
        let wide_rule = vec![Rule::parse("Bash(axiomata-cli:*)").unwrap()];
        let mut wide = Gate::new(
            RuleSet {
                allow: wide_rule.clone(),
                ..RuleSet::default()
            },
            Mode::Default,
            Arc::new(Nobody),
        );
        wide.narrow("the skill wide", wide_rule);
        assert!(
            session
                .direct_call("axiomata-cli board list", Some(&wide))
                .is_some()
        );
        assert!(
            session
                .direct_call("axiomata-cli files read x", Some(&wide))
                .is_none()
        );
        // Without a declaration (a chat turn) no command is trusted, whatever the mode allows.
        let open = Gate::new(RuleSet::default(), Mode::Auto, Arc::new(Nobody));
        assert!(
            session
                .direct_call("axiomata-cli board list", Some(&open))
                .is_none()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// T94: with `/sandbox required on` and no sandbox here (or the sandbox off), no command runs — the escape past
    /// it neither. Where the sandbox works, its settings switched off stand in for the missing one.
    #[tokio::test]
    async fn with_the_sandbox_required_and_none_here_no_command_runs() {
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-required-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("work")).unwrap();
        crate::sandbox::choices::Choices {
            git_read_only: false,
            required: true,
        }
        .save(&dir)
        .unwrap();
        let ambient: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(key, _)| key == "PATH")
            .chain(std::iter::once((
                "HOME".into(),
                dir.clone().into_os_string(),
            )))
            .collect();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/sh"),
            profile: false,
            ambient,
            sandbox: Some(crate::permissions::settings::SandboxSettings {
                enabled: false,
                ..Default::default()
            }),
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir.join("work"), options).unwrap());
        let (ok, text) = bash(&tools, "echo ran").await;
        assert!(
            !ok && text.contains("/sandbox required is on") && !text.contains("\nran"),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn a_sandboxed_command_cannot_write_outside_and_the_escape_runs_unsandboxed() {
        if !crate::sandbox::available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "glut-shell-sandbox-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("work")).unwrap();
        // Outside every writable place: the workspace's `target/`.
        let outside =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/glut-shell-sandbox-outside");
        std::fs::create_dir_all(&outside).unwrap();
        let target = outside
            .canonicalize()
            .unwrap()
            .join(format!("probe-{}", std::process::id()));
        let ambient: Vec<(OsString, OsString)> = std::env::vars_os()
            .filter(|(key, _)| key == "PATH" || key == "TMPDIR")
            .chain(std::iter::once((
                "HOME".into(),
                dir.clone().into_os_string(),
            )))
            .collect();
        let options = ShellOptions {
            shell: PathBuf::from("/bin/bash"),
            profile: false,
            ambient,
            sandbox: Some(Default::default()),
            direct: Vec::new(),
            beside: Default::default(),
            keychain: Default::default(),
            confinement: Default::default(),
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir.join("work"), options).unwrap());

        let (ok, text) = bash(&tools, &format!("echo x > {}", target.display())).await;
        // Seatbelt refuses the write; Linux's sandbox sees that place read-only.
        assert!(
            !ok && (text.contains("Operation not permitted")
                || text.contains("Read-only file system")),
            "{text}"
        );
        assert!(!target.exists());
        assert_eq!(
            bash(&tools, "echo inside > a.txt && cat a.txt").await,
            (true, "inside\n".into())
        );

        let escape = json!({"command": format!("echo x > {}", target.display()), "dangerously_disable_sandbox": true});
        let (ok, text) = tools.call("bash", &escape.to_string()).await;
        assert!(ok, "{text}");
        assert!(target.exists(), "outside the sandbox it ran");
        std::fs::remove_file(&target).unwrap();

        // The network goes through the run's proxy, which only sandboxed commands are pointed at.
        let (ok, text) = bash(&tools, "echo \"$HTTPS_PROXY|$no_proxy|\"").await;
        assert!(ok, "{text}");
        assert!(
            text.starts_with("http://127.0.0.1:") && text.ends_with("||\n"),
            "{text}"
        );
        let escape =
            json!({"command": "echo \"[$HTTPS_PROXY]\"", "dangerously_disable_sandbox": true});
        assert_eq!(
            tools.call("bash", &escape.to_string()).await,
            (true, "[]\n".into())
        );
    }
}
