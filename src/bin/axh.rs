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
use clap::{Parser, Subcommand};

/// The environment variable the key is read from.
const KEY_ENV: &str = axiomata_harness::config::API_KEY_ENV;

#[derive(Parser)]
#[command(name = "axh", version, about = "Axiomata's own agent harness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Runs one prompt and writes its events as JSON lines.
    Run {
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

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let Command::Run {
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
    } = Cli::parse().command;

    let fail = |message: String| {
        JsonLines.emit(Event::Error { message });
        ExitCode::from(1)
    };
    if prompt.is_none() && skill.is_none() {
        return fail("name a prompt, or run a skill with --skill <name>".into());
    }
    let cwd = match cwd.canonicalize() {
        Ok(cwd) => cwd,
        Err(err) => return fail(format!("cannot use {}: {err}", cwd.display())),
    };
    let mut config = RunConfig::new(&cwd);
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
    let tools = match setup::toolbox(
        &cwd,
        &catalog,
        gate,
        servers,
        built.sandbox,
        Some(env_search()),
        &mut sink,
    )
    .await
    {
        Ok(tools) => tools,
        Err(err) => return fail(err.to_string()),
    };

    // Ctrl-C, SIGTERM and SIGHUP end the run cleanly, with its `finished` line and every background job stopped.
    let cancel = Cancel::new();
    axiomata_harness::run::cancel_on_signals(&cancel);

    let outcome = run(&config, &model, &tools, &prompt, &mut sink, &cancel).await;
    ExitCode::from(match outcome.reason {
        FinishReason::Answered => 0,
        FinishReason::Failed => 1,
        FinishReason::MaxTurns | FinishReason::Timeout | FinishReason::Cancelled => 2,
    })
}
