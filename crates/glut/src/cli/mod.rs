//! `glut` — the harness as a program of its own: runs one prompt headless and writes every step as a JSON line on
//! stdout (`docs/plans/harness.md`, H3). Logs go to stderr, so stdout stays machine-readable.
//!
//! The key comes from the environment (`GLUT_API_KEY`) or, for a provider of `~/.glut/providers.json`, from its own
//! variable or the secret store (`glut providers`, `docs/plans/glut-providers.md`) — never from an argument: an
//! argument shows in `ps` and in a shell's history.
//!
//! Exit codes: 0 answered, 1 failed (model API, bad folder), 2 stopped at a limit (turns, time) or cancelled.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

#[macro_use]
mod texts;

pub mod keychain;
pub mod mcp_servers;
pub mod models;
pub mod providers;

use crate::commands::{PermissionsView, Registry};
use crate::mcp;
use crate::permissions::settings::Trust;
use crate::permissions::{GateOptions, Mode, Nobody, build_gate};
use crate::setup::{self, Draft, Prepared};
use crate::skills;
use crate::{
    Cancel, Effort, Event, EventSink, FinishReason, Model, ModelEndpoint, RunConfig, SharedSink,
    Usage, run,
};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

/// The environment variable the key is read from.
const KEY_ENV: &str = crate::config::API_KEY_ENV;

/// Glut's commands without its screen: a run headless, its journal and snapshots, the savings, the providers and
/// models. `glut` offers each as a subcommand of its own.
#[derive(Subcommand)]
pub enum Command {
    /// Runs one prompt and writes its events as JSON lines.
    Run(Box<RunArgs>),
    /// Goes on with a run that was lost before it finished (durable runs, `docs/plans/durable.md`): from its journal in
    /// `~/.glut/runs`, with the run's own flags. Without a run the newest unfinished one; --list shows them.
    Resume {
        /// The run, as `glut` named it when it started (on stderr).
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
    /// What Glut lean saved (`docs/plans/glut-lean.md`, N9): compacted outputs, unchanged re-reads and relocated
    /// results per kind, from `~/.glut/lean.jsonl` — every Glut run's, the app's too.
    Gain {
        /// Only the last N days.
        #[arg(long)]
        days: Option<u32>,
    },
    /// The model providers runs use (`docs/plans/glut-providers.md`): ~/.glut/providers.json, keys in a secret store.
    #[command(subcommand)]
    Providers(providers::ProvidersCommand),
    /// The models the providers offer (`docs/plans/glut-providers.md`, PV2), from lists kept in ~/.glut/models.
    #[command(subcommand)]
    Models(models::ModelsCommand),
    /// The MCP servers: ~/.glut/mcp.json and the project's .mcp.json, signing in to a hosted one and out again.
    #[command(subcommand)]
    Mcp(mcp_servers::McpCommand),
    /// The Keychain for the commands Glut runs: shut but to the listed programs, which ask for it each time.
    #[command(subcommand)]
    Keychain(keychain::KeychainCommand),
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

/// `glut`'s web search: the providers' keys from the environment (`EXA_API_KEY`, `TINYFISH_API_KEY`, `TAVILY_API_KEY`;
/// Exa answers without one) and no budget — the app keeps the limits for its own runs (glut-round CP3).
fn env_search() -> crate::tools::search::WebSearch {
    use crate::tools::search::{SearchKeys, Unlimited, WebSearch};
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

/// `glut run`'s flags — kept in a run's journal, so `glut resume` builds the run again as it began.
#[derive(Args, Clone, Serialize, Deserialize)]
pub struct RunArgs {
    /// A provider of ~/.glut/providers.json (glut providers list); without it and without --base-url the default one.
    #[arg(long, conflicts_with = "base_url")]
    #[serde(default)]
    provider: Option<String>,
    /// A one-off endpoint instead of a provider: the API's base, without /v1 (https://openrouter.ai/api,
    /// http://localhost:11434); the key from GLUT_API_KEY.
    #[arg(long)]
    #[serde(default)]
    base_url: Option<String>,
    /// The model; with a provider, its default model when left out.
    #[arg(long)]
    #[serde(default)]
    model: Option<String>,
    /// How hard the model reasons: none, minimal, low, medium, high, xhigh or max. Otherwise a skill's or a
    /// command's `effort:`, else `effort` in ~/.glut/settings.json, else the provider's own default.
    #[arg(long)]
    #[serde(default)]
    effort: Option<String>,
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
    /// The model's context window in tokens, over `contextWindow` in ~/.glut/settings.json and the provider's model
    /// list: the run compacts its older steps shortly before it is full.
    #[arg(long)]
    #[serde(default)]
    context_window: Option<u32>,
    /// End the run before its next model call once it has used this many tokens (input less cache reads, plus
    /// output).
    #[arg(long)]
    #[serde(default)]
    max_tokens: Option<u64>,
    /// End the run before its next model call once it has cost this many USD, at the model's `prices` in
    /// ~/.glut/settings.json (without them a run has no budget in money, and says so).
    #[arg(long)]
    #[serde(default)]
    max_usd: Option<f64>,
    /// Never compact the conversation automatically (`"compact": {"auto": false}` in ~/.glut/settings.json).
    #[arg(long)]
    #[serde(default)]
    no_auto_compact: bool,
    /// Send what the tools answer as it is, without hiding the keys in it (T58; `"mask": false` in
    /// ~/.glut/secrets.json, which `/secrets off` writes).
    #[arg(long)]
    #[serde(default)]
    no_mask: bool,
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
    /// A folder of skills beside `~/.glut/skills` (HS6, K2); repeatable.
    #[arg(long)]
    skill_dir: Vec<PathBuf>,
    /// A folder the run works in beside --cwd (T60c), the owner's word alone: the file tools reach it by its full
    /// path, commands write there, the snapshots record it. Refused: /, the home folder, one holding --cwd or a
    /// secret; repeatable.
    #[arg(long)]
    #[serde(default)]
    folder: Vec<String>,
    /// The run's MCP servers from this file alone (`{"mcpServers": …}`, Claude Code's shape and flag), instead of
    /// ~/.glut/mcp.json and the project's .mcp.json.
    #[arg(long)]
    #[serde(default)]
    mcp_config: Option<PathBuf>,
    /// Work as Glut's terminal session does in a project (TUI T35): its guides, the map of where plans, to-dos and
    /// the memory are kept, and the `memory` tool. Off: the run knows only what its flags say.
    #[arg(long)]
    #[serde(default)]
    project: bool,
    /// An image the prompt carries (T45: PNG, JPEG, GIF or WebP, scaled before it is sent); repeatable.
    #[arg(long)]
    #[serde(default)]
    image: Vec<PathBuf>,
    /// The prompt — extra text beside `--skill`, or a slash command (`/review src/`), read from
    /// `~/.glut/commands/` and, in a trusted folder, the project's `.glut/commands/` (HS4). Without `--skill` a
    /// prompt is required; with one it defaults to "Run the skill.".
    prompt: Option<String>,
}

/// Runs one of the headless commands; its logs go to stderr, so stdout stays machine-readable.
pub async fn dispatch(command: Command) -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
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
        Command::Run(args) => execute(*args, None).await,
        Command::Resume { run, list, discard } => resume_command(run, list, discard).await,
        Command::Gain { days } => gain(days),
        Command::Providers(command) => providers::run(command).await,
        Command::Models(command) => models::run(command).await,
        Command::Mcp(command) => mcp_servers::run(command).await,
        Command::Keychain(command) => keychain::run(command),
    }
}

/// The count of savings in the harness's home (`$GLUT_HOME` or `$HOME`, as the app's runs find it).
fn savings_file() -> Option<PathBuf> {
    crate::permissions::settings::home_dir()
        .ok()
        .map(|home| home.join(".glut").join(crate::lean::FILE))
}

/// `glut gain`: the count of savings as a table.
fn gain(days: Option<u32>) -> ExitCode {
    let fail = |message: String| {
        eprintln!("glut gain: {message}");
        ExitCode::from(1)
    };
    let Some(path) = savings_file() else {
        return fail(say!(gain_no_home).to_owned());
    };
    match crate::lean::report(&path, days) {
        Ok(table) => {
            print!("{table}");
            ExitCode::SUCCESS
        }
        Err(err) => fail(say!(gain_unreadable, err = err)),
    }
}

/// A run's context window: the flag, else the window the owner set for the model in Glut (`/context`, T30), else the
/// owner's `contextWindow`, else the provider's list; none, the run asks the provider when it needs one (#111, K2;
/// PV2).
fn window_for(
    flag: Option<u32>,
    set: Option<u32>,
    settings: Option<u32>,
    listed: Option<u32>,
) -> Option<u32> {
    flag.or(set).or(settings).or(listed)
}

/// What a provider run starts from: its id (the journal keeps it) and its model list, where there is one.
#[derive(Debug)]
struct FromProvider {
    id: String,
    /// The wire protocol the provider speaks: which client the run talks to it with (PV3).
    protocol: crate::providers::catalog::Protocol,
    list: Option<crate::providers::models::ModelList>,
    /// The windows the owner set for the provider's models (T30).
    windows: std::collections::BTreeMap<String, u32>,
    /// A signed-in subscription's credential (PV4): the run's bearer token at every call.
    credential: Option<crate::subscription::Credential>,
}

/// The run's endpoint: a one-off `--base-url` with `GLUT_API_KEY`, else a provider of `~/.glut/providers.json` — then
/// with the provider's id, so the journal keeps the provider and model the run began with, not "the default", and
/// its model list (PV2), which gives the run the model's window and prices where the owner's settings name none.
async fn endpoint(
    provider: Option<&str>,
    base_url: Option<String>,
    model: Option<String>,
) -> Result<(ModelEndpoint, Option<FromProvider>), String> {
    if let Some(base_url) = base_url {
        // The key goes in a header, never in the URL — and a journal keeps the URL as it was given.
        if has_userinfo(&base_url) {
            return Err(
                "--base-url must not hold a user or a password; the key comes from GLUT_API_KEY"
                    .into(),
            );
        }
        let model = model.ok_or("--base-url needs --model")?;
        return Ok((
            ModelEndpoint {
                base_url,
                api_key: std::env::var(KEY_ENV).ok().filter(|key| !key.is_empty()),
                model,
                effort: None,
                versioned: false,
            },
            None,
        ));
    }
    let home = crate::permissions::settings::home_dir().map_err(|err| err.to_string())?;
    let providers_file = crate::providers::Providers::load(&home).map_err(|err| err.to_string())?;
    // The store is let go before the model list is fetched: it is no thing to hold across a wait.
    let resolved = {
        let store = providers_file.store(&home).map_err(|err| err.to_string())?;
        crate::providers::resolve(
            &providers_file,
            store.as_ref(),
            provider,
            model.as_deref(),
            |name| std::env::var(name).ok(),
        )?
    };
    eprintln!(
        "glut: {}",
        say!(
            run_provider,
            provider = resolved.id,
            model = resolved.endpoint.model
        )
    );
    // A subscription's list is asked with the sign-in's token of the moment; the run's own calls ask theirs.
    let credential = resolved.credential(&home);
    let mut listing = resolved.endpoint.clone();
    if let Some(credential) = &credential {
        listing.api_key = Some(credential.bearer().await.map_err(|err| err.to_string())?);
    }
    let list =
        match crate::providers::models::current(&home, &resolved.id, &listing, resolved.protocol)
            .await
        {
            Ok(list) => {
                if list.get(&resolved.endpoint.model).is_none() {
                    eprintln!(
                        "glut: {}",
                        say!(
                            model_unlisted,
                            model = resolved.endpoint.model,
                            provider = resolved.id
                        )
                    );
                }
                Some(list)
            }
            Err(reason) => {
                eprintln!(
                    "glut: {}",
                    say!(no_model_list, provider = resolved.id, reason = reason)
                );
                None
            }
        };
    // A gateway's model goes to the part of it that speaks its API.
    let (routed, protocol) =
        crate::providers::route(resolved.endpoint, resolved.protocol, list.as_ref())?;
    Ok((
        routed,
        Some(FromProvider {
            windows: providers_file
                .providers
                .get(&resolved.id)
                .map(|entry| entry.windows.clone())
                .unwrap_or_default(),
            id: resolved.id,
            protocol,
            list,
            credential,
        }),
    ))
}

/// What builds a named agent's own model in `glut run` (T37a).
struct CliModels {
    /// The run's own `--base-url` endpoint and protocol, when it has no provider.
    one_off: Option<(ModelEndpoint, crate::providers::catalog::Protocol)>,
    provider: Option<String>,
    /// The run's model, for an agent that names only an effort.
    model: String,
    /// The owner's prices by model id.
    prices: std::collections::HashMap<String, crate::config::Prices>,
    /// The owner's effort, for an agent that names none.
    effort: Option<crate::Effort>,
}

impl crate::agents::ModelSource for CliModels {
    fn model<'a>(
        &'a self,
        provider: Option<&'a str>,
        model: Option<&'a str>,
        effort: Option<crate::Effort>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<crate::agents::Picked, String>> + Send + 'a>,
    > {
        Box::pin(async move {
            let model = match (provider, model) {
                (None, None) => Some(self.model.as_str()),
                (_, model) => model,
            };
            let (mut endpoint, protocol, listed, window, credential) =
                match (provider, &self.one_off) {
                    (None, Some((endpoint, protocol))) => {
                        let mut endpoint = endpoint.clone();
                        if let Some(model) = model {
                            model.clone_into(&mut endpoint.model);
                        }
                        (endpoint, *protocol, None, None, None)
                    }
                    _ => {
                        let id = provider.or(self.provider.as_deref());
                        let (endpoint, from) = endpoint(id, None, model.map(str::to_owned)).await?;
                        let from = from.ok_or("no provider to build the agent's model with")?;
                        let info = from
                            .list
                            .as_ref()
                            .and_then(|list| list.get(&endpoint.model));
                        let listed = info.and_then(|info| info.prices);
                        let window = from
                            .windows
                            .get(&endpoint.model)
                            .copied()
                            .or_else(|| info.and_then(|info| info.context_window));
                        (endpoint, from.protocol, listed, window, from.credential)
                    }
                };
            endpoint.effort = effort.or(self.effort);
            let prices = self.prices.get(&endpoint.model).copied().or(listed);
            let model =
                Model::signed(endpoint, protocol, credential).map_err(|err| err.to_string())?;
            Ok(crate::agents::Picked {
                model: Arc::new(model),
                prices,
                window,
            })
        })
    }
}

/// Whether `url` names a user or a password before its host (`https://user:pw@host`).
fn has_userinfo(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.split(['/', '?', '#'])
        .next()
        .is_some_and(|authority| authority.contains('@'))
}

/// Where `glut` keeps the journals of its own runs (durable runs, D5) — the app keeps its own elsewhere.
fn journal_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".glut").join("runs"))
}

/// `glut resume`: lists the unfinished runs, gives one up, or goes on with one — built from the flags it began with.
async fn resume_command(run: Option<String>, list: bool, discard: bool) -> ExitCode {
    use crate::journal;
    let fail = |message: String| {
        eprintln!("glut resume: {message}");
        ExitCode::from(1)
    };
    let Some(dir) = journal_dir() else {
        return fail(say!(home_unset).into());
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
                format!("  {}", say!(resume_running))
            } else {
                String::new()
            };
            println!("{}  {}  {what}{state}", entry.run, entry.started_at);
        }
        return ExitCode::SUCCESS;
    }
    // Named, never guessed: a resume runs with the flags the journal holds, so the owner picks which.
    let Some(run) = run else {
        return fail(say!(resume_name_run).into());
    };
    let Some(entry) = unfinished.iter().find(|entry| entry.run == run) else {
        return fail(say!(resume_no_run, run = run));
    };
    if discard {
        return match journal::discard(&dir, &entry.run) {
            Ok(()) => {
                println!("{}", say!(resume_given_up, run = entry.run));
                ExitCode::SUCCESS
            }
            Err(reason) => fail(reason),
        };
    }
    let args = match serde_json::from_value::<RunArgs>(entry.meta["args"].clone()) {
        Ok(args) => args,
        Err(err) => {
            return fail(say!(resume_not_glut_run, run = entry.run, err = err));
        }
    };
    // What the run goes on with, said before it does: the flags come from the journal, not from this command line.
    let flags = format!(
        "{} --model {} --effort {} --cwd {} --permission-mode {} --allow {:?} --deny {:?}",
        match (&args.base_url, &args.provider) {
            (Some(base_url), _) => format!("--base-url {base_url}"),
            (None, Some(provider)) => format!("--provider {provider}"),
            (None, None) => say!(resume_default_provider).to_owned(),
        },
        args.model
            .as_deref()
            .unwrap_or(say!(resume_providers_model)),
        args.effort.as_deref().unwrap_or(say!(resume_from_settings)),
        args.cwd.display(),
        args.permission_mode
            .as_deref()
            .unwrap_or(say!(resume_from_settings)),
        args.allow,
        args.deny
    );
    eprintln!(
        "glut resume: {}",
        say!(resume_with, run = entry.run, flags = flags)
    );
    execute(args, Some(entry.run.clone())).await
}

/// One run of `glut run` — or of `glut resume`, then `resumed` names the run whose journal it goes on from.
async fn execute(args: RunArgs, resumed: Option<String>) -> ExitCode {
    let journal_args = args.clone();
    let RunArgs {
        provider,
        base_url,
        model,
        effort,
        cwd,
        instructions_file,
        max_turns,
        timeout_secs,
        context_window,
        no_auto_compact,
        no_mask,
        max_tokens,
        max_usd,
        permission_mode,
        allow,
        ask,
        deny,
        skill,
        skill_dir,
        folder,
        mcp_config,
        project,
        image,
        prompt,
    } = args;

    let fail = |message: String| {
        JsonLines.emit(Event::Error { message });
        ExitCode::from(1)
    };
    if prompt.is_none() && skill.is_none() {
        return fail("name a prompt, or run a skill with --skill <name>".into());
    }
    // A relative `--folder` is below where glut was started, as `--cwd` is; kept so, a resume finds the same folder.
    let folder: Vec<String> = folder
        .into_iter()
        .map(|typed| match std::env::current_dir() {
            Ok(here) if !typed.starts_with(['~', '/']) => here.join(&typed).display().to_string(),
            _ => typed,
        })
        .collect();
    let (endpoint, provider) = match endpoint(provider.as_deref(), base_url, model).await {
        Ok(found) => found,
        Err(reason) => return fail(reason),
    };
    // A one-off `--base-url` speaks chat completions; a provider its own protocol.
    let protocol = provider
        .as_ref()
        .map_or_else(Default::default, |from| from.protocol);
    let effort = match effort.as_deref().map(Effort::parse_or_explain).transpose() {
        Ok(effort) => effort,
        Err(reason) => return fail(format!("--effort: {reason}")),
    };
    let cwd = match cwd.canonicalize() {
        Ok(cwd) => cwd,
        Err(err) => return fail(format!("cannot use {}: {err}", cwd.display())),
    };
    let mut config = RunConfig::new(&cwd);
    // Durable runs (D5): every run journals its steps; one the process lost goes on with `glut resume`.
    if let Some(dir) = journal_dir() {
        let run = resumed.clone().unwrap_or_else(|| {
            format!(
                "glut-{}-{}",
                std::process::id(),
                chrono::Local::now().timestamp_millis()
            )
        });
        if resumed.is_none() {
            eprintln!("glut: {}", say!(run_started, run = run));
        }
        let mut kept = journal_args;
        kept.cwd = cwd.clone();
        kept.folder.clone_from(&folder);
        // A resume goes on with the provider and model the run began with, whatever the default is by then.
        if let Some(from) = &provider {
            kept.provider = Some(from.id.clone());
            kept.model = Some(endpoint.model.clone());
        }
        config.journal = Some(crate::journal::JournalSpec {
            dir,
            run,
            meta: serde_json::json!({ "args": kept }),
        });
    }
    // Glut 2 (D): snapshots before changes, for `glut undo`.
    config.snapshots = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".glut").join("snapshots"));
    // Glut lean (N9): the count of savings, where `glut gain` and the app's runs keep it.
    config.savings = savings_file();
    // The folders the run reads skills from, beside the owner's and the project's (HS6, K2).
    config.skill_dirs = skill_dir;
    // T45: images are kept in the owner's `~/.glut/images` — the prompt's, and those `read_file` shows the model.
    match crate::permissions::settings::home_dir() {
        Ok(home) => {
            for (at, path) in image.iter().enumerate() {
                let number = u32::try_from(at + 1).unwrap_or(u32::MAX);
                match crate::images::read_file(path)
                    .and_then(|bytes| crate::images::keep(&home, &bytes, number))
                {
                    Ok(kept) => config.prompt_images.push(kept),
                    Err(reason) => return fail(format!("--image {}: {reason}", path.display())),
                }
            }
            config.images_home = Some(home);
        }
        Err(err) if !image.is_empty() => {
            return fail(format!("--image: no home to keep images in ({err})"));
        }
        Err(_) => {}
    }
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
    let endpoint = ModelEndpoint { effort, ..endpoint };
    let mode = match permission_mode
        .as_deref()
        .map(|text| (text, Mode::parse(text)))
    {
        None => None,
        Some((_, Some(mode))) => Some(mode),
        Some((text, None)) => return fail(format!("unknown permission mode {text:?}")),
    };
    // Without a home there is no owner's settings file: a relative `.glut/` would be the project's, read as the owner's.
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
    else {
        return fail("HOME is not set to an absolute folder".into());
    };
    // T58: the owner's own formats and values let through; the run's key is found exactly wherever it shows.
    config.masks = match crate::masks::Owner::load(&home) {
        Ok(owner) if owner.masks() && !no_mask => {
            let masks = crate::masks::Masks::new(&owner);
            if let Some(key) = &endpoint.api_key {
                masks.know("the model's key", key);
            }
            Some(masks)
        }
        Ok(_) => None,
        Err(err) => return fail(err.to_string()),
    };
    let trusted = match Trust::load(&home) {
        Ok(trust) => trust.contains(&cwd),
        Err(err) => return fail(err.to_string()),
    };
    // T35: the project's guides and map before the caller's instructions, the memory's folder for its tool.
    let memory = project.then(|| {
        let (guides, notes) = crate::guide::read(&home, &cwd);
        for note in notes {
            eprintln!("glut: {note}");
        }
        let found = crate::project::look(
            &home,
            &cwd,
            &guides
                .iter()
                .map(|guide| guide.path.clone())
                .collect::<Vec<_>>(),
        );
        let mut text = found.prompt();
        if let Some(guides) = crate::guide::instructions(&guides, &home) {
            text = format!("{text}\n\n{guides}");
        }
        if let Some(own) = config.instructions.take() {
            text = format!("{text}\n\n{own}");
        }
        config.instructions = Some(text);
        found.memory
    });
    // Skills (HS6, K1/K2): the owner's `~/.glut/skills`, the folders of `--skill-dir` and — in a folder the owner
    // trusted once, the same trust the project's allow rules need — the project's `.glut/skills`. A skill that
    // cannot be used is a note on stderr, never an error (K3).
    let (catalog, notes) = skills::Catalog::load(&home, &cwd, trusted, &config.skill_dirs);
    for note in notes {
        eprintln!("glut: {}", say!(skill_left_out, note = note));
    }
    // T37a: the named subagents, the owner's and — in a trusted folder — the project's.
    let (agents, notes) = crate::agents::Catalog::load(&home, &cwd, trusted);
    for note in notes {
        eprintln!("glut: {note}");
    }
    config.agents = Arc::new(agents);
    // T37c: Glut's own maker skills, where no skill of the owner's, the project's or a folder's has their name.
    let catalog = Arc::new(catalog.with_glut_own());
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
        eprintln!("glut: {note}");
    }
    // HS7: the hooks run with what the settings say about the sandbox, like the shell's own commands (D5).
    // #111, K2/K6: the flags over the owner's settings; without a window the run asks the provider's list.
    config.compaction = built.compaction(&endpoint.model);
    // PV2: what the provider's list knows, where the owner's settings say nothing.
    let listed = |model: &str| {
        provider
            .as_ref()
            .and_then(|from| from.list.as_ref())
            .and_then(|list| list.get(model))
            .cloned()
    };
    config.hooks = built.hooks;
    config.sandbox = Some(built.sandbox.clone());
    config.refusals = built.refusals;
    config.nudge = built.nudge;
    config.remind = built.nudge;
    // The owner's `maxTurns` (T73b) where `--max-turns` named none.
    if max_turns.is_none()
        && let Some(turns) = built.max_turns
    {
        config.max_turns = turns;
    }
    config.parallel_subagents = built.parallel_subagents;
    if let Some(window) = context_window {
        if window < crate::config::MIN_CONTEXT_WINDOW {
            return fail(format!(
                "--context-window {window}: at least {}",
                crate::config::MIN_CONTEXT_WINDOW
            ));
        }
        config.compaction.context_window = Some(window);
    }
    if no_auto_compact {
        config.compaction.auto = false;
    }

    // Slash commands (HS4), with the same home, folder and trust the gate was built with (C1).
    let mut registry = match Registry::load(&home, &cwd, trusted) {
        Ok(registry) => registry,
        Err(err) => return fail(err.to_string()),
    };
    registry.set_permissions(PermissionsView::of(&built.gate, &built.sandbox));
    // The flag first, else the owner's settings; a skill's or a command's `effort:` still wins in `prepare`.
    let mut endpoint = endpoint;
    endpoint.effort = endpoint.effort.or(built.effort);
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
        mut config,
        endpoint,
        gate,
    } = draft;
    // The window of the model the run ends up with — a skill or a command may name its own (C2, K5).
    config.compaction.context_window = window_for(
        context_window,
        provider
            .as_ref()
            .and_then(|from| from.windows.get(&endpoint.model).copied()),
        built.context_windows.get(&endpoint.model).copied(),
        listed(&endpoint.model).and_then(|info| info.context_window),
    );
    // C6 of docs/plans/glut-cards.md: the budget at the prices of the model the run ends up with (a skill or a
    // command may name its own).
    config.budget = crate::Budget {
        max_tokens,
        max_usd,
        prices: built
            .prices
            .get(&endpoint.model)
            .copied()
            .or_else(|| listed(&endpoint.model).and_then(|info| info.prices)),
        paid_usd: 0.0,
    };
    if max_usd.is_some() && config.budget.prices.is_none() {
        eprintln!("glut: {}", say!(max_usd_no_prices, model = endpoint.model));
    }
    // A named agent's own model (T37a): built as the run's is — a provider of `providers.json`, or the run's one-off
    // endpoint with another model id.
    config.models = Some(crate::agents::Models(Arc::new(CliModels {
        one_off: provider.is_none().then(|| (endpoint.clone(), protocol)),
        provider: provider.as_ref().map(|from| from.id.clone()),
        model: endpoint.model.clone(),
        prices: built.prices.clone(),
        effort: built.effort,
    })));
    // The model is built after the command and the skill: either may name its own (C2, K5).
    let credential = provider.as_ref().and_then(|from| from.credential.clone());
    let model = match Model::signed(endpoint, protocol, credential) {
        Ok(model) => model,
        Err(err) => return fail(err.to_string()),
    };
    // MCP servers (HS5, M1–M3). `glut` has no terminal, so a project's server nobody approved is left out and said
    // so on stderr rather than asked about (M2).
    // A named file stands for the two places; whoever names it stands for what it starts.
    let servers = match &mcp_config {
        Some(file) => mcp::config::named(file),
        None => mcp::config::load(&home, &cwd)
            .and_then(|entries| mcp::approval::permitted(entries, &home, &cwd, false)),
    };
    let servers = match servers {
        Ok(servers) => servers,
        Err(err) => return fail(err.to_string()),
    };
    let mut beside = Vec::new();
    for typed in &folder {
        match crate::folders::check(typed, &cwd, &home, &built.sandbox.deny_read) {
            Ok(checked) => beside.push(checked),
            Err(reason) => return fail(format!("--folder {reason}")),
        }
    }
    let mut tools = match setup::toolbox(
        &cwd,
        &catalog,
        gate,
        servers,
        built.sandbox,
        setup::Extras {
            search: Some(env_search()),
            direct: Vec::new(),
            memory,
            glut_files: Some(crate::tools::glut_files::GlutFiles::new(
                &home, &cwd, trusted,
            )),
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
        tools.withdraw(crate::skills::tool::NAME);
    }
    for checked in beside {
        if let Err(crate::folders::NotAdded::Overlaps(other)) = tools.folders().add(checked) {
            return fail(format!(
                "--folder: one lies in the other or holds it: {other}"
            ));
        }
    }

    // Ctrl-C, SIGTERM and SIGHUP end the run cleanly, with its `finished` line and every background job stopped.
    let cancel = Cancel::new();
    crate::run::cancel_on_signals(&cancel);

    let outcome = match &resumed {
        None => run(&config, &model, &tools, &prompt, &mut sink, &cancel).await,
        Some(id) => match crate::resume(&config, &model, &tools, id, &mut sink, &cancel).await {
            Ok((outcome, _)) => outcome,
            Err(reason) => return fail(reason),
        },
    };
    ExitCode::from(match outcome.reason {
        FinishReason::Answered => 0,
        FinishReason::Failed | FinishReason::Refused => 1,
        FinishReason::MaxTurns
        | FinishReason::Timeout
        | FinishReason::Cancelled
        | FinishReason::Budget => 2,
    })
}

/// `glut undo`: lists a folder's runs with snapshots, or puts the folder back before a run or one of its steps.
fn undo(cwd: &std::path::Path, run: Option<String>, step: Option<u32>, list: bool) -> ExitCode {
    use crate::snapshot;
    let fail = |message: String| {
        eprintln!("glut undo: {message}");
        ExitCode::from(1)
    };
    let Some(root) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .map(|home| home.join(".glut").join("snapshots"))
    else {
        return fail(say!(home_unset).into());
    };
    let cwd = match cwd.canonicalize() {
        Ok(cwd) => cwd,
        Err(err) => return fail(say!(cannot_use, path = cwd.display(), err = err)),
    };
    let runs = snapshot::runs(&root, &cwd);
    if list {
        for record in &runs {
            let steps: Vec<String> = record.steps.iter().map(|s| s.turn.to_string()).collect();
            println!(
                "{}",
                say!(undo_steps, run = record.run, steps = steps.join(", "))
            );
        }
        return ExitCode::SUCCESS;
    }
    let Some(run) = run.or_else(|| runs.first().map(|record| record.run.clone())) else {
        return fail(say!(undo_no_snapshots, path = cwd.display()));
    };
    match snapshot::restore_all(&root, &cwd, &run, step) {
        Ok((commit, missed)) => {
            match step {
                Some(step) => println!(
                    "{}",
                    say!(
                        undo_back_step,
                        path = cwd.display(),
                        step = step,
                        run = run,
                        commit = format!("{commit:.8}")
                    )
                ),
                None => println!(
                    "{}",
                    say!(
                        undo_back,
                        path = cwd.display(),
                        run = run,
                        commit = format!("{commit:.8}")
                    )
                ),
            }
            for what in &missed {
                eprintln!("glut undo: {}", say!(undo_beside_missed, what = what));
            }
            if missed.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(reason) => fail(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_is_the_flags_then_the_one_set_in_glut_then_the_owners_then_the_lists() {
        let all = window_for(Some(8_192), Some(16_384), Some(32_768), Some(131_072));
        assert_eq!(all, Some(8_192));
        let set = window_for(None, Some(16_384), Some(32_768), Some(131_072));
        assert_eq!(set, Some(16_384));
        assert_eq!(
            window_for(None, None, Some(32_768), Some(131_072)),
            Some(32_768)
        );
        assert_eq!(window_for(None, None, None, Some(131_072)), Some(131_072));
        assert_eq!(window_for(None, None, None, None), None);
    }

    #[test]
    fn a_journal_of_a_provider_run_reads_without_a_base_url() {
        let args: RunArgs = serde_json::from_value(serde_json::json!({
            "provider": "openrouter", "model": "m", "cwd": "/w", "instructions_file": null,
            "max_turns": null, "timeout_secs": null, "permission_mode": null, "allow": [], "ask": [],
            "deny": [], "skill": null, "skill_dir": [], "prompt": "hi"
        }))
        .unwrap();
        assert_eq!(args.provider.as_deref(), Some("openrouter"));
        assert_eq!(args.base_url, None);
    }

    #[tokio::test]
    async fn a_base_url_needs_a_model_and_no_password() {
        assert!(
            endpoint(None, Some("https://x.test/api".into()), None)
                .await
                .unwrap_err()
                .contains("--model")
        );
        assert!(
            endpoint(
                None,
                Some("https://u:p@x.test/api".into()),
                Some("m".into())
            )
            .await
            .unwrap_err()
            .contains("password")
        );
        let (one_off, provider) =
            endpoint(None, Some("https://x.test/api".into()), Some("m".into()))
                .await
                .unwrap();
        assert!(!one_off.versioned);
        assert!(provider.is_none());
    }

    /// A journal written before `--effort` existed still resumes, with no effort of its own.
    #[test]
    fn a_journaled_run_without_an_effort_still_reads() {
        let args: RunArgs = serde_json::from_value(serde_json::json!({
            "base_url": "https://openrouter.ai/api", "model": "m", "cwd": "/w", "instructions_file": null,
            "max_turns": null, "timeout_secs": null, "permission_mode": null, "allow": [], "ask": [],
            "deny": [], "skill": null, "skill_dir": [], "prompt": "hi"
        }))
        .unwrap();
        assert_eq!(args.effort, None);
        assert_eq!((args.context_window, args.no_auto_compact), (None, false));
    }
}
