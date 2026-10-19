//! Snapshots of the work folder and undo (Glut 2, D, G11 — after Opencode's snapshots): before the first step of a run
//! that may change files, and before every later such step, the folder is recorded in a shadow git store; `axh undo`
//! puts it back as it was before the run, or before one step.
//!
//! * **Outside the folder**: the store is `<root>/<hash of the folder>` (the harness's `~/.axh/snapshots`, mode 0700),
//!   used with `--git-dir` and `--work-tree` — nothing is written into the folder, its own `.git` is never touched, and
//!   its `.gitignore` decides what is recorded, plus `target/`, `node_modules/`, nested repositories and every file
//!   over [`MAX_FILE_BYTES`]. A folder of more than [`MAX_FILES`] files, the home folder or `/` get no snapshots.
//! * **Isolated from the owner's git setup**: a cleared environment, no global or system config (so no filters, hooks,
//!   fsmonitor or signing), no attributes or excludes file of the owner's, and a time limit on every git call.
//! * **One chain per run** (`refs/axh/<run>`, built with `write-tree`/`commit-tree`, its own index file): runs on the
//!   same folder share neither an index nor a history, and when a run's record is older than [`KEEP`] (G11) its ref
//!   goes and `gc` frees what only it held. One lock per store serialises snapshots, restores and sweeps.
//! * **Undo restores the recorded files and removes the ones the run added** — the owner's own changes to them during
//!   the run included; what is left out is never touched.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long a run's record and its snapshots are kept (G11).
pub const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// At most one garbage collection of a store a day.
const GC_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Folders never recorded whatever the `.gitignore` says: build output and installed packages, large and rebuilt.
const ALWAYS_LEFT_OUT: &str = "target/\nnode_modules/\n.git/\n";
/// A file larger than this is left out of the snapshots (an attachment, a build artifact): undo does not touch it.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// A folder with more files than this to record gets no snapshots: recording it would cost more than a step.
pub const MAX_FILES: usize = 20_000;
/// How long one git call may take before the snapshot is given up.
const GIT_LIMIT: Duration = Duration::from_secs(60);

/// One step a run recorded: the turn and the commit of the folder just before it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub turn: u32,
    pub commit: String,
}

/// A run's record: its steps, oldest first; the first is the folder before the run changed anything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub run: String,
    pub steps: Vec<Step>,
}

/// The snapshots of one work folder for one run.
#[derive(Debug)]
pub struct Snapshots {
    git_dir: PathBuf,
    work_tree: PathBuf,
    record: Record,
}

/// The store of `work_tree` under `root`.
pub fn store_of(root: &Path, work_tree: &Path) -> PathBuf {
    let digest = Sha256::digest(work_tree.to_string_lossy().as_bytes());
    let name: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
    root.join(name)
}

/// A run id as a ref and file name: letters, digits, `-` and `_` only.
fn checked_run(run: &str) -> Result<&str, String> {
    let ok = !run.is_empty()
        && run.len() <= 100
        && run
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(run)
        .ok_or_else(|| format!("{run:?} is no run id"))
}

/// A commit id from a record: hex only, so nothing in a record is read as an option.
fn checked_commit(commit: &str) -> Result<&str, String> {
    let ok = matches!(commit.len(), 40 | 64) && commit.chars().all(|c| c.is_ascii_hexdigit());
    ok.then_some(commit)
        .ok_or_else(|| format!("{commit:?} is no commit id"))
}

impl Snapshots {
    /// The store for `work_tree` under `root`, made on first use, for the run `run`.
    ///
    /// # Errors
    ///
    /// Why there are no snapshots: a folder that is the home or the root, git missing, the store not writable.
    pub fn open(root: &Path, work_tree: &Path, run: &str) -> Result<Self, String> {
        let run = checked_run(run)?.to_owned();
        let work_tree = work_tree
            .canonicalize()
            .map_err(|err| format!("cannot use {}: {err}", work_tree.display()))?;
        let home =
            std::env::var_os("HOME").and_then(|home| PathBuf::from(home).canonicalize().ok());
        if work_tree == Path::new("/") || home.is_some_and(|home| home.starts_with(&work_tree)) {
            return Err("no snapshots of the home folder or a folder above it".into());
        }
        if root
            .canonicalize()
            .is_ok_and(|root| root.starts_with(&work_tree))
        {
            return Err("no snapshots of a folder that holds the snapshot store".into());
        }
        let git_dir = store_of(root, &work_tree);
        if !git_dir.join("HEAD").exists() {
            make_private_dir(root)?;
            make_private_dir(&git_dir)?;
            init(&git_dir)?;
            std::fs::create_dir_all(git_dir.join("info")).map_err(|err| err.to_string())?;
            std::fs::write(
                git_dir.join("folder"),
                work_tree.to_string_lossy().as_bytes(),
            )
            .map_err(|err| err.to_string())?;
        }
        Ok(Self {
            git_dir,
            work_tree,
            record: Record {
                run,
                steps: Vec::new(),
            },
        })
    }

    /// Records the folder before `turn`'s first change; once per turn. Returns the commit when one was made.
    ///
    /// # Errors
    ///
    /// A git failure or time-out, a folder grown too large, or a record that cannot be written.
    pub fn take(&mut self, turn: u32) -> Result<Option<String>, String> {
        if self
            .record
            .steps
            .last()
            .is_some_and(|step| step.turn == turn)
        {
            return Ok(None);
        }
        let _locked = lock(&self.git_dir)?;
        self.write_exclude()?;
        let index = self.index();
        let git = |args: &[&str]| git_with_index(&self.git_dir, &self.work_tree, &index, args);
        git(&["add", "--all"])?;
        let tree = git(&["write-tree"])?.trim().to_owned();
        let message = format!("{} before step {turn}", self.record.run);
        let mut args = vec!["commit-tree", tree.as_str(), "-m", message.as_str()];
        let parent = self.record.steps.last().map(|step| step.commit.clone());
        if let Some(parent) = &parent {
            args.extend(["-p", parent.as_str()]);
        }
        let commit = git(&args)?.trim().to_owned();
        checked_commit(&commit)?;
        git(&["update-ref", &self.reference(), &commit])?;
        self.record.steps.push(Step {
            turn,
            commit: commit.clone(),
        });
        self.write_record()?;
        Ok(Some(commit))
    }

    /// The store's exclude list: the always-left-out folders, nested repositories (git would record them as a bare
    /// link, so undo could not put them back — they are left out, and undo does not touch them) and the files over
    /// [`MAX_FILE_BYTES`] among what git would record now; refuses a folder with more than [`MAX_FILES`] of them.
    fn write_exclude(&self) -> Result<(), String> {
        let index = self.index();
        let listed = git_with_index(
            &self.git_dir,
            &self.work_tree,
            &index,
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
                "--directory",
            ],
        )?;
        let entries: Vec<&str> = listed.split('\0').filter(|f| !f.is_empty()).collect();
        if entries.len() > MAX_FILES {
            return Err(format!(
                "the folder has more than {MAX_FILES} files to record"
            ));
        }
        let mut exclude = String::from(ALWAYS_LEFT_OUT);
        for entry in entries {
            let path = self.work_tree.join(entry);
            let nested_repo = entry.ends_with('/') && path.join(".git").exists();
            let large = std::fs::symlink_metadata(&path)
                .is_ok_and(|meta| meta.is_file() && meta.len() > MAX_FILE_BYTES);
            // A name with pattern characters cannot be written into the list as itself; it is recorded instead.
            if (nested_repo || large) && !entry.contains(['\n', '*', '?', '[', '\\', '!', '#']) {
                exclude.push('/');
                exclude.push_str(entry);
                exclude.push('\n');
            }
        }
        std::fs::write(self.git_dir.join("info").join("exclude"), exclude)
            .map_err(|err| err.to_string())
    }

    fn index(&self) -> PathBuf {
        self.git_dir.join(format!("index-{}", self.record.run))
    }

    fn reference(&self) -> String {
        format!("refs/axh/{}", self.record.run)
    }

    fn write_record(&self) -> Result<(), String> {
        let runs = self.git_dir.join("runs");
        std::fs::create_dir_all(&runs).map_err(|err| err.to_string())?;
        let json = serde_json::to_string_pretty(&self.record).map_err(|err| err.to_string())?;
        std::fs::write(runs.join(format!("{}.json", self.record.run)), json)
            .map_err(|err| err.to_string())
    }
}

/// The runs recorded for `work_tree`, newest first.
pub fn runs(root: &Path, work_tree: &Path) -> Vec<Record> {
    let Ok(work_tree) = work_tree.canonicalize() else {
        return Vec::new();
    };
    let dir = store_of(root, &work_tree).join("runs");
    let mut found: Vec<(SystemTime, Record)> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let modified = entry.metadata().ok()?.modified().ok()?;
            let text = std::fs::read_to_string(entry.path()).ok()?;
            Some((modified, serde_json::from_str(&text).ok()?))
        })
        .collect();
    found.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    found.into_iter().map(|(_, record)| record).collect()
}

/// Puts `work_tree` back as it was before `run` (or before its step `step`): the recorded files as they were, the
/// files added since removed. Returns the commit restored.
///
/// # Errors
///
/// No such run or step, a malformed record, or a git failure.
pub fn restore(
    root: &Path,
    work_tree: &Path,
    run: &str,
    step: Option<u32>,
) -> Result<String, String> {
    let run = checked_run(run)?;
    let work_tree = work_tree
        .canonicalize()
        .map_err(|err| format!("cannot use {}: {err}", work_tree.display()))?;
    let record = runs(root, &work_tree)
        .into_iter()
        .find(|record| record.run == run)
        .ok_or_else(|| format!("no snapshots of a run {run:?} for {}", work_tree.display()))?;
    let target = match step {
        None => record.steps.first(),
        Some(turn) => record.steps.iter().find(|s| s.turn == turn),
    }
    .ok_or_else(|| {
        format!(
            "run {run} has no snapshot before step {}",
            step.unwrap_or(0)
        )
    })?;
    let commit = checked_commit(&target.commit)?;
    let git_dir = store_of(root, &work_tree);
    let _locked = lock(&git_dir)?;
    let index = git_dir.join(format!("index-{run}"));
    let git = |args: &[&str]| git_with_index(&git_dir, &work_tree, &index, args);
    // The index becomes the snapshot; the folder follows it, and what the index does not know (added since) goes —
    // what was left out stays, it was never recorded.
    git(&["read-tree", &format!("{commit}^{{tree}}")])?;
    git(&["checkout-index", "--all", "--force"])?;
    git(&["clean", "--force", "-d", "--quiet"])?;
    Ok(commit.to_owned())
}

/// Removes the records older than [`KEEP`] and their refs from every store under `root`, and collects a store's
/// garbage at most once a day. Best effort: a store that cannot be swept is left as it is.
pub fn sweep(root: &Path) {
    let now = SystemTime::now();
    let old = |path: &Path, age: Duration| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > age)
    };
    for store in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let git_dir = store.path();
        let marker = git_dir.join("swept");
        if marker.exists() && !old(&marker, GC_EVERY) {
            continue;
        }
        let Ok(folder) = std::fs::read_to_string(git_dir.join("folder")) else {
            continue;
        };
        let Ok(_locked) = lock(&git_dir) else {
            continue;
        };
        let folder = Path::new(&folder);
        for record in std::fs::read_dir(git_dir.join("runs"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if !old(&record.path(), KEEP) {
                continue;
            }
            let run = record
                .path()
                .file_stem()
                .and_then(|s| s.to_str())
                .map(str::to_owned);
            if let Some(run) = run.filter(|run| checked_run(run).is_ok()) {
                let _ = git(
                    &git_dir,
                    folder,
                    &["update-ref", "-d", &format!("refs/axh/{run}")],
                );
                let _ = std::fs::remove_file(git_dir.join(format!("index-{run}")));
            }
            let _ = std::fs::remove_file(record.path());
        }
        // Objects only an expired run's ref held are unreachable now; the lock keeps a running snapshot out.
        let _ = git(&git_dir, folder, &["gc", "--quiet", "--prune=now"]);
        let _ = std::fs::write(&marker, b"");
    }
}

/// A folder only the owner can enter: the snapshots hold the folder's files, untracked secrets included.
fn make_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("cannot make {}: {err}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
            .map_err(|err| format!("cannot protect {}: {err}", dir.display()))?;
    }
    Ok(())
}

/// The store's lock, held while a snapshot, a restore or a sweep works on it.
fn lock(git_dir: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(git_dir.join("lock"))
        .map_err(|err| format!("cannot lock the snapshot store: {err}"))?;
    file.lock()
        .map_err(|err| format!("cannot lock the snapshot store: {err}"))?;
    Ok(file)
}

/// Makes the shadow store: a bare repository, used with a work tree only after (`git init` takes no `--work-tree`).
fn init(git_dir: &Path) -> Result<(), String> {
    let mut command = isolated(Command::new("git"));
    command.args(["init", "--quiet", "--bare"]).arg(git_dir);
    run_limited(command, "init").map(drop)
}

/// `command` with a cleared environment — `PATH` and `HOME` only, and nothing that points git at the owner's config.
fn isolated(mut command: Command) -> Command {
    command.env_clear();
    for key in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn git(git_dir: &Path, work_tree: &Path, args: &[&str]) -> Result<String, String> {
    run_limited(
        git_command(git_dir, work_tree, None, args),
        args.first().unwrap_or(&""),
    )
}

fn git_with_index(
    git_dir: &Path,
    work_tree: &Path,
    index: &Path,
    args: &[&str],
) -> Result<String, String> {
    run_limited(
        git_command(git_dir, work_tree, Some(index), args),
        args.first().unwrap_or(&""),
    )
}

/// Git on the shadow store, cut off from the owner's own setup; with `index`, the run's own index file.
fn git_command(git_dir: &Path, work_tree: &Path, index: Option<&Path>, args: &[&str]) -> Command {
    let mut command = isolated(Command::new("git"));
    command
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(work_tree)
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
            "-c",
            "core.excludesFile=/dev/null",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=axh",
            "-c",
            "user.email=axh@localhost",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args);
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command
}

/// Runs `command` within [`GIT_LIMIT`], its output read on threads so a large answer never blocks it.
fn run_limited(mut command: Command, what: &str) -> Result<String, String> {
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not run git: {err}"))?;
    let read = |pipe: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_string(&mut text);
            }
            text
        })
    };
    let out = read(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let err = read(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    );
    let deadline = Instant::now() + GIT_LIMIT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "git {what} took longer than {} s",
                    GIT_LIMIT.as_secs()
                ));
            }
            Err(e) => return Err(format!("git {what}: {e}")),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        Err(format!("git {what} failed: {}", stderr.trim()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-snap-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn undo_puts_the_folder_back_before_the_run_or_a_step_and_leaves_its_own_git_alone() {
        let (root, folder) = (temp("root"), temp("folder"));
        std::fs::write(folder.join("a.txt"), "one").unwrap();
        std::fs::write(folder.join(".gitignore"), "ignored.log\n").unwrap();
        std::fs::write(folder.join("ignored.log"), "keep").unwrap();
        std::fs::create_dir_all(folder.join("target")).unwrap();
        std::fs::write(folder.join("target/out"), "build").unwrap();
        std::fs::write(
            folder.join("big.bin"),
            vec![0u8; (MAX_FILE_BYTES + 1) as usize],
        )
        .unwrap();

        let mut snaps = Snapshots::open(&root, &folder, "run1").unwrap();
        assert!(snaps.take(1).unwrap().is_some());
        assert!(snaps.take(1).unwrap().is_none(), "once per turn");
        std::fs::write(folder.join("a.txt"), "two").unwrap();
        std::fs::write(folder.join("new.txt"), "added").unwrap();
        snaps.take(2).unwrap().unwrap();
        std::fs::write(folder.join("a.txt"), "three").unwrap();
        // Another run on the same folder keeps its own chain and index.
        let mut other = Snapshots::open(&root, &folder, "run2").unwrap();
        other.take(1).unwrap().unwrap();

        // Before step 2: a.txt "two", new.txt there.
        restore(&root, &folder, "run1", Some(2)).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).unwrap(),
            "two"
        );
        assert!(folder.join("new.txt").exists());
        // Before the run: a.txt "one", new.txt gone; what is left out — ignored, build output, a large file — stays.
        restore(&root, &folder, "run1", None).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).unwrap(),
            "one"
        );
        assert!(!folder.join("new.txt").exists());
        assert_eq!(
            std::fs::read_to_string(folder.join("ignored.log")).unwrap(),
            "keep"
        );
        assert_eq!(
            std::fs::read_to_string(folder.join("target/out")).unwrap(),
            "build"
        );
        assert!(folder.join("big.bin").exists());
        assert!(
            !folder.join(".git").exists(),
            "nothing is written into the folder"
        );

        assert_eq!(runs(&root, &folder).len(), 2);
        assert!(restore(&root, &folder, "nope", None).is_err());
        assert!(restore(&root, &folder, "run1", Some(9)).is_err());
        assert!(restore(&root, &folder, "../x", None).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(store_of(&root, &folder))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        sweep(&root);
        assert_eq!(
            runs(&root, &folder).len(),
            2,
            "fresh records outlive the sweep"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn an_expired_run_loses_its_ref_and_its_record() {
        let (root, folder) = (temp("root-exp"), temp("folder-exp"));
        std::fs::write(folder.join("a.txt"), "one").unwrap();
        let mut snaps = Snapshots::open(&root, &folder, "old").unwrap();
        snaps.take(1).unwrap().unwrap();
        let store = store_of(&root, &folder);
        let record = store.join("runs/old.json");
        let past = SystemTime::now() - KEEP - Duration::from_secs(60);
        File::options()
            .write(true)
            .open(&record)
            .unwrap()
            .set_modified(past)
            .unwrap();
        sweep(&root);
        assert!(runs(&root, &folder).is_empty());
        assert!(
            git(&store, &folder, &["show-ref"])
                .unwrap_or_default()
                .trim()
                .is_empty()
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn the_home_folder_root_and_bad_ids_get_no_snapshots() {
        let root = temp("root-home");
        assert!(Snapshots::open(&root, Path::new("/"), "r").is_err());
        if let Some(home) = std::env::var_os("HOME") {
            assert!(Snapshots::open(&root, Path::new(&home), "r").is_err());
        }
        assert!(Snapshots::open(&root, &root, "bad id").is_err());
        assert!(checked_commit("--index-output=/tmp/x").is_err());
        assert!(checked_commit(&"a".repeat(40)).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }
}
