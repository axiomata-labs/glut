//! The session's event log (TUI8d): every step of its turns as the run reported it, and
//! the owner's messages between them, one JSON line each with the time in `at` — what `glut watch` follows from another
//! terminal, and what a host reads for its own views (Axiomata's meter and Flow tiles read a card's). The owner's
//! sessions keep theirs beside the session (`~/.glut/sessions/<id>.events.jsonl`), a host names its own file.
//!
//! The streamed pieces of text and thinking are left out: the whole text follows as one event, and the thinking is
//! the screen's alone. A log that reached [`MAX_LOG_BYTES`] says so in its last line and takes no more.

use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use glut::Event;

/// The largest a session's log grows: a few thousand steps of results cut to the event's share, far more than any
/// session the owner would follow.
pub const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;
/// The end of a session's log file name, beside the session's `<id>.json`.
pub const SUFFIX: &str = ".events.jsonl";
/// The `event` of an owner's message: a name no step of a run has.
const OWNER: &str = "owner";
/// The `event` of the line that ends a full log.
const FULL: &str = "log_full";
/// The `event` of a log's first line, which names the session's folder: `glut watch` finds a folder's newest session
/// by it, also one whose first turn still runs (a session is kept only after its first message).
const HEAD: &str = "session";

/// Where the session's log goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Beside the session in `~/.glut/sessions`, following it when the owner opens another.
    Sessions(PathBuf),
    /// One file the host names, whatever session the screen shows.
    At(PathBuf),
    /// Nowhere.
    Off,
}

/// The session's log, shared by the screen (the owner's messages, a session opened) and the run's event sink.
pub struct EventLog {
    target: Target,
    /// The session's folder, for the log's first line.
    cwd: PathBuf,
    open: Mutex<Open>,
}

/// The file written now.
#[derive(Default)]
struct Open {
    path: Option<PathBuf>,
    file: Option<File>,
    bytes: u64,
}

/// A line of a log, read back.
#[derive(Debug, Clone, PartialEq)]
pub enum Logged {
    /// A step of a run.
    Event(Event),
    /// The owner's message.
    Owner(String),
    /// The log reached its limit: nothing after it was written.
    Full,
    /// The log's first line: the session's folder.
    Head(PathBuf),
}

/// The log of the owner's session `id` in `~/.glut/sessions` under `home`.
pub fn session_log(home: &Path, id: &str) -> PathBuf {
    home.join(".glut")
        .join("sessions")
        .join(format!("{id}{SUFFIX}"))
}

/// `line` read back; `None` for a line that is none of a log's (a torn last line, another program's).
pub fn read(line: &str) -> Option<Logged> {
    // Parsed once: a long log is read line by line at the viewer's start.
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    match value.get("event").and_then(serde_json::Value::as_str)? {
        OWNER => Some(Logged::Owner(text("text"))),
        FULL => Some(Logged::Full),
        HEAD => Some(Logged::Head(PathBuf::from(text("cwd")))),
        _ => serde_json::from_value(value).ok().map(Logged::Event),
    }
}

/// The newest log in `~/.glut/sessions` under `home` of a session in `cwd`, by when it was last written.
pub fn newest(home: &Path, cwd: &Path) -> Option<PathBuf> {
    use std::io::BufRead as _;
    let entries = std::fs::read_dir(home.join(".glut").join("sessions")).ok()?;
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(SUFFIX))
        .filter(|entry| {
            let Ok(file) = std::fs::File::open(entry.path()) else {
                return false;
            };
            let mut first = String::new();
            let _ = std::io::BufReader::new(file).read_line(&mut first);
            read(first.trim_end()) == Some(Logged::Head(cwd.to_path_buf()))
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

impl EventLog {
    /// A log to `target` of a session in `cwd`. A session's log starts with [`EventLog::follow`].
    pub fn new(target: Target, cwd: PathBuf) -> Self {
        let open = match &target {
            Target::At(path) => Open {
                path: Some(path.clone()),
                ..Open::default()
            },
            Target::Sessions(_) | Target::Off => Open::default(),
        };
        EventLog {
            target,
            cwd,
            open: Mutex::new(open),
        }
    }

    /// The screen shows session `id` now: its steps go to its own log. A host's file stays the same.
    pub fn follow(&self, id: &str) {
        let Target::Sessions(home) = &self.target else {
            return;
        };
        let path = session_log(home, id);
        let mut open = self.lock();
        if open.path.as_ref() != Some(&path) {
            *open = Open {
                path: Some(path),
                ..Open::default()
            };
        }
    }

    /// A step of a run; the streamed pieces are not kept.
    pub fn event(&self, event: &Event) {
        if streamed(event) {
            return;
        }
        if let Ok(value) = serde_json::to_value(event) {
            self.write(value);
        }
    }

    /// The owner's message, as it went to the run.
    pub fn owner(&self, text: &str) {
        self.write(serde_json::json!({ "event": OWNER, "text": text }));
    }

    /// Writes `value` with the time, opening the file on the first line. A log that cannot be written is given up
    /// quietly: the session goes on without it, and the screen is no place for a write error on every step.
    fn write(&self, mut value: serde_json::Value) {
        if self.target == Target::Off {
            return;
        }
        let Some(fields) = value.as_object_mut() else {
            return;
        };
        let at = chrono::Utc::now().to_rfc3339();
        fields.insert("at".into(), serde_json::Value::String(at.clone()));
        let Ok(mut line) = serde_json::to_string(&value) else {
            return;
        };
        line.push('\n');
        let mut open = self.lock();
        let Open { path, file, bytes } = &mut *open;
        let Some(path) = path.as_ref() else {
            return;
        };
        if file.is_none() {
            *file = open_append(path);
            *bytes = file
                .as_ref()
                .and_then(|file| file.metadata().ok())
                .map_or(0, |meta| meta.len());
        }
        let Some(writer) = file.as_mut() else {
            return;
        };
        if *bytes == 0 {
            let head =
                serde_json::json!({ "event": HEAD, "cwd": self.cwd.to_string_lossy(), "at": at });
            if writeln!(writer, "{head}").is_ok() {
                *bytes += head.to_string().len() as u64 + 1;
            }
        }
        if *bytes >= MAX_LOG_BYTES {
            return;
        }
        if *bytes + line.len() as u64 > MAX_LOG_BYTES {
            let _ = writeln!(writer, "{}", serde_json::json!({ "event": FULL }));
            *bytes = MAX_LOG_BYTES;
            return;
        }
        if writer.write_all(line.as_bytes()).is_ok() {
            *bytes += line.len() as u64;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Open> {
        self.open
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

/// The streamed pieces of text and thinking, a subagent's too.
fn streamed(event: &Event) -> bool {
    match event {
        Event::TextDelta { .. } | Event::ReasoningDelta { .. } => true,
        Event::Sub { event, .. } => streamed(event),
        _ => false,
    }
}

/// `path` opened to append, made for the owner alone (a log holds what the tools read) and never through a link.
fn open_append(path: &Path) -> Option<File> {
    use std::os::unix::fs::OpenOptionsExt as _;
    // The owner's alone, as the sessions' folder is: the log may be written before the session's first save makes it.
    if let Some(dir) = path.parent() {
        glut::providers::make_private_dir(dir).ok()?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        // Never written through a link someone left in its place.
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(path)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("glut-tui-log-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn lines(path: &Path) -> Vec<Logged> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter_map(read)
            .collect()
    }

    #[test]
    fn a_session_s_steps_and_the_owner_s_messages_come_back_without_the_streamed_pieces() {
        let home = temp("session");
        let log = EventLog::new(Target::Sessions(home.clone()), PathBuf::from("/w"));
        log.owner("before any session: nowhere");
        log.follow("s1");
        log.owner("Fix the parser");
        log.event(&Event::TextDelta {
            turn: 1,
            text: "Fi".into(),
        });
        log.event(&Event::AssistantText {
            turn: 1,
            text: "Fixed.".into(),
        });
        let path = session_log(&home, "s1");
        assert_eq!(
            lines(&path),
            [
                Logged::Head(PathBuf::from("/w")),
                Logged::Owner("Fix the parser".into()),
                Logged::Event(Event::AssistantText {
                    turn: 1,
                    text: "Fixed.".into()
                })
            ]
        );
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let line = std::fs::read_to_string(&path).unwrap();
        assert!(line.lines().all(|line| line.contains("\"at\":")), "{line}");
        // Another session opened: its own log — later than the first by more than the file system's clock tells apart
        // (Linux's can be a few milliseconds coarse).
        std::thread::sleep(std::time::Duration::from_millis(20));
        log.follow("s2");
        log.owner("Next");
        assert_eq!(
            lines(&session_log(&home, "s2")),
            [
                Logged::Head(PathBuf::from("/w")),
                Logged::Owner("Next".into())
            ]
        );
        assert_eq!(
            newest(&home, Path::new("/w")),
            Some(session_log(&home, "s2"))
        );
        assert_eq!(newest(&home, Path::new("/elsewhere")), None);
        assert_eq!(lines(&path).len(), 3);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_host_s_file_stays_whatever_session_is_shown_and_off_writes_nothing() {
        let dir = temp("host");
        let file = dir.join("glut-events.jsonl");
        let log = EventLog::new(Target::At(file.clone()), dir.clone());
        log.follow("s1");
        log.owner("Start");
        assert_eq!(
            lines(&file),
            [Logged::Head(dir.clone()), Logged::Owner("Start".into())]
        );
        assert!(!dir.join(".glut").exists());
        let off = EventLog::new(Target::Off, dir.clone());
        off.follow("s1");
        off.owner("nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_full_log_says_so_once_and_takes_nothing_more() {
        let dir = temp("full");
        let file = dir.join("log.jsonl");
        let log = EventLog::new(Target::At(file.clone()), dir.clone());
        log.owner("first");
        log.lock().bytes = MAX_LOG_BYTES - 10;
        log.owner("too long for what is left");
        log.owner("after");
        assert_eq!(
            lines(&file),
            [
                Logged::Head(dir.clone()),
                Logged::Owner("first".into()),
                Logged::Full
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_line_of_another_kind_is_none_of_the_log_s() {
        assert_eq!(read("not json"), None);
        assert_eq!(read(r#"{"event":"no_such_step"}"#), None);
        assert_eq!(read(r#"{"text":"no event"}"#), None);
    }
}
