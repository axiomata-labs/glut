//! The watch while a command runs (T59, the owner's choice of 2026-11-01): bubblewrap cannot refuse a name that is
//! not there yet, so what a command makes that Seatbelt would refuse — a `.git/commondir`, a `.cargo/config.toml`, a
//! new `.git` — exists until it is moved away. Moved only after the command, it could be run meanwhile by a tool of
//! the owner's (an editor's git, rust-analyzer, a prompt's `git status`). So inotify follows every folder the search
//! went through, and every one made during the command, and such a name goes to the quarantine the moment it appears.
//! It narrows the window to the time a move takes; it does not close it. The search after the command
//! ([`super::scan::Watch::settle`]) still checks everything.

use std::collections::HashMap;
use std::ffi::{CString, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread::JoinHandle;

use super::scan::Rules;

/// The most folders a new one brings along that the watch follows at once: a command that makes a deep tree in one
/// go is still checked whole after it.
const MOST_NEW_FOLDERS: usize = 10_000;

/// The most folders one watch follows: watches are counted per user (8192 on kernels before 5.11), and the owner's
/// editors need theirs. Past it, only the check after the command sees in.
const MOST_WATCHES: usize = 4096;

/// The most notes one watch keeps; past it, a count.
const MOST_NOTES: usize = 50;

/// A watch over a command's folders, until it is stopped.
pub struct Watcher {
    /// Written to wake the watch's thread when it is to stop.
    wake: libc::c_int,
    thread: Option<JoinHandle<Vec<String>>>,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("Watcher")
    }
}

impl Watcher {
    /// Watches `folders` by `rules`, moving what they refuse below `quarantine`; `None` where inotify cannot start.
    pub fn start(rules: Rules, folders: Vec<PathBuf>, quarantine: PathBuf) -> Option<Self> {
        // SAFETY: plain system calls; each descriptor is closed by its owner's `Drop`.
        let fd = unsafe { libc::inotify_init1(libc::IN_CLOEXEC | libc::IN_NONBLOCK) };
        if fd < 0 {
            return None;
        }
        let mut watch = Watch {
            fd,
            limit: watch_limit(),
            folders: HashMap::new(),
            rules,
            quarantine,
            moved: 0,
            blind: 0,
            overflowed: false,
            notes: Vec::new(),
            untold: 0,
        };
        // SAFETY: as above.
        let wake = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if wake < 0 {
            return None;
        }
        // Followed before the command starts: nothing it makes comes before its folder is watched.
        for folder in folders {
            watch.follow(&folder);
        }
        let woken = wake;
        let thread = match std::thread::Builder::new()
            .name("glut-sandbox-watch".into())
            .spawn(move || watch.run(woken))
        {
            Ok(thread) => thread,
            Err(_) => {
                // SAFETY: this watcher's own descriptor, not handed anywhere.
                unsafe { libc::close(wake) };
                return None;
            }
        };
        Some(Watcher {
            wake,
            thread: Some(thread),
        })
    }

    /// Stops the watch once it has taken what is waiting, and returns its notes.
    pub fn stop(mut self) -> Vec<String> {
        self.end()
    }

    fn end(&mut self) -> Vec<String> {
        let Some(thread) = self.thread.take() else {
            return Vec::new();
        };
        let one: u64 = 1;
        // SAFETY: writes eight bytes of this frame to the watcher's own eventfd.
        unsafe {
            libc::write(
                self.wake,
                (&raw const one).cast(),
                std::mem::size_of::<u64>(),
            )
        };
        let notes = thread.join().unwrap_or_default();
        // SAFETY: the thread is gone; the descriptor is closed once.
        unsafe { libc::close(self.wake) };
        notes
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.end();
    }
}

/// The watch's own state, on its thread.
struct Watch {
    fd: libc::c_int,
    /// The most folders followed at once ([`MOST_WATCHES`], a quarter of the system's limit where that is less).
    limit: usize,
    folders: HashMap<libc::c_int, PathBuf>,
    rules: Rules,
    quarantine: PathBuf,
    moved: usize,
    /// Folders the watch could not follow (the system's limit of watches): only the check after the command sees
    /// into them.
    blind: usize,
    /// Events the kernel dropped for a full queue.
    overflowed: bool,
    notes: Vec<String>,
    /// Notes past [`MOST_NOTES`], counted.
    untold: usize,
}

/// A quarter of the user's inotify watches, at most [`MOST_WATCHES`].
fn watch_limit() -> usize {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()
        .and_then(|text| text.trim().parse::<usize>().ok())
        .map_or(MOST_WATCHES, |most| (most / 4).min(MOST_WATCHES))
}

impl Drop for Watch {
    fn drop(&mut self) {
        // SAFETY: the watch's own descriptor, closed once, here.
        unsafe { libc::close(self.fd) };
    }
}

impl Watch {
    /// Follows `folder`: what is made in it is asked about.
    fn follow(&mut self, folder: &Path) {
        if self.folders.len() >= self.limit {
            self.blind += 1;
            return;
        }
        let Ok(path) = CString::new(folder.as_os_str().as_bytes()) else {
            return;
        };
        let mask = libc::IN_CREATE | libc::IN_MOVED_TO | libc::IN_ONLYDIR | libc::IN_DONT_FOLLOW;
        // SAFETY: `path` is a NUL-terminated string that lives for the call.
        let wd = unsafe { libc::inotify_add_watch(self.fd, path.as_ptr(), mask) };
        if wd >= 0 {
            self.folders.insert(wd, folder.to_path_buf());
        } else if std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
            self.blind += 1;
        }
    }

    /// Answers what is made until `wake` is written to, then once more what is waiting.
    fn run(mut self, wake: libc::c_int) -> Vec<String> {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let mut ready = [
                libc::pollfd {
                    fd: self.fd,
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: wake,
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            // SAFETY: two `pollfd`s of this frame.
            let waiting = unsafe { libc::poll(ready.as_mut_ptr(), 2, -1) };
            if waiting < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                break;
            }
            if ready[0].revents & libc::POLLIN != 0 {
                self.take(&mut buffer);
            }
            if ready[1].revents & libc::POLLIN != 0 {
                // What came in up to the stop: taken, then the watch ends.
                self.take(&mut buffer);
                break;
            }
        }
        if self.untold > 0 {
            self.notes
                .push(format!("[and {} more such moves]\n", self.untold));
        }
        if self.blind > 0 || self.overflowed {
            self.notes.push(format!(
                "[the watch could not follow every folder while the command ran ({} not followed{}): the check after \
                 it covered them]\n",
                self.blind,
                if self.overflowed { ", events dropped" } else { "" }
            ));
        }
        std::mem::take(&mut self.notes)
    }

    /// Reads everything waiting and answers each new entry.
    fn take(&mut self, buffer: &mut [u8]) {
        loop {
            // SAFETY: reads into `buffer`, which outlives the call; the descriptor does not block.
            let read = unsafe { libc::read(self.fd, buffer.as_mut_ptr().cast(), buffer.len()) };
            let Ok(read) = usize::try_from(read) else {
                return;
            };
            if read == 0 {
                return;
            }
            self.answer_all(&buffer[..read]);
        }
    }

    /// Answers the events in `events`, as the kernel wrote them.
    fn answer_all(&mut self, events: &[u8]) {
        let head = std::mem::size_of::<libc::inotify_event>();
        let mut at = 0;
        let mut overflowed = false;
        while at + head <= events.len() {
            // SAFETY: the kernel wrote a whole event header here; read unaligned, the buffer is bytes.
            let event: libc::inotify_event =
                unsafe { std::ptr::read_unaligned(events[at..].as_ptr().cast()) };
            let length = event.len as usize;
            let name_bytes = &events[at + head..(at + head + length).min(events.len())];
            let name = OsStr::from_bytes(
                name_bytes
                    .split(|byte| *byte == 0)
                    .next()
                    .unwrap_or_default(),
            );
            at += head + length;
            if event.mask & libc::IN_Q_OVERFLOW != 0 {
                overflowed = true;
                continue;
            }
            // A folder gone or no longer watched: its number may come again for another.
            if event.mask & libc::IN_IGNORED != 0 {
                self.folders.remove(&event.wd);
                continue;
            }
            let Some(folder) = self.folders.get(&event.wd).cloned() else {
                continue;
            };
            if !name.is_empty() {
                self.answer(&folder, name, event.mask & libc::IN_ISDIR != 0);
            }
        }
        if overflowed {
            self.overflowed = true;
            self.look_again();
        }
    }

    /// After the kernel dropped events: every followed folder looked through at once, rather than at the end.
    fn look_again(&mut self) {
        let folders: Vec<PathBuf> = self.folders.values().cloned().collect();
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                if self.rules.refuses(&folder, &name.to_string_lossy()) {
                    self.answer(&folder, &name, false);
                }
            }
        }
    }

    /// What `name`, new in `folder`, becomes: moved away if the rules refuse it, else followed if it is a folder —
    /// with everything already in it, made before the watch could follow.
    fn answer(&mut self, folder: &Path, name: &OsStr, is_folder: bool) {
        let path = folder.join(name);
        if self.rules.refuses(folder, &name.to_string_lossy()) {
            // One that was there before only moved with a folder (a repository renamed with it): no new one.
            if self.rules.knows(&path) {
                return;
            }
            self.moved += 1;
            // A folder of its own for each: the same name made again is another thing to keep.
            let held = self.quarantine.join(self.moved.to_string());
            let note = match super::scan::move_away(&path, &held, self.moved) {
                // Moved already by the listing of its new folder, or gone with a folder renamed meanwhile.
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
                Ok(to) => format!(
                    "[refused by the sandbox: the command made {}, which no command may; Glut moved it to {} at once]\n",
                    path.display(),
                    to.display()
                ),
                Err(err) => format!(
                    "[refused by the sandbox: the command made {}, which no command may; Glut could not move it away \
                     at once ({err}) — the check after the command tries again]\n",
                    path.display()
                ),
            };
            if self.notes.len() < MOST_NOTES {
                self.notes.push(note);
            } else {
                self.untold += 1;
            }
            return;
        }
        // A git folder's own folders (its objects) are no place for what the rules ask about.
        if !is_folder || self.rules.is_git_dir(folder) {
            return;
        }
        let mut pending = vec![path];
        let mut followed = 0;
        while let Some(dir) = pending.pop() {
            if followed >= MOST_NEW_FOLDERS {
                break;
            }
            followed += 1;
            self.follow(&dir);
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let kind = entry.file_type().ok();
                if self.rules.refuses(&dir, &name.to_string_lossy())
                    && !self.rules.knows(&entry.path())
                {
                    self.answer(&dir, &name, false);
                } else if kind.is_some_and(|kind| kind.is_dir()) {
                    pending.push(entry.path());
                }
            }
        }
    }
}
