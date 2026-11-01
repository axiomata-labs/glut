//! The Linux sandbox (T59): bubblewrap runs Glut's own program as a helper inside the command's namespaces. The
//! helper bridges `127.0.0.1:<port>` there to the run's proxy outside (over a Unix socket Glut serves), then starts the
//! command under a seccomp filter: no new Unix socket (the owner's ssh-agent, Docker, D-Bus, Glut's window sockets), no
//! new namespace nor mount (bubblewrap's mounts hold only in the namespace they were made in), no `io_uring` (it
//! passes the filter by), no kernel keyring. The helper itself stays outside the filter: it alone reaches the bridge.

use std::ffi::OsString;
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt as _, ExitStatusExt as _};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use seccompiler::{
    BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter,
    SeccompRule,
};

use super::bwrap::HELPER_FLAG;

/// Where bubblewrap is looked for: the system's own, never a folder on a project's `PATH`.
const BWRAP_PLACES: [&str; 3] = ["/usr/bin/bwrap", "/bin/bwrap", "/usr/local/bin/bwrap"];

/// The helper's program, once the host said it serves the helper ([`serve_helper`]).
static HELPER: OnceLock<PathBuf> = OnceLock::new();

/// Whether the sandbox works here, tried once: `Err` holds why not and what helps.
static READY: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// The exit code of a helper that could not start the command.
const HELPER_FAILED: i32 = 125;

/// Runs the helper when this process was started as one, and returns its exit code; otherwise remembers that this
/// program serves the helper and returns `None`. A host calls it first thing in `main`, before any thread starts.
pub fn serve_helper() -> Option<i32> {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|arg| arg == HELPER_FLAG) {
        return Some(helper(&args[2..]));
    }
    if let Ok(program) = std::env::current_exe() {
        // A program replaced while it runs (a new `cargo install`) reads `<path> (deleted)` here: the new one at the
        // same path serves as the helper as well.
        let program = program
            .to_str()
            .and_then(|text| text.strip_suffix(" (deleted)"))
            .map_or(program.clone(), PathBuf::from);
        let _ = HELPER.set(program);
    }
    None
}

/// Uses `program` as the helper (a test's own build of `glut`).
pub fn use_helper(program: PathBuf) {
    let _ = HELPER.set(program);
}

/// The helper's program, where it is set — in the library's own tests, a built `glut` named by `GLUT_TEST_HELPER`
/// (the test binary cannot serve it), so the shell's sandbox tests run on Linux too.
pub(crate) fn helper_program() -> Option<&'static Path> {
    #[cfg(test)]
    if HELPER.get().is_none()
        && let Some(program) = std::env::var_os("GLUT_TEST_HELPER")
    {
        let _ = HELPER.set(PathBuf::from(program));
    }
    HELPER.get().map(PathBuf::as_path)
}

/// Bubblewrap, when the sandbox works here; else why not and what helps (tried once).
pub(crate) fn ready() -> Result<&'static Path, &'static str> {
    READY
        .get_or_init(try_ready)
        .as_ref()
        .map(PathBuf::as_path)
        .map_err(String::as_str)
}

fn try_ready() -> Result<PathBuf, String> {
    if helper_program().is_none() {
        return Err("this program does not serve the Linux sandbox's helper".into());
    }
    if filters().is_err() {
        return Err(format!(
            "the sandbox's seccomp filter is not built for {}",
            std::env::consts::ARCH
        ));
    }
    let Some(bwrap) = BWRAP_PLACES
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
    else {
        return Err("bubblewrap is not installed (the package `bubblewrap`)".into());
    };
    let tried = std::process::Command::new(&bwrap)
        .args([
            "--die-with-parent",
            "--unshare-all",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--",
            "/bin/true",
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .output();
    match tried {
        Ok(out) if out.status.success() => Ok(bwrap),
        Ok(out) => {
            let said = String::from_utf8_lossy(&out.stderr).trim().to_owned();
            Err(format!(
                "bubblewrap cannot make a sandbox here ({said}){}",
                namespace_help()
            ))
        }
        Err(err) => Err(format!("bubblewrap does not start: {err}")),
    }
}

/// What helps where namespaces are refused: Ubuntu's AppArmor rule, or a container without the right.
fn namespace_help() -> &'static str {
    let restricted =
        std::fs::read_to_string("/proc/sys/kernel/apparmor_restrict_unprivileged_userns")
            .is_ok_and(|value| value.trim() == "1");
    if restricted {
        "; Ubuntu's AppArmor refuses it: give bwrap an AppArmor profile that allows `userns` \
         (/etc/apparmor.d/bwrap), or set kernel.apparmor_restrict_unprivileged_userns=0"
    } else if Path::new("/.dockerenv").exists() || Path::new("/run/.containerenv").exists() {
        "; in a container it needs the right to make namespaces and its own /proc (Docker: \
         --security-opt seccomp=unconfined --security-opt systempaths=unconfined, or --privileged)"
    } else {
        ""
    }
}

/// The two filters a command starts under: what is refused (`EPERM`), and `clone3` answered "not here" (`ENOSYS`) —
/// its flags lie in memory a filter cannot read, and the C library falls back to `clone` on that answer.
fn filters() -> Result<Vec<BpfProgram>, String> {
    let arch = std::env::consts::ARCH
        .try_into()
        .map_err(|err: seccompiler::BackendError| err.to_string())?;
    let always = |numbers: &[libc::c_long]| -> Vec<(i64, Vec<SeccompRule>)> {
        numbers.iter().map(|number| (*number, Vec::new())).collect()
    };
    let flag = |bits: libc::c_int| -> Result<SeccompRule, String> {
        let bits = bits as u64;
        SeccompCondition::new(
            0,
            SeccompCmpArgLen::Qword,
            SeccompCmpOp::MaskedEq(bits),
            bits,
        )
        .and_then(|condition| SeccompRule::new(vec![condition]))
        .map_err(|err| err.to_string())
    };
    // A socket of any family but the network's own (the command's namespace holds those) and netlink's: a Unix one
    // reaches the owner's agents, a vsock the host of a VM, and the rest are no business of a build.
    let other_family = [libc::AF_INET, libc::AF_INET6, libc::AF_NETLINK]
        .iter()
        .map(|family| {
            SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Ne, *family as u64)
        })
        .collect::<Result<Vec<_>, _>>()
        .and_then(SeccompRule::new)
        .map_err(|err| err.to_string())?;
    let mut refused = always(&[
        libc::SYS_unshare,
        libc::SYS_setns,
        libc::SYS_mount,
        libc::SYS_umount2,
        libc::SYS_pivot_root,
        libc::SYS_open_tree,
        libc::SYS_move_mount,
        libc::SYS_fsopen,
        libc::SYS_fsconfig,
        libc::SYS_fsmount,
        libc::SYS_fspick,
        libc::SYS_mount_setattr,
        libc::SYS_io_uring_setup,
        libc::SYS_io_uring_enter,
        libc::SYS_io_uring_register,
        libc::SYS_keyctl,
        libc::SYS_add_key,
        libc::SYS_request_key,
        libc::SYS_bpf,
        // Into another process's memory or its open files: the helper is outside the filter.
        libc::SYS_process_vm_readv,
        libc::SYS_process_vm_writev,
        libc::SYS_pidfd_getfd,
    ]);
    refused.push((libc::SYS_socket, vec![other_family]));
    refused.push((
        libc::SYS_clone,
        [
            libc::CLONE_NEWUSER,
            libc::CLONE_NEWNS,
            libc::CLONE_NEWNET,
            libc::CLONE_NEWPID,
            libc::CLONE_NEWIPC,
            libc::CLONE_NEWUTS,
            libc::CLONE_NEWCGROUP,
        ]
        .into_iter()
        .map(flag)
        .collect::<Result<_, _>>()?,
    ));
    let refused = SeccompFilter::new(
        refused.into_iter().collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        arch,
    )
    .map_err(|err| err.to_string())?;
    let absent = SeccompFilter::new(
        always(&[libc::SYS_clone3]).into_iter().collect(),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::ENOSYS as u32),
        arch,
    )
    .map_err(|err| err.to_string())?;
    let mut programs = [refused, absent]
        .into_iter()
        .map(|filter| BpfProgram::try_from(filter).map_err(|err| err.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    if cfg!(target_arch = "x86_64") {
        programs.push(no_x32());
    }
    Ok(programs)
}

/// On x86_64, every call of the x32 ABI refused: its numbers carry bit 30 and match none the filters name, so a
/// kernel that has x32 would let `socket`, `unshare` and the rest through under them.
fn no_x32() -> BpfProgram {
    /// `AUDIT_ARCH_X86_64`: `EM_X86_64` (62), 64-bit, little-endian.
    const ARCH_X86_64: u32 = 62 | 0x8000_0000 | 0x4000_0000;
    const X32_BIT: u32 = 0x4000_0000;
    const RET_ERRNO: u32 = 0x0005_0000;
    const RET_ALLOW: u32 = 0x7fff_0000;
    let step = |code: u16, jt: u8, jf: u8, k: u32| seccompiler::sock_filter { code, jt, jf, k };
    // `seccomp_data`: the call's number at offset 0, its architecture at 4.
    let (load, jump_eq, jump_set, ret) = (0x20, 0x15, 0x45, 0x06);
    vec![
        step(load, 0, 0, 4),
        step(jump_eq, 0, 3, ARCH_X86_64),
        step(load, 0, 0, 0),
        step(jump_set, 0, 1, X32_BIT),
        step(ret, 0, 0, RET_ERRNO | libc::ENOSYS as u32),
        step(ret, 0, 0, RET_ALLOW),
    ]
}

/// The helper inside the sandbox: `[--check <root>]… [--relay <port> <socket>] -- <program> <args…>`.
fn helper(args: &[OsString]) -> i32 {
    match start(args) {
        Ok(status) => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(reason) => {
            eprintln!("glut's sandbox: {reason}");
            HELPER_FAILED
        }
    }
}

fn start(mut args: &[OsString]) -> Result<std::process::ExitStatus, String> {
    let mut checked = Vec::new();
    let mut relay = None;
    let command = loop {
        match args {
            [flag, root, more @ ..] if flag == "--check" => {
                checked.push(PathBuf::from(root));
                args = more;
            }
            [flag, port, socket, more @ ..] if flag == "--relay" => {
                let port: u16 = port
                    .to_str()
                    .and_then(|port| port.parse().ok())
                    .ok_or("the relay's port is not a number")?;
                relay = Some((port, PathBuf::from(socket)));
                args = more;
            }
            [end, command @ ..] if end == "--" => break command,
            _ => return Err("the helper's arguments are not its own".into()),
        }
    };
    let [program, rest @ ..] = command else {
        return Err("no command to run".into());
    };
    every_env_covered(&checked)?;
    // Bound before the command starts, so its first connection finds the relay.
    let listener = relay
        .as_ref()
        .map(|(port, _)| TcpListener::bind(("127.0.0.1", *port)))
        .transpose()
        .map_err(|err| format!("the relay cannot listen: {err}"))?;
    let filters = filters()?;
    // The helper stays outside the filter, so the command must not reach into it: not dumpable, no process of the
    // same user may trace it or write its memory. The command, a program of its own, is dumpable again after exec.
    // SAFETY: a plain `prctl` on this process.
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(format!(
            "cannot guard the helper: {}",
            std::io::Error::last_os_error()
        ));
    }
    let mut process = std::process::Command::new(program);
    process.args(rest);
    // SAFETY: between fork and exec only `prctl` and `seccomp` run, on memory made before the fork — no allocation,
    // no lock (this process has one thread until the relay starts after the spawn).
    unsafe {
        process.pre_exec(move || {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            for filter in &filters {
                // The system's own reason where it gave one: a filter that failed to load is told by it.
                seccompiler::apply_filter(filter).map_err(|err| match err {
                    seccompiler::Error::Prctl(err) | seccompiler::Error::Seccomp(err) => err,
                    _ => std::io::Error::from_raw_os_error(libc::EINVAL),
                })?;
            }
            Ok(())
        });
    }
    let child = process
        .spawn()
        .map_err(|err| format!("cannot start {}: {err}", Path::new(program).display()))?;
    if let (Some(listener), Some((_, socket))) = (listener, relay) {
        std::thread::spawn(move || relay_all(&listener, &socket));
    }
    reap_until(child.id())
}

/// Whether every `.env` file below `roots` is closed in here, as the mounts were meant to make it: a folder renamed
/// between Glut's search and bubblewrap's mounts would leave one open under its new name. What reads as a regular file
/// is open; a cover refuses the read, and its stand-in where none could be made is a device.
fn every_env_covered(roots: &[PathBuf]) -> Result<(), String> {
    for root in roots {
        let found = super::scan::search(root, &[], &[]);
        if !found.complete {
            return Err(format!(
                "{} holds more than {} entries: not every .env file in it can be covered",
                root.display(),
                super::scan::MAX_ENTRIES
            ));
        }
        for file in &found.env_files {
            if std::fs::File::open(file)
                .and_then(|open| open.metadata())
                .is_ok_and(|meta| meta.is_file())
            {
                return Err(format!(
                    "{} is readable in the sandbox (a folder moved as it started): the command did not run",
                    file.display()
                ));
            }
        }
    }
    Ok(())
}

/// Waits as the namespace's first process does: every orphan that ends here is reaped, until the command itself
/// ends — then the helper goes, and with it every process left in the namespace.
fn reap_until(command: u32) -> Result<std::process::ExitStatus, String> {
    let command = libc::pid_t::try_from(command).map_err(|_| "the command has no process id")?;
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: waits for any child; `status` is this frame's own.
        let ended = unsafe { libc::waitpid(-1, &mut status, 0) };
        if ended == command {
            return Ok(std::process::ExitStatus::from_raw(status));
        }
        if ended < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EINTR) {
                return Err(format!("cannot wait for the command: {err}"));
            }
        }
    }
}

/// Hands every connection on `listener` to the proxy's bridge at `socket`, until the helper ends.
fn relay_all(listener: &TcpListener, socket: &Path) {
    for client in listener.incoming().flatten() {
        let socket = socket.to_path_buf();
        std::thread::spawn(move || {
            if let Ok(upstream) = UnixStream::connect(&socket) {
                relay_one(client, upstream);
            }
        });
    }
}

/// Copies both ways until either side ends its half, then ends the other's.
fn relay_one(client: TcpStream, upstream: UnixStream) {
    let (Ok(mut client_in), Ok(mut upstream_out)) = (client.try_clone(), upstream.try_clone())
    else {
        return;
    };
    let out = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_in, &mut upstream_out);
        let _ = upstream_out.shutdown(Shutdown::Write);
    });
    let (mut upstream_in, mut client_out) = (upstream, client);
    let _ = std::io::copy(&mut upstream_in, &mut client_out);
    let _ = client_out.shutdown(Shutdown::Write);
    let _ = out.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filters_build_for_this_machine() {
        let built = filters().expect("filters");
        assert_eq!(
            built.len(),
            if cfg!(target_arch = "x86_64") { 3 } else { 2 }
        );
        assert!(built.iter().all(|program| !program.is_empty()));
    }
}
