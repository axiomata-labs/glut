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

use axiomata_harness::commands::{Expanded, PermissionsView, Registry, Source};
use axiomata_harness::mcp;
use axiomata_harness::permissions::settings::Trust;
use axiomata_harness::permissions::{GateOptions, Mode, Nobody, build_gate};
use axiomata_harness::skills::{self, SkillTool};
use axiomata_harness::tools::Tool;
use axiomata_harness::tools::files::{SkillRoots, file_tools};
use axiomata_harness::tools::shell::{ShellOptions, shell_tools};
use axiomata_harness::{
    Cancel, Event, EventSink, FinishReason, ModelEndpoint, OpenAiCompat, RunConfig, SharedSink,
    SkillHow, Toolbox, Usage, run,
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
    let mut endpoint = ModelEndpoint {
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
    let mut tools = Toolbox::new();
    match file_tools(&cwd, SkillRoots::new(&catalog.folders())) {
        Ok(files) => tools.extend(files),
        Err(err) => return fail(err.to_string()),
    }
    // The `skill` tool only while there is at least one skill to load (K1.2).
    if !catalog.is_empty() {
        tools.extend([
            Box::new(SkillTool::new(Arc::clone(&catalog), sink.clone())) as Box<dyn Tool>
        ]);
    }
    let options = GateOptions {
        mode,
        allow,
        ask,
        deny,
        trusted,
    };
    // Headless: nobody to ask (P6).
    let (mut gate, untrusted, sandbox, hooks, project_hooks) =
        match build_gate(&home, &cwd, options, Arc::new(Nobody)) {
            Ok(parts) => parts,
            Err(err) => return fail(err.to_string()),
        };
    if untrusted {
        eprintln!(
            "axh: the project's .axh/ allow rules and mode are left out: this folder is not trusted"
        );
    }
    // HS7, B1: the owner's hooks run; a project's are parsed and left out, and said so.
    if project_hooks {
        eprintln!(
            "axh: the project's own .axh/ hooks are not run: only ~/.axh/settings.json hooks run"
        );
    }
    // HS7: the hooks run with what the settings say about the sandbox, like the shell's own commands (D5).
    config.hooks = hooks;
    config.sandbox = Some(sandbox.clone());
    if !sandbox.enabled || !axiomata_harness::sandbox::available() {
        eprintln!("axh: commands run without a sandbox: only the permission rules hold them");
    }

    // Slash commands (HS4), built after the gate with the same home, folder and trust it was built with (C1).
    let mut registry = match Registry::load(&home, &cwd, trusted) {
        Ok(registry) => registry,
        Err(err) => return fail(err.to_string()),
    };
    registry.set_permissions(PermissionsView::of(&gate, &sandbox));
    let mut prompt = prompt.unwrap_or_default();
    endpoint = match registry.expand(&prompt) {
        // An ordinary prompt: no command was named (C6).
        None => endpoint,
        Some(Err(message)) => return fail(message),
        // A built-in is answered without a model: its text is the answer (C5).
        Some(Ok(Expanded::Builtin { name, text })) => {
            sink.emit(Event::Command {
                name,
                source: Source::Builtin.as_str().to_owned(),
                prompt_chars: 0,
            });
            sink.emit(Event::AssistantText { turn: 0, text });
            sink.emit(Event::Finished {
                reason: FinishReason::Answered,
                turns: 0,
                usage: Usage::default(),
            });
            return ExitCode::SUCCESS;
        }
        Some(Ok(Expanded::Prompt {
            name,
            prompt: expanded,
            allowed_tools,
            model,
        })) => {
            let source = registry.source(&name).unwrap_or(Source::User);
            sink.emit(Event::Command {
                source: source.as_str().to_owned(),
                name: name.clone(),
                prompt_chars: expanded.chars().count(),
            });
            // C3: the command's tools narrow the gate on top of its rules, never in their place.
            if !allowed_tools.is_empty() {
                gate.restrict_name = Some(name);
                gate.restrict = Some(allowed_tools);
            }
            prompt = expanded;
            match model {
                Some(model) => ModelEndpoint { model, ..endpoint },
                None => endpoint,
            }
        }
    };
    // A skill as the run (HS6, K1.1): its body is the run's instructions, its frontmatter may narrow the tools,
    // pick the model and set the time limit (K5). After the commands, so a command's own word wins where both
    // name one.
    if let Some(name) = skill {
        let Some(chosen) = catalog.get(&name) else {
            return fail(format!(
                "no skill named {name:?}; known: {}",
                catalog.known()
            ));
        };
        if let Err(err) = chosen.start_run(&cwd, &mut config, &mut endpoint, &mut gate) {
            return fail(err.to_string());
        }
        sink.send(Event::Skill {
            name,
            how: SkillHow::Run,
        });
        if prompt.trim().is_empty() {
            prompt = skills::DEFAULT_PROMPT.to_owned();
        }
    }
    // The gate enters the toolbox only after the command and the skill had their say about it (C3, K5).
    tools.set_gate(Arc::new(gate));
    // The model is built after the commands: one of them may name its own (C2).
    let model = match OpenAiCompat::new(endpoint) {
        Ok(model) => model,
        Err(err) => return fail(err.to_string()),
    };
    // MCP servers (HS5): after the gate, before the shell (M1–M3). `axh` has no terminal, so a project's server
    // nobody approved is left out and said so on stderr rather than asked about (M2).
    let entries = match mcp::config::load(&home, &cwd) {
        Ok(entries) => entries,
        Err(err) => return fail(err.to_string()),
    };
    let entries = match mcp::approval::permitted(entries, &home, &cwd, false) {
        Ok(entries) => entries,
        Err(err) => return fail(err.to_string()),
    };
    tools.extend(mcp::start(entries, &mut sink).await);
    // The shell after the gate: the settings that built it also say how the sandbox runs (HS3b).
    let shell = ShellOptions {
        sandbox: Some(sandbox),
        ..ShellOptions::from_environment()
    };
    match shell_tools(&cwd, shell) {
        Ok(shell) => tools.extend(shell),
        Err(err) => return fail(err.to_string()),
    }

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
