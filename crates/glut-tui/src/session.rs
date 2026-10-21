//! One Glut session in a folder (TUI1a): what `axh run` builds once per run — provider, gate, tools, settings — built
//! once and kept, so every message of the session is a turn that goes on from the conversation before it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glut::permissions::settings::Trust;
use glut::permissions::{Answer, AnswerFuture, AskRequest, Asker, GateOptions, build_gate};
use glut::providers::{self, Providers, models};
use glut::{
    Cancel, Event, EventSink, Message, OpenAiCompat, Prices, RunConfig, RunOutcome, SharedSink,
    Toolbox, mcp, setup, skills,
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

/// The parts every turn uses.
pub struct Parts {
    pub config: RunConfig,
    pub model: OpenAiCompat,
    pub tools: Toolbox,
    pub sink: SharedSink,
    /// The provider's id and the model, for the header.
    pub provider: String,
    pub model_name: String,
    /// What the model costs, from the owner's settings or the provider's list.
    pub prices: Option<Prices>,
    /// Said once at the start: skills or servers left out, a model the list does not know.
    pub notes: Vec<String>,
}

/// Builds the session: the provider and its model, the gate with the screen as the one who answers, the tools.
///
/// # Errors
///
/// A message when there is no provider to use, the folder cannot be used, or the settings are broken.
pub async fn build(start: Start, reports: mpsc::UnboundedSender<Report>) -> Result<Parts, String> {
    let home = glut::permissions::settings::home_dir().map_err(|err| err.to_string())?;
    let cwd = start
        .cwd
        .canonicalize()
        .map_err(|err| format!("cannot use {}: {err}", start.cwd.display()))?;
    let mut notes = Vec::new();

    let list = Providers::load(&home).map_err(|err| err.to_string())?;
    let store = list.store(&home).map_err(|err| err.to_string())?;
    let resolved = providers::resolve(
        &list,
        store.as_ref(),
        start.provider.as_deref(),
        start.model.as_deref(),
        |name| std::env::var(name).ok(),
    )?;
    let models = match models::current(&home, &resolved.id, &resolved.endpoint).await {
        Ok(models) => Some(models),
        Err(reason) => {
            notes.push(format!("no model list of {}: {reason}", resolved.id));
            None
        }
    };
    let listed = models
        .as_ref()
        .and_then(|list| list.get(&resolved.endpoint.model))
        .cloned();
    if models.is_some() && listed.is_none() {
        notes.push(format!(
            "{} is not in {}'s model list",
            resolved.endpoint.model, resolved.id
        ));
    }

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
    let model_name = resolved.endpoint.model.clone();
    config.compaction = built.compaction(&model_name);
    if config.compaction.context_window.is_none() {
        config.compaction.context_window = listed.as_ref().and_then(|info| info.context_window);
    }
    config.hooks = built.hooks.clone();
    config.sandbox = Some(built.sandbox.clone());
    config.refusals = built.refusals;
    config.snapshots = Some(home.join(".axh").join("snapshots"));
    config.savings = Some(home.join(".axh").join(glut::lean::FILE));
    let prices = built
        .prices
        .get(&model_name)
        .copied()
        .or_else(|| listed.as_ref().and_then(|info| info.prices));

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

    let mut endpoint = resolved.endpoint;
    endpoint.effort = built.effort;
    let model = OpenAiCompat::new(endpoint).map_err(|err| err.to_string())?;
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
    Ok(Parts {
        config,
        model,
        tools,
        sink,
        provider: resolved.id,
        model_name,
        prices,
        notes,
    })
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

/// Runs one turn in the background: `prompt` after `conversation`, every step to the screen, the end as
/// [`Report::Done`].
pub fn turn(
    parts: Arc<Parts>,
    conversation: Vec<Message>,
    prompt: String,
    cancel: Cancel,
    reports: mpsc::UnboundedSender<Report>,
) {
    let kept = conversation.clone();
    let run = tokio::spawn(async move {
        let mut sink = parts.sink.clone();
        glut::run_continuing(
            &parts.config,
            &parts.model,
            &parts.tools,
            conversation,
            &prompt,
            &mut sink,
            &cancel,
        )
        .await
    });
    // A turn that panicked still ends: the screen gets the conversation as it was before it, and an error.
    tokio::spawn(async move {
        let done = match run.await {
            Ok((outcome, after)) => Report::Done(outcome, after),
            Err(err) => {
                let _ = reports.send(Report::Event(Event::Error {
                    message: format!("the turn broke off: {err}"),
                }));
                Report::Done(
                    RunOutcome {
                        reason: glut::FinishReason::Failed,
                        answer: None,
                        turns: 0,
                        usage: glut::Usage::default(),
                        refusals: 0,
                    },
                    kept,
                )
            }
        };
        let _ = reports.send(done);
    });
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
