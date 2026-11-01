//! Glut cleans its own `~/.glut` (T82, `glut clean`, `/clean`): the snapshots, journals and `lean.jsonl` by the
//! library's passes, the sessions — with their event logs and long results — and the images no kept session holds by
//! this one; the exports and the Linux sandbox's quarantine are only counted, they are the owner's files. The owner's
//! terms are in `~/.glut/clean.json` ([`glut::clean::Keep`]); a pass runs on its own once a day as Glut starts, in
//! the background. Axiomata never touches `~/.glut`: it calls `glut clean --json` and shows what it says.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use glut::clean::{Keep, Tally};
use serde::{Deserialize, Serialize};

use crate::store::Session;

/// The version of [`Report`]'s shape, for a reader of `--json` (Axiomata's Storage settings).
pub const VERSION: u32 = 1;
/// How often a pass runs on its own.
pub const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// An image younger than this stays though no kept session names it: one pasted into a draft or a session not yet
/// saved, in this window or another.
const IMAGE_GRACE: Duration = Duration::from_secs(24 * 60 * 60);
/// The file whose time says when the last pass ended, in `~/.glut`.
const MARKER: &str = "cleaned";
/// The lock one pass at a time holds, in `~/.glut`: two windows starting together sweep once.
const LOCK: &str = "clean.lock";
/// The kinds a report lists, in its order: the fixed words of `--json`.
pub const ITEMS: [&str; 9] = [
    "snapshots",
    "journals",
    "sessions",
    "results",
    "images",
    "exports",
    "lean",
    "quarantine",
    "other",
];

/// What one pass found and did, as `glut clean --json` writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub version: u32,
    pub dry_run: bool,
    /// All of `~/.glut` as the pass found it.
    pub total_bytes: u64,
    /// What went — or would go, in a dry run.
    pub freeable_bytes: u64,
    /// When the pass before this one ended.
    pub last_run: Option<DateTime<Utc>>,
    /// One line for each of [`ITEMS`].
    pub items: Vec<Tally>,
}

/// One pass over `~/.glut` below `home`, by the terms `keep`; `dry` only counts.
pub fn clean(home: &Path, keep: &Keep, dry: bool) -> Report {
    let glut_dir = home.join(glut::providers::DIR);
    let last_run = last_run(home);
    let total_bytes = glut::clean::size_of(&glut_dir);
    let snapshots = glut::snapshot::clean(
        &glut_dir.join("snapshots"),
        glut::clean::span(keep.snapshots),
        dry,
    );
    let journals = glut::journal::clean(
        &glut_dir.join("runs"),
        glut::clean::span(keep.journals),
        dry,
    );
    let (sessions, gone) = sessions(home, keep, dry);
    let results = results(home, keep, &gone, dry);
    let images = images(home, &gone, dry);
    let mut exports = Tally::new("exports", None);
    exports.bytes = glut::clean::size_of(&crate::export::dir(home));
    exports.count = glut::clean::entries_in(&crate::export::dir(home));
    let lean = glut::lean::clean(
        &glut_dir.join(glut::lean::FILE),
        glut::clean::span(keep.lean),
        dry,
    );
    // What the Linux sandbox moved out of a project (T59): only counted — the owner looks and decides.
    let held = glut_dir.join("quarantine");
    let mut quarantine = Tally::new("quarantine", None);
    quarantine.bytes = glut::clean::size_of(&held);
    quarantine.count = glut::clean::entries_in(&held);
    let mut items = vec![
        snapshots, journals, sessions, results, images, exports, lean, quarantine,
    ];
    let mut other = Tally::new("other", None);
    other.bytes = total_bytes.saturating_sub(items.iter().map(|item| item.bytes).sum());
    items.push(other);
    if !dry {
        mark_done(home);
    }
    Report {
        version: VERSION,
        dry_run: dry,
        total_bytes,
        freeable_bytes: items.iter().map(|item| item.freeable_bytes).sum(),
        last_run,
        items,
    }
}

/// When the last pass ended, if one did.
pub fn last_run(home: &Path) -> Option<DateTime<Utc>> {
    let path = home.join(glut::providers::DIR).join(MARKER);
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(DateTime::<Utc>::from)
}

fn mark_done(home: &Path) {
    let dir = home.join(glut::providers::DIR);
    let _ = glut::providers::make_private_dir(&dir)
        .and_then(|()| std::fs::write(dir.join(MARKER), b""));
}

/// The age of a file or folder by its last change; none when it cannot be read.
fn age(path: &Path, now: SystemTime) -> Duration {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .map(|modified| now.duration_since(modified).unwrap_or_default())
        .unwrap_or_default()
}

/// The sessions older than their term go with their event logs; the ids of those gone, or that would go.
fn sessions(home: &Path, keep: &Keep, dry: bool) -> (Tally, HashSet<String>) {
    let mut tally = Tally::new("sessions", Some(keep.sessions));
    let dir = Session::dir(home);
    let now = SystemTime::now();
    // A session's files by its id: its file and its event log; the newer of them says when it was last worked in.
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let id = name
            .strip_suffix(crate::log::SUFFIX)
            .or_else(|| name.strip_suffix(".json"));
        if let Some(id) = id.filter(|id| is_id(id)) {
            groups.entry(id.to_owned()).or_default().push(entry.path());
        }
    }
    let term = glut::clean::span(keep.sessions);
    let mut gone = HashSet::new();
    for (id, files) in groups {
        let size: u64 = files.iter().map(|file| glut::clean::size_of(file)).sum();
        tally.bytes += size;
        tally.count += 1;
        let newest = files
            .iter()
            .map(|file| age(file, now))
            .min()
            .unwrap_or_default();
        if newest <= term {
            continue;
        }
        // Every file is tried; one another hand removed meanwhile counts as gone.
        let left = !dry
            && files
                .iter()
                .map(std::fs::remove_file)
                .filter(|removed| {
                    removed
                        .as_ref()
                        .is_err_and(|err| err.kind() != std::io::ErrorKind::NotFound)
                })
                .count()
                > 0;
        if left {
            continue;
        }
        tally.freeable_bytes += size;
        tally.freeable_count += 1;
        gone.insert(id);
    }
    (tally, gone)
}

/// The long results of a session gone go with it; those of no session after their own term.
fn results(home: &Path, keep: &Keep, gone: &HashSet<String>, dry: bool) -> Tally {
    let mut tally = Tally::new("results", Some(keep.results));
    let dir = crate::store::results_dir(home, "unnamed")
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let sessions = Session::dir(home);
    let now = SystemTime::now();
    let term = glut::clean::span(keep.results);
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let id = entry.file_name().to_string_lossy().into_owned();
        let size = glut::clean::size_of(&path);
        tally.bytes += size;
        tally.count += 1;
        let orphan = !sessions.join(format!("{id}.json")).exists() && age(&path, now) > term;
        if !gone.contains(&id) && !orphan {
            continue;
        }
        let removed = dry
            || match entry.file_type() {
                Ok(kind) if kind.is_dir() => std::fs::remove_dir_all(&path),
                _ => std::fs::remove_file(&path),
            }
            .is_ok();
        if removed {
            tally.freeable_bytes += size;
            tally.freeable_count += 1;
        }
    }
    tally
}

/// The images no kept session holds — the ones `gone` names do not count — nor an unfinished run's journal, once past
/// their grace; none when the sessions cannot all be read, for one may hold any of them. An image is matched by its
/// file's name (its hash): a session's path to it may be spelt another way, and is never a path to remove.
fn images(home: &Path, gone: &HashSet<String>, dry: bool) -> Tally {
    let mut tally = Tally::new("images", None);
    let dir = glut::images::dir(home);
    let files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path())
        .collect();
    for file in &files {
        tally.bytes += glut::clean::size_of(file);
        tally.count += 1;
    }
    let readable = match std::fs::read_dir(Session::dir(home)) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .count(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => return tally,
    };
    let mut read = 0;
    let mut held: HashSet<std::ffi::OsString> = HashSet::new();
    Session::each(home, |session| {
        read += 1;
        if !gone.contains(&session.id) {
            held.extend(
                session
                    .images()
                    .filter_map(|image| image.path.file_name().map(Into::into)),
            );
        }
    });
    if read != readable {
        return tally;
    }
    let Some(journals) = journal_texts(home) else {
        return tally;
    };
    let now = SystemTime::now();
    for file in files {
        let Some(name) = file.file_name() else {
            continue;
        };
        // Only a file Glut keeps goes: named by its hash, of an image's type.
        if held.contains(name)
            || journals
                .iter()
                .any(|text| text.contains(&*name.to_string_lossy()))
            || glut::images::kept_media(&file).is_none()
            || age(&file, now) <= IMAGE_GRACE
        {
            continue;
        }
        let size = glut::clean::size_of(&file);
        if dry || std::fs::remove_file(&file).is_ok() {
            tally.freeable_bytes += size;
            tally.freeable_count += 1;
        }
    }
    tally
}

/// The unfinished runs' journals as text (`glut run --image` holds its images there until `glut resume`); none when
/// the folder cannot be read.
fn journal_texts(home: &Path) -> Option<Vec<String>> {
    let dir = home.join(glut::providers::DIR).join("runs");
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Some(Vec::new()),
        Err(_) => return None,
    };
    Some(
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| std::fs::read(entry.path()).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .collect(),
    )
}

/// A session's id as its file names it.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// What a pass in the background ended with, for the screen to say.
#[derive(Debug)]
pub enum Done {
    /// The daily pass: said only when it freed something.
    Auto(Report),
    /// `/clean now`: said whole.
    Asked(Report),
    /// `/clean`: the dry run, said whole.
    Listed(Report),
    /// `/clean` or `/clean now` that could not run.
    Failed(String),
}

/// The passes ended and not told yet.
static DONE: Mutex<Vec<Done>> = Mutex::new(Vec::new());
/// Wakes the screen when a pass ended.
static READY: OnceLock<tokio::sync::Notify> = OnceLock::new();

/// What the screen waits on to take [`take_done`].
pub fn ready() -> &'static tokio::sync::Notify {
    READY.get_or_init(tokio::sync::Notify::new)
}

/// Whether the last pass ended less than [`EVERY`] ago.
fn ran_lately(home: &Path) -> bool {
    last_run(home).is_some_and(|last| {
        Utc::now()
            .signed_duration_since(last)
            .to_std()
            .is_ok_and(|since| since < EVERY)
    })
}

/// A pass in the background, if the last one ended more than [`EVERY`] ago and no other runs: its report waits in
/// [`take_done`]. `told` is called with it too (`glut run` says it on stderr). A file of terms that does not read
/// stops it: Glut never removes by terms it cannot read. The handle lets a short process wait for it.
pub fn auto(
    home: &Path,
    told: impl FnOnce(&Report) + Send + 'static,
) -> Option<std::thread::JoinHandle<()>> {
    if ran_lately(home) {
        return None;
    }
    let keep = Keep::load(home).ok()?;
    let home = home.to_path_buf();
    Some(std::thread::spawn(move || {
        // Another window may have ended its pass while this one waited for the lock.
        let pass = locked(&home, || {
            (!ran_lately(&home)).then(|| clean(&home, &keep, false))
        });
        if let Ok(Some(report)) = pass {
            told(&report);
            remember(Done::Auto(report));
        }
    }))
}

/// A pass the owner asked for (`/clean now`), in the background.
pub fn now(home: &Path, keep: Keep) {
    let home = home.to_path_buf();
    std::thread::spawn(move || {
        remember(match locked(&home, || clean(&home, &keep, false)) {
            Ok(report) => Done::Asked(report),
            Err(reason) => Done::Failed(reason),
        });
    });
}

/// The dry run of `/clean`, in the background: it walks every store and reads every session.
pub fn list(home: &Path, keep: Keep) {
    let home = home.to_path_buf();
    std::thread::spawn(move || remember(Done::Listed(clean(&home, &keep, true))));
}

fn remember(done: Done) {
    if let Ok(mut list) = DONE.lock() {
        list.push(done);
    }
    ready().notify_one();
}

/// `pass` under the lock of `~/.glut`.
///
/// # Errors
///
/// [`clean_busy`](crate::texts) when another pass holds it, or why the lock cannot be made.
fn locked<T>(home: &Path, pass: impl FnOnce() -> T) -> Result<T, String> {
    let dir = home.join(glut::providers::DIR);
    glut::providers::make_private_dir(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let path = dir.join(LOCK);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(pass()),
        Err(std::fs::TryLockError::WouldBlock) => Err(tr!(clean_busy).to_owned()),
        Err(std::fs::TryLockError::Error(err)) => Err(format!("{}: {err}", path.display())),
    }
}

/// The passes ended since the last look.
pub fn take_done() -> Vec<Done> {
    DONE.lock()
        .map(|mut done| std::mem::take(&mut *done))
        .unwrap_or_default()
}

/// Bytes as a person reads them: `523 MB`, `1.5 MB`, `48 KB`.
pub fn shown(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// What a pass freed, said once it ended on its own; none when nothing went.
pub fn freed(report: &Report) -> Option<String> {
    (report.freeable_bytes > 0).then(|| tr!(clean_auto, freeable = shown(report.freeable_bytes)))
}

/// The report for a person: a line for each kind with something in it, and the sum.
pub fn lines(report: &Report) -> Vec<String> {
    let mut lines = Vec::new();
    for item in report
        .items
        .iter()
        .filter(|item| item.bytes > 0 || item.count > 0)
    {
        let name = crate::texts::t()
            .clean_names
            .get(
                ITEMS
                    .iter()
                    .position(|known| *known == item.name)
                    .unwrap_or(ITEMS.len()),
            )
            .copied()
            .unwrap_or(&item.name);
        let mut line = tr!(clean_item, name = name, bytes = shown(item.bytes));
        if item.freeable_bytes > 0 || item.freeable_count > 0 {
            line.push_str(&if report.dry_run {
                tr!(clean_item_would_go, bytes = shown(item.freeable_bytes))
            } else {
                tr!(clean_item_went, bytes = shown(item.freeable_bytes))
            });
        }
        if let Some(keep) = &item.keep {
            line.push_str(&tr!(clean_item_kept, keep = keep));
        }
        lines.push(line);
    }
    lines.push(if report.dry_run {
        tr!(
            clean_total_dry,
            total = shown(report.total_bytes),
            freeable = shown(report.freeable_bytes)
        )
    } else {
        tr!(
            clean_total_done,
            total = shown(report.total_bytes),
            freeable = shown(report.freeable_bytes)
        )
    });
    // After a pass, the one before it would only mislead.
    if report.dry_run {
        lines.push(match report.last_run {
            Some(when) => tr!(
                clean_last,
                when = when.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")
            ),
            None => tr!(clean_never).to_owned(),
        });
    }
    lines
}

/// `glut clean`'s commands beside the pass itself.
#[derive(clap::Subcommand)]
pub enum Command {
    /// The terms Glut keeps its data for, or one of them set: snapshots, journals, sessions, results or lean, in days
    /// (30d).
    Keep {
        /// snapshots, journals, sessions, results or lean.
        kind: Option<String>,
        /// Days, as 30d.
        #[arg(requires = "kind")]
        term: Option<String>,
    },
}

/// `glut clean`: the pass (`dry`: only counted), as lines or as JSON, or a `keep` command.
pub fn headless(command: Option<Command>, dry: bool, json: bool) -> std::process::ExitCode {
    let fail = |reason: &str| {
        eprintln!("glut clean: {reason}");
        std::process::ExitCode::from(1)
    };
    let home = match glut::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    if let Some(Command::Keep { kind, term }) = command {
        return match keep_command(&home, kind.as_deref(), term.as_deref()) {
            Ok(said) => {
                println!("{said}");
                std::process::ExitCode::SUCCESS
            }
            Err(reason) => fail(&reason),
        };
    }
    let keep = match Keep::load(&home) {
        Ok(keep) => keep,
        Err(err) => return fail(&err.to_string()),
    };
    let report = if dry {
        clean(&home, &keep, true)
    } else {
        match locked(&home, || clean(&home, &keep, false)) {
            Ok(report) => report,
            Err(reason) => return fail(&reason),
        }
    };
    if json {
        match serde_json::to_string_pretty(&report) {
            Ok(text) => println!("{text}"),
            Err(err) => return fail(&err.to_string()),
        }
    } else {
        for line in lines(&report) {
            println!("{line}");
        }
    }
    std::process::ExitCode::SUCCESS
}

/// `keep` without words: every term; with a kind and a term: that one set and kept. A file of terms that does not
/// read is set anew from the defaults and this term — so the owner mends it by a command, never by hand.
///
/// # Errors
///
/// A message when the words are not a kind and a term, the file cannot be read for the list, or cannot be written.
pub fn keep_command(home: &Path, kind: Option<&str>, term: Option<&str>) -> Result<String, String> {
    let loaded = Keep::load(home).map_err(|err| err.to_string());
    match (kind, term) {
        (None, _) => {
            let keep = loaded?;
            Ok(glut::clean::KINDS
                .iter()
                .map(|kind| {
                    format!(
                        "{kind} {}",
                        glut::clean::term(keep.days(kind).unwrap_or_default())
                    )
                })
                .collect::<Vec<_>>()
                .join(" · "))
        }
        (Some(kind), Some(term)) => {
            let mut keep = loaded.clone().unwrap_or_default();
            keep.set(kind, term)?;
            keep.save(home).map_err(|err| err.to_string())?;
            let said = tr!(clean_keep_set, kind = kind, keep = term);
            Ok(match loaded {
                Ok(_) => said,
                Err(broken) => format!("{broken}\n{said}"),
            })
        }
        (Some(_), None) => Err(tr!(clean_usage).to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    fn home(label: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("glut-clean-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        home
    }

    fn aged(path: &Path, days: u64) {
        let past = SystemTime::now() - Duration::from_secs(days * 24 * 60 * 60);
        File::options()
            .read(true)
            .open(path)
            .unwrap()
            .set_modified(past)
            .unwrap();
    }

    fn session_with_image(home: &Path, image: &Path) -> Session {
        let mut session = Session::new(Path::new("/w"), "p", "m");
        let mut message = glut::Message::user("look");
        message.images.push(glut::images::Image {
            number: 1,
            path: image.to_path_buf(),
            media: "image/png".into(),
            width: 1,
            height: 1,
            bytes: 4,
        });
        session.conversation.push(message);
        session.save(home).unwrap();
        session
    }

    #[test]
    fn an_old_session_goes_with_its_log_results_and_the_images_only_it_held() {
        let home = home("sessions");
        let images = glut::images::dir(&home);
        std::fs::create_dir_all(&images).unwrap();
        let (only_old, shared) = (
            images.join(format!("{}.png", "a".repeat(64))),
            images.join(format!("{}.png", "b".repeat(64))),
        );
        for image in [&only_old, &shared] {
            std::fs::write(image, b"\x89PNG").unwrap();
            aged(image, 3);
        }
        let old = session_with_image(&home, &only_old);
        let young = session_with_image(&home, &shared);
        let mut also = Session::load(&home, &old.id).unwrap();
        let second = glut::images::Image {
            path: shared.clone(),
            ..also.conversation[0].images[0].clone()
        };
        also.conversation[0].images.push(second);
        also.save(&home).unwrap();
        let log = Session::dir(&home).join(format!("{}{}", old.id, crate::log::SUFFIX));
        std::fs::write(&log, "{}\n").unwrap();
        let results = crate::store::results_dir(&home, &old.id);
        std::fs::create_dir_all(&results).unwrap();
        std::fs::write(results.join("r1.txt"), "long").unwrap();
        let session_file = Session::dir(&home).join(format!("{}.json", old.id));
        aged(&session_file, 100);
        aged(&log, 100);

        let counted = clean(&home, &Keep::default(), true);
        let item = |report: &Report, name: &str| {
            report
                .items
                .iter()
                .find(|item| item.name == name)
                .unwrap()
                .clone()
        };
        assert_eq!(item(&counted, "sessions").freeable_count, 1);
        assert_eq!(item(&counted, "results").freeable_count, 1);
        assert_eq!(
            item(&counted, "images").freeable_count,
            1,
            "the shared one stays"
        );
        assert!(
            session_file.exists() && results.exists() && only_old.exists(),
            "a dry run removes nothing"
        );
        assert_eq!(counted.items.len(), ITEMS.len());

        let done = clean(&home, &Keep::default(), false);
        assert!(done.freeable_bytes > 0);
        assert!(!session_file.exists() && !log.exists() && !results.exists() && !only_old.exists());
        assert!(shared.exists());
        assert!(Session::load(&home, &young.id).is_ok());
        assert!(last_run(&home).is_some());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn results_of_no_session_go_after_their_term_and_exports_are_only_counted() {
        let home = home("results");
        let stray = crate::store::results_dir(&home, "20260101-000000000-1");
        std::fs::create_dir_all(&stray).unwrap();
        std::fs::write(stray.join("r.txt"), "x").unwrap();
        let fresh = crate::store::results_dir(&home, "20260101-000000000-2");
        std::fs::create_dir_all(&fresh).unwrap();
        aged(&stray, 40);
        let exports = crate::export::dir(&home);
        std::fs::create_dir_all(&exports).unwrap();
        std::fs::write(exports.join("old.md"), "kept").unwrap();
        aged(&exports.join("old.md"), 400);
        let done = clean(&home, &Keep::default(), false);
        assert!(!stray.exists() && fresh.exists());
        assert!(exports.join("old.md").exists());
        let exported = done
            .items
            .iter()
            .find(|item| item.name == "exports")
            .unwrap();
        assert_eq!(
            (
                exported.count,
                exported.freeable_count,
                exported.keep.clone()
            ),
            (1, 0, None)
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_json_names_its_fields_for_a_reader_outside() {
        let report = Report {
            version: VERSION,
            dry_run: true,
            total_bytes: 3,
            freeable_bytes: 1,
            last_run: None,
            items: vec![Tally::new("snapshots", Some(7))],
        };
        let json = serde_json::to_value(&report).unwrap();
        for field in [
            "version",
            "dryRun",
            "totalBytes",
            "freeableBytes",
            "lastRun",
            "items",
        ] {
            assert!(json.get(field).is_some(), "{field}");
        }
        let item = &json["items"][0];
        for field in [
            "name",
            "bytes",
            "freeableBytes",
            "count",
            "freeableCount",
            "keep",
        ] {
            assert!(item.get(field).is_some(), "{field}");
        }
    }

    #[test]
    fn a_term_is_set_by_its_kind_and_kept() {
        let home = home("keep");
        let said = keep_command(&home, Some("sessions"), Some("30d")).unwrap();
        assert!(said.contains("30d"), "{said}");
        assert_eq!(Keep::load(&home).unwrap().sessions, 30);
        assert!(keep_command(&home, Some("images"), Some("30d")).is_err());
        assert!(keep_command(&home, Some("sessions"), Some("30")).is_err());
        let listed = keep_command(&home, None, None).unwrap();
        assert!(
            listed.contains("sessions 30d") && listed.contains("snapshots 7d"),
            "{listed}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_image_an_unfinished_runs_journal_names_stays() {
        let home = home("journal-image");
        let images = glut::images::dir(&home);
        std::fs::create_dir_all(&images).unwrap();
        let name = format!("{}.png", "c".repeat(64));
        std::fs::write(images.join(&name), b"\x89PNG").unwrap();
        aged(&images.join(&name), 3);
        let runs = home.join(".glut/runs");
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::write(
            runs.join("run-1.jsonl"),
            format!("{{\"path\":\"/elsewhere/{name}\"}}\n"),
        )
        .unwrap();
        let counted = clean(&home, &Keep::default(), true);
        let item = counted
            .items
            .iter()
            .find(|item| item.name == "images")
            .unwrap();
        assert_eq!(
            item.freeable_count, 0,
            "held by its name, wherever the path says it is"
        );
        std::fs::remove_file(runs.join("run-1.jsonl")).unwrap();
        clean(&home, &Keep::default(), false);
        assert!(!images.join(&name).exists());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_broken_file_of_terms_stops_the_pass_and_keep_mends_it() {
        let home = home("broken");
        std::fs::write(Keep::path(&home), r#"{"sessions": 0}"#).unwrap();
        assert!(
            auto(&home, |_| {}).is_none(),
            "no pass by terms that do not read"
        );
        assert!(keep_command(&home, None, None).is_err());
        let said = keep_command(&home, Some("sessions"), Some("30d")).unwrap();
        assert!(said.contains("30d"), "{said}");
        assert_eq!(Keep::load(&home).unwrap().sessions, 30);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn bytes_read_as_a_person_says_them() {
        assert_eq!(shown(512), "512 B");
        assert_eq!(shown(48_000), "48 KB");
        assert_eq!(shown(1_500_000), "1.5 MB");
        assert_eq!(shown(523_000_000), "523 MB");
    }
}
