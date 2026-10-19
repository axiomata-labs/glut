//! The shell tools (`docs/plans/harness.md`, HS2, S1–S6): `bash` runs a command in the owner's shell, in the
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
    let b = family.builtin();
    format!(
        "exec 2>&1\n{aliases}\
         if [ -n \"$AXH_SNAPSHOT\" ]; then . \"$AXH_SNAPSHOT\" >/dev/null 2>&1; fi\n\
         {b}cd \"$AXH_CWD\" || exit 1\n\
         {b}eval \"$AXH_COMMAND\"\n\
         __axh_status=$?\n\
         if [ -n \"$AXH_CWD_FILE\" ]; then {b}pwd -P > \"$AXH_CWD_FILE\" 2>/dev/null; fi\n\
         exit $__axh_status\n"
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

/// Whether a command runs in the sandbox, and then with the gate its proxy asks about hosts.
enum Sandboxing {
    On(Option<Arc<Gate>>),
    /// The escape (`dangerously_disable_sandbox`), already asked for by the gate.
    Off,
}

/// A run's sandbox once its first command needs it.
struct Sandbox {
    proxy: Option<Proxy>,
    /// The Seatbelt profile, with the proxy's port as the one way to the network.
    profile: String,
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
        let sandbox = self
            .sandbox
            .get_or_init(|| async move {
                // Without a proxy a command still runs, it just reaches no network: the profile then opens no port.
                let proxy = Proxy::start(gate).await.ok();
                let mut policy = policy.clone();
                policy.proxy_port = proxy.as_ref().map(Proxy::port);
                Sandbox {
                    profile: policy.profile(),
                    proxy,
                }
            })
            .await;
        Some(sandbox)
    }

    /// A command ready to spawn: the shell running `script` for `command` in `cwd`.
    async fn command(
        &self,
        command: &str,
        cwd: &Path,
        cwd_file: Option<&Path>,
        sandbox: Sandboxing,
        script: &str,
    ) -> Command {
        let snapshot = self.snapshot().await;
        let mut proxy_env = Vec::new();
        let sandbox = match sandbox {
            Sandboxing::On(gate) => self.sandbox(gate).await,
            Sandboxing::Off => None,
        };
        let mut process = match sandbox {
            // The profile goes in as an argument: nothing on disk for a command to change between two calls.
            Some(sandbox) => {
                if let Some(proxy) = &sandbox.proxy {
                    proxy_env = proxy.environment();
                }
                let mut process = Command::new(crate::sandbox::SANDBOX_EXEC);
                process.arg("-p").arg(&sandbox.profile).arg(&self.shell);
                process
            }
            None => Command::new(&self.shell),
        };
        process
            .arg("-c")
            .arg(script)
            .env_clear()
            .envs(self.env.iter().cloned())
            .envs(proxy_env)
            .env("AXH_COMMAND", command)
            .env("AXH_CWD", cwd)
            .env("AXH_SNAPSHOT", snapshot.as_deref().unwrap_or(Path::new("")))
            .env("AXH_CWD_FILE", cwd_file.unwrap_or(Path::new("")))
            .current_dir(&self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .kill_on_drop(true);
        process
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
        let mut child = self
            .command(
                command,
                &cwd,
                Some(&note),
                sandbox,
                &command_script(self.family),
            )
            .await
            .spawn()
            .map_err(|err| format!("could not start {}: {err}", self.shell.display()))?;
        // Dropped with this future (the run timed out or was stopped while waiting here): the group goes with it.
        let group = GroupGuard(Group::of(&child));
        let output = Arc::new(Mutex::new(Capture::keep_head(MAX_FOREGROUND_BYTES)));
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
            && !analysis.protected
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
    ) -> Result<String, String> {
        let (cwd, _) = self.start_dir();
        let child = Command::new(&program.path)
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
        let output = tokio::time::timeout(limit, child.wait_with_output())
            .await
            .map_err(|_| {
                format!(
                    "[timed out after {} s; {} was stopped]",
                    limit.as_secs(),
                    program.name
                )
            })?
            .map_err(|err| format!("[{} could not be waited for: {err}]", program.name))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        if text.len() > MAX_FOREGROUND_BYTES {
            let mut end = MAX_FOREGROUND_BYTES;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            text.truncate(end);
            text.push_str("\n[output cut]");
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
        let mut process = self
            .command(
                command,
                &cwd,
                None,
                Sandboxing::On(gate),
                &hook_script(self.family),
            )
            .await;
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
        Ok(HookOutput {
            code: status.and_then(|status| status.code()),
            stdout: out
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .text(),
            stderr: err
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .text(),
            timed_out,
        })
    }

    /// Takes the folder a command ended in as the next command's, if it lies inside the run's folder; otherwise
    /// returns to the run's folder and says so. No note (the command ended with `exit`) keeps the folder as it was.
    fn carry_folder(&self, note: &Path) -> Option<String> {
        let ended = std::fs::read(note).ok()?;
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
        let mut child = self
            .command(command, &cwd, None, sandbox, &command_script(self.family))
            .await
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
            let text = match status {
                Ok(status) => describe_exit(status),
                Err(err) => format!("could not be waited for: {err}"),
            };
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

/// A capped output buffer: the first bytes (a foreground command) or the latest ones (a job), and how many were
/// written in all.
struct Capture {
    data: Vec<u8>,
    cap: usize,
    keep_tail: bool,
    total: u64,
}

impl Capture {
    fn keep_head(cap: usize) -> Self {
        Capture {
            data: Vec::new(),
            cap,
            keep_tail: false,
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
            total: 0,
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.total += bytes.len() as u64;
        if self.keep_tail {
            self.data.extend_from_slice(bytes);
            if self.data.len() > 2 * self.cap {
                let mut cut = self.data.len() - self.cap;
                // Never into the middle of a character.
                while cut < self.data.len() && is_continuation(self.data[cut]) {
                    cut += 1;
                }
                self.data.drain(..cut);
            }
        } else {
            let room = self.cap.saturating_sub(self.data.len());
            self.data.extend_from_slice(&bytes[..bytes.len().min(room)]);
        }
    }

    /// Everything kept, with a note when something was dropped.
    fn text(&self) -> String {
        let mut text = String::from_utf8_lossy(&self.data).into_owned();
        let dropped = self.total - self.data.len() as u64;
        if dropped > 0 {
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

/// A new private folder for the session's files. Its name carries the time as well as the pid and a counter, so a
/// folder a crashed run left behind under the same pid does not stand in the way.
fn scratch_dir() -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "axh-shell-{}-{nanos}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
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
        }
    }

    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_> {
        let stop = context.stop;
        Box::pin(async move {
            let args: BashArgs = parse(arguments)?;
            if args.command.trim().is_empty() {
                return Err("the command is empty".into());
            }
            let sandbox = if args.dangerously_disable_sandbox {
                Sandboxing::Off
            } else {
                Sandboxing::On(context.gate)
            };
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
            if let Sandboxing::On(gate) = &sandbox
                && let Some((program, words)) = self.0.direct_call(&args.command, gate.as_deref())
            {
                return self.0.run_direct(program, &words, limit).await;
            }
            self.0.run(&args.command, limit, &stop, sandbox).await
        })
    }
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
            "axh-shell-test-{}-{}",
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
        let (_dir, tools) = setup("/bin/bash", &[("AXH_API_KEY", "sk-secret"), ("LANG", "C")]);
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
            "axh-home-{}-{}",
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
            "axh-shell-direct-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let outside =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/axh-shell-direct-outside");
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

    #[tokio::test]
    async fn a_sandboxed_command_cannot_write_outside_and_the_escape_runs_unsandboxed() {
        if !crate::sandbox::available() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "axh-shell-sandbox-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("work")).unwrap();
        // Outside every writable place: the workspace's `target/`.
        let outside =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/axh-shell-sandbox-outside");
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
        };
        let mut tools = Toolbox::new();
        tools.extend(shell_tools(&dir.join("work"), options).unwrap());

        let (ok, text) = bash(&tools, &format!("echo x > {}", target.display())).await;
        assert!(!ok && text.contains("Operation not permitted"), "{text}");
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
