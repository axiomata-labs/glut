//! A program in a pseudo-terminal of a fixed size: what it writes comes back through a channel, with when it came;
//! what the scene types goes in.

use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread;

use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
use rustix::termios::{Winsize, tcsetwinsize};

/// The program running, and its terminal.
pub struct Terminal {
    /// The program; its process group is ended with the take.
    pub child: Child,
    input: File,
    /// What the program wrote, chunk by chunk; closed when it ends.
    pub output: Receiver<Vec<u8>>,
}

impl Terminal {
    /// Starts `program` in `folder` on a new terminal of `cols` × `rows`, with the environment of an xterm that
    /// shows true colour.
    ///
    /// # Errors
    ///
    /// The terminal or the program cannot be started.
    pub fn start(
        program: &[String],
        folder: &Path,
        env: &[(&str, &Path)],
        cols: u16,
        rows: u16,
    ) -> Result<Self, String> {
        let fail = |what: &str, err: std::io::Error| format!("{what}: {err}");
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)
            .map_err(|err| fail("openpt", err.into()))?;
        // The recorder's end of the terminal must not reach Glut or the commands it runs: one of them could type
        // into it (an answer to Glut's own question). macOS has no flag for this on `openpt`, so it is set here.
        rustix::io::fcntl_setfd(&master, rustix::io::FdFlags::CLOEXEC)
            .map_err(|err| fail("close-on-exec", err.into()))?;
        grantpt(&master).map_err(|err| fail("grantpt", err.into()))?;
        unlockpt(&master).map_err(|err| fail("unlockpt", err.into()))?;
        let name = ptsname(&master, Vec::new()).map_err(|err| fail("ptsname", err.into()))?;
        let slave: OwnedFd = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name.to_string_lossy().as_ref())
            .map_err(|err| fail("open the terminal", err))?
            .into();
        let size = Winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        tcsetwinsize(&slave, size).map_err(|err| fail("set its size", err.into()))?;

        // Every fallible step before the program starts: nothing after the start may leave it running unreaped.
        let input = File::from(master.try_clone().map_err(|err| fail("dup", err))?);
        let (first, rest) = program.split_first().ok_or("no program to run")?;
        let mut command = Command::new(first);
        command
            .args(rest)
            .current_dir(folder)
            .stdin(Stdio::from(
                slave.try_clone().map_err(|err| fail("dup", err))?,
            ))
            .stdout(Stdio::from(
                slave.try_clone().map_err(|err| fail("dup", err))?,
            ))
            .stderr(Stdio::from(slave))
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            // English all through: Glut's words and the language the model is asked to answer in.
            .env("LANG", "en_US.UTF-8")
            .env("LC_ALL", "en_US.UTF-8")
            .env_remove("LC_MESSAGES")
            .env_remove("TERM_PROGRAM")
            .env_remove("TMUX")
            .envs(env.iter().map(|(name, value)| (*name, *value)));
        // SAFETY: runs in the child between fork and exec. It calls only setsid and one ioctl, both async-signal-safe
        // system calls, on fd 0, which std has already made the new terminal; nothing allocates or takes a lock (no
        // `io::stdin()`, whose first use builds a buffer). It makes that terminal the session's controlling one, as a
        // terminal emulator does, so the program reads keys from it and gets its signals.
        unsafe {
            command.pre_exec(|| {
                rustix::process::setsid().map_err(std::io::Error::from)?;
                rustix::process::ioctl_tiocsctty(rustix::stdio::stdin())
                    .map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        let child = command
            .spawn()
            .map_err(|err| fail(&format!("start {first}"), err))?;

        let mut reader = File::from(master);
        let (send, output) = channel();
        thread::spawn(move || {
            let mut buffer = [0u8; 16 * 1024];
            // Ends when the program closes the terminal: the read then fails or returns nothing.
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 || send.send(buffer[..read].to_vec()).is_err() {
                    break;
                }
            }
        });
        Ok(Terminal {
            child,
            input,
            output,
        })
    }

    /// Ends the program and every process it started (its own process group: it leads a session of its own),
    /// and waits for it.
    pub fn end(&mut self) {
        let group = rustix::process::Pid::from_child(&self.child);
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::TERM);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// Sends `bytes` as typed.
    ///
    /// # Errors
    ///
    /// The terminal is gone.
    pub fn send(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.input
            .write_all(bytes)
            .and_then(|()| self.input.flush())
            .map_err(|err| format!("cannot type: {err}"))
    }
}
