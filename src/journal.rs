//! A run's journal (durable runs, `docs/plans/durable.md`, D5): one JSON line per step, appended and synced before
//! the step goes on, so a run the process lost — a crash, a quit, a rebuild — can go on from its last saved step.
//!
//! * The file is `<dir>/<run>.jsonl`, the owner's alone (0600 in a 0700 folder). It opens with a `start` line, then
//!   the conversation's messages as they are added, a `call` line before a tool call runs, and a `progress` line after
//!   each model call (the steps and tokens so far). A finished run deletes its journal (D8): a journal that is there
//!   belongs to a run that never finished.
//! * While a run writes its journal it holds the file's lock; [`open`] for a resume takes the same lock, so a run is
//!   never resumed while it still runs — in this process or in another.
//! * A torn last line (the process died while writing it) is passed by; a damaged line before it makes the journal
//!   unusable rather than resumed from a wrong state.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::{Message, Role, ToolCall, Usage};

/// How long an unfinished journal stays resumable (D8), as the snapshots do.
pub const KEEP: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// The extension of a journal file.
const EXTENSION: &str = "jsonl";
/// The largest journal read back: a run's conversation, tool results included, stays far below it.
const MAX_JOURNAL_BYTES: u64 = 64 * 1024 * 1024;
/// The most of a journal [`list`] reads to find its `start` line.
const MAX_START_BYTES: u64 = 64 * 1024;
/// How long a resume waits for a journal's lock to come free before it calls the run still going. A process being
/// started at the moment the run closed its journal holds a copy of the file until it starts its program; that window
/// is short, but under load (the test suite, a busy app) it is long enough to read a finished run as a living one.
const LOCK_PATIENCE: Duration = Duration::from_millis(200);
/// How often a resume tries the lock again while it waits.
const LOCK_RETRY: Duration = Duration::from_millis(10);

/// Where a run keeps its journal and what it is called: set on [`crate::RunConfig::journal`] by a caller that wants
/// the run to be resumable.
#[derive(Debug, Clone)]
pub struct JournalSpec {
    pub dir: PathBuf,
    /// The run's id, `[A-Za-z0-9_-]`, at most 100 characters: the file's name.
    pub run: String,
    /// What the caller needs to rebuild the run (its kind, the skill, a chat's session id) — data the harness keeps
    /// and hands back, never reads. No key belongs here.
    pub meta: Value,
}

/// One line of a journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Line {
    Start {
        run: String,
        cwd: PathBuf,
        model: String,
        meta: Value,
        started_at: String,
    },
    /// A message added to the conversation.
    Message { message: Message },
    /// A tool call about to run: a call with this line and without its result was under way when the run stopped.
    Call { id: String },
    /// The steps and tokens so far, after a model call.
    Progress { turns: u32, usage: Usage },
}

/// What a journal holds, read back for a resume.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub run: String,
    pub cwd: PathBuf,
    pub model: String,
    pub meta: Value,
    pub started_at: String,
    /// The conversation as the run left it.
    pub messages: Vec<Message>,
    /// The last reply's tool calls that have no result yet, with whether each had started.
    pub open_calls: Vec<(ToolCall, bool)>,
    pub turns: u32,
    pub usage: Usage,
}

/// An unfinished journal, as [`list`] reports it.
#[derive(Debug, Clone, PartialEq)]
pub struct Unfinished {
    pub run: String,
    pub meta: Value,
    pub started_at: String,
    /// When it last changed.
    pub modified: SystemTime,
    /// A process holds it: the run is still going.
    pub running: bool,
}

/// The journal a run writes; the lock is held as long as this lives.
#[derive(Debug)]
pub struct Journal {
    file: File,
    path: PathBuf,
}

/// A run id that can be a file name: nothing else ever is (the id may come from a caller's record).
pub fn checked_run(run: &str) -> Result<&str, String> {
    let ok = !run.is_empty()
        && run.len() <= 100
        && run
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then_some(run)
        .ok_or_else(|| format!("{run:?} is no run id"))
}

fn path_of(dir: &Path, run: &str) -> Result<PathBuf, String> {
    Ok(dir.join(format!("{}.{EXTENSION}", checked_run(run)?)))
}

impl Journal {
    /// Starts the journal of a new run: the file made (it must not exist), locked, its `start` line written.
    ///
    /// # Errors
    ///
    /// A bad run id, a folder or file that cannot be made, a journal of that id already there.
    pub fn create(spec: &JournalSpec, cwd: &Path, model: &str) -> Result<Self, String> {
        make_private_dir(&spec.dir)?;
        let path = path_of(&spec.dir, &spec.run)?;
        let mut options = OpenOptions::new();
        options.create_new(true).append(true).read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&path)
            .map_err(|err| format!("cannot start the journal {}: {err}", path.display()))?;
        if let Err(err) = file.try_lock() {
            // Made but not ours: a file without its `start` line would only lie around.
            let _ = std::fs::remove_file(&path);
            return Err(format!("cannot lock the journal {}: {err}", path.display()));
        }
        let mut journal = Journal { file, path };
        journal.write(&Line::Start {
            run: spec.run.clone(),
            cwd: cwd.to_owned(),
            model: model.to_owned(),
            meta: spec.meta.clone(),
            started_at: chrono::Local::now().to_rfc3339(),
        })?;
        Ok(journal)
    }

    /// Appends `line` and syncs it to the disk before the run goes on.
    ///
    /// # Errors
    ///
    /// The line could not be written or synced.
    pub fn write(&mut self, line: &Line) -> Result<(), String> {
        self.write_all(std::slice::from_ref(line))
    }

    /// Appends `lines` in one write and syncs them once.
    ///
    /// # Errors
    ///
    /// The lines could not be written or synced.
    pub fn write_all(&mut self, lines: &[Line]) -> Result<(), String> {
        let mut text = String::new();
        for line in lines {
            text.push_str(&serde_json::to_string(line).map_err(|err| err.to_string())?);
            text.push('\n');
        }
        self.file
            .write_all(text.as_bytes())
            .and_then(|()| self.file.sync_data())
            .map_err(|err| format!("cannot write the journal {}: {err}", self.path.display()))
    }

    /// The run finished: its journal goes (D8). Best effort — a journal that stays is swept after [`KEEP`].
    pub fn finish(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// The file's exclusive lock, tried again for [`LOCK_PATIENCE`] while it is held.
fn lock_patiently(file: &File) -> Result<(), std::fs::TryLockError> {
    let deadline = std::time::Instant::now() + LOCK_PATIENCE;
    loop {
        match file.try_lock() {
            Err(std::fs::TryLockError::WouldBlock) if std::time::Instant::now() < deadline => {
                std::thread::sleep(LOCK_RETRY);
            }
            done => return done,
        }
    }
}

/// Opens the journal of `run` for a resume: locked (a run still going holds it), and read back.
///
/// # Errors
///
/// No such journal, the run still going, or a journal that cannot be read.
pub fn open(dir: &Path, run: &str) -> Result<(Journal, Recorded), String> {
    let path = path_of(dir, run)?;
    let file =
        open_existing(&path, true).map_err(|err| format!("no unfinished run {run}: {err}"))?;
    lock_patiently(&file).map_err(|_| format!("the run {run} is still going"))?;
    let (recorded, good) = read(&file, &path)?;
    if recorded.run != run {
        return Err(format!("the journal {} names another run", path.display()));
    }
    // A torn tail goes before anything is appended: the next line would otherwise be glued onto it, and a second
    // loss would find the journal damaged in the middle.
    if file.metadata().map(|meta| meta.len()).unwrap_or(0) > good {
        file.set_len(good)
            .map_err(|err| format!("cannot mend the journal {}: {err}", path.display()))?;
    }
    Ok((Journal { file, path }, recorded))
}

/// Opens a journal that is there: never through a link, never a pipe or a device that would block or never end, and
/// only one of this user's — the folder is the owner's alone, but a process outside the sandbox could still plant one.
fn open_existing(path: &Path, append: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true).append(append);
    #[cfg(unix)]
    {
        use rustix::fs::OFlags;
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32);
    }
    let file = options.open(path).map_err(|err| err.to_string())?;
    let meta = file.metadata().map_err(|err| err.to_string())?;
    if !meta.is_file() {
        return Err("not a regular file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != rustix::process::getuid().as_raw() {
            return Err("not the owner's file".into());
        }
    }
    Ok(file)
}

/// The journal read back, and how many bytes of it are whole lines.
fn read(file: &File, path: &Path) -> Result<(Recorded, u64), String> {
    let size = file.metadata().map(|meta| meta.len()).unwrap_or(0);
    if size > MAX_JOURNAL_BYTES {
        return Err(format!(
            "the journal {} is larger than {MAX_JOURNAL_BYTES} bytes",
            path.display()
        ));
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(
        &mut std::io::Read::take(file, MAX_JOURNAL_BYTES),
        &mut bytes,
    )
    .map_err(|err| format!("cannot read the journal {}: {err}", path.display()))?;
    // Complete lines end in a newline; what follows the last one was being written when the process died.
    let complete = bytes
        .iter()
        .rposition(|b| *b == b'\n')
        .map_or(0, |at| at + 1);
    let lines: Vec<&[u8]> = bytes[..complete]
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .collect();
    let last = lines.len().saturating_sub(1);
    let mut parsed = Vec::new();
    let mut good = 0u64;
    for (index, text) in lines.iter().enumerate() {
        match serde_json::from_slice::<Line>(text) {
            Ok(line) => {
                parsed.push(line);
                good += text.len() as u64 + 1;
            }
            // The process died while this line was written: what came before is the run's last saved state.
            Err(_) if index == last => break,
            Err(err) => {
                return Err(format!(
                    "the journal {} is damaged at line {}: {err}",
                    path.display(),
                    index + 1
                ));
            }
        }
    }
    let mut lines = parsed.into_iter();
    let Some(Line::Start {
        run,
        cwd,
        model,
        meta,
        started_at,
    }) = lines.next()
    else {
        return Err(format!("the journal {} has no start", path.display()));
    };
    let mut recorded = Recorded {
        run,
        cwd,
        model,
        meta,
        started_at,
        messages: Vec::new(),
        open_calls: Vec::new(),
        turns: 0,
        usage: Usage::default(),
    };
    let mut started: Vec<String> = Vec::new();
    for line in lines {
        match line {
            Line::Message { message } => {
                // A call id belongs to its reply: providers count ids up afresh (`call_0`) in each one.
                if message.role == Role::Assistant {
                    started.clear();
                }
                recorded.messages.push(message);
            }
            Line::Call { id } => started.push(id),
            Line::Progress { turns, usage } => {
                recorded.turns = turns;
                recorded.usage = usage;
            }
            Line::Start { .. } => {
                return Err(format!("the journal {} starts twice", path.display()));
            }
        }
    }
    recorded.open_calls = open_calls(&recorded.messages, &started);
    Ok((recorded, good))
}

/// The last assistant message's tool calls that no tool message answers, each with whether it had started.
fn open_calls(messages: &[Message], started: &[String]) -> Vec<(ToolCall, bool)> {
    let Some(at) = messages.iter().rposition(|m| m.role == Role::Assistant) else {
        return Vec::new();
    };
    let answered: Vec<&str> = messages[at + 1..]
        .iter()
        .filter_map(|m| m.tool_call_id.as_deref())
        .collect();
    messages[at]
        .tool_calls
        .iter()
        .filter(|call| !answered.contains(&call.id.as_str()))
        .map(|call| (call.clone(), started.contains(&call.id)))
        .collect()
}

/// The unfinished journals under `dir`, newest first.
pub fn list(dir: &Path) -> Vec<Unfinished> {
    let mut found: Vec<Unfinished> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let run = path
                .file_stem()?
                .to_str()
                .filter(|_| path.extension().is_some_and(|ext| ext == EXTENSION))?
                .to_owned();
            checked_run(&run).ok()?;
            let modified = entry.metadata().ok()?.modified().ok()?;
            let file = open_existing(&path, false).ok()?;
            let first = BufReader::new(std::io::Read::take(&file, MAX_START_BYTES))
                .lines()
                .next()?
                .ok()?;
            let Ok(Line::Start {
                meta, started_at, ..
            }) = serde_json::from_str::<Line>(&first)
            else {
                return None;
            };
            // A shared lock fails while the run holds its exclusive one.
            let running = file.try_lock_shared().is_err();
            Some(Unfinished {
                run,
                meta,
                started_at,
                modified,
                running,
            })
        })
        .collect();
    found.sort_by_key(|unfinished| std::cmp::Reverse(unfinished.modified));
    found
}

/// Deletes the journal of `run` — the owner gave it up. A run still going keeps it.
///
/// # Errors
///
/// A bad id, the run still going, or the file cannot be removed.
pub fn discard(dir: &Path, run: &str) -> Result<(), String> {
    let (journal, _) = open(dir, run)?;
    std::fs::remove_file(&journal.path).map_err(|err| err.to_string())
}

/// Removes the unfinished journals older than [`KEEP`] (D8); a run still going is left alone.
pub fn sweep(dir: &Path) {
    let now = SystemTime::now();
    for unfinished in list(dir) {
        let age = now.duration_since(unfinished.modified).unwrap_or_default();
        if !unfinished.running && age > KEEP {
            let _ = discard(dir, &unfinished.run);
        }
    }
}

/// A folder only the owner can enter: a journal holds the conversation, tool results included.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::FunctionCall;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-journal-{tag}-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spec(dir: &Path, run: &str) -> JournalSpec {
        JournalSpec {
            dir: dir.to_owned(),
            run: run.to_owned(),
            meta: serde_json::json!({"kind": "skill", "name": "news-web"}),
        }
    }

    fn calling(ids: &[&str]) -> Message {
        Message {
            role: Role::Assistant,
            content: None,
            tool_calls: ids
                .iter()
                .map(|id| ToolCall {
                    id: (*id).into(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: "read_file".into(),
                        arguments: "{}".into(),
                    },
                })
                .collect(),
            tool_call_id: None,
        }
    }

    #[test]
    fn a_journal_reads_back_as_the_run_left_it() {
        let dir = scratch("read");
        let mut journal = Journal::create(&spec(&dir, "run-1"), Path::new("/w"), "m").unwrap();
        journal
            .write(&Line::Message {
                message: Message::user("go"),
            })
            .unwrap();
        journal
            .write(&Line::Message {
                message: calling(&["a", "b", "c"]),
            })
            .unwrap();
        let usage = Usage {
            input_tokens: 10,
            cached_input_tokens: 0,
            output_tokens: 2,
        };
        journal.write(&Line::Progress { turns: 1, usage }).unwrap();
        journal.write(&Line::Call { id: "a".into() }).unwrap();
        journal
            .write(&Line::Message {
                message: Message::tool("a", "done"),
            })
            .unwrap();
        journal.write(&Line::Call { id: "b".into() }).unwrap();
        // Still running: no resume, and the listing says so.
        assert!(open(&dir, "run-1").unwrap_err().contains("still going"));
        assert!(list(&dir)[0].running);
        drop(journal);

        let (_held, recorded) = open(&dir, "run-1").unwrap();
        assert_eq!(recorded.meta["name"], "news-web");
        assert_eq!(recorded.cwd, Path::new("/w"));
        assert_eq!(recorded.messages.len(), 3);
        assert_eq!((recorded.turns, recorded.usage), (1, usage));
        let open: Vec<(&str, bool)> = recorded
            .open_calls
            .iter()
            .map(|(call, started)| (call.id.as_str(), *started))
            .collect();
        assert_eq!(open, [("b", true), ("c", false)]);
    }

    #[test]
    fn a_torn_last_line_is_mended_and_a_damaged_one_before_is_not_passed_by() {
        let dir = scratch("torn");
        let mut journal = Journal::create(&spec(&dir, "run-t"), Path::new("/w"), "m").unwrap();
        journal
            .write(&Line::Message {
                message: Message::user("go"),
            })
            .unwrap();
        drop(journal);
        let path = dir.join("run-t.jsonl");
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"kind\":\"message\",\"mess").unwrap();
        // Resumed, the torn tail is cut off before anything is appended, and the journal reads whole again.
        let (mut resumed, recorded) = open(&dir, "run-t").unwrap();
        assert_eq!(recorded.messages.len(), 1);
        resumed.write(&Line::Call { id: "x".into() }).unwrap();
        drop(resumed);
        assert!(open(&dir, "run-t").is_ok());
        // A damaged line before the last one is no torn tail: the journal is not trusted.
        file.write_all(b"{\"kind\":\"oops\"}\n{\"kind\":\"call\",\"id\":\"y\"}\n")
            .unwrap();
        assert!(open(&dir, "run-t").unwrap_err().contains("damaged at line"));
    }

    #[test]
    fn a_journal_without_its_task_can_still_be_given_up() {
        let dir = scratch("taskless");
        drop(Journal::create(&spec(&dir, "run-e"), Path::new("/w"), "m").unwrap());
        assert!(open(&dir, "run-e").unwrap().1.messages.is_empty());
        discard(&dir, "run-e").unwrap();
    }

    #[test]
    fn a_link_or_a_pipe_is_no_journal() {
        let dir = scratch("planted");
        let elsewhere = scratch("elsewhere");
        let real = Journal::create(&spec(&elsewhere, "run-real"), Path::new("/w"), "m").unwrap();
        drop(real);
        std::os::unix::fs::symlink(elsewhere.join("run-real.jsonl"), dir.join("run-link.jsonl"))
            .unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(dir.join("run-pipe.jsonl"))
            .status()
            .unwrap();
        assert!(status.success());
        assert!(list(&dir).is_empty());
        assert!(open(&dir, "run-link").is_err());
        assert!(open(&dir, "run-pipe").is_err());
        // A journal copied under another name is not that run's.
        std::fs::copy(elsewhere.join("run-real.jsonl"), dir.join("run-copy.jsonl")).unwrap();
        assert!(
            open(&dir, "run-copy")
                .unwrap_err()
                .contains("names another run")
        );
    }

    #[test]
    fn a_finished_run_leaves_nothing_and_ids_are_file_names_only() {
        let dir = scratch("finish");
        let journal = Journal::create(&spec(&dir, "run-f"), Path::new("/w"), "m").unwrap();
        assert!(Journal::create(&spec(&dir, "run-f"), Path::new("/w"), "m").is_err());
        journal.finish();
        assert!(list(&dir).is_empty());
        for bad in ["", "../x", "a/b", "a.b"] {
            assert!(
                Journal::create(&spec(&dir, bad), Path::new("/w"), "m").is_err(),
                "{bad}"
            );
            assert!(open(&dir, bad).is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let kept = Journal::create(&spec(&dir, "run-p"), Path::new("/w"), "m").unwrap();
            let mode = std::fs::metadata(dir.join("run-p.jsonl"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            drop(kept);
            discard(&dir, "run-p").unwrap();
            assert!(list(&dir).is_empty());
        }
    }
}
