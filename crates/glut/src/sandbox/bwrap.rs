//! The Linux sandbox's command line (T59): bubblewrap's mounts made from a [`Policy`] and what [`super::scan`] found.
//! Built on every system so its rules are tested on the Mac too; only [`super::linux`] runs it.
//!
//! The command sees the whole system read-only, its writable places bound over that, the protected parts of those
//! bound read-only again (a mount cannot be moved or removed, so a `.git` stays where it is), the secrets covered by
//! a folder and a file nobody may open (so a read is refused, as on macOS), and a network, processes, IPC and host name
//! of its own. Every capability is dropped, so a command started by root is no root inside.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::Policy;
use super::scan::Found;

/// The argument the program `glut` is started with inside the sandbox to act as its helper ([`super::linux`]).
pub const HELPER_FLAG: &str = "--glut-sandbox-helper";

/// The name of the folder nobody may open (mode 000) in [`Policy::covers`]: what a secret folder is covered by.
pub const COVER_FOLDER: &str = "folder";
/// The name of the empty file nobody may open (mode 000) in [`Policy::covers`]: what a secret file is covered by.
pub const COVER_FILE: &str = "file";

/// The cover `name` of `policy`, where it is in place: of the kind its name says, and open to nobody.
fn cover(policy: &Policy, name: &str) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let path = policy.covers.join(name);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    let kind = if name == COVER_FOLDER {
        meta.is_dir()
    } else {
        meta.is_file() && meta.len() == 0
    };
    (kind && meta.permissions().mode() & 0o777 == 0).then_some(path)
}

/// Makes the covers of `policy` where they are missing (Linux, before a command).
pub fn make_covers(policy: &Policy) {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let _ = std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&policy.covers);
    let folder = policy.covers.join(COVER_FOLDER);
    if std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&folder)
        .is_ok()
    {
        let _ = std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o000));
    }
    let file = policy.covers.join(COVER_FILE);
    if std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .is_ok()
    {
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000));
    }
}

/// Programs that hand a job to the system, run later outside the sandbox (T94): systemd's, cron's and at's. They
/// would fail anyway — D-Bus is a Unix socket the filter refuses, and a setuid program gets no rights under
/// no-new-privs — so they are covered, never left to try.
const LAUNCHERS: [&str; 10] = [
    "/usr/bin/systemctl",
    "/bin/systemctl",
    "/usr/bin/systemd-run",
    "/bin/systemd-run",
    "/usr/bin/crontab",
    "/bin/crontab",
    "/usr/bin/at",
    "/usr/bin/batch",
    "/usr/bin/loginctl",
    "/bin/loginctl",
];

/// What one path becomes in the sandbox; a path named twice takes the strongest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Mount {
    /// Writable, as on the host.
    Write,
    /// Read-only.
    Read,
    /// Covered: no command reads it.
    Hide,
}

/// The arguments for `bwrap` running `program` under `policy`, before the program's own: everything up to and
/// including the helper's `--` ([`HELPER_FLAG`]). `relay` is the bridge to the proxy outside (`port`, its socket),
/// `None` when the command reaches no network.
pub fn arguments(
    policy: &Policy,
    found: &Found,
    helper: &Path,
    relay: Option<&Path>,
    cwd: &Path,
    program: &Path,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "--die-with-parent",
        // Its own session: no `TIOCSTI` into the terminal Glut runs in.
        "--new-session",
        "--unshare-all",
        // The helper is the namespace's first process: no reaper of bubblewrap's own stays there outside the filter,
        // where a command could write its memory (`/proc/1/mem`) and act past the filter through it.
        "--as-pid-1",
        // Started by root, bubblewrap would leave the command root's powers (reading past any file's mode among them).
        "--cap-drop",
        "ALL",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
        "--tmpfs",
        "/dev/shm",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    // A covered folder turns read-only only once everything is mounted: a working folder inside it needs its mount
    // point made there first.
    let mut read_only_after = Vec::new();
    for (path, mount) in mounts(policy, found) {
        // A path that is not there is neither bound nor covered: bubblewrap would make it on the host.
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        match mount {
            Mount::Write => push(&mut args, &["--bind"], &[&path, &path]),
            Mount::Read => push(&mut args, &["--ro-bind"], &[&path, &path]),
            // A working folder inside: an empty tmpfs, where its mount point can be made (only the way to it shows).
            Mount::Hide if meta.is_dir() && policy.roots().any(|root| root.starts_with(&path)) => {
                push(&mut args, &["--tmpfs"], &[&path]);
                read_only_after.push(path);
            }
            // Covered by what nobody may open, so a read is refused as on macOS instead of finding an empty folder or
            // file (and a write into it fails instead of vanishing); `/dev/null` and a tmpfs where those are missing.
            Mount::Hide if meta.is_dir() => match cover(policy, COVER_FOLDER) {
                Some(cover) => push(&mut args, &["--ro-bind"], &[&cover, &path]),
                None => {
                    push(&mut args, &["--tmpfs"], &[&path]);
                    read_only_after.push(path);
                }
            },
            Mount::Hide => {
                let cover = cover(policy, COVER_FILE).unwrap_or_else(|| PathBuf::from("/dev/null"));
                push(&mut args, &["--ro-bind"], &[&cover, &path]);
            }
        }
    }
    for path in &read_only_after {
        push(&mut args, &["--remount-ro"], &[path]);
    }
    push(&mut args, &["--chdir"], &[cwd]);
    args.push("--".into());
    args.push(helper.into());
    args.push(HELPER_FLAG.into());
    // The roots the helper searches again inside, before the command starts: a folder moved between Glut's search and
    // the mounts (a job still running can) would leave a `.env` uncovered.
    for root in policy.roots() {
        args.push("--check".into());
        args.push(root.into());
    }
    if let (Some(socket), Some(port)) = (relay, policy.proxy_port) {
        args.push("--relay".into());
        args.push(port.to_string().into());
        args.push(socket.into());
    }
    args.push("--".into());
    args.push(program.into());
    args
}

/// Every path the policy and the search name, ordered so a folder comes before what lies in it: bubblewrap mounts in
/// order, and a later mount over a folder would cover what was mounted inside it.
fn mounts(policy: &Policy, found: &Found) -> BTreeMap<PathBuf, Mount> {
    let mut mounts: BTreeMap<PathBuf, Mount> = BTreeMap::new();
    let mut name = |path: &Path, mount: Mount| {
        let entry = mounts.entry(path.to_path_buf()).or_insert(mount);
        *entry = (*entry).max(mount);
    };
    for path in policy
        .roots()
        .chain(&policy.writable)
        .chain(&policy.git_dirs)
        .chain(&policy.writable_files)
        .chain(&found.git_dirs)
    {
        name(path, Mount::Write);
    }
    for path in &policy.write_denied {
        name(path, Mount::Read);
    }
    for (path, guard) in &found.guarded {
        // A git folder and the folders holding others stay writable: only bound, so they cannot be moved — unless
        // the owner keeps every git folder read-only.
        let write = guard.writable && !policy.git_read_only;
        name(path, if write { Mount::Write } else { Mount::Read });
    }
    if policy.git_read_only {
        for path in policy.git_dirs.iter().chain(&found.git_dirs) {
            name(path, Mount::Read);
        }
    }
    for path in &policy.notes {
        name(path, Mount::Write);
    }
    for path in policy.read_denied.iter().chain(&found.env_files) {
        name(path, Mount::Hide);
    }
    // What hands a job to the system to run later, outside the sandbox: covered, as macOS refuses `launchctl`.
    for program in LAUNCHERS {
        name(Path::new(program), Mount::Hide);
    }
    // A working folder inside a hidden one (a Studio worktree in `~/.axiomata/worktrees`) is read and written again:
    // its own mount comes after the cover, the folder sorting after the one holding it.
    for root in policy.roots() {
        if policy
            .read_denied
            .iter()
            .any(|hidden| hidden != root && root.starts_with(hidden))
        {
            mounts.insert(root.clone(), Mount::Write);
        }
    }
    mounts
}

fn push(args: &mut Vec<OsString>, flags: &[&str], paths: &[&Path]) {
    args.extend(flags.iter().map(OsString::from));
    args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glut-bwrap-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }

    /// The pairs `flag path…` of `args` as text, for asserting on.
    fn text(args: &[OsString]) -> String {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn writable_places_come_first_and_their_protected_parts_after() {
        let root = folder("order");
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(root.join(".vscode")).unwrap();
        std::fs::write(root.join(".env"), "SECRET=1").unwrap();
        let home = folder("order-home");
        let policy = Policy::new(&root, &home, &[], &[]);
        let found = super::super::scan::search(&root, &[], &policy.git_dirs);
        let args = text(&arguments(
            &policy,
            &found,
            Path::new("/usr/bin/glut"),
            None,
            &root,
            Path::new("/bin/sh"),
        ));
        let at = |needle: String| {
            args.find(&needle)
                .unwrap_or_else(|| panic!("{needle} in {args}"))
        };
        let root_text = root.display();
        let bound = at(format!("--bind {root_text} {root_text}"));
        assert!(bound < at(format!("--ro-bind {root_text}/.vscode")));
        assert!(bound < at(format!("--ro-bind /dev/null {root_text}/.env")));
        assert!(args.ends_with(&format!(
            "-- /usr/bin/glut {HELPER_FLAG} --check {root_text} -- /bin/sh"
        )));
        assert!(!args.contains("--relay"), "no network, no relay: {args}");
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn a_hidden_folder_is_covered_and_a_working_folder_inside_it_bound_again() {
        let home = folder("hidden-home");
        let worktree = home.join(".axiomata/worktrees/a");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        let policy = Policy::new(&worktree, &home, &[], &[]);
        let args = text(&arguments(
            &policy,
            &Found::default(),
            Path::new("/usr/bin/glut"),
            None,
            &worktree,
            Path::new("/bin/sh"),
        ));
        let hidden = home.join(".axiomata");
        let covered = args
            .find(&format!("--tmpfs {}", hidden.display()))
            .expect(&args);
        let bound = args
            .find(&format!(
                "--bind {} {}",
                worktree.display(),
                worktree.display()
            ))
            .expect(&args);
        assert!(covered < bound, "{args}");
        assert!(args.contains(&format!("--tmpfs {}", home.join(".ssh").display())));
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn with_git_read_only_every_git_folder_is_bound_read_only() {
        let root = folder("git-ro");
        std::fs::create_dir_all(root.join(".git/hooks")).unwrap();
        std::fs::create_dir_all(root.join("sub/.git")).unwrap();
        let home = folder("git-ro-home");
        let mut policy = Policy::new(&root, &home, &[], &[]);
        policy.git_dirs = vec![root.join(".git")];
        let found = super::super::scan::search(&root, &[], &policy.git_dirs);
        let bound = |policy: &Policy| {
            let args = text(&arguments(
                policy,
                &found,
                Path::new("/g"),
                None,
                &root,
                Path::new("/bin/sh"),
            ));
            [".git", "sub/.git"].map(|git| {
                let path = root.join(git).display().to_string();
                (
                    args.contains(&format!("--bind {path} {path}")),
                    args.contains(&format!("--ro-bind {path} {path}")),
                )
            })
        };
        assert_eq!(
            bound(&policy),
            [(true, false), (true, false)],
            "writable by default"
        );
        policy.git_read_only = true;
        assert_eq!(bound(&policy), [(false, true), (false, true)]);
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn the_relay_is_named_only_with_a_port() {
        let root = folder("relay");
        let home = folder("relay-home");
        let mut policy = Policy::new(&root, &home, &[], &[]);
        policy.proxy_port = Some(4242);
        let args = text(&arguments(
            &policy,
            &Found::default(),
            Path::new("/g"),
            Some(Path::new("/s/proxy.sock")),
            &root,
            Path::new("/bin/sh"),
        ));
        assert!(
            args.contains(&format!(
                "/g {HELPER_FLAG} --check {} --relay 4242 /s/proxy.sock -- /bin/sh",
                root.display()
            )),
            "{args}"
        );
        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(home).unwrap();
    }
}
