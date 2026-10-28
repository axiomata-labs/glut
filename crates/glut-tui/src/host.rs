//! What a host adds to a session of the TUI (`docs/plans/glut-tui.md` T20, TUI8a): the program that runs the screen
//! for its own ends — Axiomata's Studio pane for a card — says how the session is built ([`Launch`]) and takes part
//! in it ([`Host`]): the run's settings, the budget before each turn, a second place to answer the gate's questions,
//! the steps for its own log, the session's state, the conversation after each turn. Through a [`HostLink`] it tells
//! the screen what happened outside: the studio stopped the session, the app's theme changed.
//!
//! The standalone `glut` is the plain host: [`Standalone`] takes part in nothing, and [`Launch::owner`] builds the
//! session from the owner's own `~/.glut` as before. The TUI depends on nothing of a host's — a host depends on it.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use glut::config::ModelEndpoint;
use glut::permissions::{AnswerFuture, AskRequest, BuiltGate, GateOptions};
use glut::providers::catalog::Protocol;
use glut::tools::search::WebSearch;
use glut::{Event, Message, Prices, RunConfig};
use tokio::sync::mpsc;

use crate::session::Report;

/// A future a host returns from a trait method: boxed, as `async fn` in a trait object cannot be.
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How the session is built.
pub struct Launch {
    /// The folder the session works in.
    pub cwd: PathBuf,
    /// The gate's start: `mode: None` takes the settings' (a session starts in build when they name none), and
    /// `trusted` decides whether the project's own `.glut/` rules, skills and servers count.
    pub gate: GateOptions,
    /// The folder is trusted when the owner trusted it (`~/.glut/trust.json`), whatever `gate.trusted` says.
    pub owner_trust: bool,
    /// What the model is told before the conversation.
    pub instructions: Instructions,
    /// Which MCP servers the session starts.
    pub servers: Servers,
    /// The skills' folders beside the owner's and the project's; `None` takes the run's default.
    pub skill_dirs: Option<Vec<PathBuf>>,
    /// The web search.
    pub search: Search,
    /// The model the session starts with.
    pub engine: EngineChoice,
    /// Whether the session is kept under `~/.glut/sessions/` for `glut -c`/`-r`. A host that keeps its sessions itself
    /// ([`Host::kept`]) says no: they would be found twice.
    pub keep_sessions: bool,
    /// Whether the mode stays as `gate.mode` set it: no Shift+Tab, no other mode out of plan mode. A host whose
    /// rights were chosen for the session's role says yes.
    pub mode_locked: bool,
    /// Whether the owner's `!cmd` and `!!cmd` run: a host that keeps the app's keys in reach of the folder says no.
    pub owner_shell: bool,
    /// Whether a turn under way is left as it is when the terminal closes (SIGHUP) or the process is ended
    /// (SIGTERM), so its journal stays for the next start to go on from ([`crate::Open::lost_turn`]); otherwise it
    /// is called off.
    pub keep_lost_turns: bool,
    /// Where the session's event log goes (TUI8d), which `glut watch` follows. [`Log::Sessions`] writes none when
    /// `keep_sessions` is false: a log beside a session that is never kept would stay behind alone.
    pub log: Log,
    /// Whether the owner may switch to another project from the screen (T44, `/projects`): Glut starts anew in its
    /// folder. A host that picks the folder itself (Axiomata's master) says no; a host that may not split never
    /// offers it.
    pub projects: bool,
    /// The language the host shows its own app in (T48): Glut's owner's `/language` wins over it, and it over the
    /// system's. `None` for Glut on its own.
    pub language: Option<glut::lang::Lang>,
}

impl Launch {
    /// The owner's own session in `cwd`: Glut's settings, guides, servers, skills and providers, as `glut` has them;
    /// trusted when the owner trusted the folder.
    pub fn owner(cwd: PathBuf, provider: Option<String>, model: Option<String>) -> Self {
        Launch {
            cwd,
            gate: GateOptions {
                mode: None,
                allow: Vec::new(),
                ask: Vec::new(),
                deny: Vec::new(),
                trusted: false,
            },
            owner_trust: true,
            instructions: Instructions::Guides,
            servers: Servers::Owner,
            skill_dirs: None,
            search: Search::Owner,
            engine: EngineChoice::Providers { provider, model },
            keep_sessions: true,
            mode_locked: false,
            owner_shell: true,
            keep_lost_turns: false,
            log: Log::Sessions,
            projects: true,
            language: None,
        }
    }
}

/// What the model is told before the conversation.
pub enum Instructions {
    /// The owner's and the project's guides (`AGENTS.md`, T24), read by Glut itself.
    Guides,
    /// The host's own text, and nothing Glut reads.
    Given(String),
}

/// The session's web search.
pub enum Search {
    /// With the search providers' keys from the environment, as `glut` has it.
    Owner,
    /// The host's, with its keys and its daily limit.
    Given(WebSearch),
    /// None: the host turned it off.
    Off,
}

/// Which MCP servers the session starts.
#[derive(Debug, Clone)]
pub enum Servers {
    /// `~/.glut/mcp.json` and the project's `.mcp.json` as the owner approved them.
    Owner,
    /// `~/.glut/mcp.json` alone, never a project's — a plan's run (T60b), where nobody is there to approve one.
    OwnerAlone,
    /// The servers of the file at this path alone (a card's team server), never the owner's nor the project's.
    Only(PathBuf),
    /// The owner's servers as [`Servers::Owner`] has them, and the file's beside them (`glut --mcp-config`, a master's
    /// team server): a name both have is the file's, as the one who named it asked for it.
    Also(PathBuf),
}

/// The model the session starts with.
pub enum EngineChoice {
    /// A provider of `~/.glut/providers.json` (the default one when `None`) and its model (its default when `None`);
    /// `/model`, `/provider` and `/effort` switch among them.
    Providers {
        /// The provider's id; the default one when `None`.
        provider: Option<String>,
        /// The model; the provider's default when `None`.
        model: Option<String>,
    },
    /// The host's model, fixed for the session: the screen offers no switch.
    Fixed {
        /// What the screen shows as the provider.
        provider: String,
        /// Where the model is reached, with its key.
        endpoint: ModelEndpoint,
        /// How it is spoken to.
        protocol: Protocol,
        /// Its prices, where the owner's settings name none.
        prices: Option<Prices>,
        /// The model's window and where it comes from.
        window: Option<(u32, &'static str)>,
    },
}

/// Where the session's event log goes: its steps and the owner's messages, one JSON line each ([`crate::log`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Log {
    /// Beside the session in `~/.glut/sessions` (`<id>.events.jsonl`).
    Sessions,
    /// The host's file — Axiomata's card writes its channel's `glut-events.jsonl`.
    At(PathBuf),
    /// None.
    Off,
}

/// What a turn is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// The owner's message.
    Message,
    /// A run that was cut off (the pane closed, the app quit) goes on from its journal: what it used is counted
    /// already.
    Resumed,
}

/// The session's state, as a host shows it elsewhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// A turn runs.
    Working,
    /// The gate waits for the owner's answer.
    Waiting,
    /// Between turns: the owner's next message is awaited.
    Idle,
    /// The screen ended.
    Ended,
}

/// What a host takes part in. Every method has a default that takes part in nothing.
///
/// `event`, `state` and `kept` are called on the screen's own path (the event sink, the end of a turn): they must be
/// quick and must not panic — the screen waits for them, and a panic there loses the turn's end.
pub trait Host: Send + Sync + 'static {
    /// The run's settings once the session is built, before its first turn: the host's limits of steps and time,
    /// its code mode. `built` is the gate as the settings made it (its compaction, its refusal limits). The context
    /// window set here stands only where the engine knows none of its own.
    fn configure(&self, config: &mut RunConfig, built: &BuiltGate) {
        let _ = (config, built);
    }

    /// Before each turn, on the turn's own copy of the settings: what is left of the host's budget, the turn's
    /// journal (a [`Turn::Resumed`] one reads the lost run from its folder). An `Err` refuses the turn with its
    /// reason: nothing reaches the model, the conversation stays as it was, and the owner's message goes back into
    /// the input. A turn called off while this waits is refused too.
    fn before_turn<'a>(
        &'a self,
        config: &'a mut RunConfig,
        turn: Turn,
    ) -> HostFuture<'a, Result<(), String>> {
        let _ = (config, turn);
        Box::pin(async { Ok(()) })
    }

    /// Before a paid call outside a turn (`/compact`, `/handoff write`): an `Err` says why not. What it used reaches
    /// [`Host::event`] as an [`Event::Usage`] once it is done.
    fn may_spend(&self) -> Result<(), String> {
        Ok(())
    }

    /// A lost run [`Open::lost_turn`](crate::Open::lost_turn) named could not go on, and why: its journal is the
    /// host's to give up.
    fn not_resumed(&self, run: &str, reason: &str) {
        let _ = (run, reason);
    }

    /// A question of the gate. `on_screen` is the screen's dialog; the host may wait on it alone (the default) or
    /// race it with an answer of its own — a dialog whose answer is no longer awaited closes by itself.
    fn ask<'a>(&'a self, request: &'a AskRequest, on_screen: AnswerFuture<'a>) -> AnswerFuture<'a> {
        let _ = request;
        on_screen
    }

    /// Every step of a turn, the streamed pieces too.
    fn event(&self, event: &Event) {
        let _ = event;
    }

    /// The task list the screen shows changed: a `todo` of the run, or emptied by `/clear`, another session or a
    /// rewind — which say nothing as an [`Event`].
    fn todos(&self, items: &[glut::planning::TodoItem]) {
        let _ = items;
    }

    /// The session's state changed.
    fn state(&self, state: State) {
        let _ = state;
    }

    /// Whether the screen may split into panes of sessions of the owner's own (T36a, `/split`): a host whose session
    /// is one pane of its own studio says no — the default.
    fn may_split(&self) -> bool {
        false
    }

    /// The conversation after a turn, a compaction, a rewind or `/clear`.
    fn kept(&self, conversation: &[Message]) {
        let _ = conversation;
    }
}

/// The plain host of the standalone `glut`.
pub struct Standalone;

impl Host for Standalone {
    fn may_split(&self) -> bool {
        true
    }
}

/// What a host tells the screen from outside the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signal {
    /// The turn under way ends now, and why (the studio's limit); the session itself goes on.
    Stop(String),
    /// The app's theme: the screen recolours, the owner's own `/theme` choice is not changed.
    Theme(String),
    /// A line for the stream.
    Note(String),
}

/// The host's way to the screen, handed to [`crate::run`]'s `linked` once the session is built.
#[derive(Clone)]
pub struct HostLink(pub(crate) mpsc::UnboundedSender<Report>);

impl HostLink {
    /// Tells the screen `signal`; `false` once the screen is gone.
    pub fn send(&self, signal: Signal) -> bool {
        self.0.send(Report::Host(signal)).is_ok()
    }
}
