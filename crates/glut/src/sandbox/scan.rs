//! What a mount cannot say as a pattern (T59): Seatbelt refuses any `.env` at any depth and any new `.git`, `.envrc`
//! or protected name by a regex, existing or not; bubblewrap covers only what is there when the command starts. So
//! before every command the roots are searched for what to cover, and right after it for what it made that a pattern
//! would have refused — moved to a quarantine below `~/.glut`, never deleted.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::{GIT_RUNS, PROTECTED_IN_ROOT};

/// Folders searched no further where they carry their tool's mark: what a build or a package manager fills, where a
/// project keeps no `.env` of its own. One of such a name without its mark is searched like any other — a command
/// could name a folder so to hide a `.git` in it. Git folders are searched on their own ([`guard_git_dir`]).
const SKIPPED: [(&str, &[&str]); 5] = [
    ("target", &["CACHEDIR.TAG"]),
    (
        "node_modules",
        &[
            ".package-lock.json",
            ".yarn-state.yml",
            ".modules.yaml",
            ".yarn-integrity",
        ],
    ),
    (".venv", &["pyvenv.cfg"]),
    ("venv", &["pyvenv.cfg"]),
    (".jj", &["repo"]),
];

/// The most entries one search looks at: a folder beside the working one may hold many projects. Past it the search
/// stops, [`Found::complete`] says so, and no command runs ([`super::Policy::command`]): what was not seen is not
/// covered.
pub const MAX_ENTRIES: usize = 500_000;

/// What one search found, in real paths.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    /// The `.env` files (`.env`, `.env.*`, `*.env`, `.envrc`, `.envrc.*`) no command reads.
    pub env_files: Vec<PathBuf>,
    /// What a command may not move, and only a git folder may write: the protected names in a root, every `.git` and
    /// `.envrc` at any depth, a protected name at any depth in a folder beside the working one, and every git folder's
    /// runnable parts — each with what it is now, so one replaced is told from one kept.
    pub guarded: BTreeMap<PathBuf, Guard>,
    /// The git folders found below the roots (a nested repository's or a submodule's `.git`), beside the run's own.
    pub git_dirs: Vec<PathBuf>,
    /// Whether the search saw everything ([`MAX_ENTRIES`]).
    pub complete: bool,
}

/// One guarded path as the search saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Guard {
    /// What makes it this file: another one under the same name is another file.
    id: Identity,
    /// A git folder or one holding git folders (`modules`, `worktrees`): written by commits, only kept in place.
    pub writable: bool,
}

/// A guarded path's file as one search saw it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
    /// When its inode last changed (seconds, nanoseconds).
    changed: (i64, i64),
    size: u64,
    /// Where a link leads.
    target: Option<PathBuf>,
}

/// Searches the working folder `root`, the folders `beside` it and the repository's `git_dirs`.
pub fn search(root: &Path, beside: &[PathBuf], git_dirs: &[PathBuf]) -> Found {
    let mut found = Found {
        complete: true,
        ..Found::default()
    };
    let mut seen = 0usize;
    for name in PROTECTED_IN_ROOT {
        guard(&mut found, root.join(name), false);
    }
    walk(root, false, &mut found, &mut seen);
    for folder in beside {
        walk(folder, true, &mut found, &mut seen);
    }
    for dir in git_dirs.iter().chain(found.git_dirs.clone().iter()) {
        guard_git_dir(dir, &mut found);
    }
    found.env_files.sort();
    found.env_files.dedup();
    found
}

/// What appeared in `after` that `before` did not hold: made by the command that ran between them. One that was there
/// before under another name only moved with a folder (`mv packages/a packages/b` with a repository in it).
pub fn appeared(before: &Found, after: &Found) -> Vec<PathBuf> {
    let known: std::collections::HashSet<(u64, u64)> = before
        .guarded
        .values()
        .map(|guard| (guard.id.device, guard.id.inode))
        .collect();
    let mut new: Vec<PathBuf> = after
        .guarded
        .iter()
        .filter(|(path, now)| {
            before
                .guarded
                .get(*path)
                .is_none_or(|then| then.id != now.id)
        })
        .filter(|(path, now)| {
            before.guarded.contains_key(*path) || !known.contains(&(now.id.device, now.id.inode))
        })
        .map(|(path, _)| path.clone())
        .collect();
    // A new folder brings what is in it along: only its top is moved.
    new.sort();
    let mut tops: Vec<PathBuf> = Vec::new();
    for path in new {
        if !tops.iter().any(|top| path.starts_with(top)) {
            tops.push(path);
        }
    }
    tops
}

/// Moves each of `paths` below `quarantine`, keeping its whole path there; returns where each went, or why it could
/// not. Glut does this outside the sandbox, so every folder on the way is opened without following a link — one a
/// command put there would lead elsewhere. Where the quarantine lies on another file system, the path is renamed in
/// place to a name nothing runs (`.glut-refused-<n>-<name>`).
pub fn quarantine(paths: &[PathBuf], quarantine: &Path) -> Vec<(PathBuf, Result<PathBuf, String>)> {
    paths
        .iter()
        .enumerate()
        .map(|(at, path)| {
            let moved = move_away(path, quarantine, at).map_err(|err| err.to_string());
            (path.clone(), moved)
        })
        .collect()
}

fn move_away(path: &Path, quarantine: &Path, at: usize) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(std::io::Error::other("not a path to move"));
    };
    let target = quarantine.join(path.strip_prefix("/").unwrap_or(path));
    let held = target.parent().unwrap_or(quarantine);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(held)?;
    let from = open_without_links(parent)?;
    let to = rustix::fs::open(
        held,
        rustix::fs::OFlags::DIRECTORY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::RDONLY,
        rustix::fs::Mode::empty(),
    )?;
    match rustix::fs::renameat(&from, name, &to, name) {
        Ok(()) => Ok(target),
        Err(rustix::io::Errno::XDEV) => {
            let mut refused = std::ffi::OsString::from(format!(".glut-refused-{at}-"));
            refused.push(name);
            rustix::fs::renameat(&from, name, &from, refused.as_os_str())?;
            Ok(parent.join(refused))
        }
        Err(err) => Err(err.into()),
    }
}

/// The folder `path` (absolute), opened one part at a time, none of them a link.
fn open_without_links(path: &Path) -> std::io::Result<rustix::fd::OwnedFd> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::RDONLY;
    let mut folder = rustix::fs::open("/", flags, Mode::empty())?;
    for part in path.components() {
        match part {
            std::path::Component::RootDir => {}
            std::path::Component::Normal(name) => {
                folder = rustix::fs::openat(&folder, name, flags, Mode::empty())?
            }
            _ => return Err(std::io::Error::other("not a plain path")),
        }
    }
    Ok(folder)
}

/// A command's search from before it ran, to tell what it made once it ended.
#[derive(Debug)]
pub struct Watch {
    /// `None` once settled.
    before: Option<Found>,
    root: PathBuf,
    beside: Vec<PathBuf>,
    git_dirs: Vec<PathBuf>,
    quarantine: PathBuf,
}

impl Watch {
    /// Watches a command about to run under `policy`, with `before` searched for it.
    pub fn new(policy: &super::Policy, before: Found) -> Self {
        Watch {
            before: Some(before),
            root: policy.root.clone(),
            beside: policy.beside.clone(),
            git_dirs: policy.git_dirs.clone(),
            quarantine: policy.quarantine.clone(),
        }
    }

    /// Moves what the command made that the sandbox refuses to the quarantine, and says so for its output.
    pub fn settle(mut self) -> Option<String> {
        self.settle_now()
    }

    fn settle_now(&mut self) -> Option<String> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let before = self.before.take()?;
        let after = search(&self.root, &self.beside, &self.git_dirs);
        let made = appeared(&before, &after);
        let mut notes = String::new();
        if !after.complete {
            notes.push_str(&format!(
                "[the folders hold more than {MAX_ENTRIES} entries: what the command made past them was not checked]\n"
            ));
        }
        if !made.is_empty() {
            let secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_secs());
            let held = self.quarantine.join(format!(
                "{secs}-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            for (path, moved) in quarantine(&made, &held) {
                notes.push_str(&match moved {
                    Ok(to) => format!(
                        "[refused by the sandbox: the command made {}, which no command may; Glut moved it to {}]\n",
                        path.display(),
                        to.display()
                    ),
                    Err(err) => format!(
                        "[refused by the sandbox: the command made {}, which no command may; Glut could not move it \
                         away ({err}) — do not use it]\n",
                        path.display()
                    ),
                });
            }
        }
        (!notes.is_empty()).then_some(notes)
    }
}

impl Drop for Watch {
    /// A command whose run stopped or timed out before it was settled is settled all the same: what it made goes,
    /// though nobody reads the note.
    fn drop(&mut self) {
        let _ = self.settle_now();
    }
}

/// Records `path` as guarded if it is there (a link too, never followed).
fn guard(found: &mut Found, path: PathBuf, writable: bool) {
    if let Ok(meta) = std::fs::symlink_metadata(&path) {
        let writable = writable && meta.is_dir();
        // A file system may give a new file the number of one just removed, even within the same tick of its clock:
        // the size and a link's target tell them apart too. Not for a folder, whose time and size move with every
        // entry made in it (a commit, an editor's save). What the sandbox mounts cannot be replaced at all; this
        // tells what it cannot mount (a link) when it is.
        let id = if meta.is_dir() {
            Identity {
                device: meta.dev(),
                inode: meta.ino(),
                ..Identity::default()
            }
        } else {
            Identity {
                device: meta.dev(),
                inode: meta.ino(),
                changed: (meta.ctime(), meta.ctime_nsec()),
                size: meta.len(),
                target: std::fs::read_link(&path).ok(),
            }
        };
        found.guarded.insert(path, Guard { id, writable });
    }
}

/// Whether `name` is one of the `.env` files.
fn is_env(name: &str) -> bool {
    name == ".env"
        || name.starts_with(".env.")
        || (name.ends_with(".env") && name.len() > ".env".len())
        || name == ".envrc"
        || name.starts_with(".envrc.")
}

/// Searches `top` without following a link; `beside`: a folder beside the working one, where a protected name counts
/// at any depth and in any case.
fn walk(top: &Path, beside: bool, found: &mut Found, seen: &mut usize) {
    // Breadth first: past the limit, what is left uncovered lies deepest.
    let mut pending = std::collections::VecDeque::from([top.to_path_buf()]);
    while let Some(dir) = pending.pop_front() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            *seen += 1;
            if *seen > MAX_ENTRIES {
                found.complete = false;
                return;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if lower == ".git" {
                guard(found, path.clone(), true);
                if kind.is_dir() {
                    found.git_dirs.push(path);
                }
                continue;
            }
            if beside && (PROTECTED_IN_ROOT.contains(&lower.as_str()) || lower == ".jj")
                || lower == ".envrc"
            {
                guard(found, path.clone(), false);
            }
            // A file, or a link to one in this folder: a link to a folder (`ln -s /usr .env`) or out of it covers
            // nothing of the project's and would cover what it names.
            let env_file = kind.is_file()
                || kind.is_symlink()
                    && path
                        .canonicalize()
                        .is_ok_and(|real| real.starts_with(top) && real.is_file());
            if is_env(&name) && env_file {
                found.env_files.push(path.clone());
            }
            if kind.is_dir() && !skipped(&name, &path) {
                pending.push_back(path);
            }
        }
    }
}

/// Whether the folder `name` at `path` is one a tool fills, by the mark it leaves ([`SKIPPED`]).
fn skipped(name: &str, path: &Path) -> bool {
    SKIPPED.iter().any(|(skip, marks)| {
        *skip == name
            && marks
                .iter()
                .any(|mark| std::fs::symlink_metadata(path.join(mark)).is_ok())
    })
}

/// A git folder's runnable parts, and those of the git folders below it (`modules/a`, `worktrees/x`).
fn guard_git_dir(dir: &Path, found: &mut Found) {
    let mut pending = vec![(dir.to_path_buf(), 0u8)];
    while let Some((git, depth)) = pending.pop() {
        for part in GIT_RUNS {
            guard(found, git.join(part), false);
        }
        // Submodules nest: a few levels, never without end.
        if depth >= 4 {
            continue;
        }
        for holder in ["modules", "worktrees"] {
            let holder = git.join(holder);
            guard(found, holder.clone(), true);
            let Ok(entries) = std::fs::read_dir(&holder) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    guard(found, entry.path(), true);
                    pending.push((entry.path(), depth + 1));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> PathBuf {
        // Numbered, not timed: Linux's clock can give two tests at once the same moment.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "glut-scan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    #[test]
    fn env_files_are_found_at_any_depth_but_not_in_build_folders() {
        let root = folder();
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/x")).unwrap();
        std::fs::create_dir_all(root.join("venv")).unwrap();
        for file in [
            ".env",
            "a/.env.local",
            "a/b/prod.env",
            "node_modules/.package-lock.json",
            "node_modules/x/.env",
            "venv/.env",
            "a/environment",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        // A link to a folder named so covers nothing; one to a file in the project does.
        std::os::unix::fs::symlink("/usr", root.join("a/.env.dir")).unwrap();
        std::os::unix::fs::symlink(root.join(".env"), root.join("a/b/.env")).unwrap();
        let found = search(&root, &[], &[]);
        assert_eq!(
            found.env_files,
            vec![
                root.join(".env"),
                root.join("a/.env.local"),
                root.join("a/b/.env"),
                root.join("a/b/prod.env"),
                // Without npm's mark, a `venv` is searched like any folder.
                root.join("venv/.env"),
            ]
        );
        assert!(found.complete);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn what_a_command_made_is_told_apart_and_moved_whole() {
        let root = folder();
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::write(root.join(".git/config"), "").unwrap();
        let before = search(&root, &[], &[root.join(".git")]);
        assert!(before.guarded.contains_key(&root.join(".git/hooks")));
        std::fs::create_dir_all(root.join("sub/.git/hooks")).unwrap();
        std::fs::write(root.join("sub/.envrc"), "").unwrap();
        std::fs::create_dir_all(root.join(".vscode")).unwrap();
        std::fs::write(root.join(".vscode/tasks.json"), "").unwrap();
        let after = search(&root, &[], &[root.join(".git")]);
        let new = appeared(&before, &after);
        // Replaced under the same name: another file.
        std::fs::remove_file(root.join(".git/config")).unwrap();
        std::fs::write(root.join(".git/config"), "[core]").unwrap();
        let replaced = appeared(&after, &search(&root, &[], &[root.join(".git")]));
        assert_eq!(replaced, vec![root.join(".git/config")]);
        assert_eq!(
            new,
            vec![
                root.join(".vscode"),
                root.join("sub/.envrc"),
                root.join("sub/.git")
            ]
        );
        let held = folder();
        let moved = quarantine(&new, &held);
        assert!(moved.iter().all(|(_, to)| to.is_ok()), "{moved:?}");
        assert!(!root.join("sub/.git").exists());
        assert!(
            held.join(root.strip_prefix("/").unwrap())
                .join("sub/.git/hooks")
                .is_dir()
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(held).unwrap();
    }

    #[test]
    fn a_repository_moved_with_its_folder_is_no_new_one() {
        let root = folder();
        std::fs::create_dir_all(root.join("packages/a/.git/hooks")).unwrap();
        let before = search(&root, &[], &[]);
        std::fs::rename(root.join("packages/a"), root.join("packages/b")).unwrap();
        std::fs::create_dir_all(root.join("packages/c/.git")).unwrap();
        let after = search(&root, &[], &[]);
        assert_eq!(
            appeared(&before, &after),
            vec![root.join("packages/c/.git")]
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn beside_a_protected_name_counts_at_any_depth_in_any_case() {
        let root = folder();
        let beside = folder();
        std::fs::create_dir_all(beside.join("project/.VSCode")).unwrap();
        let found = search(&root, std::slice::from_ref(&beside), &[]);
        assert!(found.guarded.contains_key(&beside.join("project/.VSCode")));
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(beside).unwrap();
    }
}
