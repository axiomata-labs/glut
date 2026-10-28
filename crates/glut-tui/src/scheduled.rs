//! `glut schedule …` (`docs/plans/glut-tui.md`, T60b): plans made, listed, held and removed, and a plan's run — the
//! one launchd starts at its times (`--scheduled`) or the owner's own. A run is a session of the owner's like any
//! (`/resume`, `/search`), built with nobody there: every ask refused, the plan's mode and rules alone beside the
//! owner's `~/.glut/`, its budget and time limit always, and the end said by a macOS notification.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use glut::permissions::{Answer, AnswerFuture, AskRequest, GateOptions};
use glut::{Event, Mode};
use tokio::sync::mpsc;

use crate::host::{EngineChoice, Host, Launch, Servers};
use crate::schedule::{self, Agents, Outcome, Plan, Record, Schedules, When};
use crate::session::{self, Report};

/// How long past its time limit a run gets to end on its own (the model's last reply, its tools called off) before
/// the process gives up on it.
const GRACE: Duration = Duration::from_secs(120);
/// How long a run called off gets to end and be kept: launchd kills a job 20 s after its SIGTERM.
const STOP_WAIT: Duration = Duration::from_secs(15);
/// How many refused asks a run's record names.
const MOST_REFUSED: usize = 20;
/// How many characters of a run's answer its record keeps: the first line, as a tile shows it.
const SAID_CHARS: usize = 200;

/// `glut schedule`'s commands.
#[derive(clap::Subcommand)]
pub enum Command {
    /// Make a plan, or change the one of that name and keep what is not given again.
    Add(Box<AddArgs>),
    /// Every plan: when it runs next and how it ran last.
    List,
    /// A plan's run now, as it runs at its times.
    Run {
        /// The plan's name.
        name: String,
        /// Started by launchd: the end is told by a notification.
        #[arg(long)]
        scheduled: bool,
        /// Started by the owner's key, not at its time: a paused plan runs too, and its times stay as they were.
        #[arg(long, requires = "scheduled")]
        by_hand: bool,
    },
    /// Hold a plan: it does not run until resume.
    Pause {
        /// The plan's name.
        name: String,
    },
    /// A held plan runs at its times again.
    Resume {
        /// The plan's name.
        name: String,
    },
    /// A plan and its record of runs gone; its sessions stay.
    Remove {
        /// The plan's name.
        name: String,
    },
}

/// `glut schedule add`'s flags: each one left out keeps what the plan of that name had.
#[derive(clap::Args)]
pub struct AddArgs {
    /// The plan's name: small letters, digits and -, such as morning-tests.
    name: String,
    /// The time it runs at, such as 07:00.
    #[arg(long, conflicts_with = "every")]
    at: Option<String>,
    /// The days it runs on with --at: mo-fr, sa,su; every day without.
    #[arg(long)]
    days: Option<String>,
    /// How often it runs: 30m, 2h, 1d.
    #[arg(long)]
    every: Option<String>,
    /// The folder it works in; where the plan is made otherwise.
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// A provider of ~/.glut/providers.json; the default one otherwise, fixed from now on.
    #[arg(long)]
    provider: Option<String>,
    /// The model; the provider's default otherwise, fixed from now on.
    #[arg(long)]
    model: Option<String>,
    /// What it may do without a rule: ask (the default), build, plan, dont_ask or auto. Nobody is there to answer:
    /// every ask is refused.
    #[arg(long)]
    mode: Option<String>,
    /// A rule it may do, in Claude Code's format (`Bash(cargo test:*)`); repeatable.
    #[arg(long)]
    allow: Vec<String>,
    /// A rule it is refused; repeatable.
    #[arg(long)]
    deny: Vec<String>,
    /// A folder it works in beside its own (T60c); repeatable.
    #[arg(long)]
    folder: Vec<String>,
    /// The most a run may cost in USD; 1 otherwise.
    #[arg(long)]
    max_usd: Option<f64>,
    /// The most minutes a run may take; 30 otherwise.
    #[arg(long)]
    max_minutes: Option<u32>,
    /// The most tokens a run on a subscription may use (500k, 1M); 1M otherwise.
    #[arg(long, value_parser = tokens_arg)]
    max_tokens: Option<u64>,
    /// The rules and folders given take the place of the plan's, instead of joining them.
    #[arg(long)]
    replace: bool,
    /// The task: a message, or /skill <name> [what for].
    #[arg(trailing_var_arg = true)]
    task: Vec<String>,
}

/// Runs `glut schedule`'s `command`.
pub async fn headless(command: Command) -> ExitCode {
    let home = match glut::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return failed(&err.to_string()),
    };
    let done = match command {
        Command::Add(args) => match std::env::current_dir() {
            Ok(here) => add(&home, *args, &here, &schedule::Launchd),
            Err(err) => Err(err.to_string()),
        },
        Command::List => list(&home),
        Command::Run {
            name,
            scheduled,
            by_hand,
        } => return run(&home, &name, scheduled, by_hand).await,
        Command::Pause { name } => hold(&home, &name, true, &schedule::Launchd),
        Command::Resume { name } => hold(&home, &name, false, &schedule::Launchd),
        Command::Remove { name } => remove(&home, &name, &schedule::Launchd),
    };
    match done {
        Ok(said) => {
            println!("{said}");
            ExitCode::SUCCESS
        }
        Err(reason) => failed(&reason),
    }
}

/// `reason` on stderr, and the exit code of a failure.
fn failed(reason: &str) -> ExitCode {
    eprintln!("glut: {reason}");
    ExitCode::from(1)
}

/// The program a plan's LaunchAgent starts: this one, by its full path.
fn program() -> Result<PathBuf, String> {
    std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|err| err.to_string())
}

/// Where a plan's commands may write: its folder, the folders beside it and the sandbox's writable places — what lies
/// there may be the model's by the next run, which launchd starts outside the sandbox.
fn writable_places(
    plan: &Plan,
    home: &Path,
    allow_write: &[PathBuf],
    deny_read: &[PathBuf],
) -> Vec<PathBuf> {
    let mut places = glut::sandbox::Policy::new(&plan.folder, home, allow_write, &[]).writable;
    places.push(plan.folder.clone());
    places.extend(
        plan.folders
            .iter()
            .filter_map(|typed| glut::folders::check(typed, &plan.folder, home, deny_read).ok())
            .map(|folder| folder.real().to_path_buf()),
    );
    places
}

/// Whether `path` lies in one of `places`, by its real path — a folder not made yet by its nearest one that is
/// (`/var/…` is `/private/var/…`), as a run could make it.
fn reached(path: &Path, places: &[PathBuf]) -> bool {
    let real = path
        .ancestors()
        .find_map(|above| {
            let rest = path.strip_prefix(above).ok()?;
            above.canonicalize().ok().map(|real| real.join(rest))
        })
        .unwrap_or_else(|| path.to_path_buf());
    places.iter().any(|place| real.starts_with(place))
}

/// The program a plan's agent starts must lie outside where its commands write: else a run could leave the next one
/// a program of its own, started by launchd outside the sandbox.
fn program_outside(program: &Path, places: &[PathBuf]) -> Result<(), String> {
    if reached(program, places) {
        return Err(tr!(schedule_program_writable, program = program.display()));
    }
    Ok(())
}

/// `path`'s folders a plan's run may find programs in: the absolute ones outside where its commands write.
fn path_outside(path: &str, places: &[PathBuf]) -> String {
    let kept: Vec<PathBuf> = std::env::split_paths(path)
        .filter(|dir| dir.is_absolute() && !reached(dir, places))
        .collect();
    std::env::join_paths(kept)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `glut schedule add`: the plan made or changed, installed, and what it may do said.
fn add(home: &Path, args: AddArgs, here: &Path, agents: &dyn Agents) -> Result<String, String> {
    schedule::check_name(&args.name)?;
    let mut schedules = Schedules::load(home)?;
    let kept = schedules.get(&args.name).cloned();
    let mut plan = plan_of(home, args, here, kept.as_ref())?;
    let places = writable_places(&plan, home, &[], &[]);
    let program = program()?;
    program_outside(&program, &places)?;
    plan.path = path_outside(&std::env::var("PATH").unwrap_or_default(), &places);
    // launchd opens the log there before the run makes the folder.
    glut::providers::make_private_dir(&schedule::dir(home)).map_err(|err| err.to_string())?;
    if let Err(reason) = schedule::install(home, &plan, &program, agents) {
        // The agent of the plan as it was goes back; a new plan leaves none behind.
        match &kept {
            Some(kept) => {
                let _ = schedule::install(home, kept, &program, agents);
            }
            None => schedule::uninstall(home, &plan, agents),
        }
        return Err(reason);
    }
    let said = shown(home, &plan, kept.is_some());
    schedules.put(plan);
    schedules.save(home)?;
    Ok(said)
}

/// `/schedule add …` (T60b): `glut schedule add`'s words, read as the command line reads them, in the session's
/// folder `here`.
///
/// # Errors
///
/// What is wrong with the words or the plan, for the owner to read.
pub(crate) fn add_line(
    home: &Path,
    line: &str,
    here: &Path,
    agents: &dyn Agents,
) -> Result<String, String> {
    /// `glut schedule add`'s flags without the program's name.
    #[derive(clap::Parser)]
    #[command(no_binary_name = true, disable_help_flag = true)]
    struct Words {
        #[command(flatten)]
        args: AddArgs,
    }
    use clap::Parser as _;
    let words = Words::try_parse_from(split(line)?).map_err(|err| {
        let said = err.to_string();
        said.lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim_start_matches("error: ")
            .to_owned()
    })?;
    add(home, words.args, here, agents)
}

/// A line cut into words at spaces, a word in "…" or '…' kept whole as the shell keeps it.
fn split(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut begun = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(open), c) if c == open => quote = None,
            (Some(_), c) => word.push(c),
            (None, '"' | '\'') if !begun => {
                quote = Some(c);
                begun = true;
            }
            (None, c) if c.is_whitespace() => {
                if begun {
                    words.push(std::mem::take(&mut word));
                    begun = false;
                }
            }
            (None, c) => {
                word.push(c);
                begun = true;
            }
        }
    }
    if quote.is_some() {
        return Err(tr!(schedule_quote_open).to_owned());
    }
    if begun {
        words.push(word);
    }
    Ok(words)
}

/// `r` on the Runs tab (T60b): the plan's run started beside Glut, as launchd starts it — its end a notification and a
/// tile, Glut free meanwhile and the run going on when Glut ends.
///
/// # Errors
///
/// Why it did not start.
pub(crate) fn run_beside(name: &str) -> Result<(), String> {
    use std::os::unix::process::CommandExt as _;
    let mut child = std::process::Command::new(program()?)
        .args(["schedule", "run", "--scheduled", "--by-hand", name])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        // A group of its own: the terminal's ⌃C and its closing are Glut's, never the run's.
        .process_group(0)
        .spawn()
        .map_err(|err| err.to_string())?;
    // Waited for beside the screen, so its end leaves no zombie behind while Glut is open.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The plan `args` make of `kept` (none for a new one), a new one in the folder `here` unless `--cwd` names one.
fn plan_of(home: &Path, args: AddArgs, here: &Path, kept: Option<&Plan>) -> Result<Plan, String> {
    let when = match (&args.at, &args.every, kept.map(|plan| &plan.when)) {
        (Some(at), _, kept) => {
            let (hour, minute) = schedule::parse_at(at)?;
            let days = match (&args.days, kept) {
                (Some(days), _) => schedule::parse_days(days)?,
                (None, Some(When::At { days, .. })) => days.clone(),
                (None, _) => Vec::new(),
            };
            When::At { hour, minute, days }
        }
        (None, Some(_), _) if args.days.is_some() => {
            return Err(tr!(schedule_days_without_at).to_owned());
        }
        (None, Some(every), _) => When::Every {
            minutes: schedule::parse_every(every)?,
        },
        (None, None, Some(When::At { hour, minute, days })) => When::At {
            hour: *hour,
            minute: *minute,
            days: match &args.days {
                Some(days) => schedule::parse_days(days)?,
                None => days.clone(),
            },
        },
        (None, None, Some(When::Every { .. })) if args.days.is_some() => {
            return Err(tr!(schedule_days_without_at).to_owned());
        }
        (None, None, Some(kept)) => kept.clone(),
        (None, None, None) => return Err(tr!(schedule_when_missing).to_owned()),
    };
    let folder = match (&args.cwd, kept) {
        (Some(cwd), _) => here.join(cwd),
        (None, Some(plan)) => plan.folder.clone(),
        (None, None) => here.to_path_buf(),
    };
    if !folder.is_dir() {
        return Err(tr!(schedule_folder_gone, folder = folder.display()));
    }
    let folder = glut::folders::check_working(&folder, home, &[])?;
    let (provider, model) = engine_of(home, &args, kept)?;
    let task = if args.task.is_empty() {
        kept.map(|plan| plan.task.clone())
            .ok_or_else(|| tr!(schedule_task_missing).to_owned())?
    } else {
        args.task.join(" ")
    };
    let mode = match (&args.mode, kept) {
        (Some(mode), _) => schedule::parse_mode(mode)?,
        (None, Some(plan)) => plan.mode,
        (None, None) => Mode::Default,
    };
    // Given again, a list joins the plan's (the hint after a refused ask names one rule), unless --replace.
    let replace = args.replace;
    let given_or_kept = |given: Vec<String>, kept: Option<&Vec<String>>| {
        let mut list = if replace {
            Vec::new()
        } else {
            kept.cloned().unwrap_or_default()
        };
        for item in given {
            if !list.contains(&item) {
                list.push(item);
            }
        }
        list
    };
    let max_usd = args
        .max_usd
        .or(kept.map(|plan| plan.max_usd))
        .unwrap_or(schedule::DEFAULT_MAX_USD);
    let max_minutes = args
        .max_minutes
        .or(kept.map(|plan| plan.max_minutes))
        .unwrap_or(schedule::DEFAULT_MAX_MINUTES);
    let max_tokens = args.max_tokens.or(kept.and_then(|plan| plan.max_tokens));
    let plan = Plan {
        name: args.name,
        task,
        when,
        provider,
        model,
        mode,
        allow: given_or_kept(args.allow, kept.map(|plan| &plan.allow)),
        deny: given_or_kept(args.deny, kept.map(|plan| &plan.deny)),
        folders: given_or_kept(args.folder, kept.map(|plan| &plan.folders)),
        max_usd,
        max_minutes,
        max_tokens,
        paused: kept.is_some_and(|plan| plan.paused),
        path: std::env::var("PATH").unwrap_or_default(),
        changed: Utc::now(),
        folder,
    };
    plan.check()?;
    // Checked again at every run, against the sandbox's own list: here so a plan never made is said at once.
    for typed in &plan.folders {
        glut::folders::check(typed, &plan.folder, home, &[])
            .map_err(|reason| format!("--folder {reason}"))?;
    }
    Ok(plan)
}

/// `--max-tokens` as written: `500000`, `500k`, `1M`.
fn tokens_arg(word: &str) -> Result<u64, String> {
    crate::looping::count(word).ok_or_else(|| tr!(schedule_tokens_bad).to_owned())
}

/// The subscription a plan's provider is, if it is one (PV4): its runs count tokens.
fn subscription(home: &Path, provider: &str) -> Option<&'static glut::providers::catalog::Known> {
    let list = glut::providers::Providers::load(home).ok()?;
    let entry = list.providers.get(provider)?;
    glut::providers::catalog::known(&entry.kind)
        .filter(|known| known.plan != glut::providers::catalog::Plan::None)
}

/// The provider and model a plan runs on: the ones given, else the plan's, else the default provider and its model
/// — named now, so a later change of the default never moves a plan.
fn engine_of(home: &Path, args: &AddArgs, kept: Option<&Plan>) -> Result<(String, String), String> {
    if args.provider.is_none()
        && let Some(plan) = kept
    {
        return Ok((
            plan.provider.clone(),
            args.model.clone().unwrap_or_else(|| plan.model.clone()),
        ));
    }
    let list = glut::providers::Providers::load(home).map_err(|err| err.to_string())?;
    let (id, entry) = list.pick(args.provider.as_deref())?;
    let model = args
        .model
        .clone()
        .or_else(|| entry.model.clone())
        .ok_or_else(|| format!("no model for {id}: name one with --model"))?;
    Ok((id.to_owned(), model))
}

/// What a plan does and may do, as `add` says it.
fn shown(home: &Path, plan: &Plan, changed: bool) -> String {
    let folder = crate::projects::shown_path(&plan.folder, home);
    let mut lines = vec![
        if changed {
            tr!(schedule_changed, name = plan.name)
        } else {
            tr!(schedule_made, name = plan.name)
        },
        tr!(
            schedule_shown_when,
            when = plan.when_shown(),
            folder = folder
        ),
        tr!(
            schedule_shown_task,
            task = crate::transcript::clean(&plan.task)
        ),
        tr!(
            schedule_shown_model,
            provider = plan.provider,
            model = plan.model
        ),
        tr!(
            schedule_shown_mode,
            mode = crate::session::mode_label(plan.mode),
            means = means(plan.mode)
        ),
    ];
    if !plan.allow.is_empty() {
        lines.push(tr!(schedule_shown_allow, rules = plan.allow.join(", ")));
    }
    if !plan.deny.is_empty() {
        lines.push(tr!(schedule_shown_deny, rules = plan.deny.join(", ")));
    }
    if !plan.folders.is_empty() {
        lines.push(tr!(
            schedule_shown_folders,
            folders = plan.folders.join(", ")
        ));
    }
    lines.push(tr!(
        schedule_shown_limits,
        usd = glut::lang::usd(plan.max_usd, 2),
        minutes = plan.max_minutes
    ));
    if let Some(known) = subscription(home, &plan.provider) {
        lines.push(tr!(
            schedule_shown_tokens,
            name = known.name,
            n = crate::looping::tokens_shown(
                plan.max_tokens.unwrap_or(schedule::DEFAULT_MAX_TOKENS)
            )
        ));
        // Its terms allow interactive use only: the owner's decision, said where the plan is made (PV4 Q8).
        if known.plan == glut::providers::catalog::Plan::InteractiveOnly {
            lines.push(tr!(plan_interactive_only, name = known.name));
        }
    }
    lines.push(tr!(schedule_shown_owner_rules).to_owned());
    lines.push(tr!(schedule_shown_logged_in).to_owned());
    if plan.paused {
        lines.push(tr!(schedule_shown_paused, name = plan.name));
    }
    lines.join("\n  ")
}

/// What a mode lets a plan's run do, with nobody there to ask.
pub(crate) fn means(mode: Mode) -> &'static str {
    match mode {
        Mode::Default => tr!(schedule_means_ask),
        Mode::AcceptEdits => tr!(schedule_means_build),
        Mode::Plan => tr!(schedule_means_plan),
        Mode::DontAsk => tr!(schedule_means_dont_ask),
        Mode::Auto => tr!(schedule_means_auto),
    }
}

/// `glut schedule list`: a line for every plan.
pub(crate) fn list(home: &Path) -> Result<String, String> {
    let schedules = Schedules::load(home)?;
    if schedules.plans.is_empty() {
        return Ok(tr!(schedule_none).to_owned());
    }
    let now = Utc::now();
    let lines: Vec<String> = schedules
        .plans
        .iter()
        .map(|plan| {
            let runs = schedule::runs(home, &plan.name);
            let last = runs.last();
            let next = match plan.next(schedule::last_timed(&runs), now) {
                Some(next) => next
                    .with_timezone(&chrono::Local)
                    .format(glut::lang::date_time())
                    .to_string(),
                None => tr!(schedule_paused_mark).to_owned(),
            };
            let last = last.map_or_else(
                || tr!(schedule_never).to_owned(),
                |run| {
                    tr!(
                        schedule_last,
                        outcome = run.outcome.said(),
                        ago = crate::projects::ago(run.ended, now),
                        cost = cost_shown(run.cost)
                    )
                },
            );
            tr!(
                schedule_line,
                name = plan.name,
                when = plan.when_shown(),
                folder = crate::projects::shown_path(&plan.folder, home),
                next = next,
                last = last
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

/// What a run cost, `—` where the prices are unknown.
pub fn cost_shown(cost: Option<f64>) -> String {
    cost.map_or_else(|| "—".to_owned(), |cost| glut::lang::usd(cost, 2))
}

/// `glut schedule pause|resume`: the plan held or running again, its agent let go or given back.
pub(crate) fn hold(
    home: &Path,
    name: &str,
    paused: bool,
    agents: &dyn Agents,
) -> Result<String, String> {
    let mut schedules = Schedules::load(home)?;
    let mut plan = schedules
        .get(name)
        .cloned()
        .ok_or_else(|| tr!(schedule_unknown, name = crate::transcript::clean(name)))?;
    plan.paused = paused;
    if !paused {
        // An `--every` plan counts again from now, not from before it was held.
        plan.changed = Utc::now();
    }
    schedule::install(home, &plan, &program()?, agents)?;
    schedules.put(plan);
    schedules.save(home)?;
    Ok(if paused {
        tr!(schedule_paused, name = name)
    } else {
        tr!(schedule_resumed, name = name)
    })
}

/// `glut schedule remove`: the plan, its agent and its record gone; its sessions stay.
pub(crate) fn remove(home: &Path, name: &str, agents: &dyn Agents) -> Result<String, String> {
    let mut schedules = Schedules::load(home)?;
    let plan = schedules
        .take(name)
        .ok_or_else(|| tr!(schedule_unknown, name = crate::transcript::clean(name)))?;
    schedule::uninstall(home, &plan, agents);
    schedules.save(home)?;
    schedule::forget_runs(home, name);
    Ok(tr!(schedule_removed, name = name))
}

/// `glut schedule run`: the plan's run, kept in its record and told — by a notification when launchd started it,
/// on stdout when the owner did.
async fn run(home: &Path, name: &str, scheduled: bool, by_hand: bool) -> ExitCode {
    let started = Utc::now();
    let plan = match Schedules::load(home).and_then(|schedules| {
        schedules
            .get(name)
            .cloned()
            .ok_or_else(|| tr!(schedule_unknown, name = crate::transcript::clean(name)))
    }) {
        Ok(plan) => plan,
        Err(reason) => {
            if scheduled {
                schedule::notify(&format!("Glut · {name}"), &reason);
            }
            return failed(&reason);
        }
    };
    // A job launchd still had while the plan was paused does nothing; the owner's own run goes.
    if plan.paused && scheduled && !by_hand {
        return ExitCode::SUCCESS;
    }
    let attempt = match schedule::Lock::take(home, name) {
        Ok(Some(_lock)) => watched(home, &plan).await,
        Ok(None) => Attempt::ended(Outcome::Skipped, tr!(schedule_running_already).to_owned()),
        Err(reason) => Attempt::ended(Outcome::Unstarted, reason),
    };
    let record = Record {
        started,
        ended: Utc::now(),
        by_hand: by_hand || !scheduled,
        outcome: attempt.outcome,
        cost: attempt.cost,
        said: attempt.said,
        session: attempt.session,
        refused: attempt.refused,
        unpriced: attempt.unpriced,
    };
    let kept = schedule::keep_run(home, name, &record);
    let told = told(&plan, &record);
    if scheduled {
        schedule::notify(&format!("Glut · {name}"), &told);
    } else {
        println!("{told}");
    }
    if let Err(reason) = kept {
        eprintln!("glut: {reason}");
    }
    if record.outcome.well() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// The plan's run, called off when it outlasts its time limit or launchd ends it (`bootout`, SIGTERM): what it did
/// until then is kept and told, as for a run that ended by itself.
async fn watched(home: &Path, plan: &Plan) -> Attempt {
    let cancel = glut::Cancel::new();
    let work = attempt(home, plan, cancel.clone());
    tokio::pin!(work);
    let limit = Duration::from_secs(u64::from(plan.max_minutes) * 60) + GRACE;
    let ended = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
        Ok(mut terminate) => tokio::select! {
            done = &mut work => return done,
            () = tokio::time::sleep(limit) => Outcome::Timeout,
            _ = terminate.recv() => Outcome::Stopped,
        },
        Err(_) => match tokio::time::timeout(limit, &mut work).await {
            Ok(done) => return done,
            Err(_) => Outcome::Timeout,
        },
    };
    cancel.cancel();
    match tokio::time::timeout(STOP_WAIT, &mut work).await {
        Ok(mut done) => {
            done.outcome = ended;
            done
        }
        Err(_) => Attempt::ended(ended, ended.said().to_owned()),
    }
}

/// A run's end as the owner is told it: how it ended, what it said, what it cost — and what it was refused.
fn told(plan: &Plan, record: &Record) -> String {
    let mut told = tr!(
        schedule_run_line,
        name = plan.name,
        outcome = record.outcome.said(),
        said = record.said,
        cost = cost_shown(record.cost)
    );
    if record.unpriced {
        told.push('\n');
        told.push_str(tr!(schedule_no_prices));
    }
    if let Some(first) = record.refused.first() {
        told.push('\n');
        told.push_str(&tr!(
            schedule_asks_refused,
            n = record.refused.len(),
            first = first,
            name = plan.name
        ));
    }
    told
}

/// How a run went, before it is kept.
struct Attempt {
    outcome: Outcome,
    cost: Option<f64>,
    said: String,
    session: Option<String>,
    refused: Vec<String>,
    unpriced: bool,
}

impl Attempt {
    fn ended(outcome: Outcome, said: String) -> Attempt {
        Attempt {
            outcome,
            cost: None,
            said: first_line(&said),
            session: None,
            refused: Vec::new(),
            unpriced: false,
        }
    }
}

/// The first line of `text` with something in it, cut to [`SAID_CHARS`] and cleaned for a terminal and a notification.
fn first_line(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    crate::transcript::clean(&crate::transcript::cut(line, SAID_CHARS))
}

/// The host of a plan's run: nobody there — every ask refused at once and noted for the record.
#[derive(Default)]
struct Unattended {
    refused: Mutex<Vec<String>>,
}

impl Host for Unattended {
    fn ask<'a>(&'a self, request: &'a AskRequest, on_screen: AnswerFuture<'a>) -> AnswerFuture<'a> {
        drop(on_screen);
        if let Ok(mut refused) = self.refused.lock()
            && refused.len() < MOST_REFUSED
        {
            let target = crate::transcript::clean(&crate::transcript::cut(&request.target, 120));
            refused.push(format!(
                "{}: {target}",
                crate::transcript::clean(&request.tool)
            ));
        }
        Box::pin(async { Answer::Deny })
    }
}

/// The session a plan's run is: the owner's own in the plan's folder, with the plan's rights alone — a project's
/// rules, skills, agents and servers never count, as if the folder were not trusted.
fn launch_of(plan: &Plan, folder: PathBuf) -> Launch {
    let mut launch = Launch::owner(
        folder,
        Some(plan.provider.clone()),
        Some(plan.model.clone()),
    );
    launch.gate = GateOptions {
        mode: Some(plan.mode),
        allow: plan.allow.clone(),
        ask: Vec::new(),
        deny: plan.deny.clone(),
        trusted: false,
    };
    launch.owner_trust = false;
    launch.servers = Servers::OwnerAlone;
    launch.mode_locked = true;
    launch.owner_shell = false;
    launch.projects = false;
    launch.engine = EngineChoice::Providers {
        provider: Some(plan.provider.clone()),
        model: Some(plan.model.clone()),
    };
    launch
}

/// The plan's one turn, and how it went.
async fn attempt(home: &Path, plan: &Plan, cancel: glut::Cancel) -> Attempt {
    let Some(folder) = plan
        .folder
        .canonicalize()
        .ok()
        .filter(|folder| folder.is_dir())
    else {
        return Attempt::ended(
            Outcome::Unstarted,
            tr!(schedule_folder_gone, folder = plan.folder.display()),
        );
    };
    let host = Arc::new(Unattended::default());
    let (reports, mut inbox) = mpsc::unbounded_channel();
    let built = session::build(
        launch_of(plan, folder.clone()),
        Arc::clone(&host) as Arc<dyn Host>,
        reports.clone(),
    )
    .await;
    let (mut parts, engine) = match built {
        Ok(built) => built,
        Err(reason) => return Attempt::ended(Outcome::Unstarted, reason),
    };
    let (deny_read, allow_write) = parts
        .config
        .sandbox
        .as_ref()
        .map(|sandbox| (sandbox.deny_read.clone(), sandbox.allow_write.clone()))
        .unwrap_or_default();
    // Asked again with the owner's settings as they are now: what was refused at making is refused at every run.
    if let Err(reason) = glut::folders::check_working(&folder, home, &deny_read) {
        return Attempt::ended(Outcome::Unstarted, reason);
    }
    let places = writable_places(plan, home, &allow_write, &deny_read);
    let path = std::env::var("PATH").unwrap_or_default();
    let unsafe_start = program()
        .and_then(|program| program_outside(&program, &places))
        .and_then(|()| {
            (path_outside(&path, &places) == path)
                .then_some(())
                .ok_or_else(|| tr!(schedule_path_writable).to_owned())
        });
    if let Err(reason) = unsafe_start {
        return Attempt::ended(Outcome::Unstarted, reason);
    }
    // `auto` with nobody there: only behind the sandbox.
    if plan.mode == Mode::Auto && !session::sandboxed(&parts) {
        return Attempt::ended(
            Outcome::Unstarted,
            tr!(schedule_auto_unsandboxed).to_owned(),
        );
    }
    for typed in &plan.folders {
        let added = glut::folders::check(typed, &folder, home, &deny_read).and_then(|checked| {
            parts
                .tools
                .folders()
                .add(checked)
                .map_err(|err| format!("{err:?}"))
        });
        if let Err(reason) = added {
            return Attempt::ended(Outcome::Unstarted, format!("--folder {reason}"));
        }
    }
    parts.config.timeout = Duration::from_secs(u64::from(plan.max_minutes) * 60);
    // A plan's run ends as its budget and its task say: nobody to ask it on (T73).
    parts.config.nudge = false;
    parts.config.budget = glut::Budget {
        max_usd: Some(plan.max_usd),
        // A subscription's use has no price, nor has a model without known prices: its tokens end the run then.
        max_tokens: (engine.plan.is_some() || engine.prices.is_none())
            .then(|| plan.max_tokens.unwrap_or(schedule::DEFAULT_MAX_TOKENS)),
        prices: engine.prices,
        ..glut::Budget::default()
    };
    let prompt = match plan.task.trim().strip_prefix("/skill ") {
        Some(rest) => {
            let (name, text) = rest
                .trim()
                .split_once(char::is_whitespace)
                .unwrap_or((rest.trim(), ""));
            crate::app::skill_prompt(name, text)
        }
        None => plan.task.trim().to_owned(),
    };
    let mut kept = crate::store::Session::new(&folder, &engine.provider, engine.model_name());
    kept.title = crate::store::title_of(&format!("{}: {}", plan.name, plan.task));
    kept.marks.push(crate::store::Mark {
        at: Some(0),
        prompt: crate::store::title_of(&plan.task),
        run: None,
    });
    let prices = engine.prices;
    let parts = Arc::new(parts);
    session::turn(
        Arc::clone(&parts),
        engine,
        session::Before {
            session: kept.id.clone(),
            conversation: Vec::new(),
            todos: Vec::new(),
            nudge: false,
            // Nobody watches a plan's run: the list keeps it on course all the more (T80).
            remind: true,
            steps: parts.config.max_turns,
            rules: Vec::new(),
        },
        session::TurnInput::Message(prompt, Vec::new()),
        cancel,
        glut::Inbox::default(),
        reports,
    );
    let mut costs = Costs {
        prices,
        ..Costs::default()
    };
    let mut error = None;
    let (outcome, conversation) = loop {
        let Some(report) = inbox.recv().await else {
            return Attempt::ended(
                Outcome::Failed,
                error.unwrap_or_else(|| Outcome::Failed.said().to_owned()),
            );
        };
        match report {
            Report::Event(event) => {
                costs.count(&event);
                match &event {
                    Event::Error { message } => error = Some(message.clone()),
                    Event::Snapshot { run, .. } => {
                        if let Some(mark) = kept.marks.last_mut()
                            && mark.run.is_none()
                        {
                            mark.run = Some(run.clone());
                        }
                    }
                    Event::Compacted { .. } => {
                        for mark in &mut kept.marks {
                            mark.at = None;
                        }
                    }
                    _ => {}
                }
            }
            Report::Ask(_, reply) => {
                let _ = reply.send(Answer::Deny);
            }
            Report::PickModel(reply) => {
                let _ = reply.send(None);
            }
            Report::Tell { reply, .. } => {
                let _ = reply.send(Err(tr!(schedule_no_panes).to_owned()));
            }
            Report::Done(outcome, conversation) => break (outcome, conversation),
            Report::Refused { reason, .. } => return Attempt::ended(Outcome::Unstarted, reason),
            _ => {}
        }
    };
    kept.conversation = conversation;
    kept.cost = costs.spent;
    let session = match kept.save(home) {
        Ok(()) => Some(kept.id.clone()),
        Err(reason) => {
            eprintln!("glut: {reason}");
            None
        }
    };
    // A failure that is the plan's usage limit (PV4) is kept as that: the owner's way out is the plan's settings.
    let limit = error
        .as_deref()
        .is_some_and(|said| said.contains(glut::responses::PLAN_LIMIT));
    let outcome_of = match Outcome::of(outcome.reason) {
        Outcome::Failed if limit => Outcome::PlanLimit,
        other => other,
    };
    let said = match (&outcome.answer, outcome_of.well()) {
        (Some(answer), true) => first_line(answer),
        _ => first_line(&error.unwrap_or_else(|| outcome_of.said().to_owned())),
    };
    let refused = host
        .refused
        .lock()
        .map(|refused| refused.clone())
        .unwrap_or_default();
    Attempt {
        outcome: outcome_of,
        cost: costs.spent,
        said,
        session,
        refused,
        unpriced: prices.is_none(),
    }
}

/// What a run's calls cost, as the screen counts a session's (`App::count_subagent`): a subagent's priced by its
/// own model where it runs on one, else by the run's.
#[derive(Default)]
struct Costs {
    /// The run's model's prices.
    prices: Option<glut::Prices>,
    /// The subagents' own prices, by their task as the run labelled it.
    sub_prices: std::collections::HashMap<String, glut::Prices>,
    /// What was spent so far; `None` while nothing priced was.
    spent: Option<f64>,
}

impl Costs {
    fn count(&mut self, event: &Event) {
        let mut task = None;
        let mut inner = event;
        while let Event::Sub {
            task: named,
            event: wrapped,
            ..
        } = inner
        {
            if named.is_some() {
                task.clone_from(named);
            }
            inner = wrapped;
        }
        match inner {
            Event::Started { prices, .. } => {
                if let Some(task) = task {
                    match prices {
                        Some(own) => self.sub_prices.insert(task, *own),
                        None => self.sub_prices.remove(&task),
                    };
                }
            }
            Event::Usage {
                input_tokens,
                cached_input_tokens,
                output_tokens,
                ..
            } => {
                let used = glut::Usage {
                    input_tokens: *input_tokens,
                    cached_input_tokens: *cached_input_tokens,
                    output_tokens: *output_tokens,
                };
                let prices = task
                    .and_then(|task| self.sub_prices.get(&task).copied())
                    .or(self.prices);
                if let Some(prices) = prices {
                    *self.spent.get_or_insert(0.0) += prices.cost_usd(&used);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use clap::Parser as _;

    use super::*;

    /// `glut schedule …` as clap reads it.
    #[derive(clap::Parser)]
    struct Cli {
        #[command(subcommand)]
        command: Command,
    }

    fn add_args(words: &[&str]) -> AddArgs {
        let mut all = vec!["glut", "add"];
        all.extend(words);
        match Cli::parse_from(all).command {
            Command::Add(args) => *args,
            _ => unreachable!(),
        }
    }

    #[derive(Default)]
    struct Seen(RefCell<Vec<String>>);

    impl Agents for Seen {
        fn start(&self, label: &str, _file: &Path) -> Result<(), String> {
            self.0.borrow_mut().push(format!("start {label}"));
            Ok(())
        }

        fn stop(&self, label: &str) {
            self.0.borrow_mut().push(format!("stop {label}"));
        }
    }

    /// `add` in the test's home folder.
    fn add_here(home: &Path, args: AddArgs, agents: &dyn Agents) -> Result<String, String> {
        add(home, args, home, agents)
    }

    fn home() -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "glut-scheduled-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(home.join("project")).unwrap();
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        std::fs::write(
            home.join(".glut").join("providers.json"),
            r#"{"default": "local", "providers": {"local": {"kind": "ollama", "model": "qwen"}}}"#,
        )
        .unwrap();
        home
    }

    #[test]
    fn a_plan_is_made_with_its_engine_fixed_and_changed_keeping_what_is_not_given() {
        let home = home();
        let project = home.join("project");
        let seen = Seen::default();
        let cwd = project.to_string_lossy().into_owned();
        let said = add_here(
            &home,
            add_args(&[
                "tests",
                "--at",
                "07:00",
                "--days",
                "mo-fr",
                "--cwd",
                &cwd,
                "--allow",
                "Bash(cargo test:*)",
                "run",
                "the",
                "tests",
            ]),
            &seen,
        )
        .unwrap();
        assert!(
            said.contains("Bash(cargo test:*)"),
            "what it may do is said: {said}"
        );
        let plan = Schedules::load(&home)
            .unwrap()
            .get("tests")
            .cloned()
            .unwrap();
        assert_eq!(plan.task, "run the tests");
        assert_eq!(
            (plan.provider.as_str(), plan.model.as_str()),
            ("local", "qwen"),
            "the default, named now"
        );
        assert_eq!(
            plan.mode,
            Mode::Default,
            "asks without a mode named, never auto"
        );
        assert_eq!(plan.folder, project.canonicalize().unwrap());
        assert_eq!((plan.max_usd, plan.max_minutes), (1.0, 30));
        // Changed: a new time, the days, the task, the rules kept.
        add_here(
            &home,
            add_args(&[
                "tests",
                "--at",
                "08:30",
                "--max-usd",
                "2",
                "--max-tokens",
                "500k",
            ]),
            &seen,
        )
        .unwrap();
        let changed = Schedules::load(&home)
            .unwrap()
            .get("tests")
            .cloned()
            .unwrap();
        assert_eq!(
            changed.when,
            When::At {
                hour: 8,
                minute: 30,
                days: vec![1, 2, 3, 4, 5]
            }
        );
        assert_eq!(
            (changed.task.as_str(), changed.max_usd),
            ("run the tests", 2.0)
        );
        assert_eq!(changed.allow, plan.allow);
        assert_eq!((plan.max_tokens, changed.max_tokens), (None, Some(500_000)));
        assert!(tokens_arg("lots").is_err() && tokens_arg("0").is_err());
        assert!(add_here(&home, add_args(&["tests", "--days", "sa"]), &seen).is_ok());
        assert!(
            add_here(
                &home,
                add_args(&["tests", "--every", "2h", "--days", "sa"]),
                &seen
            )
            .is_err()
        );
        // A new plan needs its time and its task; a command for a task is refused.
        assert!(add_here(&home, add_args(&["new", "--cwd", &cwd, "x"]), &seen).is_err());
        assert!(
            add_here(
                &home,
                add_args(&["new", "--every", "1h", "--cwd", &cwd]),
                &seen
            )
            .is_err()
        );
        assert!(
            add_here(
                &home,
                add_args(&["new", "--every", "1h", "--cwd", &cwd, "/quit"]),
                &seen
            )
            .is_err()
        );
        assert!(
            add_here(
                &home,
                add_args(&["new", "--every", "1h", "--cwd", &cwd, "--mode", "x", "y"]),
                &seen
            )
            .is_err()
        );
        assert!(
            add_here(
                &home,
                add_args(&["new", "--every", "1h", "--cwd", "/nowhere/at/all", "y"]),
                &seen
            )
            .is_err()
        );
        assert!(
            add_here(
                &home,
                add_args(&["Bad", "--every", "1h", "--cwd", &cwd, "y"]),
                &seen
            )
            .is_err()
        );
        assert!(
            add_here(
                &home,
                add_args(&[
                    "new", "--every", "1h", "--allow", "Bash(", "--cwd", &cwd, "y"
                ]),
                &seen
            )
            .is_err()
        );
        assert_eq!(
            Schedules::load(&home).unwrap().plans.len(),
            1,
            "nothing refused was kept"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_plan_never_starts_from_where_its_commands_write_and_a_list_given_again_joins() {
        let home = home();
        let seen = Seen::default();
        let project = home.join("project");
        let cwd = project.to_string_lossy().into_owned();
        // Its folder: never the home folder nor one above it.
        assert!(
            add_here(
                &home,
                add_args(&["h", "--every", "1h", "--cwd", &home.to_string_lossy(), "x"]),
                &seen
            )
            .is_err()
        );
        assert!(
            add_here(
                &home,
                add_args(&["r", "--every", "1h", "--cwd", "/", "x"]),
                &seen
            )
            .is_err()
        );
        // The program and the PATH: nothing where the plan's commands write.
        let plan = Plan {
            folder: project.canonicalize().unwrap(),
            ..crate::schedule::sample("p")
        };
        let places = writable_places(&plan, &home, &[], &[]);
        assert!(program_outside(&plan.folder.join("target/release/glut"), &places).is_err());
        assert!(program_outside(Path::new("/usr/local/bin/glut"), &places).is_ok());
        let tmp = std::env::temp_dir();
        let path = format!(
            "/usr/bin:{}:bin:{}:/bin",
            plan.folder.join(".venv/bin").display(),
            tmp.join("x").display()
        );
        assert_eq!(path_outside(&path, &places), "/usr/bin:/bin");
        // Given again, rules join the plan's; --replace takes their place.
        add_here(
            &home,
            add_args(&[
                "j",
                "--every",
                "1h",
                "--cwd",
                &cwd,
                "--allow",
                "Bash(a:*)",
                "x",
            ]),
            &seen,
        )
        .unwrap();
        add_here(
            &home,
            add_args(&["j", "--allow", "Bash(b:*)", "--allow", "Bash(a:*)"]),
            &seen,
        )
        .unwrap();
        let joined = Schedules::load(&home).unwrap().get("j").cloned().unwrap();
        assert_eq!(joined.allow, ["Bash(a:*)", "Bash(b:*)"]);
        add_here(
            &home,
            add_args(&["j", "--replace", "--allow", "Bash(c:*)"]),
            &seen,
        )
        .unwrap();
        assert_eq!(
            Schedules::load(&home).unwrap().get("j").unwrap().allow,
            ["Bash(c:*)"]
        );
        // A word in quotes stays whole; a quote inside a word is the word's.
        assert_eq!(
            split("j --at 07:00 \"run it\" check what's failing").unwrap(),
            ["j", "--at", "07:00", "run it", "check", "what's", "failing"]
        );
        assert!(split("j \"open").is_err());
        let _ = std::fs::remove_dir_all(home);
    }

    #[tokio::test]
    async fn a_paused_plans_job_does_nothing_but_the_owners_own_run_goes() {
        let home = home();
        let seen = Seen::default();
        let folder = home.join("project").join("held");
        std::fs::create_dir_all(&folder).unwrap();
        let cwd = folder.to_string_lossy().into_owned();
        add_here(
            &home,
            add_args(&["held", "--every", "1h", "--cwd", &cwd, "x"]),
            &seen,
        )
        .unwrap();
        hold(&home, "held", true, &seen).unwrap();
        std::fs::remove_dir(&folder).unwrap();
        assert_eq!(run(&home, "held", true, false).await, ExitCode::SUCCESS);
        assert!(
            schedule::runs(&home, "held").is_empty(),
            "a stale job leaves no record"
        );
        assert_eq!(run(&home, "held", true, true).await, ExitCode::from(1));
        let runs = schedule::runs(&home, "held");
        assert_eq!(runs.len(), 1);
        assert!(runs[0].by_hand, "by hand: its times stay as they were");
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_plan_paused_resumed_and_removed_takes_its_agent_and_its_record_with_it() {
        let home = home();
        let seen = Seen::default();
        let cwd = home.join("project").to_string_lossy().into_owned();
        add_here(
            &home,
            add_args(&["sync", "--every", "2h", "--cwd", &cwd, "pull"]),
            &seen,
        )
        .unwrap();
        hold(&home, "sync", true, &seen).unwrap();
        assert!(Schedules::load(&home).unwrap().get("sync").unwrap().paused);
        assert!(list(&home).unwrap().contains(tr!(schedule_paused_mark)));
        // Changed while paused, it stays paused.
        add_here(&home, add_args(&["sync", "--every", "3h"]), &seen).unwrap();
        assert!(Schedules::load(&home).unwrap().get("sync").unwrap().paused);
        hold(&home, "sync", false, &seen).unwrap();
        assert!(!Schedules::load(&home).unwrap().get("sync").unwrap().paused);
        assert!(hold(&home, "nope", true, &seen).is_err());
        remove(&home, "sync", &seen).unwrap();
        assert!(Schedules::load(&home).unwrap().plans.is_empty());
        assert_eq!(list(&home).unwrap(), tr!(schedule_none));
        assert_eq!(
            seen.0
                .borrow()
                .iter()
                .filter(|said| said.starts_with("start"))
                .count(),
            2,
            "made, and resumed: a paused plan has no agent"
        );
        let _ = std::fs::remove_dir_all(home);
    }

    #[test]
    fn a_plans_run_is_its_owners_session_with_the_plans_rights_alone() {
        let plan = Plan {
            name: "tests".into(),
            folder: "/p".into(),
            task: "t".into(),
            when: When::Every { minutes: 60 },
            provider: "local".into(),
            model: "qwen".into(),
            mode: Mode::Auto,
            allow: vec!["Bash(cargo test:*)".into()],
            deny: vec!["Bash(git push:*)".into()],
            folders: Vec::new(),
            max_usd: 1.0,
            max_minutes: 30,
            max_tokens: None,
            paused: false,
            path: String::new(),
            changed: Utc::now(),
        };
        let launch = launch_of(&plan, "/p".into());
        assert_eq!(launch.gate.mode, Some(Mode::Auto));
        assert_eq!(launch.gate.allow, plan.allow);
        assert_eq!(launch.gate.deny, plan.deny);
        assert!(launch.gate.ask.is_empty());
        assert!(
            !launch.gate.trusted && !launch.owner_trust,
            "a project's rules never count"
        );
        assert!(matches!(launch.servers, Servers::OwnerAlone));
        assert!(
            launch.mode_locked && !launch.owner_shell && !launch.projects && launch.keep_sessions
        );
        assert!(matches!(
            launch.engine,
            EngineChoice::Providers { provider: Some(ref p), model: Some(ref m) } if p == "local" && m == "qwen"
        ));
        assert_eq!(
            first_line("\n  first\u{1b}[31m line \nsecond"),
            "first [31m line"
        );
        // A subagent's tokens count, at its own model's prices where it has them.
        let per_m = |usd: f64| glut::Prices {
            input_per_m: usd,
            cached_input_per_m: None,
            output_per_m: usd,
        };
        let mut costs = Costs {
            prices: Some(per_m(1.0)),
            ..Costs::default()
        };
        let sub = |task: &str, event: Event| Event::Sub {
            depth: 1,
            task: Some(task.into()),
            event: Box::new(event),
        };
        let used = |tokens: u64| Event::Usage {
            turn: 1,
            input_tokens: tokens,
            cached_input_tokens: 0,
            output_tokens: 0,
        };
        costs.count(&used(1_000_000));
        costs.count(&sub("cheap", used(1_000_000)));
        assert_eq!(
            costs.spent,
            Some(2.0),
            "without its own prices, at the run's"
        );
        costs.sub_prices.insert("dear".into(), per_m(10.0));
        costs.count(&sub("dear", used(1_000_000)));
        assert_eq!(costs.spent, Some(12.0));
    }

    #[tokio::test]
    async fn nobody_is_there_to_answer_a_plans_run() {
        let host = Unattended::default();
        let request = AskRequest {
            tool: "bash".into(),
            target: "rm -rf build".into(),
            scopes: Vec::new(),
            from: None,
        };
        let never: AnswerFuture<'_> = Box::pin(std::future::pending());
        assert_eq!(host.ask(&request, never).await, Answer::Deny);
        assert_eq!(*host.refused.lock().unwrap(), ["bash: rm -rf build"]);
    }

    #[tokio::test]
    async fn a_run_of_a_plan_whose_folder_is_gone_is_kept_as_unstarted() {
        let home = home();
        let seen = Seen::default();
        let folder = home.join("project").join("gone");
        std::fs::create_dir_all(&folder).unwrap();
        let cwd = folder.to_string_lossy().into_owned();
        add_here(
            &home,
            add_args(&["gone", "--every", "1h", "--cwd", &cwd, "x"]),
            &seen,
        )
        .unwrap();
        std::fs::remove_dir(&folder).unwrap();
        assert_eq!(run(&home, "gone", false, false).await, ExitCode::from(1));
        let runs = schedule::runs(&home, "gone");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].outcome, Outcome::Unstarted);
        assert!(runs[0].said.contains("gone"), "{}", runs[0].said);
        // One still running: the next skips, and says so.
        let lock = schedule::Lock::take(&home, "gone").unwrap();
        assert_eq!(run(&home, "gone", false, false).await, ExitCode::from(1));
        assert_eq!(schedule::runs(&home, "gone")[1].outcome, Outcome::Skipped);
        drop(lock);
        let _ = std::fs::remove_dir_all(home);
    }
}
