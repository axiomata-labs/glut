//! The count of what Glut lean saves (`docs/plans/glut-lean.md`, N9): every compacted output, unchanged re-read and
//! relocated result is one line of `~/.axh/lean.jsonl` — the kind and the characters before and after — written by
//! every Glut run (the app's, `axh`'s, the Studio's), so `axh gain` shows where the mechanisms fire and what they
//! spare. A known command in front of a pipe is a line too, with nothing saved: it shows whether filtering inside a
//! pipe would pay (N8).
//!
//! The count is best-effort: a line that cannot be written is left out, never a reason for a call to fail.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The file's name in the harness's home folder.
pub const FILE: &str = "lean.jsonl";

/// How a known command in front of a pipe is named ("in a pipe: cargo", N8): counted to see how often it happens, but
/// nothing was spared, so it stays out of the table's sum.
pub const IN_A_PIPE: &str = "in a pipe: ";

/// Past this size in bytes the file keeps only its newer half (~25,000 lines of ~150 bytes each).
const MAX_BYTES: u64 = 4 * 1024 * 1024;

/// One saving of one call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saving {
    /// When it was made.
    pub at: DateTime<Utc>,
    /// What it was: a kind of output ("cargo"), "unchanged read", "relocated", or "in a pipe: cargo".
    pub what: String,
    /// The characters before.
    pub before: usize,
    /// The characters the conversation got instead.
    pub after: usize,
}

/// Appends `saving` to the count at `path`, the file only the owner's to read (0600); a file grown past
/// [`MAX_BYTES`] first loses its older half. A missing folder is made the owner's alone (0700), like the rest of
/// `~/.axh`.
///
/// The run loop calls this on its own task: a line is one small append, and the halving reads and writes at most
/// [`MAX_BYTES`] once in some 25,000 savings. Two processes halving at once may lose a line or two of the count, which
/// only ever is a count.
///
/// # Errors
///
/// When the folder or the file cannot be written, or the file is a link, a pipe, a device or another user's.
pub fn record(path: &Path, saving: &Saving) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)?;
    }
    let mut file = open_own(
        path,
        OpenOptions::new().create(true).append(true).read(true),
    )?;
    if file.metadata()?.len() > MAX_BYTES {
        keep_newer_half(path, &mut file)?;
        file = open_own(path, OpenOptions::new().append(true))?;
    }
    let mut line = serde_json::to_string(saving).map_err(io::Error::other)?;
    line.push('\n');
    file.write_all(line.as_bytes())
}

/// The count's file opened with `options`: never through a link, never a pipe or a device that would block the run,
/// only one of this user's, and kept at 0600 even when it was made looser.
fn open_own(path: &Path, options: &mut OpenOptions) -> io::Result<File> {
    use rustix::fs::OFlags;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let file = options
        .mode(0o600)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != rustix::process::getuid().as_raw() {
        return Err(io::Error::other(
            "the count of savings is no file of the owner's",
        ));
    }
    if meta.mode() & 0o777 != 0o600 {
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    Ok(file)
}

/// The file without its older half, cut at a line's start: written beside it and moved over it, so a link planted at
/// the path in between is replaced, never written through.
fn keep_newer_half(path: &Path, file: &mut File) -> io::Result<()> {
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut io::Read::take(&*file, 2 * MAX_BYTES), &mut bytes)?;
    let middle = bytes.len() / 2;
    let start = bytes[middle..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(bytes.len(), |at| middle + at + 1);
    let fresh = path.with_extension(format!("jsonl.{}", std::process::id()));
    // One a crash left behind would block every halving after it.
    let _ = fs::remove_file(&fresh);
    let mut out = open_own(&fresh, OpenOptions::new().create_new(true).write(true))?;
    let moved = out
        .write_all(&bytes[start..])
        .and_then(|()| fs::rename(&fresh, path));
    if moved.is_err() {
        let _ = fs::remove_file(&fresh);
    }
    moved
}

/// The sums of one kind of saving.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Total {
    /// The kind, as [`Saving::what`].
    pub what: String,
    /// How many calls it spared on.
    pub calls: usize,
    /// Their characters before.
    pub before: usize,
    /// Their characters after.
    pub after: usize,
}

impl Total {
    /// The characters spared.
    pub fn saved(&self) -> usize {
        self.before.saturating_sub(self.after)
    }
}

/// The count at `path` summed per kind, since `since` when given, the kind that saved most first; no file is no
/// savings yet. A line that does not parse (cut by a crash) is skipped.
///
/// # Errors
///
/// When the file exists but cannot be read.
pub fn totals(path: &Path, since: Option<DateTime<Utc>>) -> io::Result<Vec<Total>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut sums: BTreeMap<String, Total> = BTreeMap::new();
    for saving in text
        .lines()
        .filter_map(|line| serde_json::from_str::<Saving>(line).ok())
        .filter(|saving| since.is_none_or(|since| saving.at >= since))
    {
        let total = sums.entry(saving.what.clone()).or_insert_with(|| Total {
            what: saving.what,
            ..Total::default()
        });
        total.calls += 1;
        total.before += saving.before;
        total.after += saving.after;
    }
    let mut totals: Vec<Total> = sums.into_values().collect();
    totals.sort_by(|a, b| b.saved().cmp(&a.saved()).then(b.calls.cmp(&a.calls)));
    Ok(totals)
}

/// The count at `path` as a [`table`], of the last `days` days when given — what `axh gain` prints.
///
/// # Errors
///
/// When the file exists but cannot be read.
pub fn report(path: &Path, days: Option<u32>) -> io::Result<String> {
    // A number of days past what a date can hold is no limit at all.
    let since = days.and_then(|days| {
        Utc::now().checked_sub_signed(chrono::Duration::try_days(i64::from(days))?)
    });
    Ok(table(&totals(path, since)?))
}

/// The totals as a table for the terminal: calls, characters before and after, and the share spared.
pub fn table(totals: &[Total]) -> String {
    if totals.is_empty() {
        return "nothing counted yet\n".to_owned();
    }
    let width = totals
        .iter()
        .map(|total| total.what.chars().count())
        .max()
        .unwrap_or(0)
        .max("all (pipes left out)".len());
    let mut out = format!(
        "{:<width$}  {:>6}  {:>12}  {:>12}  {:>6}\n",
        "kind", "calls", "chars before", "chars after", "saved"
    );
    let mut all = Total::default();
    for total in totals {
        out.push_str(&row(total, width));
        if !total.what.starts_with(IN_A_PIPE) {
            all.calls += total.calls;
            all.before += total.before;
            all.after += total.after;
        }
    }
    all.what = "all (pipes left out)".to_owned();
    out.push_str(&row(&all, width));
    out
}

fn row(total: &Total, width: usize) -> String {
    let share = (total.saved() * 100).checked_div(total.before).unwrap_or(0);
    format!(
        "{:<width$}  {:>6}  {:>12}  {:>12}  {:>5}%\n",
        total.what, total.calls, total.before, total.after, share
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saving(what: &str, before: usize, after: usize) -> Saving {
        Saving {
            at: Utc::now(),
            what: what.to_owned(),
            before,
            after,
        }
    }

    fn scratch() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-lean-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        dir.join("lean.jsonl")
    }

    #[test]
    fn savings_are_summed_per_kind_the_largest_first() {
        let path = scratch();
        record(&path, &saving("git status", 900, 300)).unwrap();
        record(&path, &saving("cargo", 10_000, 1_000)).unwrap();
        record(&path, &saving("cargo", 5_000, 500)).unwrap();
        record(&path, &saving("in a pipe: cargo", 2_000, 2_000)).unwrap();
        let totals = totals(&path, None).unwrap();
        let kinds: Vec<_> = totals.iter().map(|t| t.what.as_str()).collect();
        assert_eq!(kinds, ["cargo", "git status", "in a pipe: cargo"]);
        assert_eq!(
            (totals[0].calls, totals[0].before, totals[0].after),
            (2, 15_000, 1_500)
        );
        let table = table(&totals);
        assert!(table.contains("90%"), "{table}");
        let all = table.lines().last().unwrap();
        // The pipe's 2,000 characters spared nothing and stay out of the sum.
        assert!(
            all.starts_with("all") && all.contains("15900") && all.ends_with("88%"),
            "{table}"
        );
        let mode = fs::metadata(&path).unwrap().permissions();
        assert_eq!(
            std::os::unix::fs::PermissionsExt::mode(&mode) & 0o777,
            0o600
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn older_savings_and_broken_lines_are_left_out() {
        let path = scratch();
        let mut old = saving("cargo", 100, 10);
        old.at = Utc::now() - chrono::Duration::days(10);
        record(&path, &old).unwrap();
        record(&path, &saving("git diff", 100, 80)).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        file.write_all(b"{\"at\":\"2026-").unwrap();
        let since = Utc::now() - chrono::Duration::days(7);
        let recent = totals(&path, Some(since)).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].what, "git diff");
        assert_eq!(totals(&path, None).unwrap().len(), 2);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_grown_file_keeps_its_newer_half_whole_lines_only() {
        let path = scratch();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line = serde_json::to_string(&saving("cargo", 1, 0)).unwrap();
        let lines = (MAX_BYTES as usize / line.len()) + 10;
        let grown = format!("{line}\n").repeat(lines);
        fs::write(&path, &grown).unwrap();
        record(&path, &saving("git log", 50, 5)).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.len() <= grown.len() / 2 + 2 * (line.len() + 1));
        assert!(
            text.lines()
                .all(|l| serde_json::from_str::<Saving>(l).is_ok())
        );
        assert!(text.lines().last().unwrap().contains("git log"));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn no_file_is_no_savings() {
        assert!(totals(&scratch(), None).unwrap().is_empty());
        assert_eq!(table(&[]), "nothing counted yet\n");
    }
}
