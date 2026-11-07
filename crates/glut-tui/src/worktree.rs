//! A pane's own worktree (T36a): made beside the repository for a second pane, and at its
//! end kept, taken over (merged into the branch the repository's folder was on) or discarded. Git runs only for what
//! the owner picked, never for the model — and as the owner, outside the sandbox, in a repository a model may have
//! worked in. So every call is pinned to the repository's own git folder (a `commondir` or `.git` file cannot point
//! it elsewhere), its hooks, file monitor, maintenance and network are off, and a repository whose own config could
//! make git run a program — a filter, a merge driver, a hook in config, `gpg.program`, an include — is refused before
//! git touches anything (security audit, 2026-10-24). A take-over never brings in what the sandbox keeps the model from
//! writing (`.envrc`, `.claude`, …): such a branch is the owner's to merge by hand.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Where the worktrees live, below the home folder: out of the project and out of iCloud's `~/Documents`, whose
/// conflict copies ("<name> 2.ext") a build there would breed.
const DIR: &str = ".glut/worktrees";
/// The most of git's error shown: its first lines say what was in the way.
const SAID_LINES: usize = 4;
/// How long a reading git call may take before it is ended: the screen waits for it.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How long a changing one may take (a checkout, a merge that signs): ending it midway leaves a lock file.
const TIMEOUT_CHANGING: Duration = Duration::from_secs(120);
/// How long git is given to end on its own once asked to, before it is killed.
const GRACE: Duration = Duration::from_secs(3);
/// The most of git's output read: a status of a tree with countless new files is not read whole.
const MOST_OUTPUT: u64 = 1024 * 1024;
/// The environment that would point git elsewhere than where Glut pins it, or bring config past the preflight.
const GIT_PLACES: [&str; 15] = [
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_CONFIG_PARAMETERS",
    "GIT_CONFIG_COUNT",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_SYSTEM",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_EXTERNAL_DIFF",
    "GIT_EXEC_PATH",
    "GIT_NAMESPACE",
    "GIT_REPLACE_REF_BASE",
];
/// The repository-config keys that run nothing, by section and name (`*` any subsection): anything else in the
/// repository's own config (or a worktree's) refuses the worktree — an allow list, since filters, drivers and hooks
/// come under names of the config's choosing.
const SAFE_KEYS: [&str; 41] = [
    "core.repositoryformatversion",
    "core.filemode",
    "core.bare",
    "core.logallrefupdates",
    "core.ignorecase",
    "core.precomposeunicode",
    "core.symlinks",
    "core.autocrlf",
    "core.eol",
    "core.safecrlf",
    "core.quotepath",
    "core.checkstat",
    "core.trustctime",
    "remote.*.url",
    "remote.*.pushurl",
    "remote.*.fetch",
    "remote.*.push",
    "remote.*.tagopt",
    "remote.*.prune",
    "remote.*.mirror",
    "branch.*.remote",
    "branch.*.merge",
    "branch.*.rebase",
    "branch.*.pushremote",
    "branch.*.description",
    "user.name",
    "user.email",
    "user.signingkey",
    "init.defaultbranch",
    "pull.rebase",
    "pull.ff",
    "push.default",
    "push.autosetupremote",
    "fetch.prune",
    "commit.gpgsign",
    "tag.gpgsign",
    "extensions.objectformat",
    "extensions.worktreeconfig",
    "submodule.*.active",
    "submodule.*.url",
    "submodule.*.branch",
];
/// Names a merged branch must not bring into the owner's folder at any depth, in any case: git's own attributes,
/// submodules and lfs settings (a filter or a source of the branch's choosing), and direnv's file.
const ANY_DEPTH: [&str; 4] = [".gitattributes", ".gitmodules", ".lfsconfig", ".envrc"];
/// Beside the sandbox's own list, more first names of other tools' settings that run things.
const MORE_ROOTS: [&str; 8] = [
    ".githooks",
    ".pre-commit-config.yaml",
    ".idea",
    ".zed",
    ".cursor",
    ".gemini",
    ".codex",
    ".devcontainer",
];

/// A repository as Glut pins it: its main folder and its common git folder (`<top>/.git`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub top: PathBuf,
    pub common: PathBuf,
}

/// A pane's worktree: where it is, its branch, its own git folder, the repository it belongs to, and the branch that
/// repository's folder was on when it was made (where a take-over merges — `None` on a detached head).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub place: PathBuf,
    pub branch: String,
    pub git_dir: PathBuf,
    pub repo: Repo,
    pub into: Option<String>,
    /// An arena's clone of the repository ([`clone_for_arena`]), not a worktree of it: its git folder is its own.
    pub cloned: bool,
}

/// What the closing pane's worktree holds that the repository's branch does not.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Standing {
    /// Changed or new files not committed.
    pub uncommitted: usize,
    /// Commits on its branch the repository's branch has not.
    pub ahead: usize,
}

impl Standing {
    /// Whether discarding it loses work.
    pub fn loses_work(self) -> bool {
        self.uncommitted > 0 || self.ahead > 0
    }

    /// `3 commits, 2 uncommitted files` — or `nothing new`.
    pub fn said(self) -> String {
        let texts = crate::texts::t();
        let commits = |count: usize| texts.commits_count.of(count as u64);
        let files = |count: usize| texts.uncommitted_count.of(count as u64);
        match (self.ahead, self.uncommitted) {
            (0, 0) => tr!(nothing_new).to_owned(),
            (ahead, 0) => commits(ahead),
            (0, uncommitted) => files(uncommitted),
            (ahead, uncommitted) => tr!(
                two_counts,
                first = commits(ahead),
                second = files(uncommitted)
            ),
        }
    }
}

/// Which git folder a call works on: the repository's, or a worktree's own beside the common one.
struct Pin<'a> {
    work_tree: &'a Path,
    git_dir: &'a Path,
    common: &'a Path,
}

impl Repo {
    fn pin(&self) -> Pin<'_> {
        Pin {
            work_tree: &self.top,
            git_dir: &self.common,
            common: &self.common,
        }
    }
}

impl Tree {
    fn pin(&self) -> Pin<'_> {
        Pin {
            work_tree: &self.place,
            git_dir: &self.git_dir,
            common: if self.cloned {
                &self.git_dir
            } else {
                &self.repo.common
            },
        }
    }

    /// The pin for comparing two of its trees (T61): an arena clone's store as its own work tree, so no attribute the
    /// model wrote in its files (`* -diff`, which would show the owner no line of a change it still applies) bears
    /// on what Glut shows and takes.
    fn objects_pin(&self) -> Pin<'_> {
        if !self.cloned {
            return self.pin();
        }
        Pin {
            work_tree: &self.git_dir,
            git_dir: &self.git_dir,
            common: &self.git_dir,
        }
    }

    /// The private home of an arena's clone (T61): its temporary files and package caches, inside the copy's own git
    /// folder — beside its files, never among them — and gone with it.
    pub fn private_home(&self) -> PathBuf {
        self.place.join(".git").join(PRIVATE_HOME)
    }
}

/// Where in an arena clone's git folder its private home lies ([`Tree::private_home`]).
const PRIVATE_HOME: &str = "glut-home";

/// Git on the pinned folders, its hooks, file monitor, maintenance and network off, never asking, ended after
/// [`TIMEOUT`] (a changing call after [`TIMEOUT_CHANGING`]); its output, or its error's first lines.
fn git(pin: &Pin, args: &[&str]) -> Result<String, String> {
    let (command, limit) = pinned(pin, args);
    run(command, limit)
}

/// [`git`]'s output as git wrote it, untrimmed: a patch must keep its last line's end.
fn git_bytes(pin: &Pin, args: &[&str]) -> Result<Vec<u8>, String> {
    let (command, limit) = pinned(pin, args);
    run_bytes(command, limit, None)
}

/// [`git`] for an arena (T61), over a worktree a model wrote: no global or system config and no attributes file,
/// so a `.gitattributes` the model wrote names no filter or driver that exists; `input` on its standard input.
fn git_alone(pin: &Pin, args: &[&str], input: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let (command, limit) = alone(pin, args);
    run_bytes(command, limit, input)
}

/// [`git_alone`]'s command, and how long it may take.
fn alone(pin: &Pin, args: &[&str]) -> (Command, Duration) {
    let (mut command, limit) = pinned(pin, &[]);
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args(["-c", "core.attributesFile=/dev/null"])
        .args(args);
    let limit = match args.first() {
        Some(&("add" | "apply" | "checkout")) => TIMEOUT_CHANGING,
        _ => limit,
    };
    (command, limit)
}

/// [`git_alone`]'s output as text, trimmed.
fn git_alone_text(pin: &Pin, args: &[&str]) -> Result<String, String> {
    git_alone(pin, args, None).map(|out| String::from_utf8_lossy(&out).trim().to_owned())
}

/// Git's command on the pinned folders, and how long it may take.
fn pinned(pin: &Pin, args: &[&str]) -> (Command, Duration) {
    let mut command = Command::new("git");
    command.current_dir(pin.work_tree);
    scrub(&mut command);
    command
        .env("GIT_DIR", pin.git_dir)
        .env("GIT_COMMON_DIR", pin.common)
        .env("GIT_WORK_TREE", pin.work_tree)
        .args([
            "-c",
            "gc.auto=0",
            "-c",
            "maintenance.auto=false",
            "-c",
            "protocol.allow=never",
        ])
        .args(args);
    let changing = matches!(
        args.first(),
        Some(&("worktree" | "merge" | "commit" | "branch" | "add" | "apply"))
    );
    (command, if changing { TIMEOUT_CHANGING } else { TIMEOUT })
}

/// A git call outside any repository's pins: finding one, from a folder.
fn git_in(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.current_dir(dir);
    scrub(&mut command);
    command.args(args);
    run(command, TIMEOUT)
}

/// Clears what in the environment would point git elsewhere or add config, and sets what every call of Glut's has:
/// hooks and the file monitor off, never asking on the terminal, no lazy fetch, no optional locks, and history as it
/// is (`refs/replace` is writable to a model).
fn scrub(command: &mut Command) {
    for name in GIT_PLACES {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        let name = name.to_string_lossy();
        if name.starts_with("GIT_CONFIG_KEY_") || name.starts_with("GIT_CONFIG_VALUE_") {
            command.env_remove(name.as_ref());
        }
    }
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ]);
}

/// Runs `command` with its output bounded and its time limited: past `limit` git and what it started (a filter, gpg)
/// are asked to end, then killed — a lock file it leaves is named in the message.
fn run(command: Command, limit: Duration) -> Result<String, String> {
    run_bytes(command, limit, None).map(|out| String::from_utf8_lossy(&out).trim().to_owned())
}

/// [`run`]'s output as it came.
fn run_bytes(
    mut command: Command,
    limit: Duration,
    input: Option<Vec<u8>>,
) -> Result<Vec<u8>, String> {
    use std::os::unix::process::CommandExt as _;
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A group of its own: ending it ends what it started too.
        .process_group(0)
        .spawn()
        .map_err(|err| tr!(git_cannot_run, reason = err))?;
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        let (sent, got) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = (&mut pipe).take(MOST_OUTPUT + 1).read_to_end(&mut bytes);
                // The rest is drained, not kept: git must not wait on a full pipe.
                let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            }
            let _ = sent.send(bytes);
        });
        got
    };
    // What git reads on its standard input, written beside it: a patch larger than the pipe must not stall it.
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        std::thread::spawn(move || {
            use std::io::Write as _;
            let _ = stdin.write_all(&input);
        });
    }
    let out = read(child.stdout.take().map(|pipe| Box::new(pipe) as _));
    let err = read(child.stderr.take().map(|pipe| Box::new(pipe) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < limit => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                end_group(&mut child);
                return Err(tr!(git_too_long, secs = limit.as_secs()));
            }
        }
    };
    // A helper that kept the pipes open must not hold the screen: what came within the grace is what counts.
    let out = out.recv_timeout(GRACE).unwrap_or_default();
    let err = err.recv_timeout(GRACE).unwrap_or_default();
    if out.len() as u64 > MOST_OUTPUT {
        return Err(tr!(git_too_much).to_owned());
    }
    if status.success() {
        return Ok(out);
    }
    let said = String::from_utf8_lossy(&err);
    let said: Vec<&str> = said
        .lines()
        .filter(|line| !line.trim().is_empty())
        .take(SAID_LINES)
        .collect();
    Err(crate::transcript::clean(&said.join(" · ")))
}

/// Asks git's process group to end, and kills it after [`GRACE`].
fn end_group(child: &mut std::process::Child) {
    use rustix::process::{Pid, Signal, kill_process_group};
    let group = i32::try_from(child.id()).ok().and_then(Pid::from_raw);
    if let Some(group) = group {
        let _ = kill_process_group(group, Signal::TERM);
    }
    let asked = Instant::now();
    while asked.elapsed() < GRACE {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Some(group) = group {
        let _ = kill_process_group(group, Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// The repository holding `cwd`, pinned: `Ok(None)` outside one (or without git); an `Err` for one Glut does not make
/// worktrees of — its git folder is no plain `<top>/.git` folder.
pub fn repo_of(cwd: &Path) -> Result<Option<Repo>, String> {
    let Ok(common) = git_in(
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        return Ok(None);
    };
    let common = PathBuf::from(common);
    let odd = || {
        tr!(
            no_plain_git,
            folder = crate::transcript::clean(&common.display().to_string())
        )
    };
    let kind = std::fs::symlink_metadata(&common).map_err(|_| odd())?;
    if !kind.is_dir()
        || common.file_name().is_none_or(|name| name != ".git")
        || common.join("commondir").exists()
    {
        return Err(odd());
    }
    let canonical = common.canonicalize().map_err(|_| odd())?;
    let top = canonical.parent().ok_or_else(odd)?.to_owned();
    // Only from the repository's own folder: from a worktree a take-over would merge into a folder no pane shows.
    let here = git_in(
        cwd,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
    )
    .ok()
    .and_then(|path| PathBuf::from(path).canonicalize().ok());
    if here.as_deref() != Some(top.as_path()) {
        return Err(tr!(is_worktree).into());
    }
    let repo = Repo {
        top,
        common: canonical,
    };
    checked(&repo.pin())?;
    Ok(Some(repo))
}

/// Whether a repository-config key runs nothing.
fn safe_key(key: &str) -> bool {
    let key = key.to_lowercase();
    // A subsection may hold dots itself (`branch.v1.2.merge`): the section is the first part, the name the last.
    let (Some((section, rest)), Some((_, name))) = (key.split_once('.'), key.rsplit_once('.'))
    else {
        return false;
    };
    let subsection = rest.len() > name.len();
    SAFE_KEYS.iter().any(|safe| {
        let parts: Vec<&str> = safe.split('.').collect();
        match parts.as_slice() {
            [safe_section, safe_name] => {
                !subsection && *safe_section == section && *safe_name == name
            }
            [safe_section, "*", safe_name] => {
                subsection && *safe_section == section && *safe_name == name
            }
            _ => false,
        }
    })
}

/// The keys of the repository's own config (and a worktree's) that could make git run a program — none for a
/// repository Glut makes and merges worktrees in. Reading the config runs nothing.
fn unsafe_keys(pin: &Pin) -> Result<Vec<String>, String> {
    let listed = git(
        pin,
        &["config", "--show-scope", "--list", "-z", "--includes"],
    )?;
    let mut found: Vec<String> = Vec::new();
    let mut parts = listed.split('\0');
    while let (Some(scope), Some(entry)) = (parts.next(), parts.next()) {
        if !matches!(scope, "local" | "worktree") {
            continue;
        }
        let key = crate::transcript::clean(entry.split('\n').next().unwrap_or(entry));
        if !safe_key(&key) && !found.contains(&key) {
            found.push(key);
        }
    }
    Ok(found)
}

/// Refuses a repository whose own config could make git run a program.
fn checked(pin: &Pin) -> Result<(), String> {
    let found = unsafe_keys(pin)?;
    if found.is_empty() {
        return Ok(());
    }
    let shown: Vec<&str> = found.iter().take(3).map(String::as_str).collect();
    Err(format!(
        "this repository's own config has {} — git could run a program for it, so Glut makes and merges no \
         worktree here (/split here works in this folder)",
        shown.join(", ")
    ))
}

/// A branch name for a new pane's worktree no branch of `repo` has yet: `glut/pane-2`, `glut/pane-2-2`, …
pub fn proposed_branch(repo: &Repo, number: u8) -> String {
    let base = format!("glut/pane-{number}");
    let taken = |name: &str| {
        git(
            &repo.pin(),
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/heads/{name}"),
            ],
        )
        .is_ok()
    };
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| !taken(name))
        .unwrap_or(base)
}

/// Whether `branch` is a name Glut makes a worktree on: plain ASCII (no character that turns or hides text), and one
/// git takes.
fn plain_branch(repo: &Repo, branch: &str) -> Result<(), String> {
    // A leading `-` would reach git as an option (`worktree add -b` passes it on without `--`).
    let plain = !branch.is_empty()
        && !branch.starts_with('-')
        && branch != "@"
        && branch.chars().all(|c| c.is_ascii_graphic());
    if !plain
        || git(
            &repo.pin(),
            &["check-ref-format", &format!("refs/heads/{branch}")],
        )
        .is_err()
    {
        return Err(tr!(bad_branch, branch = crate::transcript::clean(branch)));
    }
    Ok(())
}

/// Makes a worktree of `repo` on the new branch `branch`, from where `repo`'s folder is, below `home`.
///
/// # Errors
///
/// A message when the repository's config could run a program, the name is no branch name, or git refuses (the
/// branch exists, the place is taken).
pub fn add(home: &Path, repo: &Repo, branch: &str) -> Result<Tree, String> {
    checked(&repo.pin())?;
    plain_branch(repo, branch)?;
    let name = repo
        .top
        .file_name()
        .map_or("repo".into(), |name| name.to_string_lossy());
    let dir = home.join(DIR);
    let place = dir.join(format!("{name}-{}", branch.replace('/', "-")));
    if place.exists() {
        return Err(tr!(there_already, place = place.display()));
    }
    {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
            .map_err(|err| format!("{}: {err}", dir.display()))?;
        // One made before keeps the owner's alone too.
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let into = git(&repo.pin(), &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
    let place_text = place.to_string_lossy().into_owned();
    git(
        &repo.pin(),
        &["worktree", "add", "-b", branch, "--", &place_text],
    )?;
    // The worktree's own git folder, read from the `.git` file just written: nothing else has touched it yet.
    let git_dir = git_in(
        &place,
        &["rev-parse", "--path-format=absolute", "--git-dir"],
    )
    .ok()
    .and_then(|dir| PathBuf::from(dir).canonicalize().ok())
    .filter(|dir| dir.starts_with(repo.common.join("worktrees")));
    let Some(git_dir) = git_dir else {
        let _ = git(
            &repo.pin(),
            &["worktree", "remove", "--force", "--", &place_text],
        );
        let _ = git(&repo.pin(), &["branch", "-D", branch]);
        return Err(tr!(worktree_misplaced).into());
    };
    Ok(Tree {
        place,
        branch: branch.to_owned(),
        git_dir,
        repo: repo.clone(),
        into,
        cloned: false,
    })
}

/// An arena model's own repository (T61): a copy of `repo`'s current branch (over `file://`: only what it reaches, no
/// `origin`, no borrowed objects — the owner's repository stays hidden from the model) on `branch` from its `HEAD`, with refs, index, stash and
/// history of its own. Glut's own git over it never uses that git folder, which the model writes: it works in a store
/// of Glut's beside it (`<place>.store`, a bare repository in `~/.glut`, which no sandboxed command reads), whose
/// objects borrow the owner's and whose index follows the model's files — so what Glut snapshots, shows and applies
/// is hashed by Glut alone, never read through a link or an object the model planted.
///
/// # Errors
///
/// A message when the repository's config could run a program, a name is no branch name, the place is taken, or git
/// fails.
pub fn clone_for_arena(home: &Path, repo: &Repo, branch: &str) -> Result<Tree, String> {
    checked(&repo.pin())?;
    plain_branch(repo, branch)?;
    let name = repo
        .top
        .file_name()
        .map_or("repo".into(), |name| name.to_string_lossy());
    let dir = home.join(DIR);
    let place = dir.join(format!("{name}-{}", branch.replace('/', "-")));
    let store = store_of(&place);
    if place.exists() || store.exists() {
        return Err(tr!(there_already, place = place.display()));
    }
    own_dir(&dir)?;
    let into = git(&repo.pin(), &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
    let head = git(&repo.pin(), &["rev-parse", "--verify", "HEAD^{commit}"])?;
    let outside = |args: &[&std::ffi::OsStr]| {
        let mut command = Command::new("git");
        command.current_dir(&dir);
        scrub(&mut command);
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args(args);
        run(command, TIMEOUT_CHANGING)
    };
    // Over `file://`, not a plain path: only what the branch reaches travels — no object no ref holds (a secret staged
    // and taken back, a dropped stash), no other branch, no borrowed object store.
    let mut source = std::ffi::OsString::from("file://");
    source.push(&repo.top);
    let made = (|| {
        outside(&[
            "-c".as_ref(),
            "protocol.file.allow=always".as_ref(),
            "clone".as_ref(),
            "--single-branch".as_ref(),
            "--no-tags".as_ref(),
            "--no-checkout".as_ref(),
            "--quiet".as_ref(),
            "--".as_ref(),
            source.as_os_str(),
            place.as_os_str(),
        ])?;
        outside(&[
            "init".as_ref(),
            "--bare".as_ref(),
            "--quiet".as_ref(),
            store.as_os_str(),
        ])?;
        let place = place.canonicalize().map_err(|err| err.to_string())?;
        let store = store.canonicalize().map_err(|err| err.to_string())?;
        let own = place.join(".git");
        let model = Pin {
            work_tree: &place,
            git_dir: &own,
            common: &own,
        };
        git_alone_text(&model, &["remote", "remove", "origin"])?;
        git_alone_text(&model, &["checkout", "--quiet", "-b", branch, &head])?;
        own_dir(&place.join(".git").join(PRIVATE_HOME))?;
        // Glut's store reads the owner's objects and starts from the files as checked out.
        std::fs::write(
            store.join("objects/info/alternates"),
            format!("{}\n", repo.common.join("objects").display()),
        )
        .map_err(|err| err.to_string())?;
        // What the owner's own exclude file keeps out of git, the copy and the store keep out too: a worktree shared
        // it, and a model's file the owner ignores must not come back in a take.
        if let Ok(exclude) = std::fs::read(repo.common.join("info/exclude")) {
            let _ = std::fs::write(own.join("info/exclude"), &exclude);
            let _ = std::fs::write(store.join("info/exclude"), exclude);
        }
        let tree = Tree {
            place,
            branch: branch.to_owned(),
            git_dir: store,
            repo: repo.clone(),
            into,
            cloned: true,
        };
        git_alone_text(&tree.pin(), &["read-tree", &head])?;
        Ok(tree)
    })();
    if made.is_err() {
        let _ = remove_writable(&place);
        let _ = remove_writable(&store);
    }
    made
}

/// Where Glut keeps its store beside an arena's copy at `place` ([`clone_for_arena`]).
fn store_of(place: &Path) -> PathBuf {
    let mut name = place.file_name().unwrap_or_default().to_os_string();
    name.push(".store");
    place.with_file_name(name)
}

/// `dir` removed with everything in it, its folders made writable first where a tool left them read-only (Go's module
/// cache) — links never followed, so nothing outside it changes.
fn remove_writable(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    if std::fs::remove_dir_all(dir).is_ok() || !dir.exists() {
        return Ok(());
    }
    let mut left = vec![dir.to_path_buf()];
    while let Some(folder) = left.pop() {
        let meta = std::fs::symlink_metadata(&folder)?;
        if !meta.is_dir() {
            continue;
        }
        let _ = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700));
        for entry in std::fs::read_dir(&folder)?.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                left.push(entry.path());
            }
        }
    }
    std::fs::remove_dir_all(dir)
}

/// A folder of the owner's alone (0700), made with its parents where missing.
fn own_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|err| format!("{}: {err}", dir.display()))?;
    // One made before keeps the owner's alone too.
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    Ok(())
}

/// The worktree Glut made for `branch` of `repo` before (TUI9e, a layout kept for the next start), found again: its
/// place is where [`add`] puts it below `home` — never a path read from a file — and its own git folder the one the
/// repository lists for that place, not what the worktree's `.git` file (an agent's to write) says. `into` is the
/// branch a take-over merges into, as kept.
///
/// # Errors
///
/// A message when the repository's config could run a program, a name is no branch name, or the worktree is gone,
/// not the repository's, or on another branch.
pub fn reopen(
    home: &Path,
    repo: &Repo,
    branch: &str,
    into: Option<String>,
) -> Result<Tree, String> {
    checked(&repo.pin())?;
    plain_branch(repo, branch)?;
    if let Some(into) = &into {
        plain_branch(repo, into)?;
    }
    let name = repo
        .top
        .file_name()
        .map_or("repo".into(), |name| name.to_string_lossy());
    let place = home
        .join(DIR)
        .join(format!("{name}-{}", branch.replace('/', "-")));
    let place = place
        .canonicalize()
        .map_err(|_| tr!(worktree_of_gone, branch = branch, place = place.display()))?;
    // Where Glut keeps worktrees, a link at the place not followed out of it.
    let root = home
        .join(DIR)
        .canonicalize()
        .map_err(|err| err.to_string())?;
    if !place.starts_with(&root) {
        return Err(tr!(not_glut_place, place = place.display()));
    }
    let dot_git = place.join(".git");
    let listed = std::fs::read_dir(repo.common.join("worktrees"))
        .map_err(|err| format!("{}: {err}", repo.common.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|dir| {
            std::fs::read_to_string(dir.join("gitdir")).is_ok_and(|gitdir| {
                Path::new(gitdir.trim())
                    .canonicalize()
                    .is_ok_and(|at| at == dot_git)
            })
        })
        .and_then(|dir| dir.canonicalize().ok())
        .filter(|dir| dir.starts_with(repo.common.join("worktrees")));
    let Some(git_dir) = listed else {
        return Err(tr!(
            no_worktree_of,
            place = place.display(),
            repo = repo.top.display()
        ));
    };
    let tree = Tree {
        place,
        branch: branch.to_owned(),
        git_dir,
        repo: repo.clone(),
        into,
        cloned: false,
    };
    // The worktree's own config is read too before git runs there.
    checked(&tree.pin())?;
    let on = git(&tree.pin(), &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
    if on != branch {
        return Err(tr!(worktree_other_branch, branch = branch));
    }
    Ok(tree)
}

/// What `tree` holds beside the branch its repository's folder is on.
///
/// # Errors
///
/// Git could not say — which a discard must read as work it would lose.
pub fn standing(tree: &Tree) -> Result<Standing, String> {
    let uncommitted = if tree.place.exists() {
        checked(&tree.pin())?;
        git(&tree.pin(), &["status", "--porcelain", "-z"])?
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .count()
    } else {
        0
    };
    let ahead = git(
        &tree.repo.pin(),
        &[
            "rev-list",
            "--count",
            "--end-of-options",
            &format!("HEAD..refs/heads/{}", tree.branch),
        ],
    )?
    .parse::<usize>()
    .map_err(|_| tr!(git_no_number).to_owned())?;
    // Commits made on a detached head in the worktree are its work too.
    let detached = if tree.place.exists() {
        git(
            &tree.pin(),
            &[
                "rev-list",
                "--count",
                "--end-of-options",
                &format!("refs/heads/{}..HEAD", tree.branch),
            ],
        )?
        .parse::<usize>()
        .map_err(|_| tr!(git_no_number).to_owned())?
    } else {
        0
    };
    Ok(Standing {
        uncommitted,
        ahead: ahead + detached,
    })
}

/// Whether a path in a merge is one a model may not bring into the owner's folder: a name the sandbox protects at the
/// top or one of [`MORE_ROOTS`], or one of [`ANY_DEPTH`] anywhere — in any case, as the owner's disk does not tell
/// `.Claude` from `.claude`.
fn protected(path: &str) -> bool {
    let lower = path.to_lowercase();
    let first = lower.split('/').next().unwrap_or("");
    glut::sandbox::PROTECTED_IN_ROOT
        .iter()
        .chain(MORE_ROOTS.iter())
        .any(|name| name.eq_ignore_ascii_case(first))
        || lower.split('/').any(|part| ANY_DEPTH.contains(&part))
}

/// The protected paths among git's `-z` list of names.
fn protected_among(names: &str) -> Vec<String> {
    names
        .split('\0')
        .filter(|path| !path.is_empty() && protected(path))
        .map(crate::transcript::clean)
        .collect()
}

/// Refuses protected paths, naming the first few.
fn refuse_protected(branch: &str, found: &[String]) -> Result<(), String> {
    if found.is_empty() {
        return Ok(());
    }
    let shown: Vec<&str> = found.iter().take(3).map(String::as_str).collect();
    Err(tr!(
        protected_changes,
        branch = branch,
        files = shown.join(", ")
    ))
}

/// Merges `tree`'s branch into the branch its repository's folder was on when the worktree was made, then removes
/// the worktree and the branch. Only with everything committed on both sides, that folder still on that branch with no
/// merge of its own under way, nothing protected among the changes and no conflict: else nothing changes and the
/// message says what was in the way.
///
/// # Errors
///
/// What stood in the way.
pub fn take_over(tree: &Tree) -> Result<String, String> {
    let repo = tree.repo.pin();
    checked(&repo)?;
    let Some(into) = &tree.into else {
        return Err(tr!(repo_no_branch).into());
    };
    let now = git(&repo, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap_or_default();
    if &now != into {
        let now = if now.is_empty() { tr!(no_branch) } else { &now };
        return Err(tr!(repo_other_branch, now = now, into = into));
    }
    if standing(tree)?.uncommitted > 0 {
        return Err(tr!(tree_uncommitted, branch = tree.branch));
    }
    if git(&repo, &["rev-parse", "--quiet", "--verify", "MERGE_HEAD"]).is_ok() {
        return Err(tr!(repo_merging).into());
    }
    // Untracked files in the folder do not stop a merge that does not touch them; git says so when one would.
    if !git(&repo, &["status", "--porcelain", "--untracked-files=no"])?.is_empty() {
        return Err(tr!(repo_uncommitted, folder = tree.repo.top.display()));
    }
    // Both sides pinned to what they are now: a ref moved by something still running changes nothing below.
    let head = git(
        &repo,
        &["rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"],
    )?;
    let theirs = git(
        &repo,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("refs/heads/{}^{{commit}}", tree.branch),
        ],
    )?;
    let early = git(
        &repo,
        &[
            "diff",
            "--name-only",
            "-z",
            "--no-renames",
            "--end-of-options",
            &format!("{head}...{theirs}"),
        ],
    )?;
    refuse_protected(&tree.branch, &protected_among(&early))?;
    // Merged without its commit first, then judged by what it really put in the index against the pinned head.
    let merged = git(
        &repo,
        &[
            "merge",
            "--no-commit",
            "--no-ff",
            "--no-verify",
            "--no-autostash",
            "--strategy=ort",
            // Merged by its pinned id, named by its branch as a merge of the branch would be.
            "-m",
            &format!("Merge branch '{}'", tree.branch),
            "--end-of-options",
            &theirs,
        ],
    );
    let take_back = || {
        // Only the merge this call started is taken back.
        if git(&repo, &["rev-parse", "--quiet", "--verify", "MERGE_HEAD"]).is_ok() {
            let _ = git(&repo, &["merge", "--abort"]);
        }
    };
    if let Err(reason) = merged {
        take_back();
        return Err(tr!(merge_stopped, reason = reason));
    }
    let staged = match git(
        &repo,
        &[
            "diff",
            "--cached",
            "--name-only",
            "-z",
            "--no-renames",
            "--end-of-options",
            &head,
        ],
    ) {
        Ok(staged) => staged,
        Err(reason) => {
            take_back();
            return Err(tr!(merge_unreadable, reason = reason));
        }
    };
    if let Err(refused) = refuse_protected(&tree.branch, &protected_among(&staged)) {
        take_back();
        return Err(refused);
    }
    if let Err(reason) = git(&repo, &["commit", "--no-edit", "--no-verify"]) {
        take_back();
        return Err(tr!(merge_uncommitted, reason = reason));
    }
    remove(tree, false)?;
    Ok(tr!(merged, branch = tree.branch, into = into))
}

/// Removes `tree`'s worktree and its branch; `force` also when that loses work (the owner said so twice). A worktree
/// folder removed by hand leaves only git's note of it, which goes too.
///
/// # Errors
///
/// Without `force`, work it would lose (or git could not say); git's refusal.
pub fn remove(tree: &Tree, force: bool) -> Result<(), String> {
    // Asked before anything goes: removing the worktree first would leave its branch behind on a refusal.
    if !force {
        match standing(tree) {
            Ok(standing) if !standing.loses_work() => {}
            Ok(standing) => {
                return Err(tr!(
                    tree_holds,
                    branch = tree.branch,
                    held = standing.said()
                ));
            }
            Err(reason) => {
                return Err(tr!(tree_unreadable, branch = tree.branch, reason = reason));
            }
        }
    }
    let repo = tree.repo.pin();
    // An arena's clone holds nothing of the repository's: it goes with its folder, no branch of the owner's beside it.
    if tree.cloned {
        if !tree.place.exists() {
            let store = store_of(&tree.place);
            return remove_writable(&store).map_err(|err| format!("{}: {err}", store.display()));
        }
        let ours = tree.place.canonicalize().ok().is_some_and(|place| {
            place
                .parent()
                .is_some_and(|dir| dir.ends_with(DIR) && dir.is_dir())
        });
        if !ours {
            return Err(tr!(not_glut_worktree, place = tree.place.display()));
        }
        remove_writable(&tree.place).map_err(|err| format!("{}: {err}", tree.place.display()))?;
        let store = store_of(&tree.place);
        return remove_writable(&store).map_err(|err| format!("{}: {err}", store.display()));
    }
    if tree.place.exists() {
        // Only ever a place of Glut's own making.
        let ours = tree.place.canonicalize().ok().is_some_and(|place| {
            place
                .parent()
                .is_some_and(|dir| dir.ends_with(DIR) && dir.is_dir())
        });
        if !ours {
            return Err(tr!(not_glut_worktree, place = tree.place.display()));
        }
        let place = tree.place.to_string_lossy().into_owned();
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push("--");
        args.push(&place);
        git(&repo, &args)?;
    } else {
        git(&repo, &["worktree", "prune"])?;
    }
    git(
        &repo,
        &["branch", if force { "-D" } else { "-d" }, &tree.branch],
    )?;
    Ok(())
}

/// The most changed and new files an arena's worktree takes over from the owner's folder (T61).
const MIRROR_FILES: usize = 5_000;
/// The largest of them taken over; a bigger one is left out and named.
const MIRROR_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// The most bytes an arena's worktree takes over in all.
const MIRROR_BYTES: u64 = 500 * 1024 * 1024;
/// Git's mode of a link and of a submodule: neither is brought into the owner's folder.
const LINK_MODES: [&str; 2] = ["120000", "160000"];
/// Names an arena's winner may not bring at any depth (beyond [`protected`]'s): other tools' files that run things
/// where the owner works.
const ARENA_ANY_DEPTH: [&str; 8] = [
    ".nvim.lua",
    ".exrc",
    "mise.toml",
    ".mise.toml",
    ".tool-versions",
    ".github",
    ".pre-commit-config.yaml",
    ".githooks",
];
/// The files that make a folder beside `objects/` or `refs/` a git folder, in lower case.
const GIT_FILES: [&str; 6] = [
    "head",
    "config",
    "packed-refs",
    "commondir",
    "gitdir",
    "config.worktree",
];
/// Endings and names of key files the mirror leaves out beside the `.env` files.
const KEY_ENDINGS: [&str; 6] = [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"];
const KEY_NAMES: [&str; 4] = ["id_rsa", "id_ed25519", "id_ecdsa", "credentials.json"];

/// What [`mirror`] made of an arena's worktree (T61): the state every model starts from, as git's tree id, and what
/// it left out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mirrored {
    pub start: String,
    pub copied: usize,
    pub left_out: Vec<String>,
}

/// What a model changed in its arena worktree since the start.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changes {
    pub files: usize,
    pub added: u64,
    pub removed: u64,
}

/// What taking an arena's winner would write: its files, and those it makes executable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Taking {
    pub files: Vec<String>,
    pub executable: Vec<String>,
    /// The worktree as git's tree id when it was read: what the owner said yes to is what is applied.
    pub now: String,
}

/// Whether `name` is one of the `.env` files the sandbox keeps from every command (`sandbox::scan`'s list).
fn secret_name(name: &str) -> bool {
    let name = name.to_lowercase();
    name == ".env"
        || name.starts_with(".env.")
        || (name.ends_with(".env") && name.len() > ".env".len())
        || name == ".envrc"
        || name.starts_with(".envrc.")
}

/// Whether `name` looks like a key or a credential file, which an arena's models, with the network open, never get.
fn key_name(name: &str) -> bool {
    let name = name.to_lowercase();
    secret_name(&name)
        || KEY_NAMES
            .iter()
            .any(|key| name == *key || name.starts_with(&format!("{key}.")))
        || KEY_ENDINGS.iter().any(|ending| name.ends_with(ending))
}

/// Whether git's `path` stays below the folder it is read in: no empty, `.` or `..` part, not absolute.
fn plain_relative(path: &str) -> bool {
    !path.starts_with('/')
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// The folders above `path` in `top`, made where missing; `false` where one is a link or no folder, which a copy
/// must not go through.
fn plain_parents(top: &Path, path: &str, make: bool) -> bool {
    let mut at = top.to_path_buf();
    let parts: Vec<&str> = path.split('/').collect();
    for part in &parts[..parts.len().saturating_sub(1)] {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            Ok(meta) if meta.is_dir() => {}
            Err(err) if make && err.kind() == std::io::ErrorKind::NotFound => {
                if std::fs::create_dir(&at).is_err() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

/// Copies the owner's `from` to the new worktree's `to` without following a link on either side: `from` opened as
/// the file it is (its folders checked first), `to` written only where nothing or a plain file stands. The bytes
/// copied, or `None` where it is no plain file within `room`.
fn copy_plain(top: &Path, place: &Path, path: &str, room: u64) -> Option<u64> {
    use std::os::unix::fs::OpenOptionsExt as _;
    if !plain_parents(top, path, false) {
        return None;
    }
    let mut from = std::fs::OpenOptions::new()
        .read(true)
        // Not blocking: a pipe where a file was must not hold the screen; it is no plain file and stays out.
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(top.join(path))
        .ok()?;
    let meta = from.metadata().ok()?;
    if !meta.is_file() || meta.len() > MIRROR_FILE_BYTES || meta.len() > room {
        return None;
    }
    if !plain_parents(place, path, true) {
        return None;
    }
    let to = place.join(path);
    if std::fs::symlink_metadata(&to).is_ok_and(|there| !there.is_file()) {
        return None;
    }
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&to)
        .ok()?;
    let copied = std::io::copy(&mut (&mut from).take(meta.len()), &mut out).ok()?;
    use std::os::unix::fs::PermissionsExt as _;
    let _ = out.set_permissions(std::fs::Permissions::from_mode(
        meta.permissions().mode() & 0o755,
    ));
    Some(copied)
}

/// The owner's `HEAD` now, as a commit id: an arena takes a winner only while it has not moved.
///
/// # Errors
///
/// Git's refusal (a repository without a commit).
pub fn head(repo: &Repo) -> Result<String, String> {
    git(
        &repo.pin(),
        &["rev-parse", "--verify", "--end-of-options", "HEAD^{commit}"],
    )
}

/// Gives an arena's new worktree (T61) the owner's folder as it is now: every changed, deleted and new file git does
/// not ignore, so each model starts from what the owner sees, not from the last commit. A `.env` or key file, a link,
/// a folder (a submodule), a file over [`MIRROR_FILE_BYTES`] or past [`MIRROR_BYTES`] in all stays out and is
/// named. Its state is then kept as git's tree id: what the model changes is measured and taken over against it,
/// whatever it does to the branch.
///
/// # Errors
///
/// A config that could run a program, more than [`MIRROR_FILES`] files, git's refusal.
pub fn mirror(tree: &Tree) -> Result<Mirrored, String> {
    let repo = tree.repo.pin();
    checked(&repo)?;
    checked(&tree.pin())?;
    // Read as git wrote them: a name's leading space or bytes no UTF-8 holds stay as they are.
    let changed = git_bytes(
        &repo,
        &["diff", "--name-only", "-z", "--no-renames", "HEAD"],
    )?;
    let untracked = git_bytes(&repo, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    let paths: Vec<&[u8]> = changed
        .split(|byte| *byte == 0)
        .chain(untracked.split(|byte| *byte == 0))
        .filter(|path| !path.is_empty())
        .collect();
    if paths.len() > MIRROR_FILES {
        return Err(tr!(mirror_too_many, n = paths.len(), most = MIRROR_FILES));
    }
    let mut copied = 0;
    let mut bytes = 0;
    let mut left_out = Vec::new();
    for path in paths {
        let Ok(path) = std::str::from_utf8(path) else {
            left_out.push(crate::transcript::clean(&String::from_utf8_lossy(path)));
            continue;
        };
        if !plain_relative(path) || path.split('/').any(key_name) {
            left_out.push(crate::transcript::clean(path));
            continue;
        }
        let from = tree.repo.top.join(path);
        match std::fs::symlink_metadata(&from) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Deleted in the owner's folder: deleted here too.
                let to = tree.place.join(path);
                if plain_parents(&tree.place, path, false)
                    && std::fs::symlink_metadata(&to).is_ok_and(|there| !there.is_dir())
                {
                    let _ = std::fs::remove_file(&to);
                }
            }
            _ => match copy_plain(&tree.repo.top, &tree.place, path, MIRROR_BYTES - bytes) {
                Some(size) => {
                    bytes += size;
                    copied += 1;
                }
                None => left_out.push(crate::transcript::clean(path)),
            },
        }
    }
    git_alone(&tree.pin(), &["add", "--all"], None)?;
    let start = git_alone_text(&tree.pin(), &["write-tree"])?;
    Ok(Mirrored {
        start,
        copied,
        left_out,
    })
}

/// The worktree as git's tree id now, every file the model wrote included: what the arena's card, numbers and a
/// take are read from, taken once (a check run afterwards writes there too).
///
/// # Errors
///
/// A config that could run a program, git's refusal.
pub fn snapshot(tree: &Tree) -> Result<String, String> {
    written(tree)
}

/// [`snapshot`]'s work.
fn written(tree: &Tree) -> Result<String, String> {
    checked(&tree.pin())?;
    git_alone(&tree.pin(), &["add", "--all"], None)?;
    git_alone_text(&tree.pin(), &["write-tree"])
}

/// What the model of an arena's worktree changed from `start` (git's tree id from [`mirror`]) to `now`: files, lines added
/// and removed, new files included (a binary one counts no lines).
///
/// # Errors
///
/// A config that could run a program, git's refusal.
pub fn changes(tree: &Tree, start: &str, now: &str) -> Result<Changes, String> {
    checked(&tree.pin())?;
    let stat = git_alone(
        &tree.objects_pin(),
        &[
            "diff",
            "--numstat",
            "-z",
            "--no-renames",
            "--end-of-options",
            start,
            now,
        ],
        None,
    )?;
    let mut changes = Changes::default();
    for entry in stat
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let entry = String::from_utf8_lossy(entry);
        let mut fields = entry.splitn(3, '\t');
        let number =
            |field: Option<&str>| field.and_then(|text| text.parse::<u64>().ok()).unwrap_or(0);
        changes.added += number(fields.next());
        changes.removed += number(fields.next());
        changes.files += 1;
    }
    Ok(changes)
}

/// What the model of an arena's worktree changed from `start` to `now`, as git's text diff, for the owner to read.
///
/// # Errors
///
/// A config that could run a program, git's refusal (a diff past git's output bound among them).
pub fn diff_text(tree: &Tree, start: &str, now: &str) -> Result<String, String> {
    checked(&tree.pin())?;
    let out = git_alone(
        &tree.objects_pin(),
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--end-of-options",
            start,
            now,
        ],
        None,
    )?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Whether a path of an arena's winner may not come into the owner's folder: what a take-over refuses, a protected
/// or another tool's name at any depth, any `….git` part, a `.env` or key file.
fn arena_refuses(path: &str) -> bool {
    protected(path)
        || !plain_relative(path)
        || path.split('/').any(|part| {
            let part = part.to_lowercase();
            glut::sandbox::PROTECTED_IN_ROOT
                .iter()
                .chain(MORE_ROOTS.iter())
                .chain(ARENA_ANY_DEPTH.iter())
                .any(|name| name.eq_ignore_ascii_case(&part))
                || part.ends_with(".git")
                || key_name(&part)
        })
}

/// The folders among `paths` laid out as a git folder (one of [`GIT_FILES`] beside `objects/` or `refs/`): a repository the owner's git would run the config of once it stood in the folder.
fn git_folders(paths: &[String]) -> Vec<String> {
    // In any case: the owner's disk does not tell `HEAD` from `head`.
    let lowered: Vec<String> = paths.iter().map(|path| path.to_lowercase()).collect();
    lowered
        .iter()
        .zip(paths)
        .filter_map(|(path, shown)| {
            let (dir, name) = path.rsplit_once('/').unwrap_or(("", path.as_str()));
            let shaped = GIT_FILES.contains(&name)
                && lowered.iter().any(|other| {
                    let inside = if dir.is_empty() {
                        other.as_str()
                    } else {
                        match other
                            .strip_prefix(dir)
                            .and_then(|rest| rest.strip_prefix('/'))
                        {
                            Some(inside) => inside,
                            None => return false,
                        }
                    };
                    inside.starts_with("objects/") || inside.starts_with("refs/")
                });
            shaped.then(|| shown.clone())
        })
        .collect()
}

/// The patch of what the arena's model changed since `start`, made once, and what it writes as git's `apply` reads
/// these very bytes — checked: nothing [`arena_refuses`], no link or submodule, no git folder.
fn arena_patch(tree: &Tree, start: &str, now: Option<&str>) -> Result<(Vec<u8>, Taking), String> {
    checked(&tree.repo.pin())?;
    let now = match now {
        Some(now) => now.to_owned(),
        None => written(tree)?,
    };
    let patch = git_alone(
        &tree.objects_pin(),
        &[
            "diff",
            "--binary",
            "--no-ext-diff",
            "--no-textconv",
            "--no-renames",
            "--end-of-options",
            start,
            &now,
        ],
        None,
    )?;
    if patch.is_empty() {
        return Err(tr!(arena_nothing).into());
    }
    let repo = tree.repo.pin();
    let listed = git_alone(
        &repo,
        &["apply", "--numstat", "-z", "-"],
        Some(patch.clone()),
    )?;
    let files: Vec<String> = listed
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let entry = String::from_utf8_lossy(entry);
            entry.splitn(3, '\t').nth(2).unwrap_or("").to_owned()
        })
        .collect();
    let summary = git_alone(&repo, &["apply", "--summary", "-"], Some(patch.clone()))?;
    let summary = String::from_utf8_lossy(&summary);
    let mut refused: Vec<String> = files
        .iter()
        .filter(|path| arena_refuses(path))
        .cloned()
        .collect();
    refused.extend(git_folders(&files));
    // A link edited in place keeps its mode and shows in no summary: its headers say it. A header line stands at
    // the line's start, where no line of a file's text or of a binary's base85 can.
    for line in patch.split(|byte| *byte == b'\n') {
        let line = String::from_utf8_lossy(line);
        let header = [
            "index ",
            "old mode ",
            "new mode ",
            "new file mode ",
            "deleted file mode ",
        ]
        .iter()
        .any(|start| line.starts_with(start));
        if header
            && line
                .split_whitespace()
                .any(|word| LINK_MODES.contains(&word))
        {
            refused.push(line.into_owned());
        }
    }
    let mut executable = Vec::new();
    for line in summary.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.iter().any(|word| LINK_MODES.contains(word)) {
            refused.push(line.trim().to_owned());
        }
        // ` create mode 100755 path` and ` mode change 100644 => 100755 path`.
        if let Some(at) = words.iter().position(|word| *word == "100755")
            && let Some(path) = words.get(at + 1)
        {
            executable.push((*path).to_owned());
        }
    }
    let refused: Vec<String> = refused
        .iter()
        .map(|path| crate::transcript::clean(path))
        .collect();
    refuse_protected(&tree.branch, &refused)?;
    let clean = |paths: Vec<String>| {
        paths
            .iter()
            .map(|path| crate::transcript::clean(path))
            .collect()
    };
    Ok((
        patch,
        Taking {
            files: clean(files),
            executable: clean(executable),
            now,
        },
    ))
}

/// A bench task's check folder `place` (T61 step 2), in `~/.glut` where no model's command reads or writes: the
/// task's own `start` folder copied (files and folders alone), a git repository of its own whose one commit is that
/// start (so `git status` there tells what the model changed, whatever it did to its own branch or the shared
/// objects), the model's changes from `start` (git's tree id) to `now` laid over it as the bytes [`arena_patch`]
/// checked (no link, protected or tool name, no git folder — a change it refuses is the check's failure), and the
/// task's `check.sh` and `check/` copied into `.arena/` last, which that repository ignores. Nothing the model left
/// in its worktree besides is in it, and nothing of the model's runs beside it while the check runs.
///
/// # Errors
///
/// What the patch refuses, or what could not be made.
pub fn check_copy(
    tree: &Tree,
    start: &str,
    now: &str,
    task: &Path,
    place: &Path,
) -> Result<(), String> {
    checked(&tree.pin())?;
    let patch = match arena_patch(tree, start, Some(now)) {
        Ok((patch, _)) => Some(patch),
        Err(reason) if reason == tr!(arena_nothing) => None,
        Err(reason) => return Err(reason),
    };
    if let Ok(there) = std::fs::symlink_metadata(place) {
        if there.is_dir() {
            std::fs::remove_dir_all(place)
        } else {
            std::fs::remove_file(place)
        }
        .map_err(|err| format!("{}: {err}", place.display()))?;
    }
    if let Some(parent) = place.parent() {
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .map_err(|err| format!("{}: {err}", parent.display()))?;
    }
    copy_files(&task.join("start"), place)?;
    let scratch = |args: &[&str], input: Option<Vec<u8>>| {
        let mut command = Command::new("git");
        command.current_dir(place);
        scrub(&mut command);
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .args([
                "-c",
                "core.attributesFile=/dev/null",
                "-c",
                "core.excludesFile=/dev/null",
            ])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "user.name=Glut",
                "-c",
                "user.email=glut@localhost",
            ])
            .args(args);
        run_bytes(command, TIMEOUT_CHANGING, input)
    };
    scratch(&["init", "-q", "-b", "main"], None)?;
    scratch(&["add", "--all", "--force"], None)?;
    scratch(&["commit", "-q", "--allow-empty", "-m", "start"], None)?;
    if let Some(patch) = patch {
        scratch(&["apply", "--binary", "-"], Some(patch))?;
    }
    let exclude = place.join(".git/info/exclude");
    std::fs::create_dir_all(place.join(".git/info"))
        .and_then(|()| std::fs::write(&exclude, ".arena/\n"))
        .map_err(|err| format!("{}: {err}", exclude.display()))?;
    let arena = place.join(".arena");
    std::fs::create_dir(&arena).map_err(|err| format!("{}: {err}", arena.display()))?;
    copy_files(&task.join("check.sh"), &arena.join("check.sh"))?;
    if task.join("check").is_dir() {
        copy_files(&task.join("check"), &arena.join("check"))?;
    }
    Ok(())
}

/// Copies the owner's `from` (a file or a folder) to `to`, files and folders alone: no link is followed or made.
fn copy_files(from: &Path, to: &Path) -> Result<(), String> {
    let said = |err: std::io::Error| format!("{}: {err}", from.display());
    let meta = std::fs::symlink_metadata(from).map_err(said)?;
    if meta.is_file() {
        std::fs::copy(from, to).map_err(said)?;
    } else if meta.is_dir() {
        if !to.is_dir() {
            std::fs::create_dir(to).map_err(said)?;
        }
        for entry in std::fs::read_dir(from).map_err(said)? {
            let entry = entry.map_err(said)?;
            copy_files(&entry.path(), &to.join(entry.file_name()))?;
        }
    }
    Ok(())
}

/// What taking the arena's winner would write, for the owner to see before saying yes.
///
/// # Errors
///
/// As [`apply`] refuses.
pub fn taking(tree: &Tree, start: &str, now: Option<&str>) -> Result<Taking, String> {
    arena_patch(tree, start, now).map(|(_, taking)| taking)
}

/// Takes what the winner of an arena (T61) changed from `start` to `now` (the tree [`taking`] showed the owner)
/// into the owner's folder, uncommitted, as a diff:
/// only while the owner's `HEAD` is still `head`, when it applies as a whole to the folder as it is now (git's
/// check first — a file changed there since the start that the winner changed too is a conflict, nothing is
/// written) and brings nothing [`arena_patch`] refuses. The patch goes to git on its standard input: the bytes
/// checked are the bytes applied.
///
/// # Errors
///
/// What stood in the way, the conflict named.
pub fn apply(tree: &Tree, start: &str, now: &str, head_then: &str) -> Result<usize, String> {
    let repo = tree.repo.pin();
    checked(&repo)?;
    if head(&tree.repo)? != head_then {
        return Err(tr!(arena_head_moved).into());
    }
    let (patch, taking) = arena_patch(tree, start, Some(now))?;
    git_alone(
        &repo,
        &["apply", "--check", "--binary", "-"],
        Some(patch.clone()),
    )
    .and_then(|_| git_alone(&repo, &["apply", "--binary", "-"], Some(patch)))
    .map_err(|reason| tr!(arena_conflict, reason = reason))?;
    Ok(taking.files.len())
}

/// A repository to try worktrees on, for this module's tests and the panes'.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    pub(crate) fn plain(dir: &Path, args: &[&str]) -> Result<String, String> {
        git_in(dir, args)
    }

    /// A repository with one commit on `main` in a fresh folder, and a home beside it.
    pub(crate) fn repo(name: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("glut-tui-worktree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let repo = root.join("project");
        std::fs::create_dir_all(&repo).unwrap();
        plain(&repo, &["init", "-q", "-b", "main"]).unwrap();
        plain(&repo, &["config", "user.email", "test@example.com"]).unwrap();
        plain(&repo, &["config", "user.name", "Test"]).unwrap();
        commit(&repo, "a.txt", "one\n");
        (root.join("home"), repo)
    }

    /// `file` written and committed in `dir`.
    pub(crate) fn commit(dir: &Path, file: &str, text: &str) {
        let path = dir.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, text).unwrap();
        plain(dir, &["add", file]).unwrap();
        plain(
            dir,
            &["-c", "commit.gpgsign=false", "commit", "-q", "-m", file],
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{commit, plain, repo};
    use super::*;

    fn pinned(dir: &Path) -> Repo {
        repo_of(dir).unwrap().unwrap()
    }

    /// T61: an arena's clone has a git folder of its own — what a model does to its refs, its stash or its objects
    /// never reaches the owner's repository — and goes with its folder.
    #[test]
    fn an_arena_clone_keeps_its_git_folder_apart_from_the_owners() {
        let (home, top) = repo("arena-clone");
        commit(&top, "c.txt", "three\n");
        let repo = pinned(&top);
        let branches = || git_in(&top, &["branch", "--list"]).unwrap();
        let before = branches();
        let objects = || {
            let mut found = Vec::new();
            let mut left = vec![repo.common.join("objects")];
            while let Some(dir) = left.pop() {
                for entry in std::fs::read_dir(dir).unwrap().flatten() {
                    match entry.file_type().unwrap().is_dir() {
                        true => left.push(entry.path()),
                        false => found.push(entry.path()),
                    }
                }
            }
            found.sort();
            found
        };
        // A secret staged and taken back is an object no ref reaches: it stays behind.
        std::fs::write(top.join("secret.txt"), "KEY=hidden\n").unwrap();
        let blob = git_in(&top, &["hash-object", "-w", "secret.txt"]).unwrap();
        std::fs::remove_file(top.join("secret.txt")).unwrap();
        let objects_before = objects();
        let tree = clone_for_arena(&home, &repo, "glut/arena-1").unwrap();
        let all = git_in(
            &tree.place,
            &[
                "cat-file",
                "--batch-all-objects",
                "--batch-check=%(objectname)",
            ],
        )
        .unwrap();
        assert!(!all.contains(&blob), "no unreachable object travels");
        assert!(tree.cloned && tree.git_dir == store_of(&tree.place).canonicalize().unwrap());
        assert!(tree.private_home().is_dir());
        assert!(
            !repo.common.join("worktrees").exists(),
            "no worktree of the owner's"
        );
        assert_eq!(branches(), before, "no branch in the owner's repository");
        let model = |args: &[&str]| git_in(&tree.place, args);
        assert_eq!(
            model(&["symbolic-ref", "--short", "HEAD"]).unwrap(),
            "glut/arena-1"
        );
        assert_eq!(
            model(&["remote"]).unwrap(),
            "",
            "no way back to the owner's repository"
        );
        assert!(
            !tree.place.join(".git/objects/info/alternates").exists(),
            "no object borrowed: the owner's folder stays hidden"
        );
        // A model's commit and branch stay in the copy; Glut's store never reads its git folder.
        std::fs::write(tree.place.join("b.txt"), "two\n").unwrap();
        for args in [
            &["add", "b.txt"][..],
            &[
                "-c",
                "user.name=m",
                "-c",
                "user.email=m@m",
                "commit",
                "-qm",
                "m",
            ],
            &["branch", "other"],
        ] {
            model(args).unwrap();
        }
        let start = written(&tree).unwrap();
        // Attributes the model writes and hides show the owner every line all the same.
        std::fs::write(tree.place.join(".gitattributes"), "* -diff\n").unwrap();
        std::fs::write(tree.place.join(".gitignore"), ".gitattributes\n").unwrap();
        std::fs::write(tree.place.join("b.txt"), "two\nthree\n").unwrap();
        let now = written(&tree).unwrap();
        let shown = diff_text(&tree, &start, &now).unwrap();
        assert!(
            shown.contains("+three") && !shown.contains("Binary"),
            "{shown}"
        );
        assert_eq!(changes(&tree, &start, &now).unwrap().added, 2, "{shown}");
        let ours = git_alone_text(&tree.pin(), &["ls-tree", "--name-only", &now]).unwrap();
        assert!(
            ours.contains("b.txt"),
            "the store hashes the files as they are: {ours}"
        );
        assert_eq!(branches(), before);
        assert_eq!(
            objects(),
            objects_before,
            "the owner's objects are read, never written"
        );
        assert_eq!(
            head(&repo).unwrap(),
            git_in(&top, &["rev-parse", "HEAD"]).unwrap()
        );
        // A tool's read-only folders (Go's module cache) go with the clone.
        let locked = tree.private_home().join("go-mod/m/v1");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("f"), "x").unwrap();
        std::fs::set_permissions(&locked, std::os::unix::fs::PermissionsExt::from_mode(0o555))
            .unwrap();
        remove(&tree, true).unwrap();
        assert!(!tree.place.exists());
        assert_eq!(branches(), before);
        let taken = clone_for_arena(&home, &repo, "glut/arena-1").unwrap();
        assert!(
            clone_for_arena(&home, &repo, "glut/arena-1").is_err(),
            "a place taken is refused"
        );
        remove(&taken, true).unwrap();
    }

    #[test]
    fn an_arena_worktree_starts_from_the_folder_as_it_is_and_its_changes_go_back_as_a_diff() {
        let (home, top) = repo("arena-mirror");
        commit(&top, "gone.txt", "old\n");
        std::fs::write(top.join("a.txt"), "one changed\n").unwrap();
        std::fs::write(top.join("new.txt"), "new\n").unwrap();
        std::fs::write(top.join(".env"), "SECRET=1\n").unwrap();
        std::fs::write(top.join("server.pem"), "KEY\n").unwrap();
        std::fs::remove_file(top.join("gone.txt")).unwrap();
        let repo = pinned(&top);
        let head_then = head(&repo).unwrap();
        let tree = clone_for_arena(&home, &repo, "glut/arena-1").unwrap();
        let mirrored = mirror(&tree).unwrap();
        let read = |path: PathBuf| std::fs::read_to_string(path).unwrap();
        assert_eq!(read(tree.place.join("a.txt")), "one changed\n");
        assert_eq!(read(tree.place.join("new.txt")), "new\n");
        assert!(
            !tree.place.join("gone.txt").exists(),
            "deleted there, deleted here"
        );
        assert!(!tree.place.join(".env").exists(), "a secret stays out");
        let mut left_out = mirrored.left_out.clone();
        left_out.sort();
        assert_eq!(left_out, [".env", "server.pem"]);
        assert_eq!(
            changes(&tree, &mirrored.start, &snapshot(&tree).unwrap()).unwrap(),
            Changes::default()
        );
        // The model works: a line more, a file more, an executable script.
        std::fs::write(tree.place.join("a.txt"), "one changed\nand more\n").unwrap();
        std::fs::write(tree.place.join("b.txt"), "b\n").unwrap();
        std::fs::write(tree.place.join("run.sh"), "echo\n").unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(
            tree.place.join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        assert_eq!(
            changes(&tree, &mirrored.start, &snapshot(&tree).unwrap()).unwrap(),
            Changes {
                files: 3,
                added: 3,
                removed: 0
            }
        );
        let diff = diff_text(&tree, &mirrored.start, &snapshot(&tree).unwrap()).unwrap();
        assert!(
            diff.contains("+and more") && diff.contains("b/run.sh"),
            "{diff}"
        );
        let shown = taking(&tree, &mirrored.start, None).unwrap();
        assert_eq!(shown.files, ["a.txt", "b.txt", "run.sh"]);
        assert_eq!(shown.executable, ["run.sh"], "said before it is taken");
        assert_eq!(
            apply(&tree, &mirrored.start, &written(&tree).unwrap(), &head_then).unwrap(),
            3
        );
        assert_eq!(read(top.join("a.txt")), "one changed\nand more\n");
        assert_eq!(read(top.join("b.txt")), "b\n");
        assert_eq!(
            read(top.join(".env")),
            "SECRET=1\n",
            "the owner's own stays"
        );
        // Taken a second time it no longer fits the folder: nothing is written.
        std::fs::write(top.join("a.txt"), "the owner's\n").unwrap();
        assert!(apply(&tree, &mirrored.start, &written(&tree).unwrap(), &head_then).is_err());
        assert_eq!(read(top.join("a.txt")), "the owner's\n");
        // A HEAD moved since the start: refused before anything is looked at.
        commit(&top, "c.txt", "c\n");
        assert_eq!(
            apply(&tree, &mirrored.start, &written(&tree).unwrap(), &head_then).unwrap_err(),
            tr!(arena_head_moved)
        );
        assert!(!home.join(".glut/arena").exists(), "no patch lies anywhere");
        remove(&tree, true).unwrap();
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn an_arena_winner_brings_no_link_git_folder_or_protected_secret_or_tool_name() {
        let (home, top) = repo("arena-refuse");
        let repo = pinned(&top);
        let head_then = head(&repo).unwrap();
        let tree = clone_for_arena(&home, &repo, "glut/arena-1").unwrap();
        let start = mirror(&tree).unwrap().start;
        assert!(
            apply(&tree, &start, &written(&tree).unwrap(), &head_then).is_err(),
            "nothing changed"
        );
        std::os::unix::fs::symlink("/etc/hosts", tree.place.join("hosts")).unwrap();
        assert!(apply(&tree, &start, &written(&tree).unwrap(), &head_then).is_err());
        assert!(std::fs::symlink_metadata(top.join("hosts")).is_err());
        std::fs::remove_file(tree.place.join("hosts")).unwrap();
        for files in [
            &[".gitattributes"][..],
            &["sub/.envrc"],
            &["config.env"],
            &["keys/deploy.pem"],
            &[".claude/settings.json"],
            &["crates/x/.cargo/config.toml"],
            &["web/.vscode/tasks.json"],
            &["vendor/x.git/HEAD"],
            &["vendor/bare/HEAD", "vendor/bare/objects/info/packs"],
            &["HEAD", "refs/heads/main"],
            &["vendor/foo/head", "vendor/foo/Objects/x"],
        ] {
            for name in files {
                let path = tree.place.join(name);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, "x\n").unwrap();
            }
            assert!(
                apply(&tree, &start, &written(&tree).unwrap(), &head_then).is_err(),
                "{files:?}"
            );
            for name in files {
                assert!(!top.join(name).exists(), "{name}");
                std::fs::remove_file(tree.place.join(name)).unwrap();
            }
        }
        // A `HEAD` alone is a file like any other.
        std::fs::write(tree.place.join("HEAD"), "notes\n").unwrap();
        assert_eq!(
            apply(&tree, &start, &written(&tree).unwrap(), &head_then).unwrap(),
            1
        );
        remove(&tree, true).unwrap();
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn an_arena_winner_does_not_turn_a_tracked_link_elsewhere() {
        let (home, top) = repo("arena-link");
        std::os::unix::fs::symlink("a.txt", top.join("lk")).unwrap();
        plain(&top, &["add", "lk"]).unwrap();
        plain(
            &top,
            &["-c", "commit.gpgsign=false", "commit", "-q", "-m", "lk"],
        )
        .unwrap();
        let repo = pinned(&top);
        let head_then = head(&repo).unwrap();
        let tree = clone_for_arena(&home, &repo, "glut/arena-1").unwrap();
        let start = mirror(&tree).unwrap().start;
        std::fs::remove_file(tree.place.join("lk")).unwrap();
        std::os::unix::fs::symlink("/etc/hosts", tree.place.join("lk")).unwrap();
        assert!(taking(&tree, &start, None).is_err());
        let now = written(&tree).unwrap();
        assert!(apply(&tree, &start, &now, &head_then).is_err());
        assert_eq!(
            std::fs::read_link(top.join("lk")).unwrap(),
            Path::new("a.txt")
        );
        remove(&tree, true).unwrap();
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn a_check_copy_holds_the_tasks_start_the_checked_patch_and_the_check_alone() {
        let (home, top) = repo("arena-check-copy");
        let tree = clone_for_arena(&home, &pinned(&top), "glut/arena-1").unwrap();
        let start = mirror(&tree).unwrap().start;
        std::fs::write(tree.place.join("a.txt"), "changed\n").unwrap();
        std::fs::write(tree.place.join("b.txt"), "new\n").unwrap();
        let now = snapshot(&tree).unwrap();
        // What the model does after the snapshot is not in it.
        std::fs::write(tree.place.join("late.txt"), "late\n").unwrap();
        let task = top.parent().unwrap().join("task");
        std::fs::create_dir_all(task.join("check")).unwrap();
        std::fs::create_dir_all(task.join("start")).unwrap();
        std::fs::write(task.join("start/a.txt"), "one\n").unwrap();
        std::fs::write(task.join("check.sh"), "exit 0\n").unwrap();
        std::fs::write(task.join("check/t.py"), "pass\n").unwrap();
        let place = top.parent().unwrap().join("checks/glut-arena-1");
        std::fs::create_dir_all(place.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink("/tmp", &place).unwrap();
        check_copy(&tree, &start, &now, &task, &place).unwrap();
        assert!(
            std::fs::symlink_metadata(&place).unwrap().is_dir(),
            "the link went, a folder stands"
        );
        let read = |name: &str| std::fs::read_to_string(place.join(name)).unwrap();
        assert_eq!(read("a.txt"), "changed\n");
        assert_eq!(read("b.txt"), "new\n");
        assert!(!place.join("late.txt").exists());
        assert_eq!(read(".arena/check.sh"), "exit 0\n");
        assert_eq!(read(".arena/check/t.py"), "pass\n");
        let status = plain(&place, &["status", "--porcelain", "--untracked-files=all"]).unwrap();
        let mut changed: Vec<&str> = status
            .lines()
            .filter_map(|line| line.split_whitespace().last())
            .collect();
        changed.sort_unstable();
        assert_eq!(
            changed,
            ["a.txt", "b.txt"],
            "its own repository's one commit is the start"
        );
        // A link the model made is refused for the check as for a take.
        std::os::unix::fs::symlink("/etc/hosts", tree.place.join("hosts")).unwrap();
        let now = snapshot(&tree).unwrap();
        assert!(check_copy(&tree, &start, &now, &task, &place).is_err());
        std::fs::remove_file(tree.place.join("hosts")).unwrap();
        remove(&tree, true).unwrap();
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn an_arena_snapshot_runs_no_filter_the_models_attributes_name() {
        let (home, top) = repo("arena-filter");
        let tree = clone_for_arena(&home, &pinned(&top), "glut/arena-1").unwrap();
        // A filter in the owner's own git config, chosen by the model's attributes for every file it writes.
        let owner = top.parent().unwrap().join("owner-home");
        let mark = top.parent().unwrap().join("ran");
        std::fs::create_dir_all(&owner).unwrap();
        std::fs::write(
            owner.join(".gitconfig"),
            format!("[filter \"x\"]\n\tclean = touch {}\n", mark.display()),
        )
        .unwrap();
        std::fs::write(tree.place.join(".gitattributes"), "* filter=x\n").unwrap();
        std::fs::write(tree.place.join("b.txt"), "b\n").unwrap();
        let staged = |(mut command, limit): (Command, Duration)| {
            command.env("HOME", &owner).env_remove("XDG_CONFIG_HOME");
            run_bytes(command, limit, None).unwrap();
        };
        staged(alone(&tree.pin(), &["add", "--all"]));
        assert!(!mark.exists(), "the arena's git ran no filter");
        staged(super::pinned(
            &tree.pin(),
            &["add", "--all", "--renormalize"],
        ));
        assert!(mark.exists(), "the owner's git would have");
        remove(&tree, true).unwrap();
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn a_worktree_is_made_below_home_and_taken_over_into_the_repositorys_branch() {
        let (home, top) = repo("over");
        let repo = pinned(&top);
        assert_eq!(proposed_branch(&repo, 2), "glut/pane-2");
        let tree = add(&home, &repo, "glut/pane-2").unwrap();
        assert!(tree.place.starts_with(home.join(DIR)) && tree.place.join("a.txt").exists());
        assert_eq!(tree.into.as_deref(), Some("main"));
        assert_eq!(
            proposed_branch(&repo, 2),
            "glut/pane-2-2",
            "the name is taken now"
        );
        assert!(
            repo_of(&tree.place).is_err(),
            "worktrees are made from the repository's own folder"
        );
        commit(&tree.place, "b.txt", "two\n");
        assert_eq!(
            standing(&tree).unwrap(),
            Standing {
                uncommitted: 0,
                ahead: 1
            }
        );
        std::fs::write(tree.place.join("c.txt"), "loose\n").unwrap();
        assert!(take_over(&tree).unwrap_err().contains("not committed"));
        std::fs::remove_file(tree.place.join("c.txt")).unwrap();
        let said = take_over(&tree).unwrap();
        assert!(said.contains("merged into main"), "{said}");
        assert!(top.join("b.txt").exists() && !tree.place.exists());
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn a_conflict_changes_nothing_and_discarding_needs_force_for_work_it_loses() {
        let (home, top) = repo("conflict");
        let repo = pinned(&top);
        let tree = add(&home, &repo, "glut/pane-2").unwrap();
        commit(&tree.place, "a.txt", "theirs\n");
        commit(&top, "a.txt", "ours\n");
        let refused = take_over(&tree).unwrap_err();
        assert!(refused.contains("nothing changed"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(top.join("a.txt")).unwrap(),
            "ours\n"
        );
        assert!(
            plain(&top, &["status", "--porcelain"]).unwrap().is_empty(),
            "the merge was taken back"
        );
        assert!(standing(&tree).unwrap().loses_work());
        assert!(
            remove(&tree, false).is_err(),
            "work is not lost without force"
        );
        assert!(tree.place.exists(), "nothing went on a refusal");
        remove(&tree, true).unwrap();
        assert!(!tree.place.exists());
        assert!(add(&home, &repo, "no..name").is_err());
        // An option to git is no branch name.
        assert!(add(&home, &repo, "-D").is_err() && add(&home, &repo, "@").is_err());
        assert!(
            add(&home, &repo, "glut/\u{202e}evil").is_err(),
            "no character that turns text"
        );
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn a_repository_whose_config_could_run_a_program_gets_no_worktree() {
        let (home, top) = repo("config");
        let repo = pinned(&top);
        plain(&top, &["config", "filter.ev.smudge", "touch MARK"]).unwrap();
        let found = repo_of(&top).unwrap_err();
        assert!(found.contains("filter.ev.smudge"), "found already: {found}");
        let refused = add(&home, &repo, "glut/pane-2").unwrap_err();
        assert!(refused.contains("filter.ev.smudge"), "{refused}");
        assert!(!top.join("MARK").exists() && !home.join(DIR).exists());
        plain(&top, &["config", "--unset", "filter.ev.smudge"]).unwrap();
        plain(&top, &["config", "hook.ev.command", "touch MARK"]).unwrap();
        assert!(
            add(&home, &repo, "glut/pane-2")
                .unwrap_err()
                .contains("hook.ev.command")
        );
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn a_take_over_never_brings_in_what_the_sandbox_protects() {
        let (home, top) = repo("protected");
        let repo = pinned(&top);
        let tree = add(&home, &repo, "glut/pane-2").unwrap();
        commit(&tree.place, "sub/.envrc", "echo hi\n");
        let refused = take_over(&tree).unwrap_err();
        assert!(refused.contains("sub/.envrc"), "{refused}");
        assert!(!top.join("sub/.envrc").exists());
        assert!(
            plain(&top, &["status", "--porcelain"]).unwrap().is_empty(),
            "nothing staged is left"
        );
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn a_kept_worktree_is_found_again_by_its_branch_alone_and_never_by_what_it_says_of_itself() {
        let (home, top) = repo("kept");
        let repo = pinned(&top);
        let made = add(&home, &repo, "glut/pane-2").unwrap();
        let found = reopen(&home, &repo, "glut/pane-2", made.into.clone()).unwrap();
        assert_eq!(
            (found.git_dir.clone(), found.branch.clone()),
            (made.git_dir.clone(), made.branch.clone())
        );
        assert_eq!(found.place, made.place.canonicalize().unwrap());
        // Its `.git` file is an agent's to write: the repository's own list decides where its git folder is.
        std::fs::write(made.place.join(".git"), "gitdir: /tmp/elsewhere\n").unwrap();
        assert_eq!(
            reopen(&home, &repo, "glut/pane-2", None).unwrap().git_dir,
            made.git_dir
        );
        assert!(
            reopen(&home, &repo, "glut/pane-9", None).is_err(),
            "no such worktree"
        );
        assert!(
            reopen(&home, &repo, "../../etc", None).is_err(),
            "no branch name"
        );
        assert!(reopen(&home, &repo, "glut/pane-2", Some("-D".into())).is_err());
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_worktree_removed_by_hand_is_still_discarded_and_a_redirected_git_folder_is_refused() {
        let (home, top) = repo("gone");
        let repo = pinned(&top);
        let tree = add(&home, &repo, "glut/pane-2").unwrap();
        std::fs::remove_dir_all(&tree.place).unwrap();
        assert_eq!(standing(&tree).unwrap(), Standing::default());
        remove(&tree, false).unwrap();
        assert!(
            plain(
                &top,
                &["rev-parse", "--verify", "--quiet", "refs/heads/glut/pane-2"]
            )
            .is_err()
        );
        std::fs::write(top.join(".git/commondir"), "/tmp/elsewhere\n").unwrap();
        assert!(
            !matches!(repo_of(&top), Ok(Some(_))),
            "a git folder pointed elsewhere is refused"
        );
        let _ = std::fs::remove_dir_all(top.parent().unwrap());
    }

    #[test]
    fn standing_reads_as_the_owner_says_it_and_only_safe_keys_pass() {
        assert_eq!(Standing::default().said(), "nothing new");
        assert_eq!(
            Standing {
                uncommitted: 1,
                ahead: 3
            }
            .said(),
            "3 commits, 1 uncommitted file"
        );
        assert!(
            safe_key("remote.origin.url") && safe_key("core.bare") && safe_key("Branch.Main.Merge")
        );
        assert!(
            !safe_key("filter.lfs.smudge") && !safe_key("include.path") && !safe_key("gpg.program")
        );
        assert!(!safe_key("remote.origin.uploadpack") && !safe_key("core.sshcommand"));
        assert!(safe_key("branch.v1.2.merge"), "a subsection with dots");
        assert!(!safe_key("core.origin.url") && !safe_key("remote.url"));
        assert!(
            protected(".claude/settings.json") && protected(".Claude/x") && protected("sub/.envrc")
        );
        assert!(protected("deep/.GitAttributes") && protected(".idea/workspace.xml"));
        assert!(!protected("src/main.rs") && !protected("docs/claude.md"));
    }
}
