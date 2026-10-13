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

use axiomata_harness::permissions::settings::Trust;
use axiomata_harness::permissions::{GateOptions, Mode, Nobody, build_gate};
use axiomata_harness::tools::files::file_tools;
use axiomata_harness::tools::shell::{ShellOptions, shell_tools};
use axiomata_harness::{
    Cancel, Event, EventSink, FinishReason, ModelEndpoint, OpenAiCompat, RunConfig, Toolbox, run,
};
use clap::{Parser, Subcommand};

/// The environment variable the key is read from.
const KEY_ENV: &str = "AXH_API_KEY";

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
        /// The prompt.
        prompt: String,
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
        prompt,
    } = Cli::parse().command;

    let fail = |message: String| {
        JsonLines.emit(Event::Error { message });
        ExitCode::from(1)
    };
    let cwd = match cwd.canonicalize() {
        Ok(cwd) => cwd,
        Err(err) => return fail(format!("cannot use {}: {err}", cwd.display())),
    };
    let mut config = RunConfig::new(&cwd);
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
    let model = match OpenAiCompat::new(endpoint) {
        Ok(model) => model,
        Err(err) => return fail(err.to_string()),
    };
    let mut tools = Toolbox::new();
    match file_tools(&cwd) {
        Ok(files) => tools.extend(files),
        Err(err) => return fail(err.to_string()),
    }
    match shell_tools(&cwd, ShellOptions::from_environment()) {
        Ok(shell) => tools.extend(shell),
        Err(err) => return fail(err.to_string()),
    }
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
    let options = GateOptions {
        mode,
        allow,
        ask,
        deny,
        trusted,
    };
    // Headless: nobody to ask (P6).
    match build_gate(&home, &cwd, options, Arc::new(Nobody)) {
        Ok((gate, untrusted)) => {
            if untrusted {
                eprintln!(
                    "axh: the project's .axh/ allow rules and mode are left out: this folder is not trusted"
                );
            }
            tools.set_gate(Arc::new(gate));
        }
        Err(err) => return fail(err.to_string()),
    }

    // Ctrl-C, SIGTERM and SIGHUP end the run cleanly, with its `finished` line and every background job stopped.
    let cancel = Cancel::new();
    axiomata_harness::run::cancel_on_signals(&cancel);

    let outcome = run(&config, &model, &tools, &prompt, &mut JsonLines, &cancel).await;
    ExitCode::from(match outcome.reason {
        FinishReason::Answered => 0,
        FinishReason::Failed => 1,
        FinishReason::MaxTurns | FinishReason::Timeout | FinishReason::Cancelled => 2,
    })
}
