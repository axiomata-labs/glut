//! The arena's test bench (T61 step 2, `docs/plans/glut-tui.md`): fixed tasks from the owner's private repository
//! `glut-arena`, cloned to `~/.glut/arena/tasks` — never public (tasks that end up in training data measure memory),
//! and in `~/.glut`, which no sandboxed command reads: no model sees a task's check or its reference solution. Each
//! task runs on the same models as an arena, judged by its own check script; the results go to
//! `~/.glut/arena/results.jsonl` for `/arena results`. A set is named by its date (`sets/2026-11/`); numbers are
//! compared within a set only.
//!
//! A task is a folder: `task.md` (what the model is told), `task.json` (its limits), `start/` (the repository it
//! starts from, made a fresh git repository for every run), `check.sh` and its `check/` files. The check runs in a
//! copy Glut makes of the start and the model's checked patch (`worktree::check_copy`), never in the model's worktree.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Where the task sets live below the home folder: inside `~/.glut`, which the sandbox keeps from every command.
const ROOT: &str = ".glut/arena/tasks";
/// Where the results live below the home folder.
const DIR: &str = ".glut/arena";
/// The temporary folder's part holding the runs' start repositories: outside `~/.glut`, as the models' git reads
/// the repository their worktree belongs to.
const BENCH: &str = "glut-arena-bench";
/// The results, one JSON line per model and task.
const RESULTS: &str = "results.jsonl";
/// The most bytes of results read.
const MOST_RESULTS: u64 = 16 * 1024 * 1024;
/// The most a task's own limits may ask for: the bench runs unattended.
const MOST_MINUTES: u64 = 120;
const MOST_DOLLARS: f64 = 10.0;
/// The set name the free arena's wins are kept under.
pub const FREE: &str = "free";

/// A task's limits, from its `task.json`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limits {
    #[serde(default = "default_minutes")]
    minutes: u64,
    #[serde(default = "default_dollars")]
    dollars: f64,
}

fn default_minutes() -> u64 {
    crate::arena::TIME_LIMIT.as_secs() / 60
}

fn default_dollars() -> f64 {
    crate::arena::COST_LIMIT
}

/// A task of a set.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// Its folder's name.
    pub name: String,
    /// What the model is told.
    pub text: String,
    pub time_limit: Duration,
    pub cost_limit: f64,
    pub folder: PathBuf,
}

/// The task sets' folder.
pub fn root(home: &Path) -> PathBuf {
    home.join(ROOT)
}

/// Whether `name` is a plain folder name (a set's or a task's): it goes into paths and branch names.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// The plain folders inside `dir`, sorted; links are no folders.
fn folders(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| plain_name(name))
        .collect();
    names.sort();
    names
}

/// The sets in `root`, oldest first.
pub fn sets(root: &Path) -> Vec<String> {
    folders(&root.join("sets"))
}

/// Set `set`'s tasks in their folders' order.
///
/// # Errors
///
/// No such set, a task without its text, start or check, a `task.json` that does not read.
pub fn tasks(root: &Path, set: &str) -> Result<Vec<Task>, String> {
    // `free` names the free arena's wins among the results.
    if !plain_name(set) || set == FREE {
        return Err(tr!(bench_no_set, set = crate::transcript::clean(set)));
    }
    let dir = root.join("sets").join(set);
    let names = folders(&dir);
    if names.is_empty() {
        return Err(tr!(bench_no_set, set = crate::transcript::clean(set)));
    }
    names
        .into_iter()
        .map(|name| {
            let folder = dir.join(&name);
            let text = std::fs::read_to_string(folder.join("task.md"))
                .map_err(|err| format!("{name}/task.md: {err}"))?;
            let limits: Limits = match std::fs::read_to_string(folder.join("task.json")) {
                Ok(json) => {
                    serde_json::from_str(&json).map_err(|err| format!("{name}/task.json: {err}"))?
                }
                Err(_) => Limits {
                    minutes: default_minutes(),
                    dollars: default_dollars(),
                },
            };
            for needed in ["start", "check.sh"] {
                if !folder.join(needed).exists() {
                    return Err(format!("{name}/{needed}: {}", tr!(bench_missing)));
                }
            }
            Ok(Task {
                name,
                text: text.trim().to_owned(),
                time_limit: Duration::from_secs(limits.minutes.clamp(1, MOST_MINUTES) * 60),
                cost_limit: limits.dollars.clamp(0.01, MOST_DOLLARS),
                folder,
            })
        })
        .collect()
}

/// Copies `from` into `to`, files and folders alone: a link in a task's start is not followed nor made.
fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Where the runs' start repositories are made: the user's own folder in the temporary one (`/tmp` on Linux is
/// shared, so the name carries the user and the folder is the user's alone).
pub fn bench_dir() -> PathBuf {
    std::env::temp_dir().join(format!("{BENCH}-{}", rustix::process::getuid().as_raw()))
}

/// [`bench_dir`] made where missing, 0700 — and only when it is the user's own folder, no link.
fn own_bench_dir() -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
    let dir = bench_dir();
    let _ = std::fs::DirBuilder::new().mode(0o700).create(&dir);
    let meta =
        std::fs::symlink_metadata(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    if !meta.is_dir()
        || meta.uid() != rustix::process::getuid().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(tr!(bench_dir_not_own, dir = dir.display()));
    }
    Ok(dir)
}

/// Where the bench's checks run, below the home folder: inside `~/.glut`, which no model's command reads or writes.
pub fn checks_dir(home: &Path) -> PathBuf {
    home.join(DIR).join("checks")
}

/// A fresh git repository of `task`'s start in [`bench_dir`], one commit on `main`: the arena's worktrees are made
/// from it, and it goes with them. Git runs without the owner's global config or ignore file — a start is taken
/// whole, never filtered.
///
/// # Errors
///
/// What could not be copied, or git's refusal; nothing is left behind.
pub fn prepare(set: &str, task: &Task) -> Result<PathBuf, String> {
    static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = own_bench_dir()?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let place = dir.join(format!(
        "{set}-{}-{stamp}-{}-{run}",
        task.name,
        std::process::id()
    ));
    // A folder of its own: one that stands already is another run's.
    std::fs::create_dir(&place).map_err(|err| format!("{}: {err}", place.display()))?;
    let made = copy_tree(&task.folder.join("start"), &place)
        .map_err(|err| format!("{}: {err}", place.display()))
        .and_then(|()| {
            git_alone(&place, &["init", "-q", "-b", "main"])?;
            git_alone(&place, &["add", "--all", "--force"])?;
            git_alone(&place, &["commit", "-q", "-m", "start"])
        });
    if let Err(reason) = made {
        let _ = std::fs::remove_dir_all(&place);
        return Err(reason);
    }
    Ok(place)
}

/// Git in `dir` with nothing of the owner's around it: no global or system config, no attributes or ignore file, no
/// hooks, no signing, an identity of its own, no `GIT_*` from the environment.
fn git_alone(dir: &Path, args: &[&str]) -> Result<(), String> {
    let mut command = std::process::Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(&name);
        }
    }
    let out = command
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.attributesFile=/dev/null",
        ])
        .args([
            "-c",
            "core.excludesFile=/dev/null",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(["-c", "user.name=Glut", "-c", "user.email=glut@localhost"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| err.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(crate::transcript::clean(&String::from_utf8_lossy(
            &out.stderr,
        )))
    }
}

/// Removes a run's start repository again — only one [`prepare`] made, by its place.
pub fn forget(place: &Path) {
    let ours = place.parent() == Some(bench_dir().as_path())
        && std::fs::symlink_metadata(place).is_ok_and(|meta| meta.is_dir());
    if ours {
        let _ = std::fs::remove_dir_all(place);
    }
}

/// One model's result on one task, or one win in the free arena.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    /// The day it ran, `2026-11-02`.
    pub date: String,
    /// The set and task (the free arena: [`FREE`] and the start of the owner's task).
    pub set: String,
    pub task: String,
    /// `provider/model`.
    pub model: String,
    /// The check passed (a bench task), or the owner took it (the free arena).
    pub passed: bool,
    /// Its time, cost and tokens, as far as known.
    pub seconds: Option<u64>,
    pub dollars: Option<f64>,
    pub tokens: u64,
    /// How it ended, in words, when it did not pass by its check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended: Option<String>,
}

/// Appends `records` to the results.
///
/// # Errors
///
/// When the file cannot be written.
pub fn keep(home: &Path, records: &[Record]) -> Result<(), String> {
    let dir = home.join(DIR);
    glut::providers::make_private_dir(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let path = dir.join(RESULTS);
    let mut lines = String::new();
    for record in records {
        lines.push_str(&serde_json::to_string(record).map_err(|err| err.to_string())?);
        lines.push('\n');
    }
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    // One write: two windows appending at once never cut each other's lines.
    file.write_all(lines.as_bytes())
        .map_err(|err| format!("{}: {err}", path.display()))
}

/// Every result kept; a line that does not read is skipped.
pub fn results(home: &Path) -> Vec<Record> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let Ok(file) = std::fs::File::open(home.join(DIR).join(RESULTS)) else {
        return Vec::new();
    };
    let _ = file.take(MOST_RESULTS).read_to_end(&mut bytes);
    // A line cut by the bound or not UTF-8 is skipped, never the rest with it.
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

/// `/arena results`: per set, a grid of its tasks against its models (passed or not, and the time), each model's sums
/// below, the one ahead and why the others failed; the free arena's wins per model.
pub fn table(records: &[Record]) -> String {
    let mut sets: BTreeMap<&str, Vec<&Record>> = BTreeMap::new();
    for record in records {
        sets.entry(record.set.as_str()).or_default().push(record);
    }
    if sets.is_empty() {
        return tr!(bench_no_results).to_owned();
    }
    let mut blocks = Vec::new();
    for (set, records) in &sets {
        if *set == FREE {
            blocks.push(wins(records));
            continue;
        }
        let title = |tasks: usize, models: usize| {
            tr!(
                bench_set_title,
                set = crate::transcript::clean(set),
                tasks = tasks,
                models = models
            )
        };
        blocks.push(grid(records, title));
    }
    blocks.join("\n\n")
}

/// A bench run's end: the grid of the results it kept.
pub fn finished(set: &str, records: &[Record]) -> String {
    let records: Vec<&Record> = records.iter().collect();
    if records.is_empty() {
        return tr!(bench_no_results).to_owned();
    }
    grid(&records, |tasks, models| {
        tr!(
            bench_done,
            set = crate::transcript::clean(set),
            tasks = tasks,
            models = models
        )
    })
}

/// What a model did across a set's records.
#[derive(Default)]
struct Sum {
    runs: usize,
    passed: usize,
    seconds: u64,
    timed: usize,
    dollars: f64,
    priced: usize,
    tokens: u64,
}

impl Sum {
    fn add(&mut self, record: &Record) {
        self.runs += 1;
        self.passed += usize::from(record.passed);
        if let Some(seconds) = record.seconds {
            self.seconds += seconds;
            self.timed += 1;
        }
        if let Some(dollars) = record.dollars {
            self.dollars += dollars;
            self.priced += 1;
        }
        self.tokens += record.tokens;
    }

    /// The share passed: a model run more often has more runs, not more skill.
    fn rate(&self) -> f64 {
        self.passed as f64 / self.runs.max(1) as f64
    }

    fn mean(&self) -> Option<u64> {
        (self.timed > 0).then(|| self.seconds / self.timed as u64)
    }
}

/// A line of sums below the grid: its label and how a model's sum reads in it.
type Measure = (&'static str, Box<dyn Fn(&Sum) -> String>);

/// Seconds as the arena's clock, `—` when unknown.
fn clock(seconds: Option<u64>) -> String {
    seconds.map_or_else(
        || "—".to_owned(),
        |seconds| crate::arena::clock(Duration::from_secs(seconds)),
    )
}

/// Each model's name for a column: the part after its last `/`, unless two models share it.
fn short_names<'a>(models: &[&'a str]) -> Vec<String> {
    let last = |model: &'a str| model.rsplit('/').next().unwrap_or(model);
    models
        .iter()
        .map(|model| {
            let short = last(model);
            let shared = models.iter().filter(|other| last(other) == short).count() > 1;
            crate::transcript::clean(if shared { model } else { short })
        })
        .collect()
}

/// `text` padded to `wide` cells.
fn padded(text: &str, wide: usize) -> String {
    let count = crate::transcript::text_width(text);
    format!("{text}{}", " ".repeat(wide.saturating_sub(count)))
}

/// One set's grid: its tasks down, its models across (the best first), a line of sums per measure, then the model
/// ahead and why each failure failed.
fn grid(records: &[&Record], title: impl Fn(usize, usize) -> String) -> String {
    let mut sums: BTreeMap<&str, Sum> = BTreeMap::new();
    let mut cells: BTreeMap<(&str, &str), Sum> = BTreeMap::new();
    let mut tasks = BTreeSet::new();
    for record in records {
        sums.entry(record.model.as_str()).or_default().add(record);
        cells
            .entry((record.task.as_str(), record.model.as_str()))
            .or_default()
            .add(record);
        tasks.insert(record.task.as_str());
    }
    let mut models: Vec<&str> = sums.keys().copied().collect();
    models.sort_by(|a, b| {
        let (a, b) = (&sums[a], &sums[b]);
        b.rate().total_cmp(&a.rate()).then(
            a.mean()
                .unwrap_or(u64::MAX)
                .cmp(&b.mean().unwrap_or(u64::MAX)),
        )
    });
    let names = short_names(&models);
    let cell = |task: &str, model: &str| match cells.get(&(task, model)) {
        None => String::new(),
        Some(sum) if sum.runs == 1 => {
            let mark = if sum.passed == 1 { "✓" } else { "✗" };
            format!("{mark} {}", clock(sum.mean()))
        }
        Some(sum) => format!("{}/{} Ø {}", sum.passed, sum.runs, clock(sum.mean())),
    };
    let measures: [Measure; 4] = [
        (
            tr!(bench_sum_passed),
            Box::new(|sum: &Sum| tr!(bench_of, passed = sum.passed, runs = sum.runs)),
        ),
        (
            tr!(bench_sum_time),
            Box::new(|sum: &Sum| clock((sum.timed > 0).then_some(sum.seconds))),
        ),
        (
            tr!(bench_sum_cost),
            Box::new(|sum: &Sum| {
                // A run without a price makes the sum a floor.
                if sum.priced == sum.runs {
                    format!("${:.3}", sum.dollars)
                } else if sum.priced > 0 {
                    format!("≥ ${:.3}", sum.dollars)
                } else {
                    "—".to_owned()
                }
            }),
        ),
        (
            tr!(bench_sum_tokens),
            Box::new(|sum: &Sum| crate::meter::tokens(sum.tokens)),
        ),
    ];
    let first = std::iter::once(crate::transcript::text_width(tr!(bench_task_column)))
        .chain(tasks.iter().map(|task| crate::transcript::text_width(task)))
        .chain(
            measures
                .iter()
                .map(|(label, _)| crate::transcript::text_width(label)),
        )
        .max()
        .unwrap_or(0);
    let wide: Vec<usize> = models
        .iter()
        .zip(&names)
        .map(|(model, name)| {
            std::iter::once(crate::transcript::text_width(name))
                .chain(
                    tasks
                        .iter()
                        .map(|task| crate::transcript::text_width(&cell(task, model))),
                )
                .chain(
                    measures
                        .iter()
                        .map(|(_, value)| crate::transcript::text_width(&value(&sums[model]))),
                )
                .max()
                .unwrap_or(0)
        })
        .collect();
    let row = |label: &str, values: Vec<String>| {
        let mut line = format!("  {}", padded(label, first));
        for (value, wide) in values.iter().zip(&wide) {
            line.push_str("   ");
            line.push_str(&padded(value, *wide));
        }
        line.trim_end().to_owned()
    };
    let mut lines = vec![title(tasks.len(), models.len()), String::new()];
    lines.push(row(tr!(bench_task_column), names.clone()));
    for task in &tasks {
        let values = models.iter().map(|model| cell(task, model)).collect();
        lines.push(row(&crate::transcript::clean(task), values));
    }
    let width = first + wide.iter().map(|wide| wide + 3).sum::<usize>();
    lines.push(format!("  {}", "─".repeat(width)));
    for (label, value) in &measures {
        lines.push(row(
            label,
            models.iter().map(|model| value(&sums[model])).collect(),
        ));
    }
    // Ahead only where it passed more than the next: a tie or nothing passed names no one.
    if let [first, second, ..] = models[..]
        && sums[first].passed > 0
        && sums[first].rate() > sums[second].rate()
    {
        let sum = &sums[first];
        lines.push(String::new());
        lines.push(tr!(
            bench_ahead,
            model = names[0],
            passed = sum.passed,
            runs = sum.runs
        ));
    }
    let failed: Vec<(&str, String, &str)> = records
        .iter()
        .filter(|record| !record.passed)
        .filter_map(|record| {
            let at = models.iter().position(|model| *model == record.model)?;
            Some((
                record.task.as_str(),
                names[at].clone(),
                record.ended.as_deref()?,
            ))
        })
        .collect();
    if !failed.is_empty() {
        let task_wide = failed
            .iter()
            .map(|(task, ..)| crate::transcript::text_width(task))
            .max()
            .unwrap_or(0);
        let name_wide = failed
            .iter()
            .map(|(_, name, _)| crate::transcript::text_width(name))
            .max()
            .unwrap_or(0);
        lines.push(String::new());
        lines.push(tr!(bench_failed_title).to_owned());
        for (task, name, why) in failed {
            lines.push(format!(
                "  {}   {}   {}",
                padded(&crate::transcript::clean(task), task_wide),
                padded(&name, name_wide),
                crate::transcript::clean(why)
            ));
        }
    }
    lines.join("\n")
}

/// The free arena's wins per model, the most first.
fn wins(records: &[&Record]) -> String {
    let mut won: BTreeMap<&str, usize> = BTreeMap::new();
    for record in records {
        *won.entry(record.model.as_str()).or_default() += usize::from(record.passed);
    }
    let mut ranked: Vec<(&str, usize)> = won.into_iter().collect();
    ranked.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let wide = ranked
        .iter()
        .map(|(model, _)| crate::transcript::text_width(model))
        .max()
        .unwrap_or(0);
    let mut lines = vec![tr!(bench_wins_title).to_owned()];
    for (model, n) in ranked {
        lines.push(format!(
            "  {}  {}",
            padded(&crate::transcript::clean(model), wide),
            tr!(bench_wins, n = n)
        ));
    }
    lines.join("\n")
}

/// What `/arena tasks` alone says: every set with its tasks, or where the sets are looked for.
pub fn listing(root: &Path) -> String {
    let sets = sets(root);
    if sets.is_empty() {
        return tr!(bench_no_sets, root = root.display());
    }
    let mut lines = vec![tr!(bench_sets_title, root = root.display())];
    for set in sets {
        match tasks(root, &set) {
            Ok(tasks) => {
                let names: Vec<&str> = tasks.iter().map(|task| task.name.as_str()).collect();
                lines.push(format!("  {set}: {}", names.join(", ")));
            }
            Err(reason) => lines.push(format!("  {set}: {}", crate::transcript::clean(&reason))),
        }
    }
    lines.push(tr!(bench_usage).to_owned());
    lines.join("\n")
}

/// A bench run under way: the set, its models and the tasks still to come.
#[derive(Debug, Clone)]
pub struct Bench {
    /// The owner's home: where the tasks and the results are.
    pub home: PathBuf,
    /// The pane it was started from: every task's arena starts from it, its notes stand there.
    pub home_pane: u64,
    /// The set and the models it runs.
    pub set: String,
    pub contenders: Vec<crate::arena::Contender>,
    pub left: std::collections::VecDeque<Task>,
    /// The task the arena runs now, and its start repository.
    pub current: Option<(Task, PathBuf)>,
    /// How many tasks the set has.
    pub total: usize,
    /// The results this run kept so far, for its end.
    pub kept: Vec<Record>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("glut-bench-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        home
    }

    fn task(root: &Path, set: &str, name: &str, json: Option<&str>) {
        let dir = root.join("sets").join(set).join(name);
        std::fs::create_dir_all(dir.join("start/src")).unwrap();
        std::fs::write(dir.join("task.md"), "Fix it.\n").unwrap();
        std::fs::write(dir.join("check.sh"), "exit 0\n").unwrap();
        std::fs::write(dir.join("start/src/a.txt"), "a\n").unwrap();
        if let Some(json) = json {
            std::fs::write(dir.join("task.json"), json).unwrap();
        }
    }

    #[test]
    fn a_set_reads_its_tasks_in_order_with_their_limits_bounded() {
        let home = home("read");
        let root = root(&home);
        task(&root, "2026-11", "b-second", None);
        task(
            &root,
            "2026-11",
            "a-first",
            Some(r#"{"minutes": 500, "dollars": 0.5}"#),
        );
        std::fs::create_dir_all(root.join("sets/2026-11/.hidden")).unwrap();
        assert_eq!(sets(&root), ["2026-11"]);
        let tasks = tasks(&root, "2026-11").unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].name, "a-first");
        assert_eq!(tasks[0].text, "Fix it.");
        assert_eq!(
            tasks[0].time_limit,
            Duration::from_secs(MOST_MINUTES * 60),
            "bounded"
        );
        assert!((tasks[0].cost_limit - 0.5).abs() < f64::EPSILON);
        assert_eq!(tasks[1].time_limit, crate::arena::TIME_LIMIT);
        assert!(super::tasks(&root, "../etc").is_err());
        assert!(super::tasks(&root, "2026-12").is_err());
        std::fs::remove_file(root.join("sets/2026-11/b-second/check.sh")).unwrap();
        assert!(
            super::tasks(&root, "2026-11").is_err(),
            "a task without its check"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_task_starts_from_a_fresh_repository_that_goes_again() {
        let home = home("prepare");
        let root = root(&home);
        task(&root, "2026-11", "one", None);
        let tasks = tasks(&root, "2026-11").unwrap();
        let place = prepare("2026-11", &tasks[0]).unwrap();
        assert_eq!(
            std::fs::read_to_string(place.join("src/a.txt")).unwrap(),
            "a\n"
        );
        assert!(crate::worktree::repo_of(&place).unwrap().is_some());
        assert!(!place.join("check.sh").exists(), "the check stays outside");
        forget(&place);
        assert!(!place.exists());
        forget(&root);
        assert!(
            root.exists(),
            "only a start repository of its own making goes"
        );
        forget(&bench_dir());
        assert!(bench_dir().exists(), "nor their folder");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn results_are_kept_and_ranked_per_set_with_the_free_arenas_wins_counted() {
        let home = home("results");
        let record = |set: &str, model: &str, passed, seconds| Record {
            date: "2026-11-02".into(),
            set: set.into(),
            task: "t".into(),
            model: model.into(),
            passed,
            seconds: Some(seconds),
            dollars: Some(0.1),
            tokens: 10,
            ended: None,
        };
        keep(
            &home,
            &[
                record("2026-11", "p/slow", true, 100),
                record("2026-11", "p/fast", true, 10),
                record("2026-11", "p/fast", false, 30),
                record("2026-11", "p/slow", true, 50),
                record(FREE, "p/fast", true, 5),
            ],
        )
        .unwrap();
        let kept = results(&home);
        assert_eq!(kept.len(), 5);
        let table = table(&kept);
        let header = table.lines().nth(2).unwrap();
        let slow = header.find("slow").unwrap();
        let fast = header.find("fast").unwrap();
        assert!(slow < fast, "a higher share passed ranks first: {table}");
        assert!(
            table.contains("2/2 Ø 1:15"),
            "slow's runs and mean time: {table}"
        );
        assert!(
            table.contains(&tr!(bench_of, passed = 2, runs = 2)),
            "{table}"
        );
        assert!(table.contains(&tr!(bench_wins, n = 1)), "{table}");
        assert_eq!(super::table(&[]), tr!(bench_no_results));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_runs_end_shows_its_tasks_against_its_models_and_why_one_failed() {
        let record = |task: &str, model: &str, passed, seconds, ended: Option<&str>| Record {
            date: "2026-11-03".into(),
            set: "2026-11".into(),
            task: task.into(),
            model: model.into(),
            passed,
            seconds: Some(seconds),
            dollars: Some(0.001),
            tokens: 30_000,
            ended: ended.map(str::to_owned),
        };
        let said = finished(
            "2026-11",
            &[
                record(
                    "01-rust",
                    "openrouter/deepseek/flash",
                    false,
                    117,
                    Some("1 failed"),
                ),
                record("01-rust", "openrouter/openai/luna", true, 43, None),
                record("02-ts", "openrouter/deepseek/flash", true, 158, None),
                record("02-ts", "openrouter/openai/luna", true, 36, None),
            ],
        );
        println!("{said}");
        let lines: Vec<&str> = said.lines().collect();
        assert_eq!(
            lines[0],
            tr!(bench_done, set = "2026-11", tasks = 2, models = 2)
        );
        let header = lines[2];
        assert!(
            header.find("luna").unwrap() < header.find("flash").unwrap(),
            "the best first, by its short name: {said}"
        );
        assert!(
            lines[3].starts_with("  01-rust")
                && lines[3].contains("✗ 1:57")
                && lines[3].contains("✓ 0:43")
        );
        // The columns stand under each other.
        assert_eq!(lines[3].find('✓'), lines[4].find('✓'));
        assert!(said.contains(&tr!(bench_ahead, model = "luna", passed = 2, runs = 2)));
        let failed = said.lines().last().unwrap();
        assert!(
            failed.contains("01-rust") && failed.contains("flash") && failed.ends_with("1 failed")
        );
        assert_eq!(finished("2026-11", &[]), tr!(bench_no_results));
    }
}
