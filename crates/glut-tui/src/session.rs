//! One Glut session in a folder (TUI1): what `axh run` builds once per run — gate, tools, settings — built once and
//! kept, so every message of the session is a turn that goes on from the conversation before it. The model is the
//! session's [`Engine`], which `/model`, `/provider` and `/effort` replace between turns.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use glut::compact::Compacted;
use glut::config::ModelEndpoint;
use glut::permissions::settings::Trust;
use glut::permissions::{Answer, AnswerFuture, AskRequest, Asker, GateOptions, Mode, build_gate};
use glut::providers::{self, Providers, models};
use glut::{
    Cancel, Effort, Event, EventSink, FinishReason, Message, OpenAiCompat, Prices, RunConfig,
    RunOutcome, SharedSink, Toolbox, Usage, mcp, setup, skills,
};
use tokio::sync::{mpsc, oneshot};

/// What the session reports to the screen.
pub enum Report {
    /// A step of the turn under way.
    Event(Event),
    /// A question of the gate, waiting for the owner's answer.
    Ask(AskRequest, oneshot::Sender<Answer>),
    /// The turn ended; the conversation goes on from `conversation`.
    Done(RunOutcome, Vec<Message>),
    /// `/compact` ended: the shorter conversation, or why there is none.
    Compacted(Result<Compacted, String>, Vec<Message>),
    /// `/model`, `/provider` or `/effort` built the new engine, with what to say about it — or why not.
    Switched(Result<(Engine, Vec<String>), String>),
}

/// Sends the run's events to the screen.
struct ToScreen(mpsc::UnboundedSender<Report>);

impl EventSink for ToScreen {
    fn emit(&mut self, event: Event) {
        let _ = self.0.send(Report::Event(event));
    }
}

/// Asks the owner on the screen: the question goes to the app, the answer comes back; a screen that is gone refuses.
struct OnScreen(mpsc::UnboundedSender<Report>);

impl Asker for OnScreen {
    fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a> {
        let (reply, answer) = oneshot::channel();
        let sent = self.0.send(Report::Ask(request.clone(), reply)).is_ok();
        Box::pin(async move {
            if !sent {
                return Answer::Deny;
            }
            answer.await.unwrap_or(Answer::Deny)
        })
    }
}

/// How the session was asked to start.
pub struct Start {
    pub cwd: PathBuf,
    pub provider: Option<String>,
    pub model: Option<String>,
}

/// What stays for the whole session: the tools with their gate, the run's settings, what the owner's files say.
pub struct Parts {
    pub home: PathBuf,
    /// The run's settings without the model's window, which the engine brings.
    pub config: RunConfig,
    pub tools: Toolbox,
    pub sink: SharedSink,
    /// The owner's `contextWindow`, `prices` and `effort` of `settings.json`.
    owner_windows: HashMap<String, u32>,
    owner_prices: HashMap<String, Prices>,
    pub owner_effort: Option<Effort>,
    /// Said once at the start: skills or servers left out.
    pub notes: Vec<String>,
}

/// The model the session talks to now.
#[derive(Clone)]
pub struct Engine {
    pub provider: String,
    pub endpoint: ModelEndpoint,
    pub model: Arc<OpenAiCompat>,
    pub prices: Option<Prices>,
    /// The window Glut counts with, and where it comes from (`set in Glut`, `settings`, `the provider's list`).
    pub window: Option<(u32, &'static str)>,
}

impl Engine {
    pub fn model_name(&self) -> &str {
        &self.endpoint.model
    }

    /// The run's settings for a turn on this engine.
    pub fn config(&self, parts: &Parts) -> RunConfig {
        let mut config = parts.config.clone();
        config.compaction.context_window = self.window.map(|(tokens, _)| tokens);
        config
    }
}

/// Builds the session: the gate with the screen as the one who answers, the tools, and the engine of the provider
/// and model asked for.
///
/// # Errors
///
/// A message when there is no provider to use, the folder cannot be used, or the settings are broken.
pub async fn build(
    start: Start,
    reports: mpsc::UnboundedSender<Report>,
) -> Result<(Parts, Engine), String> {
    let home = glut::permissions::settings::home_dir().map_err(|err| err.to_string())?;
    let cwd = start
        .cwd
        .canonicalize()
        .map_err(|err| format!("cannot use {}: {err}", start.cwd.display()))?;
    let mut notes = Vec::new();

    let trusted = Trust::load(&home)
        .map_err(|err| err.to_string())?
        .contains(&cwd);
    let options = GateOptions {
        mode: None,
        allow: Vec::new(),
        ask: Vec::new(),
        deny: Vec::new(),
        trusted,
    };
    let built = build_gate(&home, &cwd, options, Arc::new(OnScreen(reports.clone())))
        .map_err(|err| err.to_string())?;
    notes.extend(built.notes().into_iter().map(str::to_owned));

    let mut config = RunConfig::new(&cwd);
    config.compaction.auto = built.compact_auto;
    config.hooks = built.hooks.clone();
    config.sandbox = Some(built.sandbox.clone());
    config.refusals = built.refusals;
    config.snapshots = Some(home.join(".axh").join("snapshots"));
    config.savings = Some(home.join(".axh").join(glut::lean::FILE));

    let (catalog, skill_notes) = skills::Catalog::load(&home, &cwd, trusted, &config.skill_dirs);
    notes.extend(
        skill_notes
            .into_iter()
            .map(|note| format!("skill left out: {note}")),
    );
    let catalog = Arc::new(catalog);
    // Nobody can be asked about a project's new server before the screen is up: it is left out and said so.
    let servers = mcp::config::load(&home, &cwd)
        .and_then(|entries| mcp::approval::permitted(entries, &home, &cwd, false))
        .map_err(|err| err.to_string())?;
    let mut sink = SharedSink::new(Box::new(ToScreen(reports)));
    let tools = setup::toolbox(
        &cwd,
        &catalog,
        built.gate,
        servers,
        built.sandbox,
        setup::Extras {
            search: Some(web_search()),
            direct: Vec::new(),
        },
        &mut sink,
    )
    .await
    .map_err(|err| err.to_string())?;
    let parts = Parts {
        home,
        config,
        tools,
        sink,
        owner_windows: built.context_windows,
        owner_prices: built.prices,
        owner_effort: built.effort,
        notes,
    };
    let (engine, engine_notes) = engine(
        &parts,
        start.provider.as_deref(),
        start.model.as_deref(),
        None,
    )
    .await?;
    let mut parts = parts;
    parts.notes.extend(engine_notes);
    Ok((parts, engine))
}

/// The engine of provider `id` (or the default one) and `model` (or the provider's default), at `effort` — `None`
/// takes the owner's `settings.json`, `Some(None)` sends none and leaves it to the provider: its key, its window and
/// prices — the window set in Glut first, then the owner's settings, then the list.
///
/// # Errors
///
/// A message when the provider is unknown, has no model, or needs a key that is nowhere.
pub async fn engine(
    parts: &Parts,
    id: Option<&str>,
    model: Option<&str>,
    effort: Option<Option<Effort>>,
) -> Result<(Engine, Vec<String>), String> {
    let mut notes = Vec::new();
    let list = Providers::load(&parts.home).map_err(|err| err.to_string())?;
    // The store is let go before the model list is fetched: it is no thing to hold across a wait.
    let resolved = {
        let store = list.store(&parts.home).map_err(|err| err.to_string())?;
        providers::resolve(&list, store.as_ref(), id, model, |name| {
            std::env::var(name).ok()
        })?
    };
    let models = match models::current(&parts.home, &resolved.id, &resolved.endpoint).await {
        Ok(models) => Some(models),
        Err(reason) => {
            notes.push(format!("no model list of {}: {reason}", resolved.id));
            None
        }
    };
    let name = resolved.endpoint.model.clone();
    let listed = models.as_ref().and_then(|list| list.get(&name)).cloned();
    if models.is_some() && listed.is_none() {
        notes.push(format!("{name} is not in {}'s model list", resolved.id));
    }
    let set = list
        .providers
        .get(&resolved.id)
        .and_then(|entry| entry.windows.get(&name).copied());
    let window = set
        .map(|tokens| (tokens, "set in Glut"))
        .or_else(|| {
            parts
                .owner_windows
                .get(&name)
                .map(|tokens| (*tokens, "settings.json"))
        })
        .or_else(|| {
            listed
                .as_ref()
                .and_then(|info| info.context_window)
                .map(|tokens| (tokens, "the provider's list"))
        });
    let prices = parts
        .owner_prices
        .get(&name)
        .copied()
        .or_else(|| listed.as_ref().and_then(|info| info.prices));
    let mut endpoint = resolved.endpoint;
    endpoint.effort = effort.unwrap_or(parts.owner_effort);
    let model = OpenAiCompat::new(endpoint.clone()).map_err(|err| err.to_string())?;
    Ok((
        Engine {
            provider: resolved.id,
            endpoint,
            model: Arc::new(model),
            prices,
            window,
        },
        notes,
    ))
}

/// Keeps `tokens` as the window of the engine's model (`/context <tokens>`, T30), in `providers.json`.
///
/// # Errors
///
/// A message when the file cannot be read or written, or the window is below what a run needs.
pub fn keep_window(home: &Path, engine: &Engine, tokens: u32) -> Result<(), String> {
    let mut list = Providers::load(home).map_err(|err| err.to_string())?;
    let Some(entry) = list.providers.get_mut(&engine.provider) else {
        return Err(format!("no provider {} in providers.json", engine.provider));
    };
    entry.windows.insert(engine.model_name().to_owned(), tokens);
    entry.check()?;
    list.save(home).map_err(|err| err.to_string())
}

/// The web search with the providers' keys from the environment, as `axh` has it.
fn web_search() -> glut::tools::search::WebSearch {
    use glut::tools::search::{SearchKeys, Unlimited, WebSearch};
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

/// Runs one turn in the background: `prompt` after `conversation` on `engine`, every step to the screen, the end as
/// [`Report::Done`] — also when the turn panics, then with the conversation as it was before it.
pub fn turn(
    parts: Arc<Parts>,
    engine: Engine,
    conversation: Vec<Message>,
    prompt: String,
    cancel: Cancel,
    reports: mpsc::UnboundedSender<Report>,
) {
    let kept = conversation.clone();
    let run = tokio::spawn(async move {
        let mut sink = parts.sink.clone();
        let config = engine.config(&parts);
        glut::run_continuing(
            &config,
            engine.model.as_ref(),
            &parts.tools,
            conversation,
            &prompt,
            &mut sink,
            &cancel,
        )
        .await
    });
    tokio::spawn(async move {
        let done = match run.await {
            Ok((outcome, after)) => Report::Done(outcome, after),
            Err(err) => {
                let _ = reports.send(Report::Event(Event::Error {
                    message: format!("the turn broke off: {err}"),
                }));
                Report::Done(
                    RunOutcome {
                        reason: FinishReason::Failed,
                        answer: None,
                        turns: 0,
                        usage: Usage::default(),
                        refusals: 0,
                    },
                    kept,
                )
            }
        };
        let _ = reports.send(done);
    });
}

/// `/compact`: the conversation summarised in the background; the old one comes back with the result, kept when
/// the summary fails — or the task panics.
pub fn compact(engine: Engine, conversation: Vec<Message>, reports: mpsc::UnboundedSender<Report>) {
    let kept = conversation.clone();
    let summary = tokio::spawn(async move {
        glut::compact::compact(engine.model.as_ref(), &conversation)
            .await
            .map_err(|err| err.to_string())
    });
    tokio::spawn(async move {
        let result = summary
            .await
            .unwrap_or_else(|err| Err(format!("the summary broke off: {err}")));
        let _ = reports.send(Report::Compacted(result, kept));
    });
}

/// `/model`, `/provider`, `/effort`: the new engine built in the background, reported as [`Report::Switched`] —
/// also when the task panics.
pub fn switch(
    parts: Arc<Parts>,
    provider: Option<String>,
    model: Option<String>,
    effort: Option<Option<Effort>>,
    reports: mpsc::UnboundedSender<Report>,
) {
    let built =
        tokio::spawn(
            async move { engine(&parts, provider.as_deref(), model.as_deref(), effort).await },
        );
    tokio::spawn(async move {
        let result = built
            .await
            .unwrap_or_else(|err| Err(format!("the switch broke off: {err}")));
        let _ = reports.send(Report::Switched(result));
    });
}

/// Whether commands run in the sandbox: the settings ask for it and the system has one.
pub fn sandboxed(parts: &Parts) -> bool {
    parts
        .config
        .sandbox
        .as_ref()
        .is_some_and(|sandbox| sandbox.enabled)
        && glut::sandbox::available()
}

/// The mode after `mode` on Shift+Tab: plan → ask → edits → auto → plan. `dont_ask` (a headless mode) goes to plan.
pub fn next_mode(mode: Mode) -> Mode {
    match mode {
        Mode::Plan => Mode::Default,
        Mode::Default => Mode::AcceptEdits,
        Mode::AcceptEdits => Mode::Auto,
        Mode::Auto | Mode::DontAsk => Mode::Plan,
    }
}

/// A mode as the screen names it.
pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Plan => "plan",
        Mode::Default => "ask",
        Mode::AcceptEdits => "edits",
        Mode::Auto => "auto",
        Mode::DontAsk => "don't ask",
    }
}

/// The folder a session works in, as the header shows it: `~/…` below the home folder.
pub fn shown_folder(cwd: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    match home.as_deref().and_then(|home| cwd.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => cwd.display().to_string(),
    }
}

/// A session that talks to nothing, for the screen's tests.
#[cfg(test)]
pub fn for_tests() -> (Parts, Engine) {
    /// Swallows the events of a test session.
    struct Nowhere;

    impl EventSink for Nowhere {
        fn emit(&mut self, _event: Event) {}
    }

    let endpoint = ModelEndpoint {
        base_url: "http://localhost:9".into(),
        api_key: None,
        model: "m".into(),
        effort: None,
        versioned: true,
    };
    let parts = Parts {
        // A folder of its own per test session: a test that sets a window or an editor writes there, never in /tmp.
        home: std::env::temp_dir().join(format!(
            "glut-tui-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|at| at.as_nanos())
                .unwrap_or_default()
        )),
        config: RunConfig::new("/tmp"),
        tools: Toolbox::new(),
        sink: SharedSink::new(Box::new(Nowhere)),
        owner_windows: HashMap::new(),
        owner_prices: HashMap::new(),
        owner_effort: None,
        notes: Vec::new(),
    };
    let engine = Engine {
        provider: "test".into(),
        model: Arc::new(OpenAiCompat::new(endpoint.clone()).expect("a test endpoint")),
        endpoint,
        prices: None,
        window: Some((32_768, "settings.json")),
    };
    (parts, engine)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shift_tab_walks_every_interactive_mode_and_comes_back() {
        let mut mode = Mode::Plan;
        let mut seen = Vec::new();
        for _ in 0..4 {
            mode = next_mode(mode);
            seen.push(mode_label(mode));
        }
        assert_eq!(seen, ["ask", "edits", "auto", "plan"]);
        assert_eq!(next_mode(Mode::DontAsk), Mode::Plan);
    }
}
