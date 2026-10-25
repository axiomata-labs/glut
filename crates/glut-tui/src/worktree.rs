//! A pane's own worktree (`docs/plans/glut-tui.md`, T36a): made beside the repository for a second pane, and at its
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
            common: &self.repo.common,
        }
    }
}

/// Git on the pinned folders, its hooks, file monitor, maintenance and network off, never asking, ended after
/// [`TIMEOUT`] (a changing call after [`TIMEOUT_CHANGING`]); its output, or its error's first lines.
fn git(pin: &Pin, args: &[&str]) -> Result<String, String> {
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
        Some(&("worktree" | "merge" | "commit" | "branch"))
    );
    run(command, if changing { TIMEOUT_CHANGING } else { TIMEOUT })
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
fn run(mut command: Command, limit: Duration) -> Result<String, String> {
    use std::os::unix::process::CommandExt as _;
    let mut child = command
        .stdin(Stdio::null())
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
        return Ok(String::from_utf8_lossy(&out).trim().to_owned());
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
    })
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
