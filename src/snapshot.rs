//! Snapshots of the work folder and undo (Glut 2, D, G11 — after Opencode's snapshots): before the first step of a run
//! that may change files, and before every later such step, the folder is recorded in a shadow git store; `axh undo`
//! puts it back as it was before the run, or before one step.
//!
//! * **Outside the folder**: the store is `<root>/<hash of the folder>` (the harness's `~/.axh/snapshots`), used with
//!   `--git-dir` and `--work-tree` — nothing is written into the folder, its own `.git` is never touched, and its
//!   `.gitignore` decides what is recorded (plus `target/` and `node_modules/`).
//! * **Isolated from the owner's git setup**: no global or system config, no hooks, no fsmonitor, no signing — a
//!   snapshot must neither start a program nor ask for a passphrase.
//! * **One record per run** (`runs/<run>.json`: the steps and their commits), kept 7 days (G11); the store is
//!   garbage-collected at most once a day.
//! * **Undo restores the recorded files and removes the ones the run added**; what the `.gitignore` leaves out is
//!   neither recorded nor touched.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long a run's record and its snapshots are kept (G11).
pub const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// At most one garbage collection of a store a day.
const GC_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Folders never recorded whatever the `.gitignore` says: build output and installed packages, large and rebuilt.
const ALWAYS_LEFT_OUT: &str = "target/\nnode_modules/\n.git/\n";

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

impl Snapshots {
    /// The store for `work_tree` under `root`, made on first use, for the run `run`.
    ///
    /// # Errors
    ///
    /// Why the store cannot be used: git missing, the store not writable.
    pub fn open(root: &Path, work_tree: &Path, run: &str) -> Result<Self, String> {
        let git_dir = store_of(root, work_tree);
        if !git_dir.join("HEAD").exists() {
            std::fs::create_dir_all(&git_dir)
                .map_err(|err| format!("cannot make {}: {err}", git_dir.display()))?;
            init(&git_dir)?;
            git(&git_dir, work_tree, &["config", "core.bare", "false"])?;
            let info = git_dir.join("info");
            std::fs::create_dir_all(&info).map_err(|err| err.to_string())?;
            std::fs::write(info.join("exclude"), ALWAYS_LEFT_OUT).map_err(|err| err.to_string())?;
            std::fs::write(
                git_dir.join("folder"),
                work_tree.to_string_lossy().as_bytes(),
            )
            .map_err(|err| err.to_string())?;
        }
        Ok(Self {
            git_dir,
            work_tree: work_tree.to_owned(),
            record: Record {
                run: run.to_owned(),
                steps: Vec::new(),
            },
        })
    }

    /// Records the folder before `turn`'s first change; once per turn. Returns the commit when one was made.
    ///
    /// # Errors
    ///
    /// A git failure or a record that cannot be written.
    pub fn take(&mut self, turn: u32) -> Result<Option<String>, String> {
        if self
            .record
            .steps
            .last()
            .is_some_and(|step| step.turn == turn)
        {
            return Ok(None);
        }
        git(&self.git_dir, &self.work_tree, &["add", "--all"])?;
        let message = format!("{} before step {turn}", self.record.run);
        git(
            &self.git_dir,
            &self.work_tree,
            &[
                "commit",
                "--quiet",
                "--allow-empty",
                "--no-verify",
                "-m",
                &message,
            ],
        )?;
        let commit = git(&self.git_dir, &self.work_tree, &["rev-parse", "HEAD"])?
            .trim()
            .to_owned();
        self.record.steps.push(Step {
            turn,
            commit: commit.clone(),
        });
        self.write_record()?;
        Ok(Some(commit))
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
    let dir = store_of(root, work_tree).join("runs");
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
/// No such run or step, or a git failure.
pub fn restore(
    root: &Path,
    work_tree: &Path,
    run: &str,
    step: Option<u32>,
) -> Result<String, String> {
    let record = runs(root, work_tree)
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
    let git_dir = store_of(root, work_tree);
    // The index becomes the snapshot; the folder follows it, and what the index does not know (added since) goes —
    // ignored files stay, they were never recorded.
    git(&git_dir, work_tree, &["read-tree", &target.commit])?;
    git(&git_dir, work_tree, &["checkout-index", "--all", "--force"])?;
    git(&git_dir, work_tree, &["clean", "--force", "-d", "--quiet"])?;
    Ok(target.commit.clone())
}

/// Removes the records older than [`KEEP`] from every store under `root` and collects a store's garbage at most once
/// a day. Best effort: a store that cannot be swept is left as it is.
pub fn sweep(root: &Path) {
    let now = SystemTime::now();
    let old = |path: &Path, age: Duration| {
        std::fs::metadata(path)
            .and_then(|meta| meta.modified())
            .is_ok_and(|modified| now.duration_since(modified).unwrap_or_default() > age)
    };
    for store in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let git_dir = store.path();
        for record in std::fs::read_dir(git_dir.join("runs"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if old(&record.path(), KEEP) {
                let _ = std::fs::remove_file(record.path());
            }
        }
        let marker = git_dir.join("swept");
        if marker.exists() && !old(&marker, GC_EVERY) {
            continue;
        }
        let Ok(folder) = std::fs::read_to_string(git_dir.join("folder")) else {
            continue;
        };
        let keep = format!("{}.seconds.ago", KEEP.as_secs());
        let _ = git(
            &git_dir,
            Path::new(&folder),
            &["reflog", "expire", &format!("--expire={keep}"), "--all"],
        );
        let _ = git(
            &git_dir,
            Path::new(&folder),
            &["gc", "--quiet", &format!("--prune={keep}")],
        );
        let _ = std::fs::write(&marker, b"");
    }
}

/// Makes the shadow store: a bare repository, used with a work tree only after (`git init` takes no `--work-tree`).
fn init(git_dir: &Path) -> Result<(), String> {
    let output = isolated(Command::new("git"))
        .args(["init", "--quiet", "--bare"])
        .arg(git_dir)
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git init failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// `command` without the owner's global and system git config, prompts or an inherited repository.
fn isolated(mut command: Command) -> Command {
    command
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdin(Stdio::null());
    command
}

/// Runs git on the shadow store, cut off from the owner's own setup.
fn git(git_dir: &Path, work_tree: &Path, args: &[&str]) -> Result<String, String> {
    let output = isolated(Command::new("git"))
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
            "commit.gpgsign=false",
            "-c",
            "user.name=axh",
            "-c",
            "user.email=axh@localhost",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .map_err(|err| format!("could not run git: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
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

        let mut snaps = Snapshots::open(&root, &folder, "run1").unwrap();
        assert!(snaps.take(1).unwrap().is_some());
        assert!(snaps.take(1).unwrap().is_none(), "once per turn");
        std::fs::write(folder.join("a.txt"), "two").unwrap();
        std::fs::write(folder.join("new.txt"), "added").unwrap();
        snaps.take(2).unwrap().unwrap();
        std::fs::write(folder.join("a.txt"), "three").unwrap();

        // Before step 2: a.txt "two", new.txt there.
        restore(&root, &folder, "run1", Some(2)).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).unwrap(),
            "two"
        );
        assert!(folder.join("new.txt").exists());
        // Before the run: a.txt "one", new.txt gone, the ignored and the build output untouched.
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
        assert!(
            !folder.join(".git").exists(),
            "nothing is written into the folder"
        );

        assert_eq!(runs(&root, &folder)[0].steps.len(), 2);
        assert!(restore(&root, &folder, "nope", None).is_err());
        assert!(restore(&root, &folder, "run1", Some(9)).is_err());
        sweep(&root);
        assert_eq!(
            runs(&root, &folder).len(),
            1,
            "a fresh record outlives the sweep"
        );
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
