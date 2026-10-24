//! The branch the footer shows (`docs/plans/glut-tui.md`, T39): read from the repository's `HEAD` file, never by
//! running git — the folder's repository may be anyone's, and the footer is drawn many times a second.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How often the branch is looked at again: a checkout in another terminal shows within this.
const RECHECK: Duration = Duration::from_secs(2);
/// The most of a `HEAD` or `.git` file read: a ref's line is short, anything longer is no repository's.
const MOST_BYTES: u64 = 1024;
/// The length a detached `HEAD`'s commit id is shown at.
const SHORT_ID: usize = 7;

/// The branch of the folder's repository, looked at again at most every [`RECHECK`].
#[derive(Debug, Default)]
pub struct Branch {
    checked: Option<Instant>,
    name: Option<String>,
}

impl Branch {
    /// The branch's name, or a detached `HEAD`'s short id; `None` outside a repository.
    pub fn name(&mut self, cwd: &Path) -> Option<&str> {
        if self.checked.is_none_or(|at| at.elapsed() >= RECHECK) {
            self.checked = Some(Instant::now());
            self.name = head_of(cwd);
        }
        self.name.as_deref()
    }
}

/// What `HEAD` names for the repository holding `cwd`: the nearest `.git` up from it, a folder or — in a worktree or
/// a submodule — a file pointing to one.
fn head_of(cwd: &Path) -> Option<String> {
    let git = cwd
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|git| git.exists())?;
    let dir = if git.is_dir() {
        git
    } else {
        let text = read_small(&git)?;
        let target = text.trim().strip_prefix("gitdir:")?.trim();
        let target = PathBuf::from(target);
        if target.is_absolute() {
            target
        } else {
            git.parent()?.join(target)
        }
    };
    named(&read_small(&dir.join("HEAD"))?)
}

/// A `HEAD` file's text as the footer shows it: `ref: refs/heads/main` is `main`, a commit id its first characters.
fn named(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref:") {
        let reference = reference.trim();
        let name = reference.strip_prefix("refs/heads/").unwrap_or(reference);
        return (!name.is_empty()).then(|| name.to_owned());
    }
    (head.len() >= SHORT_ID && head.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| head[..SHORT_ID].to_owned())
}

/// A small regular file's text, or `None` when it cannot be read, is not text, is too long to be what it should, or
/// is no regular file: the folder is anyone's, and a FIFO, a device or a link there must not hold up the screen —
/// opened without following a link and without waiting, then checked for what it is.
fn read_small(path: &Path) -> Option<String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MOST_BYTES + 1).read_to_string(&mut text).ok()?;
    (text.len() as u64 <= MOST_BYTES).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_head_names_its_branch_or_a_detached_commit_and_nothing_else() {
        assert_eq!(named("ref: refs/heads/main\n").as_deref(), Some("main"));
        assert_eq!(
            named("ref: refs/heads/feat/t39").as_deref(),
            Some("feat/t39")
        );
        assert_eq!(
            named("d3446f1c0ffee0000000000000000000000000000").as_deref(),
            Some("d3446f1")
        );
        assert_eq!(named("not a head"), None);
        assert_eq!(named("ref: "), None);
    }

    #[test]
    fn the_branch_is_found_up_the_folders_and_through_a_worktrees_git_file() {
        let root = std::env::temp_dir().join(format!("glut-tui-branch-{}", std::process::id()));
        let repo = root.join("repo");
        let inner = repo.join("src/deep");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(head_of(&inner).as_deref(), Some("main"));
        // A worktree: its `.git` is a file naming the folder that holds its own HEAD.
        let tree = root.join("tree");
        let held = repo.join(".git/worktrees/tree");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::create_dir_all(&held).unwrap();
        std::fs::write(held.join("HEAD"), "ref: refs/heads/t39\n").unwrap();
        std::fs::write(tree.join(".git"), "gitdir: ../repo/.git/worktrees/tree\n").unwrap();
        assert_eq!(head_of(&tree).as_deref(), Some("t39"));
        std::fs::write(held.join("HEAD"), "x".repeat(2000)).unwrap();
        assert_eq!(head_of(&tree), None, "too long to be a HEAD");
        std::fs::remove_file(held.join("HEAD")).unwrap();
        std::os::unix::fs::symlink(repo.join(".git/HEAD"), held.join("HEAD")).unwrap();
        assert_eq!(head_of(&tree), None, "a link is not followed");
        let _ = std::fs::remove_dir_all(&root);
    }
}
