//! Timed runs while Glut is closed (`docs/plans/glut-tui.md`, T60b): the owner's plans in `~/.glut/schedules.json`,
//! each one a LaunchAgent of its own that starts `glut schedule run --scheduled <name>` at its times. What a plan may
//! do is fixed when it is made — its folder, provider, model, mode and rules — and only the owner's `~/.glut/` counts
//! beside it, never the project's. Every run is kept in `~/.glut/schedules/<name>.jsonl`.
//!
//! launchd starts a job only while the owner is logged in (the Keychain is reached then alone), never twice at once,
//! and a time missed asleep once on waking, never more.

use std::io::{BufRead as _, Read as _};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike as _, Local, NaiveTime, TimeZone as _, Utc};
use glut::Mode;
use serde::{Deserialize, Serialize};

/// The plans' file, below `~/.glut`.
const FILE: &str = "schedules.json";
/// The folder of the runs' records, logs and locks, below `~/.glut`.
const DIR: &str = "schedules";
/// A plans file or a record larger than this is not read.
const MAX_BYTES: u64 = 1024 * 1024;
/// A plan's LaunchAgent is named this and the plan's name; `local.` says no one's domain.
const LABEL: &str = "local.glut.schedule.";
/// The most characters a plan's name has.
const MOST_NAME: usize = 40;
/// The shortest and the longest `--every`, in minutes: ten minutes, a week.
const SHORTEST_EVERY: u32 = 10;
const LONGEST_EVERY: u32 = 7 * 24 * 60;
/// What a run may cost and take when the plan says nothing (the grill's word, 2026-10-26).
pub const DEFAULT_MAX_USD: f64 = 1.0;
pub const DEFAULT_MAX_MINUTES: u32 = 30;
/// The most tokens a run on a subscription may use when the plan says nothing (PV4): its use has no price.
pub const DEFAULT_MAX_TOKENS: u64 = 1_000_000;
/// The most a plan's budget and time limit may be: what a mistyped number must not reach.
const MOST_USD: f64 = 1000.0;
const MOST_MINUTES: u32 = 24 * 60;
/// How many runs a plan's record keeps; the Runs tab shows the last ten.
const KEPT_RUNS: usize = 50;
/// The days as `--days` writes them, Monday first: ISO's numbers 1 to 7, launchd's `Weekday` too (7 is Sunday).
const DAYS: [&str; 7] = ["mo", "tu", "we", "th", "fr", "sa", "su"];

/// When a plan runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum When {
    /// At this time of day on these days (ISO numbers, Monday 1), every day when none are named.
    At {
        hour: u32,
        minute: u32,
        #[serde(default)]
        days: Vec<u32>,
    },
    /// Every so many minutes, counted by launchd from when it took the plan.
    Every { minutes: u32 },
}

/// One plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    pub name: String,
    /// The folder it works in: where it was made.
    pub folder: PathBuf,
    /// The owner's message, or `/skill <name> [what for]`.
    pub task: String,
    pub when: When,
    /// The provider and the model, fixed at making.
    pub provider: String,
    pub model: String,
    /// What it may do without a rule; every ask is refused.
    pub mode: Mode,
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
    /// Folders beside its own (T60c), the owner's word as typed.
    #[serde(default)]
    pub folders: Vec<String>,
    pub max_usd: f64,
    pub max_minutes: u32,
    /// The most tokens a run may use, counted on a subscription only (PV4); `None`: [`DEFAULT_MAX_TOKENS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub paused: bool,
    /// The `PATH` its commands find programs on: the one it was made with, as launchd gives a job almost none.
    #[serde(default)]
    pub path: String,
    /// When it was made or last changed: an `--every` plan counts from then.
    pub changed: DateTime<Utc>,
}

/// Every plan.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedules {
    #[serde(default)]
    pub plans: Vec<Plan>,
}

impl Schedules {
    /// The plans of `home`; none when there is no file.
    ///
    /// # Errors
    ///
    /// A message when the file cannot be read or holds no plans Glut reads — it is not written over then.
    pub fn load(home: &Path) -> Result<Schedules, String> {
        let path = file(home);
        let mut text = String::new();
        match std::fs::File::open(&path) {
            Ok(file) => file
                .take(MAX_BYTES + 1)
                .read_to_string(&mut text)
                .map_err(|err| tr!(schedule_file_broken, reason = err))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Schedules::default());
            }
            Err(err) => return Err(tr!(schedule_file_broken, reason = err)),
        };
        if text.len() as u64 > MAX_BYTES {
            return Err(tr!(schedule_file_broken, reason = "too large"));
        }
        let schedules: Schedules =
            serde_json::from_str(&text).map_err(|err| tr!(schedule_file_broken, reason = err))?;
        for plan in &schedules.plans {
            plan.check().map_err(|reason| {
                tr!(
                    schedule_file_broken,
                    reason = format!("{}: {reason}", plan.name)
                )
            })?;
        }
        Ok(schedules)
    }

    /// Writes the plans, readable by the owner alone: a task can say anything.
    ///
    /// # Errors
    ///
    /// A message when the file cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), String> {
        let path = file(home);
        let dir = path.parent().unwrap_or(home);
        let text = serde_json::to_string_pretty(self).map_err(|err| err.to_string())?;
        glut::providers::make_private_dir(dir)
            .and_then(|()| glut::providers::write_replacing(dir, &path, text.as_bytes(), 0o600))
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn get(&self, name: &str) -> Option<&Plan> {
        self.plans.iter().find(|plan| plan.name == name)
    }

    /// `plan` in the place of the one of its name, or after the others.
    pub fn put(&mut self, plan: Plan) {
        match self.plans.iter_mut().find(|kept| kept.name == plan.name) {
            Some(kept) => *kept = plan,
            None => self.plans.push(plan),
        }
    }

    /// The plan `name` taken out.
    pub fn take(&mut self, name: &str) -> Option<Plan> {
        let at = self.plans.iter().position(|plan| plan.name == name)?;
        Some(self.plans.remove(at))
    }
}

impl Plan {
    /// Why the plan cannot run as it is written, if it cannot: what `add` refuses, read again from the file.
    ///
    /// # Errors
    ///
    /// The first thing wrong, for the owner to read.
    pub fn check(&self) -> Result<(), String> {
        check_name(&self.name)?;
        check_task(&self.task)?;
        match &self.when {
            When::At { hour, minute, days } => {
                if *hour > 23 || *minute > 59 {
                    return Err(tr!(schedule_at_bad).to_owned());
                }
                if days.iter().any(|day| !(1..=7).contains(day)) {
                    return Err(tr!(schedule_days_bad).to_owned());
                }
            }
            When::Every { minutes } => {
                if !(SHORTEST_EVERY..=LONGEST_EVERY).contains(minutes) {
                    return Err(tr!(schedule_every_bad).to_owned());
                }
            }
        }
        for rule in self.allow.iter().chain(&self.deny) {
            glut::Rule::parse(rule)
                .map_err(|reason| tr!(schedule_rule_bad, rule = rule, reason = reason))?;
        }
        if !usable_usd(self.max_usd) {
            return Err(tr!(schedule_usd_bad).to_owned());
        }
        if !usable_minutes(self.max_minutes) {
            return Err(tr!(schedule_minutes_bad).to_owned());
        }
        if self
            .max_tokens
            .is_some_and(|tokens| !(1..=crate::looping::MOST_TOKENS).contains(&tokens))
        {
            return Err(tr!(schedule_tokens_bad).to_owned());
        }
        if !self.folder.is_absolute() {
            return Err(tr!(schedule_folder_gone, folder = self.folder.display()));
        }
        Ok(())
    }

    /// Its LaunchAgent's label.
    pub fn label(&self) -> String {
        format!("{LABEL}{}", self.name)
    }

    /// When it runs, as the owner reads it: `at 07:00 on mo–fr`, `every 2h`.
    pub fn when_shown(&self) -> String {
        match &self.when {
            When::At { hour, minute, days } if days.is_empty() || days.len() == 7 => {
                tr!(schedule_when_at, time = format!("{hour:02}:{minute:02}"))
            }
            When::At { hour, minute, days } => tr!(
                schedule_when_at_days,
                time = format!("{hour:02}:{minute:02}"),
                days = days_shown(days)
            ),
            When::Every { minutes } => tr!(schedule_when_every, every = every_shown(*minutes)),
        }
    }

    /// When it runs next after `now` — for an `--every` plan counted from its last start (or from when it was made)
    /// as launchd counts it; `None` while it is paused.
    pub fn next(
        &self,
        last_start: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> Option<DateTime<Utc>> {
        if self.paused {
            return None;
        }
        match &self.when {
            When::At { hour, minute, days } => {
                let time = NaiveTime::from_hms_opt(*hour, *minute, 0)?;
                let today = now.with_timezone(&Local).date_naive();
                (0..=7).find_map(|ahead| {
                    let date = today + chrono::Days::new(ahead);
                    let day = date.weekday().number_from_monday();
                    if !days.is_empty() && !days.contains(&day) {
                        return None;
                    }
                    // A time the clock skips (summer time) is not one launchd fires at either: the next day's.
                    let at = Local.from_local_datetime(&date.and_time(time)).earliest()?;
                    let at = at.with_timezone(&Utc);
                    (at > now).then_some(at)
                })
            }
            When::Every { minutes } => {
                let from = last_start.map_or(self.changed, |start| start.max(self.changed));
                let every = chrono::Duration::minutes(i64::from(*minutes));
                let mut next = from + every;
                // Asleep past its time: launchd makes it up once on waking — soon, not a time gone by.
                if next < now {
                    next = now;
                }
                Some(next)
            }
        }
    }
}

/// `~/.glut/schedules.json`.
fn file(home: &Path) -> PathBuf {
    home.join(".glut").join(FILE)
}

/// `~/.glut/schedules/`.
pub fn dir(home: &Path) -> PathBuf {
    home.join(".glut").join(DIR)
}

/// Where a plan's LaunchAgent is written.
pub fn agent_file(home: &Path, plan: &Plan) -> PathBuf {
    home.join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", plan.label()))
}

/// Whether `name` can name a plan: small letters, digits and `-`, a letter or a digit first — a file's name and a
/// label's part as it is.
///
/// # Errors
///
/// What a name is.
pub fn check_name(name: &str) -> Result<(), String> {
    let fits = !name.is_empty()
        && name.chars().count() <= MOST_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-');
    fits.then_some(())
        .ok_or_else(|| tr!(schedule_name_bad).to_owned())
}

/// Whether `task` can be a plan's: a message, or `/skill <name> [what for]` — no other of the owner's commands,
/// which would act for the owner with nobody there.
///
/// # Errors
///
/// What a task is.
pub fn check_task(task: &str) -> Result<(), String> {
    let task = task.trim();
    if task.is_empty() {
        return Err(tr!(schedule_task_missing).to_owned());
    }
    let Some(command) = task.strip_prefix('/') else {
        return Ok(());
    };
    let name = command.split(' ').next().unwrap_or_default();
    let skill = command
        .strip_prefix("skill ")
        .map(str::trim)
        .unwrap_or_default();
    if name == "skill" && !skill.is_empty() {
        return Ok(());
    }
    Err(tr!(schedule_task_command).to_owned())
}

/// `--at`'s time: `07:00`, `7:05`.
///
/// # Errors
///
/// What a time is.
pub fn parse_at(word: &str) -> Result<(u32, u32), String> {
    let bad = || tr!(schedule_at_bad).to_owned();
    let (hour, minute) = word.trim().split_once(':').ok_or_else(bad)?;
    let digits = |part: &str, most: u32| {
        (!part.is_empty() && part.len() <= 2 && part.chars().all(|c| c.is_ascii_digit()))
            .then(|| part.parse::<u32>().ok())
            .flatten()
            .filter(|value| *value <= most)
    };
    match (digits(hour, 23), digits(minute, 59)) {
        (Some(hour), Some(minute)) => Ok((hour, minute)),
        _ => Err(bad()),
    }
}

/// `--days`: `mo-fr`, `sa,su`, `mo,we-fr`; every day once in order, Monday 1.
///
/// # Errors
///
/// What days are.
pub fn parse_days(word: &str) -> Result<Vec<u32>, String> {
    let bad = || tr!(schedule_days_bad).to_owned();
    let day = |name: &str| {
        DAYS.iter()
            .position(|day| *day == name.trim().to_ascii_lowercase())
            .map(|at| at as u32 + 1)
    };
    let mut days = Vec::new();
    for part in word.split(',') {
        let (first, last) = match part.split_once('-') {
            Some((first, last)) => (day(first).ok_or_else(bad)?, day(last).ok_or_else(bad)?),
            None => {
                let only = day(part).ok_or_else(bad)?;
                (only, only)
            }
        };
        // A range runs on past Sunday: `fr-mo` is Friday to Monday.
        let mut at = first;
        loop {
            if !days.contains(&at) {
                days.push(at);
            }
            if at == last {
                break;
            }
            at = at % 7 + 1;
        }
    }
    days.sort_unstable();
    Ok(days)
}

/// `--every`: `30m`, `2h`, `1h30m`, `1d` — from ten minutes to a week, in minutes.
///
/// # Errors
///
/// What an interval is.
pub fn parse_every(word: &str) -> Result<u32, String> {
    let bad = || tr!(schedule_every_bad).to_owned();
    let mut minutes: u32 = 0;
    let mut digits = String::new();
    let mut units = 0;
    for c in word.trim().chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        let unit = match c {
            'm' => 1,
            'h' => 60,
            'd' => 24 * 60,
            _ => return Err(bad()),
        };
        let n: u32 = digits.parse().map_err(|_| bad())?;
        minutes = n
            .checked_mul(unit)
            .and_then(|part| minutes.checked_add(part))
            .ok_or_else(bad)?;
        digits.clear();
        units += 1;
    }
    if !digits.is_empty() || units == 0 || !(SHORTEST_EVERY..=LONGEST_EVERY).contains(&minutes) {
        return Err(bad());
    }
    Ok(minutes)
}

/// `--mode`: the screen's words (`ask`, `build`) and the settings' (`default`, `accept_edits`).
///
/// # Errors
///
/// The modes there are.
pub fn parse_mode(word: &str) -> Result<Mode, String> {
    match word.trim() {
        "ask" => Ok(Mode::Default),
        other => Mode::parse(other).ok_or_else(|| tr!(schedule_mode_bad).to_owned()),
    }
}

/// Whether `usd` can be a run's budget.
pub fn usable_usd(usd: f64) -> bool {
    usd.is_finite() && usd > 0.0 && usd <= MOST_USD
}

/// Whether `minutes` can be a run's time limit.
pub fn usable_minutes(minutes: u32) -> bool {
    (1..=MOST_MINUTES).contains(&minutes)
}

/// Days as the owner wrote them: `mo–fr` for a run of three or more, else each.
fn days_shown(days: &[u32]) -> String {
    let name = |day: u32| DAYS[(day as usize - 1) % 7];
    let mut parts = Vec::new();
    let mut at = 0;
    while at < days.len() {
        let mut end = at;
        while end + 1 < days.len() && days[end + 1] == days[end] + 1 {
            end += 1;
        }
        if end - at >= 2 {
            parts.push(format!("{}–{}", name(days[at]), name(days[end])));
        } else {
            parts.extend(days[at..=end].iter().map(|day| name(*day).to_owned()));
        }
        at = end + 1;
    }
    parts.join(",")
}

/// An interval in minutes as `--every` writes it: `30m`, `2h`, `1d2h`.
fn every_shown(minutes: u32) -> String {
    let (days, hours, minutes) = (minutes / (24 * 60), minutes % (24 * 60) / 60, minutes % 60);
    let mut text = String::new();
    for (count, unit) in [(days, 'd'), (hours, 'h'), (minutes, 'm')] {
        if count > 0 {
            text.push_str(&format!("{count}{unit}"));
        }
    }
    text
}

/// How one run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Answered,
    Failed,
    Budget,
    Timeout,
    /// Refusals ended it (#112).
    Refused,
    Stopped,
    /// The run before still ran.
    Skipped,
    /// Its folder, its provider or its settings were not there to start with.
    Unstarted,
    /// A subscription's usage limit was reached (PV4): the plan's, not the run's.
    PlanLimit,
}

impl Outcome {
    /// The end of a turn.
    pub fn of(reason: glut::FinishReason) -> Outcome {
        match reason {
            glut::FinishReason::Answered => Outcome::Answered,
            glut::FinishReason::Failed | glut::FinishReason::MaxTurns => Outcome::Failed,
            glut::FinishReason::Timeout => Outcome::Timeout,
            glut::FinishReason::Cancelled => Outcome::Stopped,
            glut::FinishReason::Refused => Outcome::Refused,
            glut::FinishReason::Budget => Outcome::Budget,
        }
    }

    /// Whether the run did what it was there for.
    pub fn well(self) -> bool {
        self == Outcome::Answered
    }

    /// As the owner reads it.
    pub fn said(self) -> &'static str {
        match self {
            Outcome::Answered => tr!(schedule_outcome_answered),
            Outcome::Failed => tr!(schedule_outcome_failed),
            Outcome::Budget => tr!(schedule_outcome_budget),
            Outcome::Timeout => tr!(schedule_outcome_timeout),
            Outcome::Refused => tr!(schedule_outcome_refused),
            Outcome::Stopped => tr!(schedule_outcome_stopped),
            Outcome::Skipped => tr!(schedule_outcome_skipped),
            Outcome::Unstarted => tr!(schedule_outcome_unstarted),
            Outcome::PlanLimit => tr!(schedule_outcome_plan_limit),
        }
    }
}

/// One run, as its plan's record keeps it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub started: DateTime<Utc>,
    pub ended: DateTime<Utc>,
    /// Started by the owner (`r`, `glut schedule run`), not by launchd at its time: the times count without it.
    #[serde(default)]
    pub by_hand: bool,
    pub outcome: Outcome,
    /// What it cost; `None` where the model's prices are unknown.
    #[serde(default)]
    pub cost: Option<f64>,
    /// The answer's first line, or why it ended.
    #[serde(default)]
    pub said: String,
    /// The kept session, for `/resume`.
    #[serde(default)]
    pub session: Option<String>,
    /// What it asked and was refused: `tool: target`.
    #[serde(default)]
    pub refused: Vec<String>,
    /// Its model's prices were unknown: its budget could not count, only its time limit.
    #[serde(default)]
    pub unpriced: bool,
}

/// When launchd last started the plan at its time, among `runs`: a run by hand or one skipped moves no time.
pub fn last_timed<'a>(runs: impl IntoIterator<Item = &'a Record>) -> Option<DateTime<Utc>> {
    runs.into_iter()
        .filter(|run| !run.by_hand && run.outcome != Outcome::Skipped)
        .map(|run| run.started)
        .max()
}

/// A plan's record of runs.
fn record_file(home: &Path, name: &str) -> PathBuf {
    dir(home).join(format!("{name}.jsonl"))
}

/// The runs of plan `name`, oldest first; a line Glut cannot read is passed over.
pub fn runs(home: &Path, name: &str) -> Vec<Record> {
    let Ok(file) = std::fs::File::open(record_file(home, name)) else {
        return Vec::new();
    };
    std::io::BufReader::new(file.take(MAX_BYTES))
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str(&line).ok())
        .collect()
}

/// Every plan's runs that ended after `since`, by the plan's name, oldest first.
pub fn ended_since(home: &Path, since: DateTime<Utc>) -> Vec<(String, Record)> {
    let Ok(schedules) = Schedules::load(home) else {
        return Vec::new();
    };
    let mut ended: Vec<(String, Record)> = schedules
        .plans
        .iter()
        .flat_map(|plan| {
            runs(home, &plan.name)
                .into_iter()
                .filter(|run| run.ended > since)
                .map(|run| (plan.name.clone(), run))
        })
        .collect();
    ended.sort_by_key(|(_, run)| run.ended);
    ended
}

/// `record` after the runs of plan `name`, the oldest let go beyond [`KEPT_RUNS`].
///
/// # Errors
///
/// A message when the record cannot be written.
pub fn keep_run(home: &Path, name: &str, record: &Record) -> Result<(), String> {
    let mut kept = runs(home, name);
    kept.push(record.clone());
    let from = kept.len().saturating_sub(KEPT_RUNS);
    let mut text = String::new();
    for record in &kept[from..] {
        text.push_str(&serde_json::to_string(record).map_err(|err| err.to_string())?);
        text.push('\n');
    }
    let dir = dir(home);
    let path = record_file(home, name);
    glut::providers::make_private_dir(&dir)
        .and_then(|()| glut::providers::write_replacing(&dir, &path, text.as_bytes(), 0o600))
        .map_err(|err| format!("{}: {err}", path.display()))
}

/// A plan's record, log and lock gone with it.
pub fn forget_runs(home: &Path, name: &str) {
    let dir = dir(home);
    for end in ["jsonl", "log", "lock"] {
        let _ = std::fs::remove_file(dir.join(format!("{name}.{end}")));
    }
}

/// Held while a plan runs: the next run of it skips while the lock is taken.
pub struct Lock(#[allow(dead_code)] std::fs::File);

impl Lock {
    /// The plan's lock, `None` while another run holds it.
    ///
    /// # Errors
    ///
    /// A message when the lock file cannot be opened.
    pub fn take(home: &Path, name: &str) -> Result<Option<Lock>, String> {
        use std::os::unix::fs::OpenOptionsExt as _;
        let dir = dir(home);
        glut::providers::make_private_dir(&dir).map_err(|err| err.to_string())?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(dir.join(format!("{name}.lock")))
            .map_err(|err| err.to_string())?;
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Ok(Some(Lock(file))),
            Err(rustix::io::Errno::WOULDBLOCK) => Ok(None),
            Err(err) => Err(err.to_string()),
        }
    }
}

/// What starts and ends a plan's LaunchAgent: launchd, or a test's stand-in.
pub trait Agents {
    /// The agent at `file`, labelled `label`, taken by launchd: what was there under the label before is let go.
    ///
    /// # Errors
    ///
    /// Why launchd did not take it.
    fn start(&self, label: &str, file: &Path) -> Result<(), String>;

    /// The agent `label` let go; none there is no error.
    fn stop(&self, label: &str);
}

/// How often a refused `bootstrap` is tried again, and the pause between.
const BOOTSTRAP_TRIES: u32 = 5;
const BOOTSTRAP_PAUSE: std::time::Duration = std::time::Duration::from_millis(200);

/// launchd, in the owner's own login session.
pub struct Launchd;

impl Launchd {
    /// `gui/<uid>`: the domain of the owner's login, where the Keychain is open.
    fn domain() -> String {
        format!("gui/{}", rustix::process::getuid().as_raw())
    }

    fn launchctl(args: &[&str]) -> Result<(), String> {
        let out = std::process::Command::new("/bin/launchctl")
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|err| err.to_string())?;
        if out.status.success() {
            return Ok(());
        }
        let said = String::from_utf8_lossy(&out.stderr);
        Err(crate::transcript::clean(said.trim()))
    }
}

impl Agents for Launchd {
    fn start(&self, label: &str, file: &Path) -> Result<(), String> {
        self.stop(label);
        // `bootout` lets go of a job while it returns: a `bootstrap` right after it may find the label still taken.
        let mut tried = 0;
        loop {
            match Self::launchctl(&["bootstrap", &Self::domain(), &file.to_string_lossy()]) {
                Err(_) if tried < BOOTSTRAP_TRIES => {
                    tried += 1;
                    std::thread::sleep(BOOTSTRAP_PAUSE);
                }
                done => return done,
            }
        }
    }

    fn stop(&self, label: &str) {
        let _ = Self::launchctl(&["bootout", &format!("{}/{label}", Self::domain())]);
    }
}

/// The plan's LaunchAgent: `program schedule run --scheduled <name>` in the plan's folder at its times, its output
/// in `~/.glut/schedules/<name>.log`.
pub fn agent_text(home: &Path, plan: &Plan, program: &Path) -> String {
    let log = dir(home).join(format!("{}.log", plan.name));
    let string = |text: &str| format!("<string>{}</string>", xml(text));
    let integer = |value: u32| format!("<integer>{value}</integer>");
    let mut when = String::new();
    match &plan.when {
        When::At { hour, minute, days } => {
            when.push_str("  <key>StartCalendarInterval</key>\n  <array>\n");
            let at = |day: Option<u32>| {
                let mut entry = format!(
                    "    <dict><key>Hour</key>{}<key>Minute</key>{}",
                    integer(*hour),
                    integer(*minute)
                );
                if let Some(day) = day {
                    entry.push_str(&format!("<key>Weekday</key>{}", integer(day)));
                }
                entry.push_str("</dict>\n");
                entry
            };
            if days.is_empty() {
                when.push_str(&at(None));
            } else {
                for day in days {
                    when.push_str(&at(Some(*day)));
                }
            }
            when.push_str("  </array>\n");
        }
        When::Every { minutes } => {
            when.push_str(&format!(
                "  <key>StartInterval</key>{}\n",
                integer(minutes * 60)
            ));
        }
    }
    // The PATH it was made with, and Glut's own home where one is set: the run reads the plans it was made in.
    let mut vars = String::new();
    if !plan.path.is_empty() {
        vars.push_str(&format!("<key>PATH</key>{}", string(&plan.path)));
    }
    if let Some(glut_home) = std::env::var_os("GLUT_HOME").filter(|value| !value.is_empty()) {
        vars.push_str(&format!(
            "<key>GLUT_HOME</key>{}",
            string(&glut_home.to_string_lossy())
        ));
    }
    let env = if vars.is_empty() {
        String::new()
    } else {
        format!("  <key>EnvironmentVariables</key>\n  <dict>{vars}</dict>\n")
    };
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20 <key>Label</key>{label}\n\
         \x20 <key>ProgramArguments</key>\n  <array>{program}{schedule}{run}{scheduled}{name}</array>\n\
         \x20 <key>WorkingDirectory</key>{folder}\n\
         {env}{when}\
         \x20 <key>StandardOutPath</key>{log}\n\
         \x20 <key>StandardErrorPath</key>{log}\n\
         </dict>\n</plist>\n",
        label = string(&plan.label()),
        program = string(&program.to_string_lossy()),
        schedule = string("schedule"),
        run = string("run"),
        scheduled = string("--scheduled"),
        name = string(&plan.name),
        folder = string(&plan.folder.to_string_lossy()),
        log = string(&log.to_string_lossy()),
    )
}

/// `text` as an XML text: `&`, `<` and `>` escaped, and what XML cannot hold at all (control characters) left out.
fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// Writes the plan's LaunchAgent and hands it to `agents` — or lets it go while the plan is paused.
///
/// # Errors
///
/// A message when the file cannot be written or launchd does not take it.
pub fn install(
    home: &Path,
    plan: &Plan,
    program: &Path,
    agents: &dyn Agents,
) -> Result<(), String> {
    let file = agent_file(home, plan);
    if plan.paused {
        agents.stop(&plan.label());
        let _ = std::fs::remove_file(&file);
        return Ok(());
    }
    let folder = file.parent().unwrap_or(home);
    std::fs::create_dir_all(folder).map_err(|err| format!("{}: {err}", folder.display()))?;
    // launchd refuses an agent others may write.
    glut::providers::write_replacing(
        folder,
        &file,
        agent_text(home, plan, program).as_bytes(),
        0o644,
    )
    .map_err(|err| format!("{}: {err}", file.display()))?;
    agents
        .start(&plan.label(), &file)
        .map_err(|reason| tr!(schedule_launchd_failed, name = plan.name, reason = reason))
}

/// The plan's LaunchAgent let go and its file gone.
pub fn uninstall(home: &Path, plan: &Plan, agents: &dyn Agents) {
    agents.stop(&plan.label());
    let _ = std::fs::remove_file(agent_file(home, plan));
}

/// A macOS notification: `text` under `title`, handed to `osascript` as its arguments — never written into the script,
/// so nothing in them is read as AppleScript.
pub fn notify(title: &str, text: &str) {
    let clean = |text: &str| crate::transcript::clean(&crate::transcript::cut(text, 200));
    let _ = std::process::Command::new("/usr/bin/osascript")
        .args([
            "-e",
            "on run argv",
            "-e",
            "display notification (item 2 of argv) with title (item 1 of argv)",
            "-e",
            "end run",
            &clean(title),
            &clean(text),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

/// A plan for the tests of the tab and the screen: every hour in `/projects/app`, asking.
#[cfg(test)]
pub(crate) fn sample(name: &str) -> Plan {
    Plan {
        name: name.to_owned(),
        folder: PathBuf::from("/projects/app"),
        task: "run the tests".to_owned(),
        when: When::Every { minutes: 60 },
        provider: "p".to_owned(),
        model: "m".to_owned(),
        mode: Mode::Default,
        allow: Vec::new(),
        deny: Vec::new(),
        folders: Vec::new(),
        max_usd: DEFAULT_MAX_USD,
        max_minutes: DEFAULT_MAX_MINUTES,
        max_tokens: None,
        paused: false,
        path: String::new(),
        changed: Utc::now(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    fn plan(name: &str, when: When) -> Plan {
        Plan {
            name: name.to_owned(),
            folder: PathBuf::from("/projects/app"),
            task: "run the tests".to_owned(),
            when,
            provider: "openrouter".to_owned(),
            model: "m".to_owned(),
            mode: Mode::Default,
            allow: vec!["Bash(cargo test:*)".to_owned()],
            deny: Vec::new(),
            folders: Vec::new(),
            max_usd: DEFAULT_MAX_USD,
            max_minutes: DEFAULT_MAX_MINUTES,
            max_tokens: None,
            paused: false,
            path: "/usr/bin:/bin".to_owned(),
            changed: Utc.with_ymd_and_hms(2026, 10, 26, 8, 0, 0).unwrap(),
        }
    }

    fn home() -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "glut-schedule-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[test]
    fn times_days_and_intervals_are_read_as_written_and_nothing_else() {
        assert_eq!(parse_at("07:00"), Ok((7, 0)));
        assert_eq!(parse_at("7:05"), Ok((7, 5)));
        for bad in [
            "24:00", "7", "07:60", "7:5:0", "-1:00", "07:0x", ":30", "007:00",
        ] {
            assert!(parse_at(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_days("mo-fr"), Ok(vec![1, 2, 3, 4, 5]));
        assert_eq!(parse_days("sa,su"), Ok(vec![6, 7]));
        assert_eq!(
            parse_days("fr-mo"),
            Ok(vec![1, 5, 6, 7]),
            "a range goes on past Sunday"
        );
        assert_eq!(parse_days("Mo,we-fr,we"), Ok(vec![1, 3, 4, 5]));
        for bad in ["", "monday", "mo-", "mo,,fr", "xx"] {
            assert!(parse_days(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_every("30m"), Ok(30));
        assert_eq!(parse_every("1h30m"), Ok(90));
        assert_eq!(parse_every("1d"), Ok(1440));
        assert_eq!(parse_every("7d"), Ok(LONGEST_EVERY));
        for bad in ["9m", "8d", "2", "h", "1x", "", "99999999999h"] {
            assert!(parse_every(bad).is_err(), "{bad}");
        }
        assert_eq!(parse_mode("ask"), Ok(Mode::Default));
        assert_eq!(parse_mode("build"), Ok(Mode::AcceptEdits));
        assert_eq!(parse_mode("auto"), Ok(Mode::Auto));
        assert!(parse_mode("yolo").is_err());
    }

    #[test]
    fn a_plan_is_named_and_tasked_as_a_file_and_the_owner_allow() {
        assert!(check_name("morning-tests").is_ok());
        assert!(check_name("t1").is_ok());
        for bad in ["", "-x", "Morning", "a b", "../x", "a.b", &"x".repeat(41)] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_task("run the tests and write a report").is_ok());
        assert!(check_task("/skill review the open branch").is_ok());
        for bad in [
            "",
            "  ",
            "/skill",
            "/skill  ",
            "/quit",
            "/loop 5m x",
            "/schedule add x",
            "/model m",
        ] {
            assert!(check_task(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_plans_file_holds_only_plans_that_can_run() {
        let home = home();
        assert_eq!(
            Schedules::load(&home),
            Ok(Schedules::default()),
            "no file, no plans"
        );
        let mut schedules = Schedules::default();
        schedules.put(plan(
            "tests",
            When::At {
                hour: 7,
                minute: 0,
                days: vec![1, 2, 3, 4, 5],
            },
        ));
        schedules.put(plan("sync", When::Every { minutes: 120 }));
        schedules.save(&home).unwrap();
        let path = file(&home);
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let read = Schedules::load(&home).unwrap();
        assert_eq!(read, schedules);
        // Changed under its name, kept in its place.
        let mut changed = plan("tests", When::Every { minutes: 60 });
        changed.max_usd = 2.0;
        schedules.put(changed.clone());
        assert_eq!(schedules.plans.len(), 2);
        assert_eq!(schedules.get("tests"), Some(&changed));
        assert_eq!(
            schedules.take("sync").map(|plan| plan.name),
            Some("sync".to_owned())
        );
        // A plan written by hand that could not run makes the file one Glut does not read — nor write over.
        let mut broken = read.clone();
        broken.plans[0].max_usd = 0.0;
        std::fs::write(&path, serde_json::to_string(&broken).unwrap()).unwrap();
        assert!(Schedules::load(&home).is_err());
        broken.plans[0].max_usd = 1.0;
        broken.plans[0].task = "/quit".to_owned();
        std::fs::write(&path, serde_json::to_string(&broken).unwrap()).unwrap();
        assert!(Schedules::load(&home).is_err());
        std::fs::write(&path, "{\"plans\": [], \"more\": 1}").unwrap();
        assert!(
            Schedules::load(&home).is_err(),
            "a field Glut does not know"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn when_a_plan_runs_next_follows_its_days_and_its_last_start() {
        let at = plan(
            "tests",
            When::At {
                hour: 7,
                minute: 0,
                days: vec![1, 2, 3, 4, 5],
            },
        );
        // Saturday at noon, local time: Monday at seven.
        let saturday = Local
            .with_ymd_and_hms(2026, 10, 31, 12, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        let next = at.next(None, saturday).unwrap().with_timezone(&Local);
        assert_eq!(
            (next.weekday(), next.format("%H:%M").to_string()),
            (chrono::Weekday::Mon, "07:00".to_owned())
        );
        // Monday at six: that very morning.
        let monday = Local
            .with_ymd_and_hms(2026, 11, 2, 6, 0, 0)
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(
            at.next(None, monday).unwrap().with_timezone(&Local).day(),
            2
        );
        let every = plan("sync", When::Every { minutes: 120 });
        let start = every.changed + chrono::Duration::minutes(30);
        assert_eq!(
            every.next(Some(start), start + chrono::Duration::minutes(10)),
            Some(start + chrono::Duration::minutes(120))
        );
        let late = start + chrono::Duration::hours(10);
        assert_eq!(
            every.next(Some(start), late),
            Some(late),
            "missed asleep: made up on waking"
        );
        let mut paused = every;
        paused.paused = true;
        assert_eq!(paused.next(None, late), None);
        assert_eq!(
            at.when_shown(),
            tr!(schedule_when_at_days, time = "07:00", days = "mo–fr")
        );
        assert_eq!(days_shown(&[1, 3, 6, 7]), "mo,we,sa,su");
        assert_eq!(every_shown(1500), "1d1h");
    }

    #[test]
    fn a_plans_agent_runs_it_at_its_times_with_nothing_read_as_markup() {
        let home = PathBuf::from("/Users/o");
        let mut weekdays = plan(
            "tests",
            When::At {
                hour: 7,
                minute: 5,
                days: vec![1, 5],
            },
        );
        weekdays.folder = PathBuf::from("/projects/a&b <c>");
        let text = agent_text(&home, &weekdays, Path::new("/opt/glut"));
        assert!(text.contains("<key>Label</key><string>local.glut.schedule.tests</string>"));
        assert!(text.contains(
            "<array><string>/opt/glut</string><string>schedule</string><string>run</string>\
             <string>--scheduled</string><string>tests</string></array>"
        ));
        assert!(text.contains("<string>/projects/a&amp;b &lt;c&gt;</string>"));
        assert_eq!(text.matches("<key>Weekday</key>").count(), 2);
        assert!(
            text.contains(
                "<key>Hour</key><integer>7</integer><key>Minute</key><integer>5</integer>"
            )
        );
        assert!(text.contains("<string>/Users/o/.glut/schedules/tests.log</string>"));
        assert!(text.contains("<key>PATH</key><string>/usr/bin:/bin</string>"));
        let daily = agent_text(
            &home,
            &plan(
                "d",
                When::At {
                    hour: 7,
                    minute: 0,
                    days: Vec::new(),
                },
            ),
            Path::new("/g"),
        );
        assert!(!daily.contains("Weekday"), "every day: no day named");
        let every = agent_text(
            &home,
            &plan("e", When::Every { minutes: 90 }),
            Path::new("/g"),
        );
        assert!(every.contains("<key>StartInterval</key><integer>5400</integer>"));
        assert_eq!(xml("a\u{7}b\tc"), "ab\tc");
    }

    /// launchd as a test sees it: what was started and stopped.
    #[derive(Default)]
    struct Seen(RefCell<Vec<String>>);

    impl Agents for Seen {
        fn start(&self, label: &str, file: &Path) -> Result<(), String> {
            assert!(file.exists());
            self.0.borrow_mut().push(format!("start {label}"));
            Ok(())
        }

        fn stop(&self, label: &str) {
            self.0.borrow_mut().push(format!("stop {label}"));
        }
    }

    #[test]
    fn a_plan_installed_runs_paused_rests_and_removed_leaves_nothing() {
        let home = home();
        let seen = Seen::default();
        let mut tests = plan("tests", When::Every { minutes: 60 });
        install(&home, &tests, Path::new("/g"), &seen).unwrap();
        let file = agent_file(&home, &tests);
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o644
        );
        tests.paused = true;
        install(&home, &tests, Path::new("/g"), &seen).unwrap();
        assert!(!file.exists(), "a paused plan has no agent");
        tests.paused = false;
        install(&home, &tests, Path::new("/g"), &seen).unwrap();
        uninstall(&home, &tests, &seen);
        assert!(!file.exists());
        assert_eq!(
            *seen.0.borrow(),
            [
                "start local.glut.schedule.tests",
                "stop local.glut.schedule.tests",
                "start local.glut.schedule.tests",
                "stop local.glut.schedule.tests"
            ]
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_plans_runs_are_kept_the_newest_fifty_and_a_second_run_finds_the_lock_taken() {
        let home = home();
        let record = |n: i64| Record {
            started: Utc.timestamp_opt(n, 0).unwrap(),
            ended: Utc.timestamp_opt(n + 1, 0).unwrap(),
            by_hand: false,
            outcome: Outcome::Answered,
            cost: Some(0.01),
            said: format!("run {n}"),
            session: None,
            refused: Vec::new(),
            unpriced: false,
        };
        for n in 0..55 {
            keep_run(&home, "tests", &record(n)).unwrap();
        }
        let kept = runs(&home, "tests");
        assert_eq!(kept.len(), KEPT_RUNS);
        assert_eq!(kept.first().map(|run| run.said.as_str()), Some("run 5"));
        assert_eq!(kept.last(), Some(&record(54)));
        let first = Lock::take(&home, "tests").unwrap();
        assert!(first.is_some());
        assert!(
            Lock::take(&home, "tests").unwrap().is_none(),
            "the run before holds it"
        );
        drop(first);
        assert!(Lock::take(&home, "tests").unwrap().is_some());
        forget_runs(&home, "tests");
        assert!(runs(&home, "tests").is_empty());
        let _ = std::fs::remove_dir_all(home);
    }
}
