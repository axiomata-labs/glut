//! `axh` — the harness as a program of its own: runs one prompt headless and writes every step as a JSON line on
//! stdout (`docs/plans/harness.md`, H3). Logs go to stderr, so stdout stays machine-readable.
//!
//! The key comes from the environment (`AXH_API_KEY`), never from an argument: an argument shows in `ps` and in a
//! shell's history.
//!
//! Exit codes: 0 answered, 1 failed (model API, bad folder), 2 stopped at a limit (turns, time) or cancelled.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use axiomata_harness::commands::{PermissionsView, Registry};
use axiomata_harness::mcp;
use axiomata_harness::permissions::settings::Trust;
use axiomata_harness::permissions::{GateOptions, Mode, Nobody, build_gate};
use axiomata_harness::setup::{self, Draft, Prepared};
use axiomata_harness::skills;
use axiomata_harness::{
    Cancel, Event, EventSink, FinishReason, ModelEndpoint, OpenAiCompat, RunConfig, SharedSink,
    Usage, run,
};
use clap::{Args, Parser, Subcommand};
use serde::{Deserialize, Serialize};

/// The environment variable the key is read from.
const KEY_ENV: &str = axiomata_harness::config::API_KEY_ENV;

#[derive(Parser)]
#[command(name = "axh", version, about = "Axiomata's own agent harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

// Parsed once at start: `Run`'s many flags make it far larger than `Undo`, and boxing them would only complicate the
// derive.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
enum Command {
    /// Runs one prompt and writes its events as JSON lines.
    Run(RunArgs),
    /// Goes on with a run that was lost before it finished (durable runs, `docs/plans/durable.md`): from its journal in
    /// `~/.axh/runs`, with the run's own flags. Without a run the newest unfinished one; --list shows them.
    Resume {
        /// The run, as `axh` named it when it started (on stderr).
        run: Option<String>,
        /// List the unfinished runs, newest first, and change nothing.
        #[arg(long)]
        list: bool,
        /// Give the run up: its journal is deleted.
        #[arg(long)]
        discard: bool,
    },
    /// Puts a folder back as it was before a run, or before one of its steps (Glut 2): the snapshots a run took
    /// before it changed anything. Without --run the newest run of the folder; --list shows the runs.
    Undo {
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        /// The run, as its `snapshot` events name it.
        #[arg(long)]
        run: Option<String>,
        /// Back to before this step of the run instead of before the whole run.
        #[arg(long)]
        step: Option<u32>,
        /// List the runs with snapshots of the folder, newest first, and change nothing.
        #[arg(long)]
        list: bool,
    },
}

/// Writes each event as one JSON line, flushed at once so a reader sees it while the run goes on.
struct JsonLines;

impl EventSink for JsonLines {
    fn emit(&mut self, event: Event) {
        let mut out = std::io::stdout().lock();
        if let Ok(line) = serde_json::to_string(&event) {
            let _ = writeln!(out, "{line}");
            let _ = out.flush();
        }
    }
}

/// `axh`'s web search: the providers' keys from the environment (`EXA_API_KEY`, `TINYFISH_API_KEY`, `TAVILY_API_KEY`;
/// Exa answers without one) and no budget — the app keeps the limits for its own runs (glut-round CP3).
fn env_search() -> axiomata_harness::tools::search::WebSearch {
    use axiomata_harness::tools::search::{SearchKeys, Unlimited, WebSearch};
    let key = |name: &str| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    };
    WebSearch::new(
        SearchKeys {
            exa: key("EXA_API_KEY"),
            tinyfish: key("TINYFISH_API_KEY"),
            tavily: key("TAVILY_API_KEY"),
        },
        Arc::new(Unlimited),
    )
}

/// `axh run`'s flags — kept in a run's journal, so `axh resume` builds the run again as it began.
#[derive(Args, Clone, Serialize, Deserialize)]
struct RunArgs {
    /// The API's base, without /v1 (https://openrouter.ai/api, http://localhost:11434).
    #[arg(long)]
    base_url: String,
    #[arg(long)]
    model: String,
    /// The folder the run works in; every file tool is confined to it.
    #[arg(long, default_value = ".")]
    cwd: PathBuf,
    /// A file whose text is appended to the system prompt (a role, AGENTS.md).
    #[arg(long)]
    instructions_file: Option<PathBuf>,
    #[arg(long)]
    max_turns: Option<u32>,
    /// The run's time limit in seconds.
    #[arg(long)]
    timeout_secs: Option<u64>,
    /// default, accept_edits, plan, dont_ask or auto; otherwise the settings files', else default. Headless, an
    /// ask is refused: name what the run may do with --allow, or pick a mode.
    #[arg(long)]
    permission_mode: Option<String>,
    /// A rule the run allows, in Claude Code's format (`Bash(cargo test:*)`, `Edit(src/**)`); repeatable.
    #[arg(long)]
    allow: Vec<String>,
    /// A rule the run asks about (refused headless); repeatable.
    #[arg(long)]
    ask: Vec<String>,
    /// A rule the run refuses; repeatable.
    #[arg(long)]
    deny: Vec<String>,
    /// Run a skill as the run's task (HS6, K1.1): its SKILL.md body becomes the run's instructions, and its
    /// frontmatter may narrow the tools, pick the model and set the time limit (K5).
    #[arg(long)]
    skill: Option<String>,
    /// A folder of skills beside `~/.axh/skills` (HS6, K2); repeatable.
    #[arg(long)]
    skill_dir: Vec<PathBuf>,
    /// The prompt — extra text beside `--skill`, or a slash command (`/review src/`), read from
    /// `~/.axh/commands/` and, in a trusted folder, the project's `.axh/commands/` (HS4). Without `--skill` a
    /// prompt is required; with one it defaults to "Run the skill.".
    prompt: Option<String>,
}

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let command = Cli::parse().command;
    if let Command::Undo {
        cwd,
        run,
        step,
        list,
    } = command
    {
        return undo(&cwd, run, step, list);
    }
    match command {
        Command::Undo { .. } => unreachable!("undo returned above"),
        Command::Run(args) => execute(args, None).await,
        Command::Resume { run, list, discard } => resume_command(run, list, discard).await,
    }
}

/// Whether `url` names a user or a password before its host (`https://user:pw@host`).
fn has_userinfo(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| authority.contains('@'))
}

/// Where `axh` keeps the journals of its own runs (durable runs, D5) — the app keeps its own elsewhere.
fn journal_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".axh").join("runs"))
}

/// `axh resume`: lists the unfinished runs, gives one up, or goes on with one — built from the flags it began with.
async fn resume_command(run: Option<String>, list: bool, discard: bool) -> ExitCode {
    use axiomata_harness::journal;
    let fail = |message: String| {
        eprintln!("axh resume: {message}");
        ExitCode::from(1)
    };
    let Some(dir) = journal_dir() else {
        return fail("HOME is not set to an absolute folder".into());
    };
    journal::sweep(&dir);
    let unfinished = journal::list(&dir);
    if list {
        for entry in &unfinished {
            let what = entry.meta["args"]["skill"]
                .as_str()
                .map(|skill| format!("skill {skill}"))
                .or_else(|| {
                    entry.meta["args"]["prompt"]
                        .as_str()
                        .map(|p| p.chars().take(60).collect())
                })
                .unwrap_or_default();
            let state = if entry.running {
                "  (still running)"
            } else {
                ""
            };
            println!("{}  {}  {what}{state}", entry.run, entry.started_at);
        }
        return ExitCode::SUCCESS;
    }
    // Named, never guessed: a resume runs with the flags the journal holds, so the owner picks which.
    let Some(run) = run else {
        return fail("name the run: axh resume <run> (axh resume --list shows them)".into());
    };
    let Some(entry) = unfinished.iter().find(|entry| entry.run == run) else {
        return fail(format!(
            "no unfinished run {run} (axh resume --list shows them)"
        ));
    };
    if discard {
        return match journal::discard(&dir, &entry.run) {
            Ok(()) => {
                println!("{} given up", entry.run);
                ExitCode::SUCCESS
            }
            Err(reason) => fail(reason),
        };
    }
    let args = match serde_json::from_value::<RunArgs>(entry.meta["args"].clone()) {
        Ok(args) => args,
        Err(err) => {
            return fail(format!(
                "the run {} was not started by axh run: {err}",
                entry.run
            ));
        }
    };
    // What the run goes on with, said before it does: the flags come from the journal, not from this command line.
    eprintln!(
        "axh resume: {} with --base-url {} --model {} --cwd {} --permission-mode {} --allow {:?} --deny {:?}",
        entry.run,
        args.base_url,
        args.model,
        args.cwd.display(),
        args.permission_mode.as_deref().unwrap_or("(settings)"),
        args.allow,
        args.deny
    );
    execute(args, Some(entry.run.clone())).await
}

/// One run of `axh run` — or of `axh resume`, then `resumed` names the run whose journal it goes on from.
async fn execute(args: RunArgs, resumed: Option<String>) -> ExitCode {
    let journal_args = args.clone();
    let RunArgs {
        base_url,
        model,
        cwd,
        instructions_file,
        max_turns,
        timeout_secs,
        permission_mode,
        allow,
        ask,
        deny,
        skill,
        skill_dir,
        prompt,
    } = args;

    let fail = |message: String| {
        JsonLines.emit(Event::Error { message });
        ExitCode::from(1)
    };
    if prompt.is_none() && skill.is_none() {
        return fail("name a prompt, or run a skill with --skill <name>".into());
    }
    // The key goes in a header, never in the URL — and a journal keeps the URL as it was given.
    if has_userinfo(&base_url) {
        return fail(
            "--base-url must not hold a user or a password; the key comes from AXH_API_KEY".into(),
        );
    }
    let cwd = match cwd.canonicalize() {
        Ok(cwd) => cwd,
        Err(err) => return fail(format!("cannot use {}: {err}", cwd.display())),
    };
    let mut config = RunConfig::new(&cwd);
    // Durable runs (D5): every run journals its steps; one the process lost goes on with `axh resume`.
    if let Some(dir) = journal_dir() {
        let run = resumed.clone().unwrap_or_else(|| {
            format!(
                "axh-{}-{}",
                std::process::id(),
                chrono::Local::now().timestamp_millis()
            )
        });
        if resumed.is_none() {
            eprintln!("axh: run {run} (if it is lost before it ends: axh resume {run})");
        }
        let mut kept = journal_args;
        kept.cwd = cwd.clone();
        config.journal = Some(axiomata_harness::journal::JournalSpec {
            dir,
            run,
            meta: serde_json::json!({ "args": kept }),
        });
    }
    // Glut 2 (D): snapshots before changes, for `axh undo`.
    config.snapshots = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".axh").join("snapshots"));
    // The folders the run reads skills from, beside the owner's and the project's (HS6, K2).
    config.skill_dirs = skill_dir;
    if let Some(path) = instructions_file {
        match std::fs::read_to_string(&path) {
            Ok(text) => config.instructions = Some(text),
            Err(err) => return fail(format!("cannot read {}: {err}", path.display())),
        }
    }
    if let Some(turns) = max_turns {
        config.max_turns = turns;
    }
    if let Some(secs) = timeout_secs {
        config.timeout = std::time::Duration::from_secs(secs);
    }
    let endpoint = ModelEndpoint {
        base_url,
        api_key: std::env::var(KEY_ENV).ok().filter(|key| !key.is_empty()),
        model,
    };
    let mode = match permission_mode
        .as_deref()
        .map(|text| (text, Mode::parse(text)))
    {
        None => None,
        Some((_, Some(mode))) => Some(mode),
        Some((text, None)) => return fail(format!("unknown permission mode {text:?}")),
    };
    // Without a home there is no owner's settings file: a relative `.axh/` would be the project's, read as the owner's.
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
    else {
        return fail("HOME is not set to an absolute folder".into());
    };
    let trusted = match Trust::load(&home) {
        Ok(trust) => trust.contains(&cwd),
        Err(err) => return fail(err.to_string()),
    };
    // Skills (HS6, K1/K2): the owner's `~/.axh/skills`, the folders of `--skill-dir` and — in a folder the owner
    // trusted once, the same trust the project's allow rules need — the project's `.axh/skills`. A skill that
    // cannot be used is a note on stderr, never an error (K3).
    let (catalog, notes) = skills::Catalog::load(&home, &cwd, trusted, &config.skill_dirs);
    for note in notes {
        eprintln!("axh: skill left out: {note}");
    }
    let catalog = Arc::new(catalog);
    // Every event through one shared sink: the loop writes to it, and the `skill` tool reports from inside a call
    // (HS6, K1.2).
    let mut sink = SharedSink::new(Box::new(JsonLines));
    let options = GateOptions {
        mode,
        allow,
        ask,
        deny,
        trusted,
    };
    // Headless: nobody to ask (P6).
    let built = match build_gate(&home, &cwd, options, Arc::new(Nobody)) {
        Ok(built) => built,
        Err(err) => return fail(err.to_string()),
    };
    for note in built.notes() {
        eprintln!("axh: {note}");
    }
    // HS7: the hooks run with what the settings say about the sandbox, like the shell's own commands (D5).
    config.hooks = built.hooks;
    config.sandbox = Some(built.sandbox.clone());

    // Slash commands (HS4), with the same home, folder and trust the gate was built with (C1).
    let mut registry = match Registry::load(&home, &cwd, trusted) {
        Ok(registry) => registry,
        Err(err) => return fail(err.to_string()),
    };
    registry.set_permissions(PermissionsView::of(&built.gate, &built.sandbox));
    let mut draft = Draft {
        config,
        endpoint,
        gate: built.gate,
    };
    let prompt = match setup::prepare(
        &mut draft,
        &registry,
        &catalog,
        prompt,
        skill.as_deref(),
        &mut sink,
    ) {
        Ok(Prepared::Prompt(prompt)) => prompt,
        // A built-in is answered without a model: its text is the answer (C5).
        Ok(Prepared::Answered) => {
            sink.emit(Event::Finished {
                reason: FinishReason::Answered,
                turns: 0,
                usage: Usage::default(),
            });
            return ExitCode::SUCCESS;
        }
        Err(err) => return fail(err.to_string()),
    };
    let Draft {
        config,
        endpoint,
        gate,
    } = draft;
    // The model is built after the command and the skill: either may name its own (C2, K5).
    let model = match OpenAiCompat::new(endpoint) {
        Ok(model) => model,
        Err(err) => return fail(err.to_string()),
    };
    // MCP servers (HS5, M1–M3). `axh` has no terminal, so a project's server nobody approved is left out and said
    // so on stderr rather than asked about (M2).
    let servers = match mcp::config::load(&home, &cwd)
        .and_then(|entries| mcp::approval::permitted(entries, &home, &cwd, false))
    {
        Ok(servers) => servers,
        Err(err) => return fail(err.to_string()),
    };
    let mut tools = match setup::toolbox(
        &cwd,
        &catalog,
        gate,
        servers,
        built.sandbox,
        setup::Extras {
            search: Some(env_search()),
            direct: Vec::new(),
        },
        &mut sink,
    )
    .await
    {
        Ok(tools) => tools,
        Err(err) => return fail(err.to_string()),
    };
    // A skill run's task is the skill's body already: loading skills would only cost a turn (glut-round CP4).
    if skill.is_some() {
        tools.withdraw(axiomata_harness::skills::tool::NAME);
    }

    // Ctrl-C, SIGTERM and SIGHUP end the run cleanly, with its `finished` line and every background job stopped.
    let cancel = Cancel::new();
    axiomata_harness::run::cancel_on_signals(&cancel);

    let outcome = match &resumed {
        None => run(&config, &model, &tools, &prompt, &mut sink, &cancel).await,
        Some(id) => {
            match axiomata_harness::resume(&config, &model, &tools, id, &mut sink, &cancel).await {
                Ok((outcome, _)) => outcome,
                Err(reason) => return fail(reason),
            }
        }
    };
    ExitCode::from(match outcome.reason {
        FinishReason::Answered => 0,
        FinishReason::Failed => 1,
        FinishReason::MaxTurns | FinishReason::Timeout | FinishReason::Cancelled => 2,
    })
}

/// `axh undo`: lists a folder's runs with snapshots, or puts the folder back before a run or one of its steps.
fn undo(cwd: &std::path::Path, run: Option<String>, step: Option<u32>, list: bool) -> ExitCode {
    use axiomata_harness::snapshot;
    let fail = |message: String| {
        eprintln!("axh undo: {message}");
        ExitCode::from(1)
    };
    let Some(root) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".axh").join("snapshots"))
    else {
        return fail("HOME is not set to an absolute folder".into());
    };
    let cwd = match cwd.canonicalize() {
        Ok(cwd) => cwd,
        Err(err) => return fail(format!("cannot use {}: {err}", cwd.display())),
    };
    let runs = snapshot::runs(&root, &cwd);
    if list {
        for record in &runs {
            let steps: Vec<String> = record.steps.iter().map(|s| s.turn.to_string()).collect();
            println!("{}  steps {}", record.run, steps.join(", "));
        }
        return ExitCode::SUCCESS;
    }
    let Some(run) = run.or_else(|| runs.first().map(|record| record.run.clone())) else {
        return fail(format!("no run has snapshots of {}", cwd.display()));
    };
    match snapshot::restore(&root, &cwd, &run, step) {
        Ok(commit) => {
            match step {
                Some(step) => println!(
                    "{} is back as before step {step} of {run} ({commit:.8})",
                    cwd.display()
                ),
                None => println!("{} is back as before {run} ({commit:.8})", cwd.display()),
            }
            ExitCode::SUCCESS
        }
        Err(reason) => fail(reason),
    }
}
