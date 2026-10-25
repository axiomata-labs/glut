//! The session on the screen: what is shown, and what a key, a click, a command or a step of the run changes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use glut::permissions::settings::NotifyWhen;
use glut::permissions::{Answer, AskRequest, Mode};
use glut::providers::{Providers, models};
use glut::{Cancel, Effort, Event, FinishReason, Message, Usage};
use tokio::sync::{mpsc, oneshot};

use crate::host::Signal;
use crate::input::Input;
use crate::picker::{Facts, Item, Pick, Picker};
use crate::prefs::{self, Prefs};
use crate::select::{self, Copied, Point, Selection};
use crate::session::{self, Engine, Parts, Report};
use crate::store::Session;
use crate::transcript::{Thinking, Tone, Transcript};

/// How long a second Ctrl+C has to come to quit.
const QUIT_WINDOW: Duration = Duration::from_millis(1500);
/// How many lines the mouse wheel scrolls.
const WHEEL_LINES: usize = 3;
/// How long a question is on the screen before a key answers it.
const ASK_SETTLE: Duration = Duration::from_millis(400);
/// How long a message waits for an image still read beside the screen (T47).
const ATTACH_WAIT: Duration = Duration::from_secs(10);
/// An image to read for the message: the dropped file's name (`None`: the clipboard's), and how to read it.
type Read = (
    Option<String>,
    Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send>,
);

/// `/init` in a folder without a guide (T35).
const INIT_NEW: &str = "/init: write a short AGENTS.md for this folder, the guide every coding agent reads here. Look \
    first — the build and test commands, how the code is laid out, what is easy to get wrong — reading only what you \
    need. Keep it under 4 KB: commands, a few lines of layout, the traps; nothing a reader finds in a minute, nothing \
    Glut's own conventions already say. Show the draft and ask before you write the file.";

/// `/init` moving the guide `claude` (a `CLAUDE.md`) into an `AGENTS.md` beside it (T35).
fn init_migrate(claude: &str) -> String {
    format!(
        "/init: move {claude} into an AGENTS.md beside it, so every harness reads one guide. Write AGENTS.md with its \
         text — shortened where it repeats itself or says what the code shows, under 4 KB if it can be —, then leave \
         {claude} as the single line `@AGENTS.md`, which Claude Code follows. Show both before you write them."
    )
}

/// `/init` checking the guide `name` there is (T35).
fn init_check(name: &str) -> String {
    format!(
        "/init: check the guide {name}, which every turn carries. Is it still true (commands, layout, \
         traps)? Is anything in it that a reader finds in a minute, or that belongs in docs/ with a pointer left \
         behind? Propose a shorter version — under 4 KB if it can be, nothing important lost — and say what you would \
         drop or move. Show it and ask before you change the file."
    )
}

/// The most messages of other panes' models held in one pane for the owner's Enter (T36c).
pub const MOST_HELD: usize = 5;

/// What a pane's model could do when it wrote to another pane (T36c): a message waiting for a turn's end goes on only
/// if the receiving pane is still no wider than this by then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub looseness: u8,
    pub granted: Vec<String>,
    /// The rules of its pane's role; `None` without one.
    pub role: Option<Vec<String>>,
}

/// When a message from another pane is taken (T36b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivered {
    /// At once: a turn started.
    Now,
    /// After the turn under way, if it ends answered.
    AfterTurn,
    /// Once the compaction, handoff or switch under way is done.
    WhenFree,
    /// On Enter there: a list was open or messages were waiting.
    OnEnter,
}

/// A question of the gate on the screen.
pub struct OpenAsk {
    pub request: AskRequest,
    reply: oneshot::Sender<Answer>,
    /// When the card came up: keys before [`ASK_SETTLE`] has passed do not answer it.
    opened: Instant,
}

/// The side panel's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    /// The run's task list.
    Plan,
    /// The gate, live (T15).
    Gate,
    /// The meter, exact (T16).
    Meter,
    /// The session's subagents (T34).
    Agents,
    /// The project's `TODO.md` (T41).
    Todos,
    /// Every key by topic, only to read (T38).
    Keys,
}

/// What runs in the background besides the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Turn,
    Compacting,
    Switching,
    HandingOff,
}

/// A switch the owner goes round with one key, its notes one line (`⇧⇥`, `⌃T`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Switch {
    Mode,
    Thinking,
}

/// What a session is about, as the reader mode says it (TUI9b).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Works,
    Asks,
    Rests,
}

pub struct App {
    pub parts: Arc<Parts>,
    pub engine: Engine,
    pub folder: String,
    pub transcript: Transcript,
    pub input: Input,
    pub picker: Option<Picker>,
    /// The entry chosen in the list of commands a typed `/` shows.
    pub suggested: usize,
    /// The names that fit the path being typed (T68), and the one chosen.
    pub paths: Option<crate::paths::Listing>,
    pub path_chosen: usize,
    /// The owner picked in the path list with ↑↓: only then is ⏎ a pick — else it sends what is typed, a path at its
    /// end and all.
    path_picked: bool,
    /// The input as it stood when Esc closed the path list or a file was taken: no list for it again.
    paths_closed: Option<String>,
    /// The conversation between turns; taken while a turn runs.
    conversation: Vec<Message>,
    /// The session as it is kept (T25).
    pub session: Session,
    /// The run's task list as `todo_write` last sent it (the Plan tab, T28).
    pub todos: Vec<glut::planning::TodoItem>,
    /// The project's `TODO.md`, the To-dos tab's (T41).
    pub project_todos: crate::todos::Todos,
    /// `glut_file`'s `pick_model` waiting for the owner's pick (T37c).
    model_pick: Option<oneshot::Sender<Option<(String, String)>>>,
    /// When its list came up: keys before [`ASK_SETTLE`] has passed do not pick from it.
    model_pick_opened: Instant,
    /// The guides `/init` checked, with the size each had then (T42).
    guide_checked: std::collections::BTreeMap<String, u64>,
    /// The guide the turn `/init` started checks or writes: its size is kept when the turn is answered (T42).
    init_guide: Option<std::path::PathBuf>,
    /// How the last turn ended, for Glimm's mood: answered, or ended by an error or a limit; `None` before the first
    /// and after one the owner stopped.
    pub last_turn: Option<bool>,
    /// Whether the side panel is shown; `None` until the owner or a first plan decides.
    pub panel: Option<bool>,
    /// Which tab the side panel shows.
    pub tab: Tab,
    /// The gate's judgements of the session, for the panel's Gate tab (T15).
    pub watch: crate::watch::Watch,
    /// What the session spent, by turn and tool, for the panel's Meter tab (T16).
    pub meter: crate::meter::Meter,
    /// The review of what turns changed (T17), kept for the session with its decisions.
    pub review: crate::review::Review,
    /// The subagent whose own stream is open over the session's (T18).
    pub agent_view: Option<crate::agents::View>,
    /// The Agents tab's chosen subagent, by its path, while its list has the keys (T34, `⌃X A`) — a path, not a row,
    /// so the choice stays on its subagent as the list reorders; `None`: the input has them.
    pub agents_list: Option<Vec<usize>>,
    /// The `CLAUDE.md` `/init` asks about while its question is open (T35), as the stream names it.
    init_claude: Option<String>,
    /// The plan `exit_plan` sent last (T19), and its view: the dialog while Glut asks, else only to read.
    pub plan: Option<crate::plan::Plan>,
    pub plan_view: Option<crate::plan::Dialog>,
    /// The plan of an `exit_plan` call whose ask has not come yet (`None` inside: its arguments held none whole).
    offered_plan: Option<Option<crate::plan::Plan>>,
    /// How many rows the open view (a subagent's, the plan) had at the last frame, for its page keys.
    pub overlay_rows: usize,
    /// Ctrl+X was pressed: the next key finishes a chord (Ctrl+X E: the editor).
    chord: bool,
    /// The input, handed to the editor by the main loop (T29), which owns the terminal.
    pub edit_request: Option<String>,
    /// A file to open in the owner's editor, by the main loop, which owns the terminal (`/memory open`).
    pub open_request: Option<std::path::PathBuf>,
    busy: Option<Busy>,
    /// The turn under way: its cancel.
    running: Option<Cancel>,
    pub ask: Option<OpenAsk>,
    pub usage: Usage,
    pub cost: Option<f64>,
    /// The prompt tokens of the last model call: how full the window is.
    pub context_used: u64,
    /// Lines the stream is scrolled up from its end; 0 follows the end.
    pub scroll: usize,
    /// The stream's lines at the last frame and its height, to bound the scroll.
    pub stream_lines: usize,
    pub stream_height: usize,
    /// What each row of the stream showed at the last frame: its screen row, its block (for a click) and its line of
    /// the stream (for a selection).
    pub rows: Vec<(u16, usize, usize)>,
    /// The stream's left column and the width its lines were laid out for, at the last frame.
    pub stream_x: u16,
    pub stream_layout: u16,
    /// The selection being dragged or made (TUI2b), and where the left button went down.
    pub selection: Option<Selection>,
    /// With the width the stream had: a press made before a resize anchors nothing.
    press: Option<(Point, u16)>,
    /// The search in the stream (Ctrl+F), while it is open.
    pub search: Option<Search>,
    /// A short word in the key line (what was copied), and when it was said.
    pub toast: Option<(String, Instant)>,
    /// Whether Glut holds the mouse; F2 hands it to the terminal and back, the main loop applies it.
    pub mouse_held: bool,
    pub mouse_request: Option<bool>,
    /// F3 or `/scrollback`: the main loop prints the stream where the terminal keeps it (TUI2c).
    pub scrollback_request: bool,
    /// When Esc was last pressed with nothing to stop: a second one soon after opens rewind (T11).
    last_esc: Option<Instant>,
    /// The input's vi mode, when the owner turned it on (`/vim`).
    pub vi: Option<crate::vi::Vi>,
    /// The turn's inbox: what Enter sends while Glut works, taken before its next step (T10).
    pub steering: Option<glut::Inbox>,
    /// Messages queued with Tab while Glut works, sent one by one as turns end.
    pub queue: Vec<String>,
    /// The queue was filled while Glut compacted, handed off or switched: it goes as soon as that is done.
    send_when_free: bool,
    /// The owner's `!cmd` outputs waiting for the conversation to be the session's again.
    pending: Vec<Message>,
    /// How many `!` commands ran: each one's block has an id of its own, whatever `/clear` did since.
    shells: usize,
    /// The folder's files for `@`, and when they were walked; the walk runs beside the screen.
    files: Option<(Instant, Vec<String>, bool)>,
    /// Where the stream ended at the last frame, the panel right of it: a click there is not the stream's.
    pub stream_width: u16,
    /// Whether the last frame drew the side panel (a narrow screen leaves it out though it is on).
    pub panel_drawn: bool,
    /// The last frame's stream was wide enough for the panel beside it: Ctrl+K opens the Keys tab, else the sheet
    /// over the stream.
    pub panel_fits: bool,
    /// The Keys sheet over the stream (T38, a window too narrow for the panel): how far it is scrolled.
    pub keys_sheet: Option<usize>,
    /// How far the Keys tab is scrolled (the wheel over the panel).
    pub keys_scroll: usize,
    /// How far the Plan, Gate or Meter tab is scrolled (TUI9a: the panel's focus, the wheel over it).
    pub panel_scroll: usize,
    /// The side panel has the keys (TUI9a, `⌃X ⇥`): ←→ the tabs, ↑↓ within one, Esc back to the input.
    pub panel_focus: bool,
    /// The reading mode (TUI9a, `⌃X ↑`): a block of the stream chosen by the keys alone.
    pub reading: Option<Reading>,
    /// What this pane asks of the screen (T36a): a split, a close, the other pane.
    pub pane_request: Option<crate::panes::Request>,
    /// The sessions the other panes hold: not opened here a second time.
    pub others_open: Vec<String>,
    /// The other panes' numbers (T36b: where `/send` and a plan go).
    pub others: Vec<u8>,
    /// The owner sent a message here since the screen last looked (T36c: it ends a chain between models).
    pub owner_spoke: bool,
    /// How many messages this session's model sent to other panes (T36c: at most [`crate::panes::MOST_TELLS`]).
    pub tells_sent: u32,
    /// How this session keeps its cache while it rests (T36e).
    pub cache_mode: crate::warm::CacheMode,
    /// The keeping at work while the session rests; dropped when a turn starts.
    keeper: Option<crate::warm::Keeper>,
    /// Until when the provider holds the cache, as far as Glut knows; `None` before it held any.
    pub warm_until: Option<Instant>,
    /// The provider has said it cached something in this session: only then does the footer mark the cache.
    pub cache_seen: bool,
    /// The agents in the background as the Agents tab lists them (T36e): one line each, told by the screen.
    pub background: Vec<String>,
    /// The role this pane plays (T36c): a named agent's prompt and tools, never its model; `None` an all-rounder.
    pub role: Option<String>,
    /// Messages of another pane's model that wait for the owner's Enter here (T36c): never sent by a turn's end.
    pub held: Vec<String>,
    /// When the last of them came: an Enter right after it (meant for something else) lets none go.
    pub(crate) held_since: Option<Instant>,
    /// Glut's very first start (TUI9c): the welcome shows the first keys until a message or a command goes.
    pub first_start: bool,
    /// The last note of a switch gone round (`⇧⇥` the mode, `⌃T` the thinking), and how long the stream was after it:
    /// the next of the same switch replaces it while it is still the last block.
    switch_note: Option<(Switch, usize)>,
    /// The highest image number given in this window (T45), for the input's marks until the message is sent.
    images_given: u32,
    /// The images still read beside the screen (T47): a message waits for them.
    attaching: usize,
    /// Since when the oldest of them is read.
    attaching_since: Option<Instant>,
    /// Whether the blocks hold their texts for a search (T47): given up once it closes.
    pub search_texts: bool,
    /// The projects the welcome shows (T44), looked up once: the newest, where this folder has no session yet.
    welcome_projects: Option<(String, Vec<crate::projects::Project>)>,
    /// The window's project (T44): this pane's folder, or — in a worktree's pane — the folder it was split from.
    pub project: std::path::PathBuf,
    /// The highest image number sent: a message's images are numbered on from it when it goes.
    images_sent: u32,
    /// An image shown whole over the stream (T46), from a click or ⏎ on its picture.
    pub picture_view: Option<glut::images::Image>,
    /// Since when Glut works (TUI9b): what the footer counts while the screen holds still.
    pub busy_since: Option<Instant>,
    /// Since when Glut has worked without a rest (T53): a chain of queued turns counts as one for a notification.
    pub working_since: Option<Instant>,
    /// The state last seen (TUI9b): the reader mode says each change in a line of its own.
    seen_state: Option<State>,
    /// How many turns had started when the state was last seen: a queued message's turn begins with no rest between.
    seen_turns: usize,
    /// Messages of another pane's model for after the turn under way, with what their sender could do (T36c).
    waiting_models: Vec<(String, Sender)>,
    /// The folder's branch for the footer (T39).
    pub branch: crate::branch::Branch,
    /// How the footer joins its segments, and whether Glut guessed it (`Some`, with the terminal it knew, if any)
    /// or the owner chose it (`None`).
    pub separators: crate::footer::Separators,
    separators_from: Option<Option<&'static str>>,
    pub frame: usize,
    quit_armed: Option<Instant>,
    pub quit: bool,
    /// The terminal closed or the process was ended: no owner is left to see the end.
    pub abandoned: bool,
    /// Where the theme comes from: the owner's `/theme`, or the host's app (TUI8e).
    theme_from: ThemeFrom,
    /// The host's first message, held while a cut-off turn goes on ([`App::resume`]).
    after_resume: Option<String>,
    reports: mpsc::UnboundedSender<Report>,
}

/// Where the screen's theme comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeFrom {
    /// The owner's kept choice (`prefs.json`), as the standalone `glut` has it.
    Owner,
    /// The host's: its app's theme, followed as it changes.
    Host,
    /// The owner's `/theme` in a hosted session: it stands for the session, the host's changes no longer reach it,
    /// and it is not kept as the owner's own.
    Pinned,
}

impl App {
    pub fn new(parts: Arc<Parts>, engine: Engine, reports: mpsc::UnboundedSender<Report>) -> Self {
        let folder = session::shown_folder(&parts.config.cwd);
        let project = parts.config.cwd.clone();
        let session = Session::new(&parts.config.cwd, &engine.provider, engine.model_name());
        // Only a Glut that never kept a preference starts for the first time (TUI9c): an owner of long standing is
        // not welcomed afresh.
        let first_start = !Prefs::kept(&parts.home);
        let prefs = Prefs::load(&parts.home);
        let panel = prefs.panel;
        let vi = prefs.vi.unwrap_or(false);
        let cache_mode = prefs.cache.unwrap_or_default();
        crate::motion::set(
            prefs.motion.unwrap_or_default(),
            prefs.reader.unwrap_or(false),
        );
        crate::links::set(prefs.links);
        crate::notify::set(prefs.notify, prefs.notify_after, parts.notify);
        crate::pictures::set_drawing(prefs.images);
        engine
            .model
            .set_cache_hour(cache_mode == crate::warm::CacheMode::Hour);
        let (separators, separators_from) = match prefs.footer {
            Some(kept) => (kept, None),
            None => {
                let (guessed, terminal) = crate::footer::guess(
                    std::env::var("TERM_PROGRAM").ok().as_deref(),
                    std::env::var("TERM").ok().as_deref(),
                );
                (guessed, Some(terminal))
            }
        };
        // The theme kept from an earlier session; an unknown name (a theme dropped since) keeps Glut's own.
        if let Some(name) = &prefs.theme {
            crate::theme::set(name);
        }
        crate::texts::apply(prefs.language.as_deref(), parts.language);
        let mut transcript = Transcript {
            thinking: Prefs::load(&parts.home).thinking.unwrap_or_default(),
            ..Transcript::default()
        };
        for note in &parts.notes {
            transcript.note(note.clone(), Tone::Quiet);
        }
        // T42: a guide grown too large is said once at the start, in orange; the Meter tab keeps saying it.
        // A guide shortened below the limit since `/init` checked it counts afresh: its kept size goes.
        let mut guide_checked = prefs.guide_checked.clone();
        let shrunk =
            crate::guides::shrunk(&parts.guide_paths, parts.guide_warn_bytes, &guide_checked);
        if !shrunk.is_empty() {
            guide_checked.retain(|path, _| !shrunk.contains(path));
            if let Err(reason) = prefs::forget_guides_checked(&parts.home, &shrunk) {
                transcript.note(reason, Tone::Fault);
            }
        }
        let nearest = parts.guide_paths.last().map(std::path::PathBuf::as_path);
        for large in
            crate::guides::too_large(&parts.guide_paths, parts.guide_warn_bytes, &guide_checked)
        {
            transcript.note(
                crate::guides::note(&large, &parts.config.cwd, nearest),
                Tone::Heed,
            );
        }
        App {
            parts,
            engine,
            folder,
            transcript,
            input: Input::new(),
            picker: None,
            suggested: 0,
            paths: None,
            path_chosen: 0,
            path_picked: false,
            paths_closed: None,
            conversation: Vec::new(),
            session,
            todos: Vec::new(),
            project_todos: crate::todos::Todos::default(),
            model_pick: None,
            model_pick_opened: Instant::now(),
            guide_checked,
            init_guide: None,
            last_turn: None,
            panel,
            tab: Tab::Plan,
            watch: crate::watch::Watch::default(),
            meter: crate::meter::Meter::default(),
            review: crate::review::Review::default(),
            agent_view: None,
            agents_list: None,
            init_claude: None,
            plan: None,
            plan_view: None,
            offered_plan: None,
            overlay_rows: 0,
            chord: false,
            edit_request: None,
            open_request: None,
            busy: None,
            running: None,
            ask: None,
            usage: Usage::default(),
            cost: None,
            context_used: 0,
            scroll: 0,
            stream_lines: 0,
            stream_height: 0,
            rows: Vec::new(),
            stream_x: 0,
            stream_layout: 0,
            selection: None,
            press: None,
            search: None,
            toast: None,
            mouse_held: true,
            mouse_request: None,
            scrollback_request: false,
            last_esc: None,
            vi: vi.then(crate::vi::Vi::default),
            steering: None,
            queue: Vec::new(),
            send_when_free: false,
            pending: Vec::new(),
            shells: 0,
            files: None,
            stream_width: u16::MAX,
            panel_drawn: false,
            panel_fits: false,
            keys_sheet: None,
            keys_scroll: 0,
            panel_scroll: 0,
            panel_focus: false,
            reading: None,
            branch: crate::branch::Branch::default(),
            pane_request: None,
            others_open: Vec::new(),
            others: Vec::new(),
            owner_spoke: false,
            tells_sent: 0,
            held: Vec::new(),
            role: None,
            cache_mode,
            background: Vec::new(),
            keeper: None,
            warm_until: None,
            cache_seen: false,
            held_since: None,
            busy_since: None,
            working_since: None,
            images_given: 0,
            attaching: 0,
            attaching_since: None,
            search_texts: false,
            welcome_projects: None,
            project,
            images_sent: 0,
            picture_view: None,
            switch_note: None,
            first_start,
            seen_state: None,
            seen_turns: 0,
            waiting_models: Vec::new(),
            separators,
            separators_from,
            frame: 0,
            quit_armed: None,
            quit: false,
            abandoned: false,
            theme_from: ThemeFrom::Owner,
            after_resume: None,
            reports,
        }
    }

    /// Goes on from `conversation`, which the host kept (a card's pane opened again): shown as an opened session.
    pub fn go_on_from(&mut self, conversation: Vec<Message>) {
        let mut session = Session::new(
            &self.parts.config.cwd,
            &self.engine.provider,
            self.engine.model_name(),
        );
        session.title = conversation
            .iter()
            .find(|message| message.role == glut::model::Role::User)
            .and_then(|message| message.content.as_deref())
            .map(crate::store::title_of)
            .unwrap_or_default();
        session.conversation = conversation;
        self.open(session);
    }

    /// Keeps the session as it stands now; a failure is said, the session goes on.
    fn keep(&mut self) {
        self.session.conversation.clone_from(&self.conversation);
        self.session.provider.clone_from(&self.engine.provider);
        self.engine.model_name().clone_into(&mut self.session.model);
        self.session.cost = self.cost;
        self.parts.host.kept(&self.conversation);
        if !self.parts.keep_sessions {
            return;
        }
        if let Err(reason) = self.session.save(&self.parts.home) {
            self.transcript
                .note(tr!(session_unkept, reason = reason), Tone::Fault);
        }
    }

    /// Opens session `session`: its conversation goes on, its stream is shown again.
    pub fn open(&mut self, session: Session) {
        self.forget_warm();
        // What waited for the other session is not this one's.
        self.queue.clear();
        self.forget_models();
        self.pending.clear();
        self.transcript = Transcript {
            thinking: self.transcript.thinking,
            ..Transcript::default()
        };
        session.transcript(&mut self.transcript);
        let when = session
            .updated
            .with_timezone(&chrono::Local)
            .format(glut::lang::date_time());
        let said = if session.title.is_empty() {
            tr!(session_opened, when = when)
        } else {
            tr!(
                session_opened_titled,
                when = when,
                title = crate::transcript::clean(&session.title)
            )
        };
        self.transcript.note(said, Tone::Fine);
        self.conversation.clone_from(&session.conversation);
        self.todos.clear();
        self.forget_views();
        self.cost = session.cost;
        self.usage = Usage::default();
        self.meter = crate::meter::Meter::default();
        self.review = crate::review::Review::default();
        self.context_used = 0;
        self.scroll = 0;
        self.session = session;
    }

    /// Once per frame (TUI9b): when Glut began to work, and — in the reader mode — a line for each change of state,
    /// as a screen reader reads what is new.
    pub fn track_state(&mut self) {
        self.track_state_saying(crate::motion::reader());
    }

    /// [`App::track_state`], the lines said when `saying`.
    fn track_state_saying(&mut self, saying: bool) {
        let now = if self.waits_for_owner() {
            State::Asks
        } else if self.working() {
            State::Works
        } else {
            State::Rests
        };
        let turns = self.meter.turns.len();
        let next_turn = std::mem::replace(&mut self.seen_turns, turns) != turns
            && now == State::Works
            && self.seen_state == Some(State::Works);
        let before = self.seen_state.replace(now);
        if before == Some(now) && !next_turn {
            return;
        }
        match now {
            // A question in a turn keeps the time it has; the next turn, or work after a rest, starts it anew.
            State::Works if before == Some(State::Asks) && self.busy_since.is_some() => {}
            State::Works => self.busy_since = Some(Instant::now()),
            State::Rests => self.busy_since = None,
            State::Asks => {}
        }
        match now {
            State::Rests => self.working_since = None,
            _ if self.working_since.is_none() => {
                self.working_since = self.busy_since.or(Some(Instant::now()))
            }
            _ => {}
        }
        if next_turn && saying {
            return self.transcript.note(tr!(state_next_message), Tone::Quiet);
        }
        if !saying || before.is_none() {
            return;
        }
        let said = match now {
            State::Works if before == Some(State::Asks) => tr!(state_works_on),
            State::Works => tr!(state_works),
            State::Asks => match &self.ask {
                Some(open) => {
                    let said = tr!(
                        state_asks_tool,
                        tool = open.request.tool,
                        target = crate::transcript::cut(
                            &glut::permissions::printable(&open.request.target),
                            120
                        )
                    );
                    return self.transcript.note(said, Tone::Heed);
                }
                None => tr!(state_asks_choose),
            },
            State::Rests => match self.last_turn {
                Some(true) => tr!(state_done),
                Some(false) => tr!(state_failed),
                None => tr!(state_stopped),
            },
        };
        self.transcript.note(said, Tone::Quiet);
    }

    /// Whether Glut works on something: a turn, a summary, a switch of model.
    pub fn working(&self) -> bool {
        self.busy.is_some()
    }

    /// Whether the welcome is up: before the first message only notes are in the stream (the guides read, …).
    pub fn welcoming(&self) -> bool {
        self.transcript
            .blocks
            .iter()
            .all(|block| matches!(block.kind, crate::transcript::Kind::Note { .. }))
    }

    /// Whether the screen moves on its own: while Glut works, and while Glimm is in view — on the welcome or at the
    /// head of the side panel.
    pub fn animated(&self) -> bool {
        self.working() || self.welcoming() || self.panel == Some(true) || self.toast().is_some()
    }

    /// The toast while it is fresh.
    pub fn toast(&self) -> Option<&str> {
        self.toast
            .as_ref()
            .filter(|(_, said)| said.elapsed() < TOAST_FOR)
            .map(|(text, _)| text.as_str())
    }

    pub fn say(&mut self, text: impl Into<String>) {
        // A toast may name a file the model made: cleaned like everything else drawn.
        self.toast = Some((crate::transcript::clean(&text.into()), Instant::now()));
    }

    /// `/export`: the session as it was saved after its last turn, into a new file or onto the clipboard (T54).
    fn export_command(&mut self, rest: &str) {
        let asked = crate::export::parse(rest);
        if self.session.conversation.is_empty() {
            return self.transcript.note(tr!(export_empty), Tone::Quiet);
        }
        let home = self.parts.home.clone();
        let format = if asked.copy {
            crate::export::Format::Markdown
        } else {
            asked.format
        };
        let text = crate::export::render(
            &crate::export::of_session(&self.session, asked.full, &home),
            format,
        );
        if asked.copy {
            return match crate::select::copy(&text) {
                Ok(_) => self.tell(tr!(export_copied)),
                Err(reason) => self.transcript.note(reason, Tone::Fault),
            };
        }
        let path = match &asked.path {
            Some(typed) => crate::export::named_path(typed, &home, &self.parts.config.cwd),
            None => {
                crate::export::default_path(&home, &self.session.cwd, format, chrono::Local::now())
            }
        };
        self.exported(crate::export::write(&path, &text, &home).map(|()| path));
    }

    /// What an export became, said where it stays.
    fn exported(&mut self, written: Result<std::path::PathBuf, String>) {
        match written {
            Ok(path) => {
                let shown = crate::projects::shown_path(&path, &self.parts.home);
                self.tell(tr!(export_written, path = shown));
            }
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// `/resume`'s ⌃E: the chosen session written out as Markdown, into `~/.glut/exports` (T54).
    fn export_listed(&mut self, id: &str) {
        let home = self.parts.home.clone();
        let session = if id == self.session.id {
            Ok(self.session.clone())
        } else {
            Session::load(&home, id)
        };
        let written = session.and_then(|session| {
            let format = crate::export::Format::Markdown;
            let text =
                crate::export::render(&crate::export::of_session(&session, false, &home), format);
            let path =
                crate::export::default_path(&home, &session.cwd, format, chrono::Local::now());
            crate::export::write(&path, &text, &home).map(|()| path)
        });
        self.exported(written);
    }

    /// `/notify`: when the owner is told a turn ended or asks (T53), or — alone — what holds now.
    fn notify_command(&mut self, rest: &str) {
        let home = self.parts.home.clone();
        let kept = Prefs::load(&home);
        if let Some(secs) = rest.strip_prefix("after") {
            let Some(secs) = secs
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|secs| *secs <= crate::notify::MAX_AFTER)
            else {
                let most = crate::notify::MAX_AFTER;
                return self
                    .transcript
                    .note(tr!(notify_after_bad, most = most), Tone::Fault);
            };
            crate::notify::set(kept.notify, Some(secs), self.parts.notify);
            return match prefs::set_notify_after(&home, secs) {
                Ok(()) => self.say(tr!(notify_after_set, n = secs)),
                Err(reason) => self.transcript.note(reason, Tone::Fault),
            };
        }
        let Some(when) = crate::notify::parse(rest) else {
            let choice = crate::notify::choice();
            let way = crate::notify::Env::read().way(choice.when);
            return self.tell(tr!(
                notify_now,
                when = crate::notify::word(choice.when),
                after = choice.after,
                way = way.label()
            ));
        };
        crate::notify::set(Some(when), kept.notify_after, self.parts.notify);
        match prefs::set_notify(&home, when) {
            Ok(()) => self.say(match when {
                NotifyWhen::On => tr!(notify_on),
                NotifyWhen::Asks => tr!(notify_asks_only),
                NotifyWhen::Off => tr!(notify_off),
                NotifyWhen::System => tr!(notify_system),
            }),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// A quiet line in the stream that stays, where a toast would pass: what happened to a pane (T36a).
    pub fn tell(&mut self, text: impl Into<String>) {
        self.transcript.note(text.into(), Tone::Quiet);
    }

    /// Says where the mouse went.
    pub fn mouse_said(&mut self, held: bool) {
        self.say(if held {
            tr!(mouse_glut)
        } else {
            tr!(mouse_terminal)
        });
    }

    /// What Glimm shows of the session: it waits for the owner's answer, works, or is pleased or gone out after the
    /// last turn — at rest before any.
    pub fn mood(&self) -> crate::glimm::Mood {
        use crate::glimm::Mood;
        if self.ask.is_some() {
            Mood::Waiting
        } else if self.working() {
            Mood::Working
        } else {
            match self.last_turn {
                Some(true) => Mood::Done,
                Some(false) => Mood::Failed,
                None => Mood::Resting,
            }
        }
    }

    /// The gate's mode now.
    pub fn mode(&self) -> Mode {
        self.parts
            .tools
            .gate()
            .map_or(Mode::Default, |gate| gate.mode())
    }

    /// The terminal closed or the process was ended: the session ends.
    pub fn abandon(&mut self) {
        self.abandoned = true;
        self.quit = true;
    }

    /// Stops the turn under way, if there is one (the session is ending).
    pub fn stop_turn(&mut self) {
        if let Some(cancel) = &self.running {
            cancel.cancel();
        }
    }

    /// Sends `prompt` as the next turn.
    pub fn send(&mut self, prompt: String) {
        self.send_with(prompt, Vec::new());
    }

    /// [`App::send`] with the images the message carries (T45).
    fn send_with(&mut self, prompt: String, images: Vec<glut::images::Image>) {
        if prompt.trim().is_empty() || self.working() {
            return;
        }
        // A picker left open (glut -r with a first message) is no place to choose from while the turn runs.
        self.picker = None;
        // The turn reads the cache itself: no keeping beside it.
        self.keeper = None;
        // Another pane's model is not the owner (T36c): its message stands apart, and only the owner's word ends a
        // chain of messages between models.
        if let Some(shown) = crate::tell::shown(&prompt) {
            self.transcript.note(shown, Tone::Heed);
        } else {
            self.owner_spoke = true;
            // The images stand under the words, each a picture of its own (T46).
            self.transcript
                .push(crate::transcript::Kind::Owner(prompt.clone()));
            for image in &images {
                self.transcript
                    .push(crate::transcript::Kind::Picture(image.clone()));
            }
        }
        // The session is named by the owner's first own message, not by a handoff read in front of it, nor by
        // another pane's model.
        if self.session.title.is_empty() && crate::tell::shown(&prompt).is_none() {
            self.session.title = crate::store::title_of(&prompt);
        }
        self.scroll = 0;
        // Whatever session the screen shows now is the one this turn's steps belong to.
        self.parts.log.follow(&self.session.id);
        self.parts.log.owner(&prompt);
        let cancel = Cancel::new();
        self.running = Some(cancel.clone());
        self.busy = Some(Busy::Turn);
        let inbox = glut::Inbox::default();
        self.steering = Some(inbox.clone());
        // Where this message begins, to rewind to before it; its run is known with its first snapshot.
        self.session.marks.push(crate::store::Mark {
            at: Some(self.conversation.len()),
            prompt: crate::store::title_of(&prompt),
            run: None,
        });
        session::turn(
            Arc::clone(&self.parts),
            self.engine.clone(),
            session::Before {
                session: self.session.id.clone(),
                conversation: std::mem::take(&mut self.conversation),
            },
            session::TurnInput::Message(prompt, images),
            cancel,
            inbox,
            self.reports.clone(),
        );
    }

    /// Goes on with the run `run` that was cut off (its journal is the host's); `then` is sent after it — unless the
    /// run had it in its conversation already.
    pub fn resume(&mut self, run: String, then: Option<String>) {
        if self.working() {
            return;
        }
        self.transcript.note(tr!(turn_resumed), Tone::Quiet);
        self.after_resume = then;
        self.parts.log.follow(&self.session.id);
        let cancel = Cancel::new();
        self.running = Some(cancel.clone());
        self.busy = Some(Busy::Turn);
        let inbox = glut::Inbox::default();
        self.steering = Some(inbox.clone());
        session::turn(
            Arc::clone(&self.parts),
            self.engine.clone(),
            session::Before {
                session: self.session.id.clone(),
                conversation: std::mem::take(&mut self.conversation),
            },
            session::TurnInput::Resume(run),
            cancel,
            inbox,
            self.reports.clone(),
        );
    }

    /// A step of the run, a question, or the end of something that ran in the background.
    pub fn on_report(&mut self, report: Report) {
        self.handle(report);
        // A `!cmd`'s output that came while the conversation was a turn's (or a compaction's) joins it once it is
        // the session's again.
        if !self.working() && self.busy.is_none() && !self.pending.is_empty() {
            self.conversation.append(&mut self.pending);
            self.keep();
        }
        // A message queued while Glut compacted, handed off or switched goes once it is free again.
        if self.busy.is_none() && std::mem::take(&mut self.send_when_free) && !self.queue.is_empty()
        {
            let next = self.queue.remove(0);
            self.send(next);
        }
    }

    fn handle(&mut self, report: Report) {
        self.settle_model_pick();
        match report {
            Report::Event(event) => {
                if let Event::Usage {
                    input_tokens,
                    cached_input_tokens,
                    output_tokens,
                    ..
                } = &event
                {
                    let call = Usage {
                        input_tokens: *input_tokens,
                        cached_input_tokens: *cached_input_tokens,
                        output_tokens: *output_tokens,
                    };
                    if let Some(prices) = self.engine.prices {
                        *self.cost.get_or_insert(0.0) += prices.cost_usd(&call);
                    }
                    self.usage += call;
                    self.context_used = *input_tokens;
                    if *cached_input_tokens > 0 {
                        self.cache_seen = true;
                    }
                }
                // A compaction in the turn rewrote what came before: no mark's place in the conversation holds.
                if let Event::Compacted { .. } = &event {
                    for mark in &mut self.session.marks {
                        mark.at = None;
                    }
                }
                // The run this turn started is known by its first snapshot: the folder before the owner's message.
                if let Event::Snapshot { run, .. } = &event
                    && let Some(mark) = self.session.marks.last_mut()
                    && mark.run.is_none()
                {
                    mark.run = Some(run.clone());
                }
                // A plan from plan mode (T19) — the session's own: a subagent never leaves plan mode.
                if let Event::ToolCall {
                    name, arguments, ..
                } = &event
                    && name == glut::planning::EXIT_PLAN
                {
                    // Held until its ask comes: a call the harness refuses (too long, from a script) asks nothing.
                    self.offered_plan = Some(crate::plan::Plan::of_call(arguments));
                }
                if let Event::Todo { items, .. } = &event {
                    self.todos.clone_from(items);
                    // The first plan opens the panel, unless the owner closed it once.
                    if self.panel.is_none() {
                        self.panel = Some(true);
                        self.tab = Tab::Plan;
                    }
                }
                self.watch.take(&event);
                self.meter.take(&event, self.engine.prices);
                self.transcript.take(&event);
                if matches!(&event, Event::ToolCall { name, .. } if name == "task") {
                    self.second_agent_opens_tab();
                }
            }
            Report::Ask(request, reply) => {
                // A chord begun before the question came does not take the first key after it.
                self.chord = false;
                // The plan's ask opens the plan's dialog (T19) over everything else: what was open goes, so no key
                // meant for it answers the plan. A plan its event could not carry whole is the ask's line, said so.
                if request.tool == glut::planning::EXIT_PLAN {
                    let plan =
                        self.offered_plan
                            .take()
                            .flatten()
                            .unwrap_or_else(|| crate::plan::Plan {
                                // The plan goes to another pane's model with `/send plan`: its note stays English.
                                text: format!(
                                    "{}\n\n*{}*",
                                    request.target,
                                    crate::texts::of(glut::lang::Lang::En).plan_head_only
                                ),
                                state: crate::plan::State::Waiting,
                            });
                    self.plan = Some(plan);
                    if self.panel.is_none() {
                        self.panel = Some(true);
                    }
                    self.tab = Tab::Plan;
                    self.plan_view = Some(crate::plan::Dialog::asking());
                    self.agent_view = None;
                    self.picker = None;
                    self.review.open = false;
                    self.keys_sheet = None;
                    self.search = None;
                } else if self.plan_view.as_ref().is_some_and(|view| view.asking) {
                    // A dialog whose ask is gone answers nothing else.
                    self.plan_view = None;
                }
                // T37b/c: what `glut_file` would write stands whole in the stream (PgUp reads it); the card shows its
                // head and tail.
                if request.tool == glut::tools::glut_files::NAME
                    && request.target.lines().count() > 1
                {
                    self.transcript.note(
                        format!("{}\n{}", tr!(glut_file_whole), request.target),
                        Tone::Heed,
                    );
                }
                self.ask = Some(OpenAsk {
                    request,
                    reply,
                    opened: Instant::now(),
                });
                self.scroll = 0;
            }
            Report::Done(outcome, conversation) => {
                // The images the turn's tools showed, under their calls (T46).
                for message in &conversation {
                    if let Some(call) = &message.tool_call_id
                        && let Some((at, count)) =
                            self.transcript.show_pictures(call, &message.images)
                    {
                        // What names a block by its place moves on with it.
                        if let Some(reading) = &mut self.reading
                            && reading.block >= at
                        {
                            reading.block += count;
                        }
                        if let Some(view) = &mut self.agent_view
                            && view.path.first().is_some_and(|first| *first >= at)
                        {
                            view.path[0] += count;
                        }
                    }
                }
                self.keep_warm(
                    outcome.reason == FinishReason::Answered,
                    outcome.warm.as_ref(),
                    &conversation,
                );
                if let Some(path) = self.init_guide.take()
                    && outcome.reason == FinishReason::Answered
                {
                    self.keep_guide_checked(&path);
                }
                // A turn that broke off before its message joined the conversation leaves no place to rewind to.
                if self
                    .session
                    .marks
                    .last()
                    .and_then(|mark| mark.at)
                    .is_some_and(|at| conversation.len() <= at)
                {
                    self.session.marks.pop();
                }
                self.conversation = conversation;
                self.conversation.append(&mut self.pending);
                self.running = None;
                self.busy = None;
                self.keep();
                // What was sent to steer and never taken (the turn ended first) waits as the next message.
                if let Some(inbox) = self.steering.take() {
                    let mut late = inbox.take();
                    late.append(&mut self.queue);
                    self.queue = late;
                }
                // A turn that ended (cancelled, timed out) while asking dropped the question: its card goes too.
                self.ask = None;
                if self.plan_view.as_ref().is_some_and(|view| view.asking) {
                    self.plan_view = None;
                }
                self.offered_plan = None;
                if let Some(plan) = &mut self.plan
                    && plan.state == crate::plan::State::Waiting
                {
                    plan.state = crate::plan::State::Dropped;
                }
                self.transcript.settle();
                self.last_turn = match outcome.reason {
                    FinishReason::Answered => Some(true),
                    FinishReason::Cancelled => None,
                    _ => Some(false),
                };
                match outcome.reason {
                    FinishReason::Answered => {}
                    FinishReason::Cancelled => self.transcript.note(tr!(stopped), Tone::Quiet),
                    other => self
                        .transcript
                        .note(tr!(turn_ended, reason = reason(other)), Tone::Fault),
                }
                // The host's first message follows a cut-off turn however it ended — unless that turn had it already: a
                // session nobody watches must not stand idle with its work unsent.
                if let Some(first) = self.after_resume.take()
                    && !told(&self.conversation, &first)
                {
                    self.send(first);
                    return;
                }
                // The next queued message follows a turn that answered; after a stop, a failure or a limit it waits
                // for Enter, or it would run into the same wall.
                // T36c: another model's message first — if this pane is still no wider than its sender was.
                if outcome.reason == FinishReason::Answered && !self.waiting_models.is_empty() {
                    let (message, sender) = self.waiting_models.remove(0);
                    if self.no_wider_than(&sender) {
                        self.send(message);
                    } else {
                        self.hold(message, tr!(held_wider));
                    }
                } else if outcome.reason == FinishReason::Answered && !self.queue.is_empty() {
                    let next = self.queue.remove(0);
                    self.send(next);
                }
            }
            Report::Compacted(result, before) => {
                self.forget_warm();
                self.busy = None;
                match result {
                    Ok(compacted) => {
                        self.spent(compacted.usage);
                        if let Some(prices) = self.engine.prices {
                            *self.cost.get_or_insert(0.0) += prices.cost_usd(&compacted.usage);
                        }
                        self.usage += compacted.usage;
                        self.conversation = compacted.conversation;
                        self.context_used = 0;
                        for mark in &mut self.session.marks {
                            mark.at = None;
                        }
                        self.keep();
                        self.transcript
                            .note(tr!(compacted, n = compacted.replaced), Tone::Fine);
                    }
                    Err(reason) => {
                        self.conversation = before;
                        self.transcript
                            .note(tr!(not_compacted, reason = reason), Tone::Fault);
                    }
                }
            }
            Report::HandedOff(result, conversation) => {
                self.forget_warm();
                self.busy = None;
                self.conversation = conversation;
                match result {
                    Ok((path, usage)) => {
                        self.spent(usage);
                        if let Some(prices) = self.engine.prices {
                            *self.cost.get_or_insert(0.0) += prices.cost_usd(&usage);
                        }
                        self.usage += usage;
                        self.transcript.note(
                            tr!(handoff_written, path = session::shown_folder(&path)),
                            Tone::Fine,
                        );
                    }
                    Err(reason) => self
                        .transcript
                        .note(tr!(no_handoff, reason = reason), Tone::Fault),
                }
            }
            Report::Host(signal) => self.on_host(signal),
            Report::Refused {
                reason,
                prompt,
                conversation,
            } => {
                if self.session.marks.last().and_then(|mark| mark.at) == Some(conversation.len()) {
                    self.session.marks.pop();
                }
                if conversation.is_empty() && self.session.title == crate::store::title_of(&prompt)
                {
                    self.session.title.clear();
                }
                self.conversation = conversation;
                self.running = None;
                self.busy = None;
                self.ask = None;
                if let Some(inbox) = self.steering.take() {
                    let mut late = inbox.take();
                    late.append(&mut self.queue);
                    self.queue = late;
                }
                // A cut-off turn that may not go on leaves the host's first message waiting for Enter.
                if let Some(first) = self.after_resume.take() {
                    self.queue.insert(0, first);
                }
                // The message is the owner's to send again once the reason is gone: back in an empty input.
                let back = !prompt.is_empty() && self.input.is_empty();
                if back {
                    self.input.insert_text(&prompt);
                }
                self.transcript.note(
                    if back {
                        tr!(not_sent_back, reason = reason)
                    } else {
                        tr!(not_sent, reason = reason)
                    },
                    Tone::Fault,
                );
                self.transcript.settle();
            }
            Report::Warmed(Ok(usage)) => {
                if let Some(prices) = self.engine.prices {
                    *self.cost.get_or_insert(0.0) += prices.cost_usd(&usage);
                }
                self.usage += usage;
                // A read that came back as a turn began is paid for, but the turn's own reads say what is cached.
                if usage.cached_input_tokens > 0 && !self.working() {
                    self.cache_seen = true;
                    self.warm_until = Some(Instant::now() + crate::warm::LIFE);
                }
                self.spent(usage);
            }
            Report::Warmed(Err(reason)) => {
                self.keeper = None;
                self.transcript
                    .note(tr!(warm_ended, reason = reason), Tone::Quiet);
            }
            // T36c: only the screen of several panes delivers; a pane alone has no one to tell.
            Report::Tell { reply, .. } => {
                let _ = reply.send(Err("there is no other pane".to_owned()));
            }
            // T37c: `glut_file` asks for a model of an agent's or a skill's own — every provider's list.
            Report::PickModel(reply) => {
                // A second ask answers the first with none: one list on the screen at a time.
                if let Some(earlier) = self.model_pick.replace(reply) {
                    let _ = earlier.send(None);
                }
                let mut picker = self.models_picker(None, true);
                picker.title = format!("{} · {}", tr!(pick_own_model), picker.title);
                picker.asks_model = true;
                self.picker = Some(picker);
                self.model_pick_opened = Instant::now();
            }
            Report::ChooseModel(provider) => {
                self.busy = None;
                self.transcript
                    .note(tr!(provider_no_model, provider = provider), Tone::Quiet);
                self.picker = Some(self.models_picker(Some(&provider), false));
            }
            Report::Shell {
                id,
                command,
                ran,
                to_model,
            } => {
                self.transcript.take(&glut::Event::ToolResult {
                    turn: 0,
                    id,
                    name: "shell".into(),
                    ok: ran.ok,
                    output: ran.output.clone(),
                    truncated: false,
                });
                // The output joins the conversation as the owner's word: the model reads it with the next message
                // (on_report puts it in once no turn holds the conversation).
                if to_model {
                    self.pending
                        .push(Message::user(crate::shell::for_the_model(&command, &ran)));
                    self.say(tr!(shell_to_model));
                }
            }
            Report::Attached { name, kept } => self.attached(name, kept),
            Report::Diff(turn, change) => match change {
                Ok(change) if change.trim().is_empty() => self
                    .transcript
                    .note(tr!(step_changed_nothing, turn = turn), Tone::Quiet),
                Ok(change) => {
                    self.shells += 1;
                    let id = format!("diff-{}", self.shells);
                    self.transcript.push(crate::transcript::Kind::Tool {
                        id,
                        name: "diff".into(),
                        target: tr!(step_changes, turn = turn),
                        state: crate::transcript::ToolState::Done,
                        gate: None,
                        output: change,
                        unfolded: true,
                        depth: 0,
                    });
                    self.scroll = 0;
                }
                Err(reason) => self
                    .transcript
                    .note(tr!(no_change_to_show, reason = reason), Tone::Fault),
            },
            Report::Review(scope, since, change) => match change {
                Ok(change) => self.review.show(scope, since, &change),
                Err(reason) => self.transcript.note(
                    tr!(
                        no_change_to_review,
                        reason = crate::transcript::clean(&reason)
                    ),
                    Tone::Fault,
                ),
            },
            Report::Restored(result) => match result {
                Ok((commit, kept)) => self.transcript.note(
                    tr!(
                        files_restored,
                        snapshot = &commit[..commit.len().min(8)],
                        kept = kept
                    ),
                    Tone::Fine,
                ),
                Err(reason) => self
                    .transcript
                    .note(tr!(files_not_restored, reason = reason), Tone::Fault),
            },
            Report::Files(found) => {
                let (files, truncated) = found;
                self.files = Some((Instant::now(), files, truncated));
                // A file picker still waiting for the walk gets its files, with what was typed in it kept.
                if let Some(picker) = &self.picker
                    && picker.title.starts_with(tr!(files_title))
                {
                    let query = picker.query.clone();
                    let mut fresh = self.file_picker();
                    fresh.query = query;
                    self.picker = Some(fresh);
                }
            }
            Report::Switched(result) => {
                self.busy = None;
                match result {
                    Ok((engine, notes)) => {
                        if self.cost.is_some() && engine.prices.is_none() {
                            self.transcript
                                .note(tr!(no_prices, model = engine.model_name()), Tone::Quiet);
                        }
                        self.engine = engine;
                        // Another model has a cache of its own: the old one's keeping ends, the life is asked anew.
                        self.forget_warm();
                        self.engine
                            .model
                            .set_cache_hour(self.cache_mode == crate::warm::CacheMode::Hour);
                        // The next start begins where the owner switched to.
                        if let Err(reason) = session::keep_choice(&self.parts.home, &self.engine) {
                            self.transcript
                                .note(tr!(choice_unkept, reason = reason), Tone::Quiet);
                        }
                        // The new model has read nothing of this session yet: the window fills on its next call.
                        self.context_used = 0;
                        for note in notes {
                            self.transcript.note(note, Tone::Quiet);
                        }
                        self.transcript.note(
                            tr!(
                                model_now,
                                provider = self.engine.provider,
                                model = self.engine.model_name()
                            ),
                            Tone::Fine,
                        );
                    }
                    Err(reason) => self.transcript.note(reason, Tone::Fault),
                }
            }
        }
    }

    /// A key, a paste, a click or the wheel.
    pub fn on_terminal(&mut self, event: TermEvent) {
        match event {
            TermEvent::Key(key) if key.kind != KeyEventKind::Release => {
                self.on_key(key);
                self.settle_model_pick();
                self.refresh_paths();
            }
            // The reading mode and the panel's focus type nothing, a paste neither (TUI9a).
            TermEvent::Paste(text)
                if self.ask.is_none()
                    && self.picker.is_none()
                    && self.keys_sheet.is_none()
                    && self.reading.is_none()
                    && !self.panel_focus =>
            {
                match &mut self.search {
                    // A paste while searching is what to search for.
                    Some(search) => {
                        search.query.push_str(&text.replace(['\n', '\r'], " "));
                        search.fresh = true;
                    }
                    None => {
                        // A paste — a drop — of image files is the images (T45).
                        let home = self.parts.home.clone();
                        match crate::pictures::pasted_paths(&text, Some(&home)) {
                            Some(paths) => self.attach_files(&paths),
                            None => self.input.paste(&text),
                        }
                        self.refresh_paths();
                    }
                }
            }
            TermEvent::Mouse(mouse) => match mouse.kind {
                // Over a list, the review or the plan the stream below takes no wheel and no click.
                MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
                    if self.picker.is_some() || self.review.open => {}
                MouseEventKind::ScrollUp if self.keys_sheet.is_some() => {
                    if let Some(scroll) = &mut self.keys_sheet {
                        *scroll = scroll.saturating_sub(WHEEL_LINES);
                    }
                }
                MouseEventKind::ScrollDown if self.keys_sheet.is_some() => {
                    if let Some(scroll) = &mut self.keys_sheet {
                        *scroll = scroll.saturating_add(WHEEL_LINES);
                    }
                }
                // The wheel over the Keys tab rolls the sheet, never the stream beside it.
                MouseEventKind::ScrollUp if self.over_keys_tab(mouse.column) => {
                    self.keys_scroll = self.keys_scroll.saturating_sub(WHEEL_LINES);
                }
                MouseEventKind::ScrollDown if self.over_keys_tab(mouse.column) => {
                    self.keys_scroll = self.keys_scroll.saturating_add(WHEEL_LINES);
                }
                // Over the Plan, Gate or Meter tab the wheel rolls the tab (TUI9a).
                MouseEventKind::ScrollUp if self.over_scrolled_tab(mouse.column) => {
                    self.panel_scroll = self.panel_scroll.saturating_sub(WHEEL_LINES);
                }
                MouseEventKind::ScrollDown if self.over_scrolled_tab(mouse.column) => {
                    self.panel_scroll = self.panel_scroll.saturating_add(WHEEL_LINES);
                }
                MouseEventKind::ScrollUp if self.plan_view.is_some() => {
                    if let Some(view) = &mut self.plan_view {
                        view.scroll = view.scroll.saturating_sub(WHEEL_LINES);
                    }
                }
                MouseEventKind::ScrollDown if self.plan_view.is_some() => {
                    if let Some(view) = &mut self.plan_view {
                        view.scroll = view.scroll.saturating_add(WHEEL_LINES);
                    }
                }
                MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left)
                    if self.overlaid() =>
                {
                    self.press = None;
                }
                MouseEventKind::ScrollUp if self.agent_view.is_some() => {
                    if let Some(view) = &mut self.agent_view {
                        view.scroll = view.scroll.saturating_add(WHEEL_LINES);
                    }
                }
                MouseEventKind::ScrollDown if self.agent_view.is_some() => {
                    if let Some(view) = &mut self.agent_view {
                        view.scroll = view.scroll.saturating_sub(WHEEL_LINES);
                    }
                }
                MouseEventKind::ScrollUp => self.scroll_by(WHEEL_LINES as isize),
                MouseEventKind::ScrollDown => self.scroll_by(-(WHEEL_LINES as isize)),
                MouseEventKind::Down(MouseButton::Left) => {
                    self.selection = None;
                    self.press = (mouse.column < self.stream_width)
                        .then(|| self.point_at(mouse.column, mouse.row))
                        .flatten()
                        .map(|point| (point, self.stream_layout));
                }
                MouseEventKind::Drag(MouseButton::Left) => self.drag(mouse.column, mouse.row),
                MouseEventKind::Up(MouseButton::Left) => self.release(mouse.row),
                _ => {}
            },
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if ctrl && matches!(key.code, KeyCode::Char('c' | 'C')) {
            return self.interrupt();
        }
        // ⌃P or ⌃] the other pane (T36a) — whatever this one shows: it is how the owner reaches the other's question.
        // ⌃P is a letter, the same key on every layout. Where `]` takes ⌥, ⌃] arrives as ⌃⌥ and the digit under it:
        // ⌃⌥6 on a German Mac, ⌃⌥9 from the owner's Voyager (seen 2026-10-24) — both taken, as the owner reaches
        // them with one finger; a terminal without the keyboard protocol sends ⌃] as ⌃5.
        let bracket = match key.code {
            KeyCode::Char(']' | '5' | '6') => true,
            KeyCode::Char('9') => alt,
            _ => false,
        };
        if ctrl
            && (bracket || matches!(key.code, KeyCode::Char('p' | 'P')))
            && key.kind != KeyEventKind::Repeat
        {
            self.chord = false;
            self.pane_request = Some(crate::panes::Request::Toggle);
            return;
        }
        self.quit_armed = None;
        // Esc Esc is two Esc in a row: any other key between them makes it none.
        if key.code != KeyCode::Esc {
            self.last_esc = None;
        }
        // A question or a list over the screen takes the keys from the To-dos list for good: after it, keys type again.
        if self.plan_asks() || self.ask.is_some() || self.picker.is_some() {
            self.project_todos.keys = false;
        }
        if self.plan_asks() {
            return self.on_plan_key(key);
        }
        if self.ask.is_some() {
            return self.on_ask_key(key);
        }
        if self.picker.is_some() {
            return self.on_picker_key(key);
        }
        if self.keys_sheet.is_some() {
            return self.on_sheet_key(key);
        }
        if self.review.open {
            let action = self.review.key(key);
            return self.review_do(action);
        }
        if self.plan_view.is_some() {
            return self.on_plan_key(key);
        }
        if self.agent_view.is_some() {
            return self.on_agent_key(key);
        }
        if self.picture_view.is_some() {
            return self.on_picture_key(key);
        }
        if self.reading.is_some() && self.on_reading_key(key) {
            return;
        }
        if self.panel_focus && self.on_panel_key(key) {
            return;
        }
        if self.agents_list.is_some() && self.on_agents_list_key(key) {
            return;
        }
        if self.project_todos.keys && self.on_todos_key(key) {
            return;
        }
        if self.search.is_some() {
            return self.on_search_key(key);
        }
        if key.code == KeyCode::F(3) {
            self.scrollback_request = true;
            return;
        }
        if key.code == KeyCode::F(2) {
            self.mouse_request = Some(!self.mouse_held);
            return;
        }
        if ctrl && matches!(key.code, KeyCode::Char('f')) {
            self.search = Some(Search::default());
            return;
        }
        // The vi mode sees a key before the input does: Normal mode's keys are its own, Insert mode types.
        if let Some(vi) = &mut self.vi
            && !self.chord
        {
            match vi.key(&mut self.input, key) {
                crate::vi::Outcome::Done => return,
                crate::vi::Outcome::Pass => {}
            }
        }
        if ctrl && matches!(key.code, KeyCode::Char('k')) {
            self.chord = false;
            return self.show_keys();
        }
        // `@` at a word's start opens the folder's files to mention; Esc keeps the `@` as typed.
        if key.code == KeyCode::Char('@') && self.input.at_word_start() {
            self.chord = false;
            return self.mention();
        }
        let suggestions = self.suggestions();
        // A chord's second key is the chord's, not the list's (`⌃X ↑` over a typed `/`).
        if !suggestions.is_empty() && !self.chord {
            let chosen = suggestions[self.suggested.min(suggestions.len() - 1)].name;
            match key.code {
                KeyCode::Up => return self.suggested = self.suggested.saturating_sub(1),
                KeyCode::Down => {
                    return self.suggested = (self.suggested + 1).min(suggestions.len() - 1);
                }
                KeyCode::Tab => {
                    self.input.clear();
                    return self.input.insert_text(&format!("/{chosen} "));
                }
                // Enter on a lone `/` or on what a command does only fills the chosen one in, as Tab does: it runs
                // only a command whose name was typed (`/` Enter must not be `/clear`, nor `/new` be it).
                KeyCode::Enter
                    if !shift
                        && !alt
                        && !self.input.text().get(1..).is_some_and(|typed| {
                            !typed.is_empty() && chosen.starts_with(typed)
                        }) =>
                {
                    self.input.clear();
                    return self.input.insert_text(&format!("/{chosen} "));
                }
                // Enter on a part of a name takes the chosen command.
                KeyCode::Enter if !shift && !alt && self.input.text() != format!("/{chosen}") => {
                    self.input.clear();
                    self.input.insert_text(&format!("/{chosen}"));
                }
                _ => {}
            }
        }
        // The path list (T68): ↑↓ pick, ⇥ completes as far as only one fits (else takes), ⏎ takes what ↑↓ picked,
        // Esc closes.
        if let Some(listing) = &self.paths
            && !self.chord
            && self.picker.is_none()
            && self.ask.is_none()
        {
            let last = listing.entries.len() - 1;
            let chosen = self.path_chosen.min(last);
            match key.code {
                KeyCode::Up if !shift && !ctrl && !alt => {
                    self.path_picked = true;
                    return self.path_chosen = chosen.saturating_sub(1);
                }
                KeyCode::Down if !shift && !ctrl && !alt => {
                    self.path_picked = true;
                    return self.path_chosen = (chosen + 1).min(last);
                }
                KeyCode::Tab => {
                    let start = listing.start.clone();
                    return match listing.common() {
                        Some(common) if listing.entries.len() > 1 => {
                            self.replace_path_start(&start, &common)
                        }
                        _ => self.take_path(chosen),
                    };
                }
                KeyCode::Enter if !shift && !alt && self.path_picked => {
                    return self.take_path(chosen);
                }
                // While Glut works, Esc stops the turn as ever; the list goes with the text it stood for.
                KeyCode::Esc => {
                    self.paths_closed = Some(self.input.text());
                    self.paths = None;
                    if !self.working() {
                        return;
                    }
                }
                _ => {}
            }
        }
        if self.picker.is_some() {
            self.chord = false;
        }
        if std::mem::take(&mut self.chord) {
            match key.code {
                KeyCode::Char('e' | 'E') => self.edit_request = Some(self.input.expanded()),
                // T36a, as Emacs has them; since T36f a new pane goes into the grid, so both split alike.
                KeyCode::Char('2' | '3') => {
                    self.pane_request = Some(crate::panes::Request::Split {
                        resume: false,
                        place: crate::panes::Place::Ask,
                        role: None,
                    })
                }
                // T36f, as Emacs widens and narrows a window: the main pane's share of the width.
                KeyCode::Char('}') => {
                    self.pane_request = Some(crate::panes::Request::Widen(crate::panes::SHARE_STEP))
                }
                KeyCode::Char('{') => {
                    self.pane_request =
                        Some(crate::panes::Request::Widen(-crate::panes::SHARE_STEP))
                }
                KeyCode::Char('0') => self.pane_request = Some(crate::panes::Request::Close),
                // T36e: an agent in the background — its role picked first —, or this pane sent there.
                KeyCode::Char('b' | 'B') => self.picker = Some(self.background_picker()),
                KeyCode::Char('h' | 'H') => self.pane_request = Some(crate::panes::Request::Hide),
                // TUI9a: the Agents tab (⌃A is the line's start, as in the shell), the reading mode, the panel's keys.
                KeyCode::Char('a' | 'A') => return self.show_agents(),
                KeyCode::Up => return self.start_reading(),
                KeyCode::Tab => return self.focus_panel(),
                _ => {}
            }
            // Another chord gives the Agents list's keys back: they are the input's after it.
            self.agents_list = None;
            return;
        }
        match key.code {
            KeyCode::Char('x') if ctrl => self.chord = true,
            KeyCode::Char('a') if ctrl => self.input.home(),
            // ⌃V: the clipboard's image into the message (T45) — the terminal's ⌘V pastes text alone.
            KeyCode::Char('v') if ctrl => self.attach_clipboard(),
            KeyCode::Char('y') if ctrl => self.show_todos(),
            KeyCode::Char('b') if ctrl => self.show_tab(Tab::Plan),
            KeyCode::Char('g') if ctrl => self.show_tab(Tab::Gate),
            // Ctrl+M is Enter to a terminal: the meter is on N.
            KeyCode::Char('n') if ctrl => self.show_tab(Tab::Meter),
            KeyCode::Char('r') if ctrl => self.open_review(crate::review::Scope::Turn),
            KeyCode::Enter if shift || alt => self.input.newline(),
            KeyCode::Char('j') if ctrl => self.input.newline(),
            KeyCode::Enter => {
                self.submit();
                // A message that went starts the next one in Insert mode, as `/vim on` does.
                if let Some(vi) = &mut self.vi
                    && self.input.is_empty()
                {
                    vi.mode = crate::vi::Mode::Insert;
                }
            }
            KeyCode::BackTab => self.cycle_mode(),
            // A selection goes first: Esc lets it go, the next one stops Glut.
            KeyCode::Esc if self.selection.is_some() => self.selection = None,
            KeyCode::Esc => {
                if let Some(cancel) = &self.running {
                    cancel.cancel();
                    self.last_esc = None;
                } else if self.working() {
                    // Compacting, handing off or switching: nothing to stop, nothing to rewind yet.
                    self.last_esc = None;
                } else if self
                    .last_esc
                    .is_some_and(|pressed| pressed.elapsed() < DOUBLE_ESC)
                {
                    // Esc Esc: go back to before one of the owner's messages (T11).
                    self.last_esc = None;
                    self.picker = Some(self.rewind_picker());
                } else {
                    self.last_esc = Some(Instant::now());
                }
            }
            KeyCode::Char('d') if ctrl && self.input.is_empty() && !self.working() => {
                self.quit = true
            }
            // Folding moves the lines a selection was on: it lets go.
            KeyCode::Char('o') if ctrl => {
                self.selection = None;
                self.transcript.unfold_all();
            }
            // Ctrl+T: every thought open, or all folded again (an "off" choice comes back as folded).
            KeyCode::Char('t') if ctrl => {
                let next = match self.transcript.thinking {
                    Thinking::Open => Thinking::Folded,
                    Thinking::Folded | Thinking::Off => Thinking::Open,
                };
                self.selection = None;
                self.set_thinking(next);
            }
            KeyCode::Char('w') if ctrl => self.input.delete_word(),
            KeyCode::Char('u') if ctrl => self.input.delete_to_start(),
            KeyCode::Char('e') if ctrl => self.input.end(),
            KeyCode::Char('b') if alt => self.input.word_left(),
            KeyCode::Char('f') if alt => self.input.word_right(),
            KeyCode::Backspace if alt || ctrl => self.input.delete_word(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left if alt || ctrl => self.input.word_left(),
            KeyCode::Right if alt || ctrl => self.input.word_right(),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Up if ctrl || alt => self.take_back(),
            KeyCode::Up if shift => self.scroll_by(1),
            KeyCode::Down if shift => self.scroll_by(-1),
            KeyCode::Up => self.input.up(),
            KeyCode::Down => self.input.down(),
            KeyCode::PageUp => self.scroll_by(self.page()),
            KeyCode::PageDown => self.scroll_by(-self.page()),
            // Tab while Glut works queues the message for after the turn (T10; Alt+Enter is a new line in many
            // terminals); idle, it is a space.
            // A command's word half typed: ⇥ completes it (`/motion c` → `/motion calm`).
            KeyCode::Tab if self.input.at_end() => {
                let text = self.input.text();
                let completed = crate::commands::completion(&text);
                match completed {
                    Some(rest) => self.input.insert_text(&rest),
                    None if self.working() => self.queue_next(),
                    None => self.input.insert(' '),
                }
            }
            KeyCode::Tab if self.working() => self.queue_next(),
            KeyCode::Tab => self.input.insert(' '),
            KeyCode::Char(c) if !ctrl => {
                self.input.insert(c);
                self.suggested = 0;
            }
            _ => {}
        }
    }

    /// The names that fit the path at the input's end (T68), read anew after every key — only while the input has
    /// the keys and no command list is up.
    fn refresh_paths(&mut self) {
        let text = self.input.text();
        let free = self.picker.is_none()
            && self.ask.is_none()
            && self.search.is_none()
            && self.reading.is_none()
            && !self.panel_focus
            && self.input.at_end()
            && self.suggestions().is_empty()
            && self.paths_closed.as_deref() != Some(text.as_str());
        let projects = self.parts.projects;
        let listing = free
            .then(|| {
                crate::paths::typed(&text, |name| {
                    (name != "projects" || projects)
                        .then(|| crate::commands::takes_path(name))
                        .flatten()
                })
            })
            .flatten()
            .and_then(|typed| crate::paths::list(typed, &self.parts.home, &self.parts.config.cwd));
        // The same folder and start keep the owner's place in the list.
        let same = matches!(
            (&self.paths, &listing),
            (Some(old), Some(new)) if old.folder == new.folder && old.start == new.start
        );
        if !same {
            self.path_chosen = 0;
            self.path_picked = false;
        }
        if self.paths_closed.as_deref() != Some(text.as_str()) {
            self.paths_closed = None;
        }
        self.paths = listing;
    }

    /// The typed start of the path's last name replaced by `name`.
    fn replace_path_start(&mut self, start: &str, name: &str) {
        for _ in start.chars() {
            self.input.backspace();
        }
        self.input.insert_text(name);
    }

    /// Takes the name chosen in the path list: a folder with its `/`, the list going on in it; a file ends the list.
    fn take_path(&mut self, chosen: usize) {
        let Some(listing) = self.paths.take() else {
            return;
        };
        let entry = &listing.entries[chosen.min(listing.entries.len() - 1)];
        let name = if entry.dir {
            format!("{}/", entry.name)
        } else {
            entry.name.clone()
        };
        self.replace_path_start(&listing.start, &name);
        if !entry.dir {
            self.paths_closed = Some(self.input.text());
        }
    }

    /// The commands that fit what is typed: the input is a `/` and part of a name, nothing after it yet.
    pub fn suggestions(&self) -> Vec<&'static crate::commands::Command> {
        let text = self.input.text();
        let Some(typed) = text.strip_prefix('/') else {
            return Vec::new();
        };
        if typed.contains([' ', '\n', '/']) {
            return Vec::new();
        }
        let mut fitting = crate::commands::fitting(typed);
        if !self.parts.projects {
            fitting.retain(|command| command.name != "projects");
        }
        fitting
    }

    /// Enter: a command when the input is one, else the next message. A command that has to wait while Glut works
    /// stays in the input, like a message; a first word with a slash in it (`/usr/bin is missing`) is a message.
    fn submit(&mut self) {
        let text = self.input.text();
        // The first word sent, whatever it is, says the welcome was read (TUI9c).
        if self.first_start && !text.trim().is_empty() {
            self.first_start = false;
            if let Err(reason) = prefs::set_welcomed(&self.parts.home) {
                self.transcript.note(reason, Tone::Fault);
            }
        }
        if self.input.broken_pastes() > 0 {
            self.transcript.note(tr!(paste_lost), Tone::Fault);
            return;
        }
        // A message waits for the images still read — a while, not for good: a clipboard program that hangs leaves
        // the message free to go without its image.
        if !text.trim_start().starts_with('/')
            && self
                .attaching_since
                .is_some_and(|since| since.elapsed() < ATTACH_WAIT)
        {
            self.say(tr!(image_reading));
            return;
        }
        if let Some(command) = text.trim().strip_prefix('/') {
            let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
            if !name.is_empty() && !name.contains('/') {
                if self.working() && waits(name, rest.trim()) {
                    self.transcript
                        .note(tr!(command_waits, name = name), Tone::Quiet);
                    return;
                }
                let command = command.to_owned();
                self.input.take();
                return self.command(&command);
            }
        }
        // `!!cmd` runs for the owner alone, at any time; `!cmd` hands its output to the model, so it waits while a
        // turn holds the conversation. Only a first `!` right before the command makes one (the input shows it).
        if let Some((alone, command)) = owners_command(&text) {
            if !alone && self.working() {
                self.transcript.note(tr!(shell_waits), Tone::Quiet);
                return;
            }
            // The command as sent: a paste in it given back.
            let expanded = self.input.take();
            let command = owners_command(&expanded).map_or(command, |(_, command)| command);
            return self.shell(command, !alone);
        }
        // A message with images (T45) goes as a turn of its own, to a model that sees them.
        let images = self.input.images();
        if !images.is_empty() {
            if self.engine.images == Some(false) {
                return self.transcript.note(
                    tr!(model_no_images, model = self.engine.model_name()),
                    Tone::Fault,
                );
            }
            if self.working() {
                return self.say(tr!(images_wait));
            }
            // Numbered now, on from the whole conversation.
            let first = self.first_to_send();
            self.input.renumber(first);
            let images = self.input.images();
            self.images_sent = images
                .iter()
                .map(|image| image.number)
                .max()
                .unwrap_or(0)
                .max(self.images_sent);
            let prompt = self.input.take();
            return self.send_with(prompt, images);
        }
        if self.working() {
            // While a turn runs, Enter steers it: the message joins before its next step. While Glut compacts or
            // switches, it waits as the next message.
            if self.input.is_empty() {
                return;
            }
            let text = self.input.take();
            // A steer or a queued message is the owner's word too (T36c: it ends a chain between models).
            self.owner_spoke = true;
            match &self.steering {
                Some(inbox) => {
                    inbox.send(text);
                    self.say(tr!(steer_taken));
                }
                None => {
                    self.queue.push(text);
                    self.send_when_free = true;
                    self.say(tr!(queued_free));
                }
            }
            return;
        }
        // Enter on an empty input lets a held message of another pane's model go on (T36c) — the owner read it in
        // the stream — before what waits in the queue (after a turn the owner stopped).
        if self.input.is_empty()
            && !self.held.is_empty()
            && self
                .held_since
                .is_none_or(|since| since.elapsed() >= ASK_SETTLE)
        {
            let next = self.held.remove(0);
            self.owner_spoke = true;
            return self.send(next);
        }
        if self.input.is_empty() && !self.queue.is_empty() {
            let next = self.queue.remove(0);
            return self.send(next);
        }
        let prompt = self.input.take();
        self.send(prompt);
    }

    /// Tab while Glut works: the message waits until the turn is done and goes as the next one.
    fn queue_next(&mut self) {
        if self.input.is_empty() {
            return;
        }
        // A queued message is text: one with images waits in the input until the turn is done (T45).
        if !self.input.images().is_empty() {
            return self.say(tr!(images_wait));
        }
        let text = self.input.take();
        self.queue.push(text);
        self.owner_spoke = true;
        self.say(tr!(queued_turn));
    }

    /// Alt+↑ (or Ctrl+↑, which macOS keeps for Mission Control): the last message waiting — queued, or sent to steer
    /// and not yet taken — back into the input.
    fn take_back(&mut self) {
        if !self.input.is_empty() {
            return self.say(tr!(input_full));
        }
        let back = self
            .queue
            .pop()
            .or_else(|| self.steering.as_ref().and_then(glut::Inbox::take_back));
        match back {
            Some(text) => self.input.insert_text(&text),
            None => self.say(tr!(nothing_waits)),
        }
    }

    /// Runs the owner's own command in the folder, beside the session; its output comes back as a report.
    fn shell(&mut self, command: String, to_model: bool) {
        if !self.parts.owner_shell {
            return self.transcript.note(tr!(no_shell), Tone::Fault);
        }
        self.shells += 1;
        let id = format!("shell-{}", self.shells);
        self.transcript.push(crate::transcript::Kind::Tool {
            id: id.clone(),
            name: if to_model { "!" } else { "!!" }.into(),
            target: command.clone(),
            state: crate::transcript::ToolState::Running,
            gate: Some(
                if to_model {
                    tr!(shell_for_glut)
                } else {
                    tr!(shell_alone)
                }
                .into(),
            ),
            output: String::new(),
            unfolded: true,
            depth: 0,
        });
        let cwd = self.parts.config.cwd.clone();
        let reports = self.reports.clone();
        tokio::spawn(async move {
            let ran = crate::shell::run(&command, &cwd).await;
            let _ = reports.send(Report::Shell {
                id,
                command,
                ran,
                to_model,
            });
        });
    }

    /// `@` and the file picker after it: a space first where the cursor is inside a word.
    fn mention(&mut self) {
        if !self.input.at_word_start() {
            self.input.insert(' ');
        }
        self.input.insert('@');
        // The walk runs beside the screen; a minute-old one is used as it is.
        let fresh = self
            .files
            .as_ref()
            .is_some_and(|(walked, _, _)| walked.elapsed() < FILES_FRESH);
        if !fresh {
            let cwd = self.parts.config.cwd.clone();
            let reports = self.reports.clone();
            tokio::task::spawn_blocking(move || {
                let found = axiomata_files::Root::dir(&cwd, axiomata_files::LinkPolicy::Contained)
                    .ok()
                    .and_then(|root| axiomata_files::index_files(&root).ok())
                    .map_or((Vec::new(), false), |index| (index.files, index.truncated));
                let _ = reports.send(Report::Files(found));
            });
        }
        self.picker = Some(self.file_picker());
    }

    /// The folder's files to mention after `@`, as the file tools see them (`.gitignore` kept) — while the walk
    /// runs, an empty list that says so. A name a terminal would act on, or that hides what it is, is left out.
    fn file_picker(&self) -> Picker {
        let Some((_, files, truncated)) = &self.files else {
            return Picker::new(tr!(files_looking), Vec::new(), None);
        };
        let items = files
            .iter()
            .filter(|path| crate::transcript::clean(path) == **path)
            .map(|path| Item::plain(path.clone(), "", Pick::File(path.clone())))
            .collect();
        let title = if *truncated {
            tr!(files_truncated).to_owned()
        } else {
            tr!(files_title).to_owned()
        };
        Picker::new(title, items, None)
    }

    /// Shift+Tab: plan → ask → build → auto → plan, from the next call on.
    fn cycle_mode(&mut self) {
        // Another mode offers other tools: the kept head is no longer the next turn's.
        self.forget_warm();
        if self.parts.mode_locked {
            return self.transcript.note(tr!(mode_locked), Tone::Fault);
        }
        let Some(gate) = self.parts.tools.gate() else {
            return;
        };
        let next = session::next_mode(gate.mode());
        gate.set_mode(next);
        let mut what = match next {
            Mode::Plan => tr!(mode_plan).to_owned(),
            Mode::Default => tr!(mode_ask).to_owned(),
            Mode::AcceptEdits => tr!(mode_build).to_owned(),
            // Auto leans on the sandbox; where there is none, the owner hears it before Glut does anything.
            Mode::Auto if session::sandboxed(&self.parts) => tr!(mode_auto).to_owned(),
            Mode::Auto => tr!(mode_auto_unsandboxed).to_owned(),
            Mode::DontAsk => tr!(mode_dont_ask).to_owned(),
        };
        if self.running.is_some() {
            what.push_str(&format!(" ({})", tr!(mode_next_message)));
        }
        let tone = if next == Mode::Auto && !session::sandboxed(&self.parts) {
            Tone::Fault
        } else {
            Tone::Quiet
        };
        self.say_switch(Switch::Mode, what, tone);
    }

    /// Where a switch went, said: one gone round again and again leaves one line, where it stands — not a wall of
    /// them.
    fn say_switch(&mut self, switch: Switch, what: impl Into<String>, tone: Tone) {
        if self.switch_note == Some((switch, self.transcript.blocks.len()))
            && let Some(last) = self.transcript.blocks.last()
            && matches!(last.kind, crate::transcript::Kind::Note { .. })
        {
            self.transcript.blocks.pop();
        }
        self.transcript.note(what, tone);
        self.switch_note = Some((switch, self.transcript.blocks.len()));
    }

    /// One of the session's own commands (`/model`, `/context` …).
    fn command(&mut self, line: &str) {
        let (name, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        match name {
            "help" | "?" => self.transcript.note(crate::commands::help(), Tone::Quiet),
            "quit" | "exit" | "q" => self.quit = true,
            "clear" => {
                self.forget_warm();
                self.selection = None;
                self.search = None;
                self.pending.clear();
                self.queue.clear();
                self.forget_models();
                self.conversation.clear();
                self.todos.clear();
                self.forget_views();
                self.last_turn = None;
                // A new session counts from nothing.
                self.cost = None;
                self.usage = Usage::default();
                self.meter = crate::meter::Meter::default();
                self.review = crate::review::Review::default();
                self.session = Session::new(
                    &self.parts.config.cwd,
                    &self.engine.provider,
                    self.engine.model_name(),
                );
                self.transcript = Transcript {
                    thinking: self.transcript.thinking,
                    ..Transcript::default()
                };
                self.context_used = 0;
                self.scroll = 0;
            }
            // The models of the provider in use only: another provider is chosen with /provider first.
            "model" if rest.is_empty() => {
                let provider = self.engine.provider.clone();
                self.picker = Some(self.models_picker(Some(&provider), false));
            }
            "model" => self.switch(
                None,
                Some(rest.to_owned()),
                Some(self.engine.endpoint.effort),
            ),
            "provider" if rest.is_empty() => self.picker = Some(self.provider_picker()),
            "provider" => self.switch(
                Some(rest.to_owned()),
                None,
                Some(self.engine.endpoint.effort),
            ),
            "effort" if rest.is_empty() => {
                self.picker = Some(effort_picker(self.engine.endpoint.effort))
            }
            "effort" => match rest {
                "default" => self.switch_effort(None),
                level => match Effort::parse_or_explain(level) {
                    Ok(effort) => self.switch_effort(Some(effort)),
                    Err(reason) => self.transcript.note(reason, Tone::Fault),
                },
            },
            "context" => self.context(rest),
            "compact" if self.refused_spend() => {}
            "compact" => {
                self.busy = Some(Busy::Compacting);
                self.transcript.note(tr!(compacting), Tone::Quiet);
                session::compact(
                    self.engine.clone(),
                    std::mem::take(&mut self.conversation),
                    self.reports.clone(),
                );
            }
            "editor" if rest.is_empty() => {
                self.picker = Some(editor_picker(Prefs::load(&self.parts.home).editor()))
            }
            "init" => self.init(rest),
            "memory" => self.memory(rest),
            "todo" => self.todo(rest),
            "skill" => self.skill(rest),
            "handoff" => match rest {
                // A paid call: written only when asked by name.
                "" => self.transcript.note(tr!(handoff_usage), Tone::Quiet),
                "write" if self.refused_spend() => {}
                "write" => {
                    self.busy = Some(Busy::HandingOff);
                    self.transcript.note(tr!(handoff_writing), Tone::Quiet);
                    session::write_handoff(
                        Arc::clone(&self.parts),
                        self.engine.clone(),
                        std::mem::take(&mut self.conversation),
                        self.reports.clone(),
                    );
                }
                "read" => match session::read_handoff(&self.parts) {
                    Ok((message, text, moved)) => {
                        self.conversation.push(message);
                        self.keep();
                        self.transcript.note(
                            format!(
                                "{}\n{}",
                                tr!(handoff_read, path = session::shown_folder(&moved)),
                                text.trim()
                            ),
                            Tone::Fine,
                        );
                    }
                    Err(reason) => self.transcript.note(reason, Tone::Fault),
                },
                other => self
                    .transcript
                    .note(tr!(handoff_unknown, word = other), Tone::Fault),
            },
            "editor" => self.set_editor(rest),
            "resume" => self.picker = Some(self.session_picker()),
            // T44: the host picks the folder where it keeps Glut in its own pane.
            "projects" if !self.parts.projects => self.tell(tr!(projects_hosted)),
            "projects" if rest.trim().is_empty() => self.picker = Some(self.projects_picker()),
            "projects" => {
                match crate::projects::folder_of(rest, &self.parts.home, &self.parts.config.cwd) {
                    Ok(folder) => self.switch_project(folder),
                    Err(reason) => self.tell(reason),
                }
            }
            "panel" => self.show_tab(Tab::Plan),
            "gate" => self.picker = Some(self.gate_picker()),
            "meter" => self.show_tab(Tab::Meter),
            "agents" => self.agents_picker(),
            "plan" => match &self.plan {
                Some(_) => self.plan_view = Some(crate::plan::Dialog::reading()),
                None => self.transcript.note(tr!(no_plan_yet), Tone::Quiet),
            },
            "diff" | "review" => self.open_review(match rest {
                "session" => crate::review::Scope::Session,
                _ => crate::review::Scope::Turn,
            }),
            "scrollback" => self.scrollback_request = true,
            "vim" => match rest {
                "on" | "" => self.set_vi(true),
                "off" => self.set_vi(false),
                other => self
                    .transcript
                    .note(tr!(vim_unknown, word = other), Tone::Fault),
            },
            "footer" => match rest {
                "powerline" => self.set_footer(crate::footer::Separators::Powerline),
                "plain" => self.set_footer(crate::footer::Separators::Plain),
                "" => {
                    let why = match self.separators_from {
                        Some(Some(terminal)) => tr!(footer_guessed, terminal = terminal),
                        Some(None) => tr!(footer_guessed_unknown).to_owned(),
                        None => tr!(your_choice).to_owned(),
                    };
                    self.transcript.note(
                        tr!(footer_now, footer = self.separators.label(), why = why),
                        Tone::Quiet,
                    );
                }
                other => self
                    .transcript
                    .note(tr!(footer_unknown, word = other), Tone::Fault),
            },
            "split" => match crate::panes::parse_split(rest) {
                Ok(request) => self.pane_request = Some(request),
                Err(reason) => self.transcript.note(reason, Tone::Fault),
            },
            "close" if rest.is_empty() => self.pane_request = Some(crate::panes::Request::Close),
            "close" => self.pane_request = Some(crate::panes::Request::End(rest.to_owned())),
            "bg" => match crate::panes::parse_background(rest) {
                Ok(request) => self.pane_request = Some(request),
                Err(reason) => self.transcript.note(reason, Tone::Fault),
            },
            "show" if rest.is_empty() => self.tell(tr!(show_usage)),
            "show" => self.pane_request = Some(crate::panes::Request::Show(rest.to_owned())),
            "hide" => self.pane_request = Some(crate::panes::Request::Hide),
            "swap" if rest.is_empty() => {
                self.pane_request = Some(crate::panes::Request::Swap(None))
            }
            "swap" => self.pane_request = Some(crate::panes::Request::Swap(Some(rest.to_owned()))),
            "peek" if rest.is_empty() => {
                self.pane_request = Some(crate::panes::Request::Peek(None))
            }
            "peek" => self.pane_request = Some(crate::panes::Request::Peek(Some(rest.to_owned()))),
            "cache" => match crate::warm::CacheMode::parse(rest) {
                Some(mode) => self.set_cache(mode),
                None => self.tell(tr!(cache_now, cache = self.cache_mode.label())),
            },
            "role" if rest.is_empty() => self.picker = Some(self.roles_picker()),
            "role" => match rest {
                "none" | "all-rounder" => self.play(None),
                name => self.play(Some(name)),
            },
            "send" => match crate::panes::parse_send(rest) {
                Ok(request) => self.pane_request = Some(request),
                Err(reason) => self.transcript.note(reason, Tone::Fault),
            },
            "steps" => self.picker = Some(self.steps_picker()),
            "motion" => match crate::motion::Motion::parse(rest) {
                Some(motion) => self.set_motion(motion),
                None => {
                    let now = Prefs::load(&self.parts.home).motion.unwrap_or_default();
                    let still = if crate::motion::still() && now == crate::motion::Motion::Full {
                        format!(" ({})", tr!(motion_held))
                    } else {
                        String::new()
                    };
                    self.tell(tr!(motion_now, motion = now.label(), still = still));
                }
            },
            "notify" => self.notify_command(rest),
            "export" => self.export_command(rest),
            "links" => match crate::links::Links::parse(rest) {
                Some(links) => {
                    crate::links::set(Some(links));
                    crate::motion::relook();
                    match prefs::set_links(&self.parts.home, links) {
                        Ok(()) if links == crate::links::Links::Clickable => {
                            self.say(tr!(links_clickable))
                        }
                        Ok(()) => self.say(tr!(links_plain)),
                        Err(reason) => self.transcript.note(reason, Tone::Fault),
                    }
                }
                None => self.tell(tr!(
                    links_now,
                    links = if crate::links::clickable() {
                        "clickable"
                    } else {
                        "plain"
                    }
                )),
            },
            "images" => match crate::pictures::Drawing::parse(rest) {
                Some(drawing) => {
                    crate::pictures::set_drawing(Some(drawing));
                    crate::motion::relook();
                    match prefs::set_images(&self.parts.home, drawing) {
                        Ok(()) => self.say(tr!(images_set, images = drawing.label())),
                        Err(reason) => self.transcript.note(reason, Tone::Fault),
                    }
                }
                None => self.tell(tr!(images_now, images = crate::pictures::drawing().label())),
            },
            "reader" => match rest {
                "on" => self.set_reader(true),
                "off" => self.set_reader(false),
                _ => self.tell(tr!(
                    reader_now,
                    reader = if crate::motion::reader() { "on" } else { "off" }
                )),
            },
            "theme" if rest.is_empty() => self.picker = Some(theme_picker()),
            "language" if rest.is_empty() => self.picker = Some(language_picker()),
            "language" => match glut::lang::Lang::parse(rest) {
                Some(lang) => self.set_language(lang),
                None => {
                    let known: Vec<&str> = glut::lang::Lang::ALL
                        .iter()
                        .map(|lang| lang.code())
                        .collect();
                    let said = tr!(
                        language_unknown,
                        name = crate::transcript::clean(rest),
                        known = known.join(" ")
                    );
                    self.transcript.note(said, Tone::Fault);
                }
            },
            "theme" => self.set_theme(rest),
            "thinking" => match rest {
                "off" => self.set_thinking(Thinking::Off),
                "folded" | "fold" | "" => self.set_thinking(Thinking::Folded),
                "open" | "on" => self.set_thinking(Thinking::Open),
                other => self
                    .transcript
                    .note(tr!(thinking_unknown, word = other), Tone::Fault),
            },
            other => self
                .transcript
                .note(tr!(no_command, name = other), Tone::Fault),
        }
    }

    /// Builds the engine of another provider or model in the background; the session goes on with the same
    /// conversation. `effort`: `None` the owner's, `Some(None)` the provider's own.
    fn switch(
        &mut self,
        provider: Option<String>,
        model: Option<String>,
        effort: Option<Option<Effort>>,
    ) {
        if self.parts.fixed_engine {
            return self.transcript.note(tr!(model_locked), Tone::Fault);
        }
        self.busy = Some(Busy::Switching);
        let provider = provider.or_else(|| Some(self.engine.provider.clone()));
        session::switch(
            Arc::clone(&self.parts),
            provider,
            model,
            effort,
            self.reports.clone(),
        );
    }

    fn switch_effort(&mut self, effort: Option<Effort>) {
        let model = Some(self.engine.model_name().to_owned());
        self.switch(None, model, Some(effort));
    }

    /// `/context`: how full the window is; `/context <tokens>` sets it for this model and keeps it.
    fn context(&mut self, rest: &str) {
        if rest.is_empty() {
            let text = match self.engine.window {
                Some((window, from)) => tr!(
                    context_known,
                    used = self.context_used,
                    window = window,
                    share = self.context_used * 100 / u64::from(window.max(1)),
                    from = match from {
                        session::WINDOW_SET_HERE => tr!(window_set_here),
                        session::WINDOW_FROM_LIST => tr!(window_from_list),
                        other => other,
                    }
                ),
                None => tr!(
                    context_unknown,
                    used = self.context_used,
                    model = self.engine.model_name()
                ),
            };
            self.transcript.note(text, Tone::Quiet);
            // T35: the guides ride along on every turn — what they cost, so a growing one is seen.
            if self.parts.guide_chars > 0 {
                self.transcript.note(
                    tr!(guides_cost, tokens = self.parts.guide_chars / 4),
                    Tone::Quiet,
                );
            }
            return;
        }
        let Some(tokens) = parse_tokens(rest) else {
            self.transcript
                .note(tr!(context_unknown_word, word = rest), Tone::Fault);
            return;
        };
        match session::keep_window(&self.parts.home, &self.engine, tokens) {
            Ok(()) => {
                self.engine.window = Some((tokens, session::WINDOW_SET_HERE));
                self.transcript.note(
                    tr!(
                        context_set,
                        model = self.engine.model_name(),
                        tokens = tokens
                    ),
                    Tone::Fine,
                );
            }
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// `/init` (T35): a short guide for the folder. With only a `CLAUDE.md` the owner is asked once whether it moves
    /// into `AGENTS.md` — a no is kept, and the guide checked is then `CLAUDE.md` itself; `--migrate` asks again.
    fn init(&mut self, rest: &str) {
        let migrate = rest.trim() == "--migrate";
        if !rest.trim().is_empty() && !migrate {
            return self.transcript.note(tr!(init_usage), Tone::Fault);
        }
        // The nearest guide the session read decides: an `AGENTS.md` is checked, a `CLAUDE.md` (read only where its
        // folder has no `AGENTS.md`) is the question — none, and a new one is written in the working folder.
        let nearest = self.parts.guide_paths.last().cloned();
        let named =
            |path: &std::path::Path, name: &str| path.file_name().is_some_and(|file| file == name);
        let shown = |path: &std::path::Path| {
            path.strip_prefix(&self.parts.config.cwd).map_or_else(
                |_| path.display().to_string(),
                |rel| rel.display().to_string(),
            )
        };
        let claude = nearest.as_ref().filter(|path| named(path, "CLAUDE.md"));
        let folder = claude
            .and_then(|path| path.parent())
            .unwrap_or(&self.parts.config.cwd)
            .display()
            .to_string();
        if migrate && let Err(reason) = prefs::set_keep_claude(&self.parts.home, &folder, false) {
            self.transcript.note(reason, Tone::Fault);
        }
        let kept = Prefs::load(&self.parts.home).keep_claude.contains(&folder);
        if let Some(claude) = claude
            && !kept
        {
            self.init_claude = Some(shown(claude));
            self.picker = Some(Picker::new(
                tr!(init_claude_title),
                vec![
                    Item::plain(
                        tr!(init_claude_yes),
                        tr!(init_claude_yes_detail),
                        Pick::Init(true),
                    ),
                    Item::plain(
                        tr!(init_claude_no),
                        tr!(init_claude_no_detail),
                        Pick::Init(false),
                    ),
                ],
                None,
            ));
            return;
        }
        let (prompt, guide) = match nearest {
            Some(path) => (init_check(&shown(&path)), path),
            None => (INIT_NEW.to_owned(), self.parts.config.cwd.join("AGENTS.md")),
        };
        self.send_init(prompt, guide);
    }

    /// `/memory` (T35): the facts the project's memory keeps, each with its description — read here, no model asked;
    /// `/memory open` hands the index to the owner's editor.
    fn memory(&mut self, rest: &str) {
        let Some(dir) = self.parts.memory.clone() else {
            return self.transcript.note(tr!(no_memory), Tone::Quiet);
        };
        match rest.trim() {
            "open" => {
                let index = dir.join(glut::tools::memory::INDEX_FILE);
                if index.is_file() {
                    self.open_request = Some(index);
                } else {
                    self.transcript
                        .note(tr!(memory_empty, folder = dir.display()), Tone::Quiet);
                }
            }
            "" => {
                let facts = crate::memory::facts(&dir);
                if facts.is_empty() {
                    return self
                        .transcript
                        .note(tr!(memory_empty, folder = dir.display()), Tone::Quiet);
                }
                let items = facts
                    .into_iter()
                    .map(|(name, about)| Item::plain(name.clone(), about, Pick::MemoryFact(name)))
                    .collect();
                self.picker = Some(Picker::new(
                    tr!(memory_title, folder = dir.display()),
                    items,
                    None,
                ));
            }
            _ => self.transcript.note(tr!(memory_usage), Tone::Fault),
        }
    }

    /// One fact of the memory, whole, in the stream.
    fn show_fact(&mut self, name: &str) {
        let Some(dir) = &self.parts.memory else {
            return;
        };
        match crate::memory::read(&dir.join(format!("{name}.md"))) {
            Some(text) => self
                .transcript
                .note(format!("{name}:\n{text}"), Tone::Quiet),
            None => self
                .transcript
                .note(tr!(cannot_read, name = name), Tone::Fault),
        }
    }

    /// `/todo` (T41): alone the To-dos tab with the keys; `open` the list in the owner's editor; `clean` drops the done
    /// items after a question; any other text is a new item at the list's end — at once, while Glut works too.
    fn todo(&mut self, rest: &str) {
        let cwd = self.parts.config.cwd.clone();
        self.project_todos.refresh(&cwd);
        let name = self.project_todos.name();
        match rest {
            "" => self.show_todos_keyed(),
            "open" => self.open_todos(),
            "clean" => {
                let done = self.project_todos.done();
                if done == 0 {
                    return self
                        .transcript
                        .note(tr!(todos_nothing_done, name = name), Tone::Quiet);
                }
                self.picker = Some(Picker::new(
                    tr!(todos_drop_title, done = done, name = name),
                    vec![
                        Item::plain(
                            tr!(todos_drop_yes),
                            tr!(todos_drop_yes_detail),
                            Pick::TodoClean,
                        ),
                        Item::plain(tr!(todos_drop_no), "", Pick::Shown),
                    ],
                    None,
                ));
            }
            item => match self.project_todos.add(&cwd, item) {
                Ok(()) => self
                    .transcript
                    .note(tr!(todos_added, name = name, item = item), Tone::Quiet),
                Err(failure) => self.transcript.note(failure.message(&name), Tone::Fault),
            },
        }
    }

    /// `/skill` (T37c): alone the skills to pick from; with a name the skill runs — the model loads it with the `skill`
    /// tool, so a maker skill brings `glut_file` for the rest of the conversation — on what follows the name.
    fn skill(&mut self, rest: &str) {
        let (name, text) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if name.is_empty() {
            if self.parts.skills.is_empty() {
                return self.transcript.note(tr!(no_skills), Tone::Quiet);
            }
            let items = self
                .parts
                .skills
                .iter()
                .map(|(name, about)| {
                    Item::plain(
                        name.clone(),
                        crate::transcript::clean(about),
                        Pick::Skill(name.clone()),
                    )
                })
                .collect();
            self.picker = Some(Picker::new(tr!(skills_title), items, None));
            return;
        }
        if !self.parts.skills.iter().any(|(known, _)| known == name) {
            let known: Vec<&str> = self
                .parts
                .skills
                .iter()
                .map(|(known, _)| known.as_str())
                .collect();
            return self.transcript.note(
                tr!(
                    no_skill,
                    name = name,
                    known = if known.is_empty() {
                        tr!(there_are_none).to_owned()
                    } else {
                        known.join(", ")
                    }
                ),
                Tone::Fault,
            );
        }
        let text = text.trim();
        let mut prompt =
            format!("Run the skill {name}: load it with the `skill` tool first, then follow it.");
        if !text.is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(text);
        }
        self.send(prompt);
    }

    /// A `pick_model` whose list is no longer on the screen (another list, a dialog, a stop took its place) is
    /// answered with none, so the tool never waits for a pick nobody can make.
    fn settle_model_pick(&mut self) {
        let shown = self.picker.as_ref().is_some_and(|picker| picker.asks_model);
        if !shown && let Some(reply) = self.model_pick.take() {
            let _ = reply.send(None);
        }
    }

    /// The owner's yes to `/todo clean`.
    fn todo_clean(&mut self) {
        let cwd = self.parts.config.cwd.clone();
        let name = self.project_todos.name();
        match self.project_todos.clean(&cwd) {
            Ok(dropped) => self
                .transcript
                .note(tr!(todos_dropped, n = dropped, name = name), Tone::Quiet),
            Err(failure) => self.transcript.note(failure.message(&name), Tone::Fault),
        }
    }

    /// `/todo open` and the tab's **o**: the list to the owner's editor, which changes, deletes and sorts.
    fn open_todos(&mut self) {
        match self.project_todos.path() {
            Some(path) => self.open_request = Some(path.to_path_buf()),
            None => self.transcript.note(
                tr!(todos_no_file, name = self.project_todos.name()),
                Tone::Quiet,
            ),
        }
    }

    /// Ctrl+Y: the To-dos tab with its list taking the keys; with the list taking them already, the panel goes.
    fn show_todos(&mut self) {
        let shown = self.panel == Some(true) && self.tab == Tab::Todos;
        if shown && self.project_todos.keys {
            self.project_todos.keys = false;
            return self.toggle_panel();
        }
        self.show_todos_keyed();
    }

    /// The To-dos tab shown, read anew, its list with the keys.
    fn show_todos_keyed(&mut self) {
        self.tab = Tab::Todos;
        if self.panel != Some(true) {
            self.toggle_panel();
        }
        self.project_todos.search_again();
        self.project_todos.refresh(&self.parts.config.cwd);
        self.project_todos.keys = true;
    }

    /// A key while the To-dos list has them: ↑↓ (j k) choose, Space or Enter ticks (or folds the done ones), a
    /// starts `/todo ` in the input, o opens the editor, Esc gives the keys back. Any other key gives them back too
    /// and is the input's — false: not taken here.
    fn on_todos_key(&mut self, key: KeyEvent) -> bool {
        if self.chord {
            return false;
        }
        let shown = self.panel == Some(true) && self.panel_drawn && self.tab == Tab::Todos;
        if !shown {
            self.project_todos.keys = false;
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            // Ctrl+Y again hides the panel; another tab's key turns to it — the list no longer has the keys.
            if key.code != KeyCode::Char('y') {
                self.project_todos.keys = false;
            }
            return false;
        }
        // Alt and Shift keep their own meanings (Shift+Enter a new line): the input's.
        if !key.modifiers.is_empty() {
            self.project_todos.keys = false;
            return false;
        }
        // A held key ticks once: each tick is a write of the file.
        if key.kind == crossterm::event::KeyEventKind::Repeat
            && matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter)
        {
            return true;
        }
        let cwd = self.parts.config.cwd.clone();
        self.project_todos.refresh(&cwd);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.project_todos.choose(-1),
            KeyCode::Down | KeyCode::Char('j') => self.project_todos.choose(1),
            KeyCode::Char(' ') | KeyCode::Enter => {
                if let Err(failure) = self.project_todos.press(&cwd) {
                    let name = self.project_todos.name();
                    self.transcript.note(failure.message(&name), Tone::Fault);
                }
            }
            KeyCode::Char('a') => {
                self.project_todos.keys = false;
                if self.input.is_empty() {
                    self.input.insert_text("/todo ");
                    // In vi's Normal mode the item is to be typed: Insert mode, as `a` means there.
                    if let Some(vi) = &mut self.vi {
                        vi.mode = crate::vi::Mode::Insert;
                    }
                } else {
                    self.transcript.note(tr!(todo_input_full), Tone::Quiet);
                }
            }
            KeyCode::Char('o') => self.open_todos(),
            KeyCode::Esc => self.project_todos.keys = false,
            _ => {
                self.project_todos.keys = false;
                return false;
            }
        }
        true
    }

    /// What the editor said of a file it had open (`/memory open`).
    pub fn opened(&mut self, result: Result<(), String>) {
        if let Err(reason) = result {
            self.transcript.note(reason, Tone::Fault);
        }
    }

    /// The owner's answer to `/init`'s question about `CLAUDE.md`.
    fn init_with(&mut self, migrate: bool) {
        let Some(claude) = self.init_claude.take() else {
            return;
        };
        let claude_path = self.parts.config.cwd.join(&claude);
        if migrate {
            let agents = claude_path.with_file_name("AGENTS.md");
            return self.send_init(init_migrate(&claude), agents);
        }
        let folder = self
            .parts
            .config
            .cwd
            .join(&claude)
            .parent()
            .map(|folder| folder.display().to_string())
            .unwrap_or_default();
        if let Err(reason) = prefs::set_keep_claude(&self.parts.home, &folder, true) {
            self.transcript.note(reason, Tone::Fault);
        }
        self.send_init(init_check(&claude), claude_path);
    }

    /// An `/init` turn, with the guide whose size is kept once it is answered (T42).
    fn send_init(&mut self, prompt: String, guide: std::path::PathBuf) {
        if self.working() {
            return;
        }
        self.init_guide = Some(guide);
        self.send(prompt);
    }

    /// The size a guide has after the `/init` turn: until it grows beyond it, it is not pointed out again (T42).
    fn keep_guide_checked(&mut self, path: &std::path::Path) {
        let Some(bytes) = crate::guides::size(path) else {
            return;
        };
        let key = path.display().to_string();
        self.guide_checked.insert(key.clone(), bytes);
        if let Err(reason) = prefs::set_guide_checked(&self.parts.home, &key, bytes) {
            self.transcript.note(reason, Tone::Fault);
        }
    }

    /// What the Meter tab says of the guides grown too large, as they are now (T42).
    pub fn guide_warnings(&self) -> Vec<String> {
        crate::guides::too_large(
            &self.parts.guide_paths,
            self.parts.guide_warn_bytes,
            &self.guide_checked,
        )
        .iter()
        .map(|large| {
            let nearest = self
                .parts
                .guide_paths
                .last()
                .map(std::path::PathBuf::as_path);
            crate::guides::note(large, &self.parts.config.cwd, nearest)
        })
        .collect()
    }

    /// Shows or hides the side panel, and keeps the choice.
    pub fn toggle_panel(&mut self) {
        let shown = !self.panel.unwrap_or(false);
        self.panel = Some(shown);
        if shown && self.stream_width < crate::view::PANEL_MIN_SCREEN {
            self.transcript.note(tr!(panel_needs_room), Tone::Quiet);
        }
        if let Err(reason) = prefs::set_panel(&self.parts.home, shown) {
            self.transcript.note(reason, Tone::Fault);
        }
    }

    /// Ctrl+K (T38): the Keys tab, as Ctrl+B is the Plan tab's; where the window has no room for the panel, the same
    /// sheet over the stream.
    fn show_keys(&mut self) {
        if self.panel_fits {
            return self.show_tab(Tab::Keys);
        }
        self.keys_sheet = Some(0);
    }

    /// A key while the Keys sheet is over the stream: ↑↓ PgUp PgDn scroll, Esc or Ctrl+K close it, any other is
    /// nobody's — the sheet is only to read.
    fn on_sheet_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let page = self.overlay_rows.max(1);
        let Some(scroll) = &mut self.keys_sheet else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.keys_sheet = None,
            KeyCode::Char('k') if ctrl => self.keys_sheet = None,
            KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Down => *scroll = scroll.saturating_add(1),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(page),
            KeyCode::PageDown => *scroll = scroll.saturating_add(page),
            _ => {}
        }
    }

    /// The mouse at `column` is over the Plan, Gate or Meter tab of a panel drawn.
    fn over_scrolled_tab(&self, column: u16) -> bool {
        self.panel == Some(true)
            && self.panel_drawn
            && matches!(self.tab, Tab::Plan | Tab::Gate | Tab::Meter)
            && column >= self.stream_width
    }

    /// The mouse at `column` is over the Keys tab of a panel drawn.
    fn over_keys_tab(&self, column: u16) -> bool {
        self.panel == Some(true)
            && self.panel_drawn
            && self.tab == Tab::Keys
            && column >= self.stream_width
    }

    /// Ctrl+B the Plan tab, Ctrl+G the Gate tab: the panel opens on it or turns to it; on it already, the panel goes.
    pub fn show_tab(&mut self, tab: Tab) {
        if self.panel == Some(true) && self.tab == tab {
            return self.toggle_panel();
        }
        self.tab = tab;
        self.panel_scroll = 0;
        // What lean spared on every run is read once per opening, not at every frame.
        if tab == Tab::Meter
            && let Some(path) = &self.parts.config.savings
        {
            self.meter.read_lean_all(path);
        }
        if self.panel != Some(true) {
            self.toggle_panel();
        }
    }

    /// The window and the turn's standing, for the Meter tab and the header.
    pub fn meter_window(&self) -> crate::meter::Window {
        let config = &self.parts.config;
        let compacts_at = self
            .engine
            .window
            .filter(|_| config.compaction.auto)
            .map(|(window, _)| glut::compact_from(window, config.max_output_tokens));
        // A budget holds per run, and a turn is one: what the turn under way (or the last) spent counts against it.
        let budget = &config.budget;
        let turn = self.meter.turns.last();
        let spent_tokens = turn.map_or(0, |turn| {
            turn.usage
                .input_tokens
                .saturating_sub(turn.usage.cached_input_tokens)
                + turn.usage.output_tokens
        });
        let budget = match (budget.max_tokens, budget.max_usd) {
            (Some(max), _) => Some(tr!(
                budget_tokens_left,
                left = crate::meter::tokens(max.saturating_sub(spent_tokens)),
                max = crate::meter::tokens(max)
            )),
            // Glut counts money only with the budget's prices.
            (None, Some(max)) if budget.prices.is_some() => Some(tr!(
                budget_money_left,
                left = glut::lang::usd(
                    (max - turn.and_then(|turn| turn.cost).unwrap_or(0.0)).max(0.0),
                    2
                ),
                max = glut::lang::usd(max, 2)
            )),
            (None, Some(max)) => Some(tr!(budget_uncounted, max = glut::lang::usd(max, 2))),
            (None, None) => None,
        };
        crate::meter::Window {
            used: self.context_used,
            window: self.engine.window,
            compacts_at,
            effort: self.engine.endpoint.effort.map(|effort| effort.to_string()),
            budget,
            usage: self.usage,
            cost: self.cost,
        }
    }

    /// How the session is guarded now, for the Gate tab.
    pub fn guard_facts(&self) -> (String, Option<bool>, Vec<String>, (usize, usize, usize)) {
        let gate = self.parts.tools.gate();
        let mode = gate.map_or("none", |gate| session::mode_label(gate.mode()));
        let sandbox = self
            .parts
            .config
            .sandbox
            .as_ref()
            .filter(|sandbox| sandbox.enabled)
            .map(|_| glut::sandbox::available());
        let granted = gate.map(|gate| gate.granted()).unwrap_or_default();
        let given: Vec<&String> = granted.iter().flat_map(|given| &given.rules).collect();
        let rules = gate.map(|gate| gate.rules()).unwrap_or_default();
        // The settings' allow rules: those an "always" answer added are counted under it.
        let allow = rules.allow.len().saturating_sub(given.len());
        (
            mode.to_owned(),
            sandbox,
            granted
                .into_iter()
                .map(|given| given.rules.join(", "))
                .collect(),
            (allow, rules.ask.len(), rules.deny.len()),
        )
    }

    /// `/gate`: what an "always" answer allowed this session, each to take back, then the settings' rules in force.
    fn gate_picker(&self) -> Picker {
        let Some(gate) = self.parts.tools.gate() else {
            return Picker::new(tr!(no_gate), Vec::new(), None);
        };
        let granted = gate.granted();
        let mut items: Vec<Item> = granted
            .iter()
            .map(|given| {
                // The rules, not the ask's label: reading and editing one path share a label.
                Item::plain(
                    format!(
                        "allow {}",
                        crate::transcript::clean(&given.rules.join(", "))
                    ),
                    tr!(gate_always_detail),
                    Pick::TakeBack(given.rules.clone()),
                )
            })
            .collect();
        // An "always" rule stands in the allow list too: shown once, above, where it can be taken back.
        let mut given: Vec<&str> = granted
            .iter()
            .flat_map(|given| given.rules.iter().map(String::as_str))
            .collect();
        let rules = gate.rules();
        let mut settings = Vec::new();
        for rule in rules.allow.iter().rev() {
            if let Some(at) = given.iter().position(|text| *text == rule.text()) {
                given.remove(at);
            } else {
                settings.push(("allow", rule.text().to_owned()));
            }
        }
        settings.reverse();
        settings.extend(rules.ask.iter().map(|rule| ("ask", rule.text().to_owned())));
        settings.extend(
            rules
                .deny
                .iter()
                .map(|rule| ("deny", rule.text().to_owned())),
        );
        items.extend(settings.into_iter().map(|(verdict, text)| {
            Item::plain(
                format!("{verdict} {}", crate::transcript::clean(&text)),
                tr!(gate_from_settings),
                Pick::Shown,
            )
        }));
        let title = tr!(
            gate_title,
            mode = session::mode_label(gate.mode()),
            always = if granted.is_empty() {
                tr!(gate_no_always).to_owned()
            } else {
                tr!(gate_always_given, n = granted.len())
            }
        );
        Picker::new(title, items, None)
    }

    /// Takes back what an "always" answer allowed: the next such call is asked again.
    fn take_back_always(&mut self, rules: &[String]) {
        let taken = self
            .parts
            .tools
            .gate()
            .is_some_and(|gate| gate.take_back(rules));
        let clean = crate::transcript::clean(&rules.join(", "));
        if taken {
            self.transcript
                .note(tr!(taken_back, rule = clean), Tone::Quiet);
        } else {
            self.transcript
                .note(tr!(not_always, rule = clean), Tone::Fault);
        }
    }

    /// The editor's text is back (or why not): it becomes the input.
    pub fn edited(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                self.input.clear();
                self.input.insert_text(text.trim_end());
            }
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Shows the thinking as `thinking` says and keeps the choice.
    fn set_thinking(&mut self, thinking: Thinking) {
        self.transcript.show_thinking(thinking);
        let said = match thinking {
            Thinking::Off => tr!(thinking_hidden),
            Thinking::Folded => tr!(thinking_folded),
            Thinking::Open => tr!(thinking_open),
        };
        match prefs::set_thinking(&self.parts.home, thinking) {
            Ok(()) => self.say_switch(Switch::Thinking, said, Tone::Quiet),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// The owner's messages of this session, newest first, to go back to before one (Esc Esc).
    fn rewind_picker(&self) -> Picker {
        let items: Vec<Item> = self
            .session
            .marks
            .iter()
            .enumerate()
            .rev()
            .map(|(at, mark)| {
                let detail = match (mark.at.is_some(), self.files_run(at).is_some()) {
                    (true, true) => tr!(rewind_files_changed),
                    (true, false) => tr!(rewind_no_file_changed),
                    (false, true) => tr!(rewind_compacted_files),
                    (false, false) => tr!(rewind_compacted_nothing),
                };
                Item::plain(
                    crate::transcript::clean(&mark.prompt),
                    detail,
                    Pick::RewindTo(at),
                )
            })
            .collect();
        let title = if items.is_empty() {
            tr!(rewind_none)
        } else {
            tr!(rewind_title)
        };
        Picker::new(title, items, None)
    }

    /// How far to go back to before mark `mark`: the conversation (while it is still as the message left it — a
    /// compaction rewrote what came before), the files (where a run's snapshot of them is still kept), both, or a
    /// fork from there.
    fn rewind_how(&self, mark: usize) -> Picker {
        let mut items = Vec::new();
        let files = self.files_run(mark).is_some();
        let talk = self.session.marks[mark].at.is_some();
        let choice = |label: &str, detail: &str, conversation, files| {
            Item::plain(
                label,
                detail,
                Pick::Rewind {
                    mark,
                    conversation,
                    files,
                },
            )
        };
        if files && talk {
            items.push(choice(
                tr!(rewind_both),
                tr!(rewind_both_detail),
                true,
                true,
            ));
        }
        if talk {
            items.push(choice(
                tr!(rewind_talk),
                tr!(rewind_talk_detail),
                true,
                false,
            ));
        }
        if files {
            items.push(choice(
                tr!(rewind_files),
                tr!(rewind_files_detail),
                false,
                true,
            ));
        }
        if talk {
            items.push(Item::plain(
                tr!(rewind_fork),
                tr!(rewind_fork_detail),
                Pick::Fork(mark),
            ));
        }
        let title = if talk {
            tr!(rewind_how)
        } else {
            tr!(rewind_how_files)
        };
        Picker::new(title, items, None)
    }

    /// The first run at or after mark `mark` that took a snapshot still kept: its first one is the folder before
    /// that message (no run between changed a file).
    fn files_run(&self, mark: usize) -> Option<String> {
        let root = self.parts.config.snapshots.as_ref()?;
        let kept = glut::snapshot::runs(root, &self.parts.config.cwd);
        self.session.marks[mark..]
            .iter()
            .filter_map(|later| later.run.clone())
            .find(|run| kept.iter().any(|record| &record.run == run))
    }

    /// Puts the files back to before `step` of `run` (its start for `None`), the folder as it was just now kept aside
    /// first in a run of its own; the outcome comes back as a report.
    fn restore_files(&self, run: String, step: Option<u32>) {
        let Some(root) = self.parts.config.snapshots.clone() else {
            return;
        };
        let cwd = self.parts.config.cwd.clone();
        let reports = self.reports.clone();
        tokio::task::spawn_blocking(move || {
            let restored = glut::snapshot::keep_now(&root, &cwd).and_then(|kept| {
                glut::snapshot::restore(&root, &cwd, &run, step).map(|commit| (commit, kept))
            });
            let _ = reports.send(Report::Restored(restored));
        });
    }

    /// The owner's message at `at` (or after it), to put back in the input.
    /// The images of the owner's message at or after `at`, to give back with its text.
    fn images_at(&self, at: usize) -> Vec<glut::images::Image> {
        self.conversation
            .get(at..)
            .and_then(|rest| {
                rest.iter()
                    .find(|message| message.role == glut::model::Role::User)
            })
            .map(|message| message.images.clone())
            .unwrap_or_default()
    }

    fn message_at(&self, at: usize) -> String {
        self.conversation
            .get(at..)
            .and_then(|rest| {
                rest.iter()
                    .find(|message| message.role == glut::model::Role::User)
            })
            .and_then(|message| message.content.clone())
            .unwrap_or_default()
    }

    /// Goes back to before mark `mark`: the conversation cut there (its message back in the input to edit), the files
    /// put back from the snapshot — or one of the two.
    fn rewind(&mut self, mark: usize, conversation: bool, files: bool) {
        if conversation {
            self.forget_warm();
        }
        if self.working() {
            return self.transcript.note(tr!(rewind_waits), Tone::Quiet);
        }
        let Some(target) = self.session.marks.get(mark).cloned() else {
            return;
        };
        let at = target.at.filter(|at| *at <= self.conversation.len());
        if conversation && at.is_none() {
            return self.transcript.note(tr!(rewind_compacted), Tone::Fault);
        }
        if files {
            match self.files_run(mark) {
                Some(run) => self.restore_files(run, None),
                None => self.transcript.note(tr!(no_snapshot), Tone::Quiet),
            }
            // A conversation that stays is told: the model must not trust what it read of the files before.
            if !conversation {
                self.pending.push(Message::user(format!(
                    "(I put the folder's files back as they were before my message \"{}\": what you read or \
                     changed since may no longer be there.)",
                    target.prompt
                )));
                self.keep();
            }
        }
        let Some(at) = at.filter(|_| conversation) else {
            return;
        };
        let message = self.message_at(at);
        let images = self.images_at(at);
        self.conversation.truncate(at);
        self.session.marks.truncate(mark);
        self.session.conversation.clone_from(&self.conversation);
        self.transcript = Transcript {
            thinking: self.transcript.thinking,
            ..Transcript::default()
        };
        self.session.transcript(&mut self.transcript);
        self.selection = None;
        self.search = None;
        self.queue.clear();
        self.forget_models();
        self.todos.clear();
        self.forget_views();
        self.last_turn = None;
        self.context_used = 0;
        self.scroll = 0;
        self.keep();
        if self.input.is_empty() {
            self.input.insert_text(&message);
            self.input.adopt(images);
        } else {
            self.transcript.note(tr!(rewind_input_full), Tone::Quiet);
        }
        self.transcript.note(tr!(rewound), Tone::Fine);
    }

    /// A new session from before mark `mark` — this one kept as it is, the new one its fork (T14) — with that message
    /// back in the input.
    fn fork(&mut self, mark: usize) {
        if self.working() {
            return self.transcript.note(tr!(fork_waits), Tone::Quiet);
        }
        let Some(target) = self.session.marks.get(mark).cloned() else {
            return;
        };
        let Some(at) = target.at.filter(|at| *at <= self.conversation.len()) else {
            return self.transcript.note(tr!(fork_compacted), Tone::Fault);
        };
        self.keep();
        let mut fork = Session::new(
            &self.parts.config.cwd,
            &self.engine.provider,
            self.engine.model_name(),
        );
        fork.parent = Some(self.session.id.clone());
        // A fork is named for what it began from; a fork of a fork is no "(fork) (fork)".
        let base = if self.session.title.is_empty() {
            target.prompt.clone()
        } else {
            self.session.title.trim_end_matches(" (fork)").to_owned()
        };
        fork.title = format!("{base} (fork)");
        fork.cost = None;
        fork.conversation = self.conversation[..at].to_vec();
        fork.marks = self.session.marks[..mark].to_vec();
        let message = self.message_at(at);
        let images = self.images_at(at);
        self.open(fork);
        self.keep();
        if self.input.is_empty() {
            self.input.insert_text(&message);
            self.input.adopt(images);
        }
        self.transcript.note(tr!(forked), Tone::Fine);
    }

    /// The snapshot steps of this session's runs, newest first (`/steps`, T13): a step's change, or the files back
    /// to before it.
    fn steps_picker(&self) -> Picker {
        let Some(root) = self.parts.config.snapshots.clone() else {
            return Picker::new(tr!(steps_no_snapshots), Vec::new(), None);
        };
        let runs = glut::snapshot::runs(&root, &self.parts.config.cwd);
        let mut items = Vec::new();
        for mark in self.session.marks.iter().rev() {
            let Some(run) = &mark.run else { continue };
            let Some(record) = runs.iter().find(|record| &record.run == run) else {
                continue;
            };
            for step in record.steps.iter().rev() {
                items.push(Item::plain(
                    tr!(
                        step_of,
                        turn = step.turn,
                        prompt = crate::transcript::clean(&mark.prompt)
                    ),
                    &step.commit[..step.commit.len().min(8)],
                    Pick::Step {
                        run: run.clone(),
                        turn: step.turn,
                    },
                ));
            }
        }
        let title = if items.is_empty() {
            tr!(steps_none)
        } else {
            tr!(steps_title)
        };
        Picker::new(title, items, None)
    }

    /// Opens the review (T17) of what changed since before the latest turn that changed files, or before the
    /// session's first; the diff comes back as a report.
    pub fn open_review(&mut self, scope: crate::review::Scope) {
        let Some(root) = self.parts.config.snapshots.clone() else {
            return self.transcript.note(tr!(review_no_snapshots), Tone::Quiet);
        };
        let kept = glut::snapshot::runs(&root, &self.parts.config.cwd);
        let changed = |mark: &&crate::store::Mark| {
            mark.run
                .as_ref()
                .is_some_and(|run| kept.iter().any(|record| &record.run == run))
        };
        let mark = match scope {
            crate::review::Scope::Turn => self.session.marks.iter().rev().find(changed),
            crate::review::Scope::Session => self.session.marks.iter().find(changed),
        };
        let Some((run, prompt)) =
            mark.and_then(|mark| Some((mark.run.clone()?, mark.prompt.clone())))
        else {
            return self.transcript.note(tr!(review_no_change), Tone::Quiet);
        };
        let cwd = self.parts.config.cwd.clone();
        let reports = self.reports.clone();
        tokio::task::spawn_blocking(move || {
            let change = glut::snapshot::diff_to_now(&root, &cwd, &run, None);
            let _ = reports.send(Report::Review(scope, prompt, change));
        });
    }

    /// Does what a key in the review asked for.
    fn review_do(&mut self, action: crate::review::Action) {
        use crate::review::Action;
        match action {
            Action::None => {}
            Action::Close => self.review.open = false,
            Action::Load(scope) => self.open_review(scope),
            Action::Reject {
                path,
                hunk,
                created,
                why,
            } => {
                if self.working() {
                    return self.transcript.note(tr!(hunk_waits), Tone::Quiet);
                }
                match self.put_back(&path, &hunk, created) {
                    Ok(()) => {
                        self.pending
                            .push(Message::user(crate::review::for_the_model(
                                true, &path, &hunk, &why,
                            )));
                        self.keep();
                        self.say(tr!(hunk_put_back));
                        let scope = self.review.scope.unwrap_or(crate::review::Scope::Turn);
                        self.open_review(scope);
                    }
                    Err(reason) => self.say(&reason),
                }
            }
            Action::Comment { path, hunk, text } => {
                self.review.commented(&path, &hunk);
                self.pending
                    .push(Message::user(crate::review::for_the_model(
                        false, &path, &hunk, &text,
                    )));
                self.keep();
                self.say(tr!(comment_sent));
            }
        }
    }

    /// Puts one hunk of `path` back, through the folder's guard: never out of the folder, never through a link, and
    /// only where the file still reads as the change left it. A file the change made goes once it is empty again.
    fn put_back(
        &self,
        path: &str,
        hunk: &crate::review::Hunk,
        created: bool,
    ) -> Result<(), String> {
        use axiomata_files::{LinkPolicy, MAX_READ_BYTES, MAX_WRITE_BYTES, Root};
        let root = Root::dir(&self.parts.config.cwd, LinkPolicy::Contained)
            .map_err(|err| err.to_string())?;
        // The old text may be a model's from an earlier turn: never into a protected place, through a link or into
        // a file with other names.
        glut::tools::files::guard_owner_write(&root, path)?;
        let file = match axiomata_files::read_text(&root, path, MAX_READ_BYTES) {
            Ok(file) => Some(file),
            Err(axiomata_files::FilesError::NotFound { .. }) => None,
            Err(err) => return Err(tr!(cannot_put_back, path = path, reason = err)),
        };
        let text = crate::review::put_back(file.as_ref().map(|file| file.content.as_str()), hunk)?;
        let gone =
            |err: axiomata_files::FilesError| tr!(cannot_put_back, path = path, reason = err);
        match file {
            Some(file) if created && text.is_empty() => {
                // Deleted only as it was read: a write in between is not lost.
                if axiomata_files::current_version(&root, path).map_err(gone)? != Some(file.version)
                {
                    return Err(tr!(review_changed_since).into());
                }
                axiomata_files::delete(&root, path).map_err(gone)
            }
            Some(file) => {
                axiomata_files::write_text(&root, path, &text, Some(&file.version), MAX_WRITE_BYTES)
                    .map(|_| ())
                    .map_err(gone)
            }
            None => {
                // A file the change deleted comes back only where nothing stands now.
                if axiomata_files::current_version(&root, path)
                    .map_err(gone)?
                    .is_some()
                {
                    return Err(tr!(review_changed_since).into());
                }
                axiomata_files::write_text(&root, path, &text, None, MAX_WRITE_BYTES)
                    .map(|_| ())
                    .map_err(gone)
            }
        }
    }

    /// A step's change shown in the stream, or the files put back to before it.
    fn step_do(&mut self, run: String, turn: u32, restore: bool) {
        let Some(root) = self.parts.config.snapshots.clone() else {
            return;
        };
        let cwd = self.parts.config.cwd.clone();
        let reports = self.reports.clone();
        if restore {
            if self.working() {
                return self.transcript.note(tr!(files_wait), Tone::Quiet);
            }
            return self.restore_files(run, Some(turn));
        }
        tokio::task::spawn_blocking(move || {
            let change = glut::snapshot::diff(&root, &cwd, &run, turn);
            let _ = reports.send(Report::Diff(turn, change));
        });
    }

    /// Turns the vi mode on (in Insert mode, as a message starts) or off, and keeps it.
    fn set_vi(&mut self, on: bool) {
        self.vi = on.then(crate::vi::Vi::default);
        let said = if on { tr!(vim_on) } else { tr!(vim_off) };
        match prefs::set_vi(&self.parts.home, on) {
            Ok(()) => self.transcript.note(said, Tone::Quiet),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Uses and keeps how the footer joins its segments.
    fn set_footer(&mut self, separators: crate::footer::Separators) {
        self.separators = separators;
        self.separators_from = None;
        match prefs::set_footer(&self.parts.home, separators) {
            Ok(()) => self
                .transcript
                .note(tr!(footer_set, footer = separators.label()), Tone::Quiet),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Uses and keeps how much the screen moves.
    fn set_motion(&mut self, motion: crate::motion::Motion) {
        let prefs = Prefs::load(&self.parts.home);
        crate::motion::set(motion, prefs.reader.unwrap_or(false));
        match prefs::set_motion(&self.parts.home, motion) {
            Ok(()) if motion == crate::motion::Motion::Full && crate::motion::still() => {
                self.transcript.note(tr!(motion_full_held), Tone::Quiet)
            }
            Ok(()) => self.say(tr!(motion_set, motion = motion.label())),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Turns the reader mode on or off and keeps it.
    fn set_reader(&mut self, on: bool) {
        let prefs = Prefs::load(&self.parts.home);
        crate::motion::set(prefs.motion.unwrap_or_default(), on);
        match prefs::set_reader(&self.parts.home, on) {
            Ok(()) if !on && crate::motion::reader_forced() => {
                self.transcript.note(tr!(reader_off_next), Tone::Quiet)
            }
            Ok(()) if on => self.transcript.note(tr!(reader_on), Tone::Quiet),
            Ok(()) => self.say(tr!(reader_off)),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Uses and keeps the theme `name`; every block takes its colours at the next frame.
    fn set_theme(&mut self, name: &str) {
        if !crate::theme::set(name) {
            let names: Vec<&str> = crate::theme::all().map(|palette| palette.name).collect();
            return self.transcript.note(
                tr!(no_theme, name = name, known = names.join(", ")),
                Tone::Fault,
            );
        }
        self.transcript.touch_all();
        // In a hosted session the owner's choice stands for this session alone: the host's app has its own.
        if self.theme_from != ThemeFrom::Owner {
            self.theme_from = ThemeFrom::Pinned;
            return self.say(tr!(theme_session, name = name));
        }
        match prefs::set_theme(&self.parts.home, name) {
            Ok(()) => self.say(tr!(theme_set, name = name)),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// `/language`: Glut speaks `lang` from the next frame on, and keeps it for the next start; the model answers in
    /// whatever language the owner writes.
    fn set_language(&mut self, lang: glut::lang::Lang) {
        crate::texts::force(lang);
        // Every pane lays its blocks out anew, as after a new look (TUI9b).
        crate::motion::relook();
        let said = match prefs::set_language(&self.parts.home, lang) {
            Ok(()) => tr!(language_set, name = lang.name()),
            Err(reason) => tr!(language_unkept, name = lang.name(), reason = reason),
        };
        self.say(said);
    }

    /// Whether the host refuses a paid call outside a turn, said in the stream.
    fn refused_spend(&mut self) -> bool {
        match self.parts.host.may_spend() {
            Ok(()) => false,
            Err(reason) => {
                self.transcript
                    .note(tr!(not_now, reason = reason), Tone::Fault);
                true
            }
        }
    }

    /// Tells the host and the log what a paid call outside a turn used.
    fn spent(&self, usage: Usage) {
        let used = Event::Usage {
            turn: 0,
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            output_tokens: usage.output_tokens,
        };
        self.parts.log.follow(&self.session.id);
        self.parts.log.event(&used);
        self.parts.host.event(&used);
    }

    /// What the host says from outside the session.
    fn on_host(&mut self, signal: Signal) {
        match signal {
            Signal::Stop(reason) => {
                if let Some(cancel) = &self.running {
                    cancel.cancel();
                    self.transcript
                        .note(tr!(stopped_by, reason = reason), Tone::Fault);
                }
            }
            Signal::Theme(name) => {
                if self.theme_from == ThemeFrom::Pinned {
                    return;
                }
                // A name the screen has no palette of leaves the colours as they are.
                if crate::theme::set(&name) {
                    self.theme_from = ThemeFrom::Host;
                    self.transcript.touch_all();
                }
            }
            Signal::Note(text) => self.transcript.note(text, Tone::Quiet),
        }
    }

    /// Closes the gate's question when its answer is no longer awaited: the host's app answered it first — or the
    /// turn was called off while it asked, which its end says itself.
    pub fn drop_answered_ask(&mut self) {
        if self.ask.as_ref().is_some_and(|ask| ask.reply.is_closed()) {
            self.ask = None;
            if self.plan_view.as_ref().is_some_and(|view| view.asking) {
                self.plan_view = None;
            }
            let called_off = self.running.as_ref().is_none_or(Cancel::is_cancelled);
            if !called_off {
                self.say(tr!(answered_elsewhere));
            }
        }
    }

    fn set_editor(&mut self, command: &str) {
        match prefs::set_editor(&self.parts.home, command) {
            Ok(command) => self
                .transcript
                .note(tr!(editor_set, command = command), Tone::Fine),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// The models of provider `only` (every provider's, each named, for `None`) from the lists Glut keeps, the current
    /// one selected.
    fn models_picker(&self, only: Option<&str>, own: bool) -> Picker {
        let mut items = Vec::new();
        let mut missing = Vec::new();
        if let Ok(list) = Providers::load(&self.parts.home) {
            for (id, entry) in &list.providers {
                if only.is_some_and(|only| id != only) {
                    continue;
                }
                match models::cached(&self.parts.home, id, entry.root().trim_end_matches('/')) {
                    Some(list) => items.extend(list.models.iter().map(|model| {
                        let label = crate::transcript::clean(&model.id).to_string();
                        Item {
                            label: match only {
                                Some(_) => label,
                                None => format!("{} · {label}", crate::transcript::clean(id)),
                            },
                            detail: model_detail(model),
                            pick: if own {
                                Pick::OwnModel {
                                    provider: id.clone(),
                                    model: model.id.clone(),
                                }
                            } else {
                                Pick::Model {
                                    provider: id.clone(),
                                    model: model.id.clone(),
                                }
                            },
                            dim: !list.allows(&model.id),
                            facts: Facts {
                                created: model.created,
                                price: model
                                    .prices
                                    .map(|prices| prices.input_per_m + prices.output_per_m),
                                window: model.context_window,
                                free: model.free(),
                            },
                        }
                    })),
                    None => missing.push(id.clone()),
                }
            }
        }
        let mut title = match only {
            Some(only) => tr!(model_of, provider = crate::transcript::clean(only)),
            None => tr!(model_title).to_owned(),
        };
        if !missing.is_empty() {
            title.push_str(&format!(
                " ({})",
                tr!(models_missing, providers = missing.join(", "))
            ));
        }
        let current = Pick::Model {
            provider: self.engine.provider.clone(),
            model: self.engine.model_name().to_owned(),
        };
        Picker::new(title, items, Some(&current)).sorting()
    }

    /// `/projects` (T44): the folders Glut worked in, pinned first, then the newest — the first that is not this one
    /// chosen — and a last entry to name another folder.
    fn projects_picker(&self) -> Picker {
        let (home, here) = (&self.parts.home, &self.project);
        let now = chrono::Utc::now();
        let projects = crate::projects::list(home);
        let mut items: Vec<Item> = projects
            .iter()
            .map(|project| {
                let mark = if &project.folder == here {
                    "● "
                } else if project.pinned {
                    "★ "
                } else {
                    ""
                };
                let when = project
                    .last
                    .map(|last| crate::projects::ago(last, now))
                    .unwrap_or_else(|| tr!(no_session_yet).into());
                let kept = match (&project.layout, project.sessions) {
                    (Some(layout), _) => layout.clone(),
                    (None, sessions) => crate::texts::t().sessions_count.of(sessions as u64),
                };
                let note = if &project.folder == here {
                    format!(" · {}", tr!(this_one))
                } else {
                    String::new()
                };
                Item::plain(
                    format!("{mark}{}", crate::transcript::clean(&project.name())),
                    format!(
                        "{} · {when} · {kept}{note}",
                        crate::transcript::clean(&crate::projects::shown_path(
                            &project.folder,
                            home
                        ))
                    ),
                    Pick::Project(project.folder.clone()),
                )
            })
            .collect();
        items.push(Item::plain(
            tr!(another_folder),
            tr!(another_folder_detail),
            Pick::Input("/projects ".into()),
        ));
        let first = projects
            .iter()
            .find(|project| &project.folder != here)
            .map(|project| Pick::Project(project.folder.clone()));
        Picker::new(tr!(projects_title), items, first.as_ref()).keyed()
    }

    /// `p` or `x` on a project of `/projects`: pinned (or no longer), or hidden — kept, and the list shown anew.
    fn pick_project_key(&mut self, key: char, folder: &std::path::Path) {
        let home = self.parts.home.clone();
        if key == 'x' && folder == self.project {
            return self.say(tr!(project_open));
        }
        // A file Glut cannot read is not written over: the pins and hides in it would be gone.
        let mut picks = match crate::projects::Picks::read(&home) {
            Ok(picks) => picks,
            Err(reason) => return self.transcript.note(reason, Tone::Fault),
        };
        let name = crate::transcript::clean(&folder.file_name().map_or_else(
            || folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        ));
        let said = if key == 'p' {
            if picks.toggle_pin(folder) {
                tr!(project_pinned, name = name)
            } else {
                tr!(project_unpinned, name = name)
            }
        } else {
            picks.hide(folder, chrono::Utc::now());
            tr!(project_hidden, name = name)
        };
        if let Err(reason) = picks.save(&home) {
            return self.transcript.note(reason, Tone::Fault);
        }
        let selected = self.picker.as_ref().map_or(0, |picker| picker.selected);
        let mut picker = self.projects_picker();
        let chosen = Pick::Project(folder.to_path_buf());
        // The same project stays chosen where it moved; a hidden one leaves the choice where it was.
        picker.selected = picker
            .shown()
            .iter()
            .position(|item| item.pick == chosen)
            .unwrap_or_else(|| selected.min(picker.shown().len().saturating_sub(1)));
        self.picker = Some(picker);
        self.say(said);
    }

    /// The projects the welcome shows (T44): where switching is this window's to do, in the project's own pane, and
    /// the folder has no session yet, the newest few others — looked up once for each session (`/clear` starts one).
    pub fn welcome_projects(&mut self) -> &[crate::projects::Project] {
        let fresh = self
            .welcome_projects
            .as_ref()
            .is_some_and(|(session, _)| *session == self.session.id);
        if !fresh {
            let here = &self.parts.config.cwd;
            let shown = if !self.parts.projects
                || here != &self.project
                || Session::heads(&self.parts.home)
                    .iter()
                    .any(|head| &head.cwd == here)
            {
                Vec::new()
            } else {
                crate::projects::list(&self.parts.home)
                    .into_iter()
                    .filter(|project| &project.folder != here)
                    .take(crate::projects::WELCOMED)
                    .collect()
            };
            self.welcome_projects = Some((self.session.id.clone(), shown));
        }
        self.welcome_projects
            .as_ref()
            .map_or(&[], |(_, shown)| shown.as_slice())
    }

    /// Glut goes on in `folder` (T44): asked of the screen, which keeps how this window stands and starts Glut anew
    /// there — asking first while Glut works.
    fn switch_project(&mut self, folder: std::path::PathBuf) {
        if folder == self.project {
            return self.tell(tr!(project_same));
        }
        self.pane_request = Some(crate::panes::Request::Switch(folder));
    }

    /// The folder's sessions, newest first, each followed by its forks as a tree; this session among them, marked.
    pub fn session_picker(&self) -> Picker {
        let mut listed = Session::list(&self.parts.home, &self.parts.config.cwd);
        let own = self.session.id.clone();
        // This session belongs in its tree even before its first save: a fork shows where it hangs. A new, empty one
        // (glut -r) is no entry.
        let started = !self.session.conversation.is_empty() || self.session.parent.is_some();
        if started && !listed.iter().any(|session| session.id == own) {
            listed.insert(0, self.session.clone());
        }
        let tree = Session::as_tree(listed);
        let mut items: Vec<Item> = tree
            .iter()
            .enumerate()
            .map(|(at, (depth, session))| {
                let branch = if *depth > 0 {
                    // A fork with a sibling below it continues the line, the last one closes it.
                    let more = tree[at + 1..]
                        .iter()
                        .take_while(|(next, _)| next >= depth)
                        .any(|(next, _)| next == depth);
                    format!(
                        "{}{}─⑂ ",
                        "   ".repeat(depth - 1),
                        if more { "├" } else { "└" }
                    )
                } else {
                    String::new()
                };
                let title = match crate::transcript::clean(&session.title) {
                    title if title.trim().is_empty() => format!("({})", tr!(no_message)),
                    title => title,
                };
                let here = if session.id == own { "● " } else { "" };
                let when = session
                    .updated
                    .with_timezone(&chrono::Local)
                    .format(glut::lang::date_time());
                let cost = session
                    .cost
                    .map(|cost| format!(" · {}", glut::lang::usd(cost, 4)))
                    .unwrap_or_default();
                let note = if session.id == own {
                    format!("{} · ", tr!(review_session))
                } else {
                    String::new()
                };
                Item::plain(
                    format!("{branch}{here}{title}"),
                    format!(
                        "{note}{when} · {} · {}{cost}",
                        session.provider, session.model
                    ),
                    Pick::Session(session.id.clone()),
                )
            })
            .collect();
        let others = tree.iter().filter(|(_, session)| session.id != own).count();
        let title = if others == 0 {
            tr!(sessions_none)
        } else {
            tr!(sessions_title)
        };
        // T44: the panes kept at the last end, first — into a window of one pane, as `glut -c` offers them.
        if self.parts.host.may_split()
            && self.others_open.is_empty()
            && let Some(layout) = crate::layout::load(&self.parts.home, &self.parts.config.cwd)
        {
            items.insert(
                0,
                Item::plain(
                    tr!(as_it_stood, layout = layout.summary()),
                    layout.details(),
                    Pick::Pane(crate::panes::Request::Restore),
                ),
            );
        }
        Picker::new(title, items, Some(&Pick::Session(own)))
    }

    /// `/resume`'s ⌫: the session named, how many images go with it, a yes asked for — not for a session open in a
    /// pane of this window; one forks came from says so.
    fn ask_delete_session(&mut self, id: &str) {
        if self.session.id == id || self.others_open.iter().any(|open| open == id) {
            return self.say(tr!(session_open_undeletable));
        }
        let Ok(session) = Session::load(&self.parts.home, id) else {
            return self.say(tr!(session_gone));
        };
        let images = session.images().count();
        let forks = Session::all(&self.parts.home)
            .iter()
            .filter(|other| other.parent.as_deref() == Some(id))
            .count();
        let title = match crate::transcript::clean(&session.title) {
            title if title.trim().is_empty() => format!("({})", tr!(no_message)),
            title => title,
        };
        let mut detail = match images {
            0 => tr!(delete_detail).to_owned(),
            1 => tr!(delete_detail_image).to_owned(),
            many => tr!(delete_detail_images, n = many),
        };
        if forks > 0 {
            detail.push_str(&format!(
                " — {}",
                crate::texts::t().delete_forks_stay.of(forks as u64)
            ));
        }
        let items = vec![
            Item::plain(
                tr!(delete_yes, title = title),
                detail,
                Pick::DeleteSession(id.to_owned()),
            ),
            Item::plain(tr!(delete_no), "", Pick::Shown),
        ];
        self.picker = Some(Picker::new(tr!(delete_title), items, None).guarded());
    }

    /// The session `id` deleted with its images no one holds any more; the list again, without it.
    fn delete_session(&mut self, id: &str) {
        // The images this window still holds stay: the open conversation's, the input's.
        let keep: Vec<std::path::PathBuf> = self
            .conversation
            .iter()
            .flat_map(|message| &message.images)
            .chain(&self.input.images())
            .map(|image| image.path.clone())
            .collect();
        match Session::delete(&self.parts.home, id, &keep) {
            Ok(0) => self.say(tr!(session_deleted)),
            Ok(1) => self.say(tr!(session_deleted_image)),
            Ok(images) => self.say(tr!(session_deleted_images, n = images)),
            Err(reason) => return self.transcript.note(reason, Tone::Fault),
        }
        self.picker = Some(self.session_picker());
    }

    fn provider_picker(&self) -> Picker {
        let items = Providers::load(&self.parts.home)
            .map(|list| {
                list.providers
                    .iter()
                    .map(|(id, entry)| {
                        Item::plain(
                            id.clone(),
                            format!(
                                "{} · {}",
                                entry.kind,
                                entry.model.as_deref().unwrap_or(tr!(no_default_model))
                            ),
                            Pick::Provider(id.clone()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Picker::new(
            tr!(provider_title),
            items,
            Some(&Pick::Provider(self.engine.provider.clone())),
        )
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        // A list Glut opened by itself takes no key typed before it showed (T37c), as an ask does.
        let own = self.picker.as_ref().is_some_and(|picker| picker.asks_model);
        if own
            && (self.model_pick_opened.elapsed() < ASK_SETTLE || key.kind == KeyEventKind::Repeat)
        {
            return;
        }
        // T36a: a merge or a discard is not picked by a key typed for something else, nor narrowed by typing.
        let guarded = self.picker.as_ref().is_some_and(|picker| picker.guarded);
        if guarded
            && (self
                .picker
                .as_ref()
                .is_some_and(|picker| picker.opened.elapsed() < ASK_SETTLE)
                || key.kind == KeyEventKind::Repeat
                || matches!(key.code, KeyCode::Char(_) | KeyCode::Backspace))
        {
            return;
        }
        let Some(picker) = &mut self.picker else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.picker = None;
                // `pick_model` waits for an answer: none picked is one.
                if let Some(reply) = self.model_pick.take() {
                    let _ = reply.send(None);
                }
            }
            KeyCode::Char('s') if ctrl => picker.next_sort(),
            // `/resume`'s ⌃E: the chosen session exported (T54) — a letter alone filters the list.
            KeyCode::Char('e') if ctrl && matches!(picker.chosen(), Some(Pick::Session(_))) => {
                if let Some(Pick::Session(id)) = picker.chosen() {
                    self.picker = None;
                    self.export_listed(&id);
                }
            }
            KeyCode::Char('f') if ctrl => picker.toggle_free(),
            KeyCode::Up => picker.up(),
            KeyCode::Down | KeyCode::Tab => picker.down(),
            // `/projects` (T44): `p` pins the chosen project, `x` hides it; no letter filters the list.
            KeyCode::Char(c @ ('p' | 'x')) if picker.keyed && !ctrl => {
                if let Some(Pick::Project(folder)) = picker.chosen() {
                    self.pick_project_key(c, &folder);
                }
            }
            KeyCode::Char(_) | KeyCode::Backspace if picker.keyed => {}
            // `/resume`'s ⌫ on a list not filtered: the chosen session deleted, asked once (T45).
            KeyCode::Backspace
                if picker.query.is_empty() && matches!(picker.chosen(), Some(Pick::Session(_))) =>
            {
                if let Some(Pick::Session(id)) = picker.chosen() {
                    self.ask_delete_session(&id);
                }
            }
            KeyCode::Backspace => picker.backspace(),
            KeyCode::Enter => {
                let chosen = picker.chosen();
                self.picker = None;
                match chosen {
                    Some(Pick::OwnModel { provider, model }) => {
                        if let Some(reply) = self.model_pick.take() {
                            let _ = reply.send(Some((provider, model)));
                        }
                    }
                    Some(Pick::Model { provider, model }) => self.switch(
                        Some(provider),
                        Some(model),
                        Some(self.engine.endpoint.effort),
                    ),
                    Some(Pick::Provider(provider)) => {
                        self.switch(Some(provider), None, Some(self.engine.endpoint.effort))
                    }
                    Some(Pick::Effort(effort)) => self.switch_effort(effort),
                    Some(Pick::Editor(command)) => self.set_editor(&command),
                    // A turn under way holds the conversation: opening another now would save the turn into it.
                    Some(Pick::Session(_)) if self.working() => {
                        self.transcript.note(tr!(session_opens_later), Tone::Quiet)
                    }
                    Some(Pick::Session(id)) if id == self.session.id => {}
                    // Two panes on one session file would write over each other's turns.
                    Some(Pick::Session(id)) if self.others_open.contains(&id) => self
                        .transcript
                        .note(tr!(session_in_other_pane), Tone::Quiet),
                    Some(Pick::Session(id)) => match Session::load(&self.parts.home, &id) {
                        Ok(session) => self.open(session),
                        Err(reason) => self.transcript.note(reason, Tone::Fault),
                    },
                    // The path goes after the `@` typed before the picker opened, in quotes when it has a space.
                    Some(Pick::File(path)) if path.contains(' ') => {
                        self.input.insert_text(&format!("\"{path}\" "))
                    }
                    Some(Pick::File(path)) => self.input.insert_text(&format!("{path} ")),
                    Some(Pick::Theme(name)) => self.set_theme(&name),
                    Some(Pick::Language(lang)) => self.set_language(lang),
                    Some(Pick::TakeBack(rules)) => self.take_back_always(&rules),
                    Some(Pick::Agent(path)) => {
                        self.agent_view = Some(crate::agents::View::new(path));
                    }
                    Some(Pick::Shown) => {}
                    Some(Pick::DeleteSession(id)) => self.delete_session(&id),
                    Some(Pick::Pane(request)) => self.pane_request = Some(request),
                    // A draft in the input stays: the command then runs as proposed, renamed later by hand if wanted.
                    Some(Pick::Input(text)) if !self.input.is_empty() => {
                        match text.strip_prefix("/split ").map(crate::panes::parse_split) {
                            Some(Ok(request)) => self.pane_request = Some(request),
                            _ => self
                                .transcript
                                .note(tr!(draft_stays, command = text), Tone::Quiet),
                        }
                    }
                    Some(Pick::Input(text)) => self.input.insert_text(&text),
                    Some(Pick::Project(folder)) => self.switch_project(folder),
                    Some(Pick::Role(name)) => self.play(name.as_deref()),
                    Some(Pick::RewindTo(mark)) => self.picker = Some(self.rewind_how(mark)),
                    Some(Pick::Fork(mark)) => self.fork(mark),
                    Some(Pick::Init(migrate)) => self.init_with(migrate),
                    Some(Pick::MemoryFact(name)) => self.show_fact(&name),
                    Some(Pick::TodoClean) => self.todo_clean(),
                    Some(Pick::Skill(name)) => self.input.insert_text(&format!("/skill {name} ")),
                    Some(Pick::Step { run, turn }) => self.picker = Some(step_do(run, turn)),
                    Some(Pick::StepDo { run, turn, restore }) => self.step_do(run, turn, restore),
                    Some(Pick::Rewind {
                        mark,
                        conversation,
                        files,
                    }) => self.rewind(mark, conversation, files),
                    None => {}
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.type_char(c)
            }
            _ => {}
        }
    }

    /// Whether something stands over the stream: a list, the review, the plan, a subagent's view.
    fn overlaid(&self) -> bool {
        self.picture_view.is_some()
            || self.picker.is_some()
            || self.keys_sheet.is_some()
            || self.review.open
            || self.plan_view.is_some()
            || self.agent_view.is_some()
    }

    /// The views over the stream that name its blocks or the session's plan go with the stream they belong to.
    fn forget_views(&mut self) {
        self.agent_view = None;
        self.reading = None;
        self.agents_list = None;
        self.plan = None;
        self.plan_view = None;
        self.offered_plan = None;
    }

    /// `⌃X A`: the Agents tab with its list taking the keys; with the list taking them already, the panel goes.
    fn show_agents(&mut self) {
        let first = crate::agents::overview(&self.transcript)
            .first()
            .map(|(path, _)| path.clone());
        let shown = self.panel == Some(true) && self.tab == Tab::Agents;
        // An empty list never takes the keys: there `⌃X A` hides at once.
        if shown && (self.agents_list.is_some() || first.is_none()) {
            self.agents_list = None;
            return self.toggle_panel();
        }
        self.tab = Tab::Agents;
        if self.panel != Some(true) {
            self.toggle_panel();
        }
        self.agents_list = first;
    }

    /// The row of the Agents tab's chosen subagent; one no longer listed gives way to the first.
    pub fn agents_chosen(&self) -> Option<usize> {
        let chosen = self.agents_list.as_ref()?;
        let list = crate::agents::overview(&self.transcript);
        Some(
            list.iter()
                .position(|(path, _)| path == chosen)
                .unwrap_or(0),
        )
    }

    /// A key while the Agents tab's list has them: ↑↓ (j k) choose, Enter opens the chosen one's view, Esc gives the
    /// keys back. Any other key gives them back too and is the input's — false: not taken here.
    fn on_agents_list_key(&mut self, key: KeyEvent) -> bool {
        let list: Vec<Vec<usize>> = crate::agents::overview(&self.transcript)
            .into_iter()
            .map(|(path, _)| path)
            .collect();
        // A chord's second key is the chord's: `⌃X A` again hides the panel.
        if self.chord {
            return false;
        }
        let shown = self.panel == Some(true) && self.panel_drawn && self.tab == Tab::Agents;
        let Some(chosen) = self.agents_chosen().filter(|_| shown && !list.is_empty()) else {
            self.agents_list = None;
            return false;
        };
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            // ⌃X starts a chord whose second key decides; another tab's key turns to it — the list lets go.
            if key.code != KeyCode::Char('x') {
                self.agents_list = None;
            }
            return false;
        }
        // Alt and Shift keep their own meanings (Alt+b a word left, Shift+Enter a new line): the input's.
        if !key.modifiers.is_empty() {
            self.agents_list = None;
            return false;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.agents_list = Some(list[chosen.saturating_sub(1)].clone())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.agents_list = Some(list[(chosen + 1).min(list.len() - 1)].clone())
            }
            KeyCode::Enter => {
                self.agent_view = Some(crate::agents::View::new(list[chosen].clone()))
            }
            KeyCode::Char('x') => self.stop_subagent(&list[chosen]),
            KeyCode::Esc => self.agents_list = None,
            _ => {
                self.agents_list = None;
                return false;
            }
        }
        true
    }

    /// x in the Agents tab (T40): the chosen subagent stops, the turn and the others go on. Only one that still runs
    /// and that the turn's run started itself — a nested one ends with its parent.
    fn stop_subagent(&mut self, path: &[usize]) {
        let Some(agent) = self.transcript.agent(path) else {
            return;
        };
        if agent.state != crate::transcript::ToolState::Running {
            return self.transcript.note(tr!(subagent_ended), Tone::Quiet);
        }
        let (id, task) = (agent.id.clone(), agent.task.clone());
        let stopped = path.len() == 1
            && self
                .running
                .as_ref()
                .is_some_and(|cancel| cancel.cancel_subagent(&id));
        if stopped {
            self.transcript.note(
                tr!(subagent_stopping, task = crate::transcript::clean(&task)),
                Tone::Quiet,
            );
        } else {
            self.transcript.note(tr!(subagent_not_alone), Tone::Quiet);
        }
    }

    /// The turn's second subagent opens the Agents tab (T34), unless the owner closed the panel: on a panel never
    /// opened it opens without being remembered, as the first plan does; a shown panel turns to it unless it shows a
    /// plan under way. The keys stay with the input.
    fn second_agent_opens_tab(&mut self) {
        if self.transcript.agents_this_turn() != 2 || self.panel == Some(false) {
            return;
        }
        if self.panel.is_none() {
            self.panel = Some(true);
        } else if self.tab == Tab::Plan && !self.todos.is_empty() {
            return;
        }
        self.tab = Tab::Agents;
    }

    /// `/agents`: the session's subagents, each under the one that started it; Enter opens one's stream.
    fn agents_picker(&mut self) {
        let found = self.transcript.agents();
        if found.is_empty() {
            return self.transcript.note(tr!(no_subagent_yet), Tone::Quiet);
        }
        let items = found
            .into_iter()
            .map(|(path, agent)| {
                let state = match agent.state {
                    crate::transcript::ToolState::Running => tr!(agent_running),
                    crate::transcript::ToolState::Done => tr!(agent_done),
                    crate::transcript::ToolState::Failed => tr!(agent_stopped),
                    crate::transcript::ToolState::Refused => tr!(agent_refused),
                };
                Item::plain(
                    format!(
                        "{}⬡ {}",
                        "  ".repeat(path.len() - 1),
                        crate::transcript::cut(&crate::transcript::clean(&agent.task), 60)
                    ),
                    state,
                    Pick::Agent(path),
                )
            })
            .collect();
        self.picker = Some(Picker::new(tr!(subagents_title), items, None));
    }

    /// A key in a subagent's view: scroll, open one of its own subagents, or go back one level.
    fn on_agent_key(&mut self, key: KeyEvent) {
        let Some(mut view) = self.agent_view.take() else {
            return;
        };
        let nested = self
            .transcript
            .agent(&view.path)
            .map(|agent| agent.stream.agent_blocks())
            .unwrap_or_default();
        match view.key(key, self.overlay_rows, nested.len().min(9)) {
            crate::agents::Step::None => self.agent_view = Some(view),
            crate::agents::Step::Back => {
                view.path.pop();
                if !view.path.is_empty() {
                    self.agent_view = Some(crate::agents::View::new(view.path));
                }
            }
            crate::agents::Step::Open(number) => {
                let mut path = view.path.clone();
                path.push(nested[number - 1]);
                self.agent_view = Some(crate::agents::View::new(path));
            }
        }
    }

    /// A message from another pane (T36b), taken as if the owner had typed it here: at once while this pane is
    /// free, else after its turn.
    pub fn deliver(&mut self, text: String) -> Delivered {
        if self.steering.is_some() {
            // A turn: after it, as Tab queues — not after a turn the owner stopped, which waits for Enter.
            self.queue.push(text);
            Delivered::AfterTurn
        } else if self.working() {
            // A compaction, a handoff, a switch: as soon as it is done, as a message typed meanwhile is.
            self.queue.push(text);
            self.send_when_free = true;
            Delivered::WhenFree
        } else if self.picker.is_some() || !self.queue.is_empty() {
            // A list open here, or messages left waiting: neither closed nor jumped — it waits with them.
            self.queue.push(text);
            Delivered::OnEnter
        } else {
            self.send(text);
            Delivered::Now
        }
    }

    /// A message of another pane's model (T36c): at once only where `may_start` (this pane is no wider than the
    /// sender's) and nothing else is open here; after the turn under way, checked again then; otherwise held for the
    /// owner's Enter, its text shown. `Err` when too many wait here already.
    pub fn deliver_model(
        &mut self,
        message: String,
        from: u8,
        may_start: bool,
        sender: Sender,
    ) -> Result<Delivered, String> {
        let free = !self.working()
            && self.picker.is_none()
            && self.queue.is_empty()
            && self.held.is_empty()
            && self.waiting_models.is_empty();
        if may_start && self.steering.is_some() {
            self.waiting_models.push((message, sender));
            return Ok(Delivered::AfterTurn);
        }
        if may_start && free {
            self.send(message);
            return Ok(Delivered::Now);
        }
        if self.held.len() >= MOST_HELD {
            return Err(format!(
                "{MOST_HELD} messages wait there for the owner already: tell the owner instead"
            ));
        }
        self.hold(message, &tr!(held_enter, pane = from));
        Ok(Delivered::OnEnter)
    }

    /// Holds another model's message for the owner's Enter, its text shown with `why`.
    fn hold(&mut self, message: String, why: &str) {
        let shown = crate::tell::shown(&message).unwrap_or_default();
        self.transcript.note(
            format!("{shown}\n— {}", tr!(held_why, why = why)),
            Tone::Heed,
        );
        self.held.push(message);
        self.held_since = Some(Instant::now());
    }

    /// Whether this pane can do no more than `sender` could: no looser mode, no "always" it lacked.
    pub fn no_wider_than(&self, sender: &Sender) -> bool {
        let mine = self.sender();
        // A role narrows: a sender with one reaches only a pane whose role names nothing beyond it.
        let within_role = match (&sender.role, &mine.role) {
            (None, _) => true,
            (Some(theirs), Some(ours)) => ours.iter().all(|rule| theirs.contains(rule)),
            (Some(_), None) => false,
        };
        mine.looseness <= sender.looseness
            && within_role
            && mine
                .granted
                .iter()
                .all(|rule| sender.granted.contains(rule))
    }

    /// What this pane's model can do now, as a sender.
    pub fn sender(&self) -> Sender {
        let granted = self
            .parts
            .tools
            .gate()
            .map(|gate| {
                gate.granted()
                    .into_iter()
                    .flat_map(|granted| granted.rules)
                    .collect()
            })
            .unwrap_or_default();
        Sender {
            looseness: self.mode().looseness(),
            granted,
            role: self.parts.tools.gate().and_then(|gate| gate.role_rules()),
        }
    }

    /// After a turn: until when the provider holds its cache, and — pinging — the keeping while the session rests.
    fn keep_warm(
        &mut self,
        answered: bool,
        warm: Option<&glut::Warm>,
        conversation: &[glut::Message],
    ) {
        use crate::warm::CacheMode;
        let anthropic = matches!(*self.engine.model, glut::Model::Anthropic(_));
        let hour = self.cache_mode == CacheMode::Hour && anthropic;
        if self.cache_seen {
            let life = if hour {
                crate::warm::HOUR
            } else {
                crate::warm::LIFE
            };
            self.warm_until = Some(Instant::now() + life);
        }
        self.keeper = None;
        let pings = match self.cache_mode {
            CacheMode::Ping => true,
            // Only Anthropic lets the hour be asked for: elsewhere it is kept by reading, as with `ping`.
            CacheMode::Hour => !anthropic,
            CacheMode::Off => false,
        };
        // Only after an answered turn: a call that failed or was stopped may have cached nothing. A ping is a paid
        // call outside a turn: the host's budget has its word first, as for `/compact` — and before every ping.
        let may_spend = self.parts.host.may_spend().is_ok();
        if let Some(warm) =
            warm.filter(|warm| answered && pings && may_spend && warm.length <= conversation.len())
        {
            self.keeper = Some(crate::warm::keep(
                Arc::clone(&self.engine.model),
                conversation[..warm.length].to_vec(),
                warm.tools.clone(),
                Arc::clone(&self.parts.host),
                self.reports.clone(),
            ));
        }
    }

    /// The cache this session kept is no head of what comes next (another conversation, a compacted one, another
    /// role or mode): its keeping ends, and the footer marks nothing until the provider caches anew.
    fn forget_warm(&mut self) {
        self.keeper = None;
        self.warm_until = None;
        self.cache_seen = false;
    }

    /// `/cache ping|1h|off`: how this session keeps its cache from now on, kept for the next start.
    fn set_cache(&mut self, mode: crate::warm::CacheMode) {
        self.cache_mode = mode;
        self.engine
            .model
            .set_cache_hour(mode == crate::warm::CacheMode::Hour);
        if mode == crate::warm::CacheMode::Off {
            self.keeper = None;
        }
        let said = match mode {
            crate::warm::CacheMode::Ping => tr!(cache_ping),
            crate::warm::CacheMode::Hour => tr!(cache_hour),
            crate::warm::CacheMode::Off => tr!(cache_off),
        };
        match prefs::set_cache(&self.parts.home, mode) {
            Ok(()) => self.tell(said),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Plays the role `name` from now on (T36c), said in the stream — or none, an all-rounder again.
    pub fn play(&mut self, name: Option<&str>) {
        match self.set_role(name) {
            Ok(()) => {
                let said = match name {
                    Some(name) => tr!(role_played, name = name, about = self.role_said(name)),
                    None => tr!(role_none).to_owned(),
                };
                self.tell(said);
            }
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Sets the role: the agent's prompt joins the system prompt and its `tools` narrow every run of this session
    /// (its subagents' too); its model never counts — the pane keeps its own.
    ///
    /// # Errors
    ///
    /// No agent of that name.
    pub fn set_role(&mut self, name: Option<&str>) -> Result<(), String> {
        // Another role is another system prompt: the kept head is no longer the next turn's.
        self.forget_warm();
        let gate = self.parts.tools.gate().cloned();
        let Some(name) = name else {
            if let Some(gate) = gate {
                gate.set_role(None);
            }
            self.parts.pane_link.set_role(None);
            self.role = None;
            return Ok(());
        };
        let agent = self.parts.config.agents.get(name).cloned().ok_or_else(|| {
            tr!(
                no_role,
                name = name,
                known = self.parts.config.agents.known()
            )
        })?;
        match (gate, &agent.tools) {
            (Some(gate), tools) => gate.set_role(
                tools
                    .clone()
                    .map(|tools| (format!("the role {}", agent.name), tools)),
            ),
            // A role that narrows needs the gate that narrows: without one it is no role at all.
            (None, Some(_)) => {
                return Err(tr!(role_needs_gate, name = name));
            }
            (None, None) => {}
        }
        self.parts
            .pane_link
            .set_role(Some((agent.name.clone(), agent.prompt.clone())));
        self.role = Some(agent.name);
        Ok(())
    }

    /// Where the role `name` comes from and what it lets the pane do, as the owner reads it: Glut's own, the owner's
    /// or the project's file (a file of the same name wins over Glut's), and its tools — or none named, no narrowing.
    pub fn role_said(&self, name: &str) -> String {
        let file = format!("{name}.md");
        let project = self
            .parts
            .config
            .cwd
            .join(".glut")
            .join("agents")
            .join(&file);
        let owner = self.parts.home.join(".glut").join("agents").join(&file);
        let source = if project.is_file() {
            tr!(source_project)
        } else if owner.is_file() {
            tr!(source_yours)
        } else {
            tr!(source_glut)
        };
        let tools = match self
            .parts
            .config
            .agents
            .get(name)
            .and_then(|agent| agent.tools.as_ref())
        {
            Some(tools) if tools.is_empty() => tr!(no_tool).to_owned(),
            Some(tools) => {
                let named: Vec<String> = tools.iter().map(|rule| rule.text().to_owned()).collect();
                glut::permissions::printable(&named.join(", "))
            }
            None => tr!(role_all_tools).to_owned(),
        };
        tr!(role_source, source = source, tools = tools)
    }

    /// `⌃X B` (T36e): the role of a new agent in the background — every role, or none.
    fn background_picker(&self) -> Picker {
        let roles: Vec<Item> = self
            .roles_picker()
            .into_items()
            .into_iter()
            .map(|mut item| {
                if let Pick::Role(role) = &item.pick {
                    item.pick = Pick::Pane(crate::panes::Request::Background {
                        name: None,
                        role: role.clone(),
                        place: crate::panes::Place::Ask,
                        sure: false,
                    });
                }
                item
            })
            .collect();
        Picker::new(tr!(background_role_title), roles, None)
    }

    /// `/role`: the roles to play — Glut's, the owner's and the project's agents — and none.
    fn roles_picker(&self) -> Picker {
        let mut items = vec![Item::plain(
            tr!(all_rounder),
            tr!(all_rounder_detail),
            Pick::Role(None),
        )];
        items.extend(self.parts.config.agents.list().iter().map(|agent| {
            let about: String = glut::permissions::printable(&agent.description)
                .chars()
                .take(120)
                .collect();
            Item::plain(
                agent.name.clone(),
                format!("{about} — {}", self.role_said(&agent.name)),
                Pick::Role(Some(agent.name.clone())),
            )
        }));
        let current = Pick::Role(self.role.clone());
        Picker::new(tr!(role_title), items, Some(&current))
    }

    /// Other models' messages go with the conversation they were shown in.
    fn forget_models(&mut self) {
        self.held.clear();
        self.held_since = None;
        self.waiting_models.clear();
    }

    /// The plan with its task list as another pane gets it to build (T36b); `None` without a plan.
    pub fn plan_to_build(&self) -> Option<String> {
        let plan = self.plan.as_ref()?;
        let mut text = format!("Build this plan:\n\n{}", plan.text);
        if !self.todos.is_empty() {
            text.push_str("\n\nTask list:");
            for item in &self.todos {
                let mark = if item.status == glut::planning::TodoStatus::Done {
                    "x"
                } else {
                    " "
                };
                text.push_str(&format!("\n- [{mark}] {}", item.text));
            }
        }
        Some(text)
    }

    /// A turn under way, for the panes' tests.
    #[cfg(test)]
    pub fn working_for_tests(&mut self) {
        self.busy = Some(Busy::Turn);
    }

    /// The pane comes into view again (T36a, `⌃]` or a click): its open question and its open model list count as
    /// shown only from now — a key meant for the other pane never answers what this one asks.
    pub fn shown_again(&mut self) {
        if let Some(open) = &mut self.ask {
            open.opened = Instant::now();
        }
        self.model_pick_opened = Instant::now();
        if let Some(picker) = &mut self.picker {
            picker.opened = Instant::now();
        }
    }

    /// Whether this session waits for the owner's word: the gate's question, the plan's, or the model list a
    /// `glut_file` call opened — what another pane's footer points to (T36a).
    pub fn waits_for_owner(&self) -> bool {
        self.ask.is_some() || self.picker.as_ref().is_some_and(|picker| picker.asks_model)
    }

    /// Whether the question open is the plan's, with its dialog on the screen.
    pub fn plan_asks(&self) -> bool {
        self.ask
            .as_ref()
            .is_some_and(|open| open.request.tool == glut::planning::EXIT_PLAN)
            && self.plan_view.as_ref().is_some_and(|view| view.asking)
    }

    /// The modes the plan's ask offers, as the owner reads them (build, auto, ask).
    pub fn plan_modes(&self) -> Vec<String> {
        self.ask
            .as_ref()
            .filter(|open| open.request.tool == glut::planning::EXIT_PLAN)
            .map(|open| {
                open.request
                    .scopes
                    .iter()
                    .map(|scope| {
                        Mode::parse(scope).map_or_else(
                            || crate::transcript::clean(scope),
                            |mode| session::mode_label(mode).to_owned(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A key in the plan's view: while Glut asks it approves (into a mode) or sends the plan back, with a note that
    /// reaches Glut before its next step; else it only scrolls and closes. Keys of the first moment after the ask came
    /// up answer nothing, as on every ask.
    fn on_plan_key(&mut self, key: KeyEvent) {
        let modes = self.plan_modes();
        // As on every ask: no held key, no Ctrl or Alt, nothing in the first moment answers.
        if self.plan_asks()
            && (key.kind == KeyEventKind::Repeat
                || key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                || self
                    .ask
                    .as_ref()
                    .is_some_and(|open| open.opened.elapsed() < ASK_SETTLE))
        {
            return;
        }
        let Some(view) = &mut self.plan_view else {
            return;
        };
        let page = self.overlay_rows.saturating_sub(4);
        // `b` hands the plan over only where there is no question to which pane.
        view.elsewhere = (self.others.len() == 1).then(|| self.others[0]);
        let choice = view.key(key, modes.len(), page);
        use crate::plan::{Choice, State};
        match choice {
            Choice::None => {}
            Choice::Close => self.plan_view = None,
            Choice::Approve(at) => {
                let Some(open) = self.ask.take() else { return };
                let answer = if at == 0 {
                    Answer::Allow
                } else {
                    Answer::AllowWithin(at as u8)
                };
                let _ = open.reply.send(answer);
                self.plan_view = None;
                let mode = modes.get(at).cloned().unwrap_or_else(|| "build".into());
                if mode == "auto" && !session::sandboxed(&self.parts) {
                    self.transcript
                        .note(tr!(plan_auto_unsandboxed), Tone::Fault);
                }
                if let Some(plan) = &mut self.plan {
                    plan.state = State::Approved(mode);
                }
            }
            Choice::BuildElsewhere(pane) => {
                // This pane's model hears why its plan was not approved here, and stays in plan mode.
                let Some(open) = self.ask.take() else { return };
                let _ = open.reply.send(Answer::Deny);
                self.plan_view = None;
                let note = format!(
                    "The owner hands your plan to pane {pane}, where another session builds it. Stay in plan mode \
                     and wait for the owner."
                );
                match &self.steering {
                    Some(inbox) => inbox.send(note),
                    None => self.queue.push(note),
                }
                self.pane_request = Some(crate::panes::Request::SendPlan {
                    to: Some(pane.to_string()),
                });
            }
            Choice::SendBack(note) => {
                let Some(open) = self.ask.take() else { return };
                let _ = open.reply.send(Answer::Deny);
                self.plan_view = None;
                if let Some(note) = &note {
                    // The note joins the conversation as the owner's word right after the plan's answer.
                    match &self.steering {
                        Some(inbox) => inbox.send(format!("About your plan: {note}")),
                        None => self.queue.push(format!("About your plan: {note}")),
                    }
                }
                if let Some(plan) = &mut self.plan {
                    plan.state = State::SentBack(note);
                }
            }
        }
    }

    /// y allows once; 1, 2, 3 for the rest of the session, as far as the card's scope with that number reaches
    /// (`git status --short`, `git status *`, `git *`), a the first of them; n or Esc refuses — plain keys only,
    /// pressed once, and not in the first moment after the card came up: a key typed (or held) for the input must
    /// not answer a question the owner has not read yet, nor Ctrl+A (a key of its own) allow for the whole session.
    fn on_ask_key(&mut self, key: KeyEvent) {
        let Some(open) = &self.ask else { return };
        if key.kind == KeyEventKind::Repeat
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            || open.opened.elapsed() < ASK_SETTLE
        {
            return;
        }
        let answer = match key.code {
            KeyCode::Char('y' | 'j') => Answer::Allow,
            KeyCode::Char('a') if !open.request.scopes.is_empty() => Answer::AllowAlways,
            KeyCode::Char(digit @ '1'..='9') => {
                let at = digit as usize - '1' as usize;
                if at >= open.request.scopes.len() {
                    return;
                }
                Answer::AllowWithin(at as u8)
            }
            KeyCode::Char('n') | KeyCode::Esc => Answer::Deny,
            _ => return,
        };
        if let Some(open) = self.ask.take() {
            let _ = open.reply.send(answer);
        }
    }

    /// Ctrl+C: closes a picker, clears the input, else stops the turn, else quits on a second press.
    fn interrupt(&mut self) {
        if let Some(open) = self.ask.take() {
            let _ = open.reply.send(Answer::Deny);
        }
        // The plan's dialog answered no with it: it goes, and the plan stands as sent back.
        if self.plan_view.as_ref().is_some_and(|view| view.asking) {
            self.plan_view = None;
            if let Some(plan) = &mut self.plan {
                plan.state = crate::plan::State::SentBack(None);
            }
        }
        if self.picker.take().is_some() {
            return;
        }
        if !self.input.is_empty() {
            self.input.clear();
            return;
        }
        if let Some(cancel) = &self.running {
            cancel.cancel();
            return;
        }
        match self.quit_armed {
            Some(at) if at.elapsed() < QUIT_WINDOW => self.quit = true,
            _ => {
                self.quit_armed = Some(Instant::now());
                self.transcript.note(tr!(quit_again), Tone::Quiet);
            }
        }
    }

    fn page(&self) -> isize {
        self.stream_height.saturating_sub(2).max(1) as isize
    }

    /// The place in the stream under the screen's cell; `None` outside the stream's rows.
    fn point_at(&self, column: u16, row: u16) -> Option<Point> {
        let (_, _, line) = self.rows.iter().find(|(at, _, _)| *at == row)?;
        Some(Point {
            line: *line,
            col: usize::from(column.saturating_sub(self.stream_x)),
        })
    }

    /// The left button moves held down: the selection follows, and the stream scrolls when the pointer leaves it at
    /// its top or bottom.
    fn drag(&mut self, column: u16, row: u16) {
        let Some((anchor, width)) = self.press else {
            return;
        };
        if width != self.stream_layout {
            self.press = None;
            return;
        }
        let (Some(first), Some(last)) = (self.rows.first().copied(), self.rows.last().copied())
        else {
            return;
        };
        let head = if row < first.0 {
            self.scroll_by(1);
            Point {
                line: first.2,
                col: 0,
            }
        } else if row > last.0 {
            self.scroll_by(-1);
            Point {
                line: last.2,
                col: usize::MAX,
            }
        } else {
            match self.point_at(column, row) {
                Some(point) => point,
                None => return,
            }
        };
        self.selection = Some(Selection {
            anchor,
            head,
            width: self.stream_layout,
        });
    }

    /// The left button is let go: a drag copies what it selected, a click (no drag) folds or unfolds the block.
    fn release(&mut self, row: u16) {
        let Some((press, _)) = self.press.take() else {
            return;
        };
        match self.selection {
            Some(selection) if !selection.is_empty() => {
                let text = select::selected_text(&selection, &self.stream_lines_text());
                if text.is_empty() {
                    return;
                }
                self.copy_text(&text);
            }
            _ => {
                self.selection = None;
                if let Some(&(_, block, _)) = self
                    .rows
                    .iter()
                    .find(|(at, _, line)| *at == row && *line == press.line)
                {
                    // A subagent's tile opens its own stream, a message with an image the image; any other block
                    // folds or unfolds.
                    if matches!(
                        self.transcript.blocks.get(block).map(|b| &b.kind),
                        Some(crate::transcript::Kind::Agent(_))
                    ) {
                        self.agent_view = Some(crate::agents::View::new(vec![block]));
                    } else if let Some(image) = self.image_of(block) {
                        self.picture_view = Some(image);
                    } else {
                        self.transcript.toggle(block);
                    }
                }
            }
        }
    }

    /// The number the next image gets: on from every image the session and the input hold.
    fn next_image(&self) -> u32 {
        self.first_to_send()
            .max(self.input.most_image() + 1)
            .max(self.images_given + 1)
    }

    /// The number the first image of the next message sent gets: on from every image the conversation holds and
    /// every one sent before.
    fn first_to_send(&self) -> u32 {
        self.conversation
            .iter()
            .flat_map(|message| &message.images)
            .map(|image| image.number)
            .max()
            .unwrap_or(0)
            .max(self.images_sent)
            + 1
    }

    /// `⌃V` (T45): the clipboard's image kept and into the message.
    fn attach_clipboard(&mut self) {
        self.attach_beside(vec![(None, Box::new(crate::pictures::from_clipboard))]);
    }

    /// Image files pasted or dropped (T45), kept and into the message in the order they came.
    fn attach_files(&mut self, paths: &[std::path::PathBuf]) {
        let reads: Vec<Read> = paths
            .iter()
            .map(|path| {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let path = path.clone();
                let read: Box<dyn FnOnce() -> Result<Vec<u8>, String> + Send> =
                    Box::new(move || glut::images::read_file(&path));
                (Some(name), read)
            })
            .collect();
        self.attach_beside(reads);
    }

    /// The images `reads` give, kept one after the other on a thread of their own (T47): the clipboard's program and
    /// the decoder take longer than a frame may wait. Each is numbered as it comes into the input.
    fn attach_beside(&mut self, reads: Vec<Read>) {
        self.attaching += reads.len();
        self.attaching_since.get_or_insert_with(Instant::now);
        let home = self.parts.home.clone();
        let reports = self.reports.clone();
        std::thread::spawn(move || {
            for (name, read) in reads {
                // A decoder that panics fails its image, and the input is told all the same.
                let kept = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    read().and_then(|bytes| glut::images::keep(&home, &bytes, 0))
                }))
                .unwrap_or_else(|_| Err(tr!(image_undecodable).into()));
                let _ = reports.send(Report::Attached { name, kept });
            }
        });
    }

    /// An image read beside the screen came: into the message, or why not.
    fn attached(&mut self, name: Option<String>, kept: Result<glut::images::Image, String>) {
        self.attaching = self.attaching.saturating_sub(1);
        if self.attaching == 0 {
            self.attaching_since = None;
        }
        let mut image = match kept {
            Ok(image) => image,
            Err(reason) => {
                match name {
                    None => self.say(reason),
                    Some(_) => self.transcript.note(reason, Tone::Fault),
                }
                return;
            }
        };
        image.number = self.next_image();
        self.images_given = image.number;
        let said = image.label();
        self.input.attach(image);
        if self.engine.images == Some(false) {
            self.transcript.note(
                tr!(image_unseen, image = said, model = self.engine.model_name()),
                Tone::Heed,
            );
        } else if let Some(name) = name {
            self.say(tr!(
                image_attached_named,
                name = crate::transcript::clean(&name),
                image = said
            ));
        } else {
            self.say(tr!(image_attached, image = said));
        }
    }

    /// The image a block of the stream is (T46).
    fn image_of(&self, block: usize) -> Option<glut::images::Image> {
        match &self.transcript.blocks.get(block)?.kind {
            crate::transcript::Kind::Picture(image) => Some(image.clone()),
            _ => None,
        }
    }

    /// A key while an image is shown whole: ⎋ closes it, `o` opens it in the system's viewer.
    fn on_picture_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.picture_view = None,
            KeyCode::Char('o') => {
                let Some(image) = self.picture_view.clone() else {
                    return;
                };
                let said = if glut::images::kept_media(&image.path).is_some() {
                    crate::pictures::open(&image.path).map(|()| tr!(image_opens, n = image.number))
                } else {
                    Err(tr!(image_not_kept, n = image.number))
                };
                match said {
                    Ok(said) | Err(said) => self.say(said),
                }
            }
            _ => {}
        }
    }

    /// `text` onto the clipboard, the footer saying how much.
    fn copy_text(&mut self, text: &str) {
        let count = text.chars().count();
        match select::copy(text) {
            Ok(Copied::Pbcopy) => self.say(crate::texts::t().copied.of(count as u64)),
            Ok(Copied::Terminal) => self.say(format!(
                "{} ({})",
                crate::texts::t().copied.of(count as u64),
                tr!(through_terminal)
            )),
            Err(reason) => self.say(reason),
        }
    }

    /// `⌃X ↑` (TUI9a): the reading mode on the stream's last block.
    fn start_reading(&mut self) {
        let Some(last) = self.transcript.blocks.len().checked_sub(1) else {
            return self.say(tr!(nothing_to_read));
        };
        self.selection = None;
        self.search = None;
        self.leave_panel();
        self.reading = Some(Reading {
            block: last,
            jump: true,
        });
        // The last block may draw nothing (a thought while thoughts are off): the nearest one that draws.
        self.read_step(0);
    }

    /// Moves the reading mode's choice `by` blocks — over those that draw nothing — and lets the stream follow.
    fn read_step(&mut self, by: isize) {
        let (width, thinking, frame) = (self.stream_layout, self.transcript.thinking, self.frame);
        let count = self.transcript.blocks.len();
        let Some(reading) = &mut self.reading else {
            return;
        };
        if count == 0 {
            self.reading = None;
            return;
        }
        let mut drawn = |at: usize| {
            !self.transcript.blocks[at]
                .lines(width, thinking, frame)
                .is_empty()
        };
        let from = reading.block.min(count - 1);
        // Upwards for a choice that only has to draw (`by` 0): the stream's end is where it starts.
        let back = by <= 0;
        let mut at = from;
        let mut left = by.unsigned_abs();
        loop {
            if left == 0 && drawn(at) {
                break;
            }
            let next = if back {
                at.checked_sub(1)
            } else {
                (at + 1 < count).then_some(at + 1)
            };
            let Some(next) = next else {
                break;
            };
            at = next;
            if drawn(at) {
                left = left.saturating_sub(1);
            }
        }
        // Past the last block that draws: the choice stays where it was.
        if !drawn(at) {
            at = from;
        }
        reading.block = at;
        reading.jump = true;
    }

    /// A key in the reading mode: ↑↓ (k j) a block, g G (Home End) the ends, ⏎ or Space folds it (a subagent's tile
    /// opens its stream), c copies it, / searches, PgUp PgDn scroll, Esc back to the input — no key types.
    ///
    /// A key with ⌃ or ⌥, an F key or ⇧⇥ goes on as ever (false: not taken here) — ⌃B, ⌃G, F3 still work.
    fn on_reading_key(&mut self, key: KeyEvent) -> bool {
        let count = self.transcript.blocks.len();
        let Some(chosen) = self
            .reading
            .map(|reading| reading.block.min(count.saturating_sub(1)))
        else {
            return false;
        };
        if count == 0 {
            self.reading = None;
            return false;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            || matches!(key.code, KeyCode::F(_) | KeyCode::BackTab)
        {
            return false;
        }
        // A held key folds once.
        if key.kind == KeyEventKind::Repeat
            && matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
        {
            return true;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.read_step(-1),
            KeyCode::Down | KeyCode::Char('j') => self.read_step(1),
            KeyCode::Home | KeyCode::Char('g') => {
                self.reading = Some(Reading {
                    block: 0,
                    jump: true,
                });
                self.read_step(0);
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.reading = Some(Reading {
                    block: count - 1,
                    jump: true,
                });
                self.read_step(0);
            }
            KeyCode::PageUp => self.scroll_by(self.page()),
            KeyCode::PageDown => self.scroll_by(-self.page()),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if matches!(
                    self.transcript.blocks[chosen].kind,
                    crate::transcript::Kind::Agent(_)
                ) {
                    self.agent_view = Some(crate::agents::View::new(vec![chosen]));
                } else if let Some(image) = self.image_of(chosen) {
                    // A picture shows whole (T46).
                    self.picture_view = Some(image);
                } else {
                    self.transcript.toggle(chosen);
                    if let Some(reading) = &mut self.reading {
                        reading.jump = true;
                    }
                }
            }
            KeyCode::Char('c') => {
                let text = self.transcript.blocks[chosen].kind.copied();
                self.copy_text(&text);
            }
            KeyCode::Char('/') => {
                self.reading = None;
                self.search = Some(Search::default());
            }
            KeyCode::Esc => self.reading = None,
            _ => {}
        }
        true
    }

    /// `⌃X ⇥` (TUI9a): the side panel takes the keys — opened first if it is hidden —, or gives them back.
    fn focus_panel(&mut self) {
        if self.panel_focus {
            return self.leave_panel();
        }
        self.search = None;
        self.reading = None;
        if self.panel != Some(true) {
            self.toggle_panel();
        }
        self.panel_focus = true;
        self.key_the_tab();
    }

    /// The panel gives the keys back to the input.
    fn leave_panel(&mut self) {
        self.panel_focus = false;
        self.agents_list = None;
        self.project_todos.keys = false;
    }

    /// The shown tab's list takes the keys where it has one (Agents, To-dos).
    fn key_the_tab(&mut self) {
        if self.tab == Tab::Agents && self.agents_list.is_none() {
            self.agents_list = crate::agents::overview(&self.transcript)
                .first()
                .map(|(path, _)| path.clone());
        }
        if self.tab == Tab::Todos && !self.project_todos.keys {
            self.project_todos.refresh(&self.parts.config.cwd);
            self.project_todos.keys = true;
        }
    }

    /// A key while the panel has them: ←→ (h l) the tabs, Esc back to the input, the rest the tab's — its list's
    /// (Agents, To-dos) or its scrolling. A key with ⌃ or ⌥ goes on as ever (false: not taken here).
    fn on_panel_key(&mut self, key: KeyEvent) -> bool {
        const TABS: [Tab; 6] = [
            Tab::Plan,
            Tab::Gate,
            Tab::Meter,
            Tab::Agents,
            Tab::Todos,
            Tab::Keys,
        ];
        if self.panel != Some(true) || !self.panel_drawn {
            self.leave_panel();
            return false;
        }
        // A search opened meanwhile (⌃F) has the keys, Esc closing it.
        if self.search.is_some() {
            return false;
        }
        if self.chord
            || key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return false;
        }
        let page = self.stream_height.saturating_sub(4).max(1);
        match key.code {
            KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l') => {
                let at = TABS.iter().position(|tab| *tab == self.tab).unwrap_or(0);
                let next = if matches!(key.code, KeyCode::Left | KeyCode::Char('h')) {
                    (at + TABS.len() - 1) % TABS.len()
                } else {
                    (at + 1) % TABS.len()
                };
                self.agents_list = None;
                self.project_todos.keys = false;
                self.show_tab(TABS[next]);
                self.key_the_tab();
                return true;
            }
            KeyCode::Esc => {
                self.leave_panel();
                return true;
            }
            _ => {}
        }
        match self.tab {
            Tab::Agents | Tab::Todos => {
                self.key_the_tab();
                let taken = if self.tab == Tab::Agents {
                    self.agents_list.is_some() && self.on_agents_list_key(key)
                } else {
                    self.on_todos_key(key)
                };
                let keyed = if self.tab == Tab::Agents {
                    self.agents_list.is_some()
                } else {
                    self.project_todos.keys
                };
                if taken && !keyed {
                    // The list gave the keys away on purpose (`a` writes `/todo ` into the input): the panel too.
                    self.leave_panel();
                } else if !taken {
                    // A key the list did not take is nobody's while the panel has the keys; the list keeps them.
                    self.key_the_tab();
                }
            }
            Tab::Keys => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.keys_scroll = self.keys_scroll.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.keys_scroll = self.keys_scroll.saturating_add(1)
                }
                KeyCode::PageUp => self.keys_scroll = self.keys_scroll.saturating_sub(page),
                KeyCode::PageDown => self.keys_scroll = self.keys_scroll.saturating_add(page),
                _ => {}
            },
            Tab::Plan | Tab::Gate | Tab::Meter => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.panel_scroll = self.panel_scroll.saturating_sub(1)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    self.panel_scroll = self.panel_scroll.saturating_add(1)
                }
                KeyCode::PageUp => self.panel_scroll = self.panel_scroll.saturating_sub(page),
                KeyCode::PageDown => self.panel_scroll = self.panel_scroll.saturating_add(page),
                _ => {}
            },
        }
        true
    }

    /// The whole stream laid out at `width`, as the terminal's scrollback gets it: folded as the screen shows it.
    pub fn stream_at(&mut self, width: u16) -> Vec<ratatui::text::Line<'static>> {
        let thinking = self.transcript.thinking;
        self.transcript
            .blocks
            .iter_mut()
            .fold(Vec::new(), |mut lines, block| {
                // A picture in the terminal's scrollback is its line: neither blank cells nor half blocks (T46).
                match &block.kind {
                    crate::transcript::Kind::Picture(image) => {
                        lines.push(ratatui::text::Line::from(format!("  {}", image.label())));
                    }
                    _ => lines.extend_from_slice(block.lines(width, thinking, 0)),
                }
                lines
            })
    }

    /// Every line of the stream as text, at the width it was last drawn: what a selection copies from.
    pub fn stream_lines_text(&mut self) -> Vec<String> {
        let (width, thinking, frame) = (self.stream_layout, self.transcript.thinking, self.frame);
        self.transcript
            .blocks
            .iter_mut()
            .flat_map(|block| {
                let picture = matches!(block.kind, crate::transcript::Kind::Picture(_));
                let lines = block.lines(width, thinking, frame);
                let last = lines.len().saturating_sub(1);
                lines
                    .iter()
                    .enumerate()
                    // A picture's rows copy as nothing, its line as itself (T46).
                    .map(|(at, line)| {
                        if picture && at < last {
                            String::new()
                        } else {
                            select::text_of(line)
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// A key while the search is open: typed text searches, Enter or ↑ goes to the match above, Shift+Enter or ↓ to
    /// the one below, PgUp/PgDn scroll, Esc closes it.
    fn on_search_key(&mut self, key: KeyEvent) {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match key.code {
            KeyCode::PageUp => return self.scroll_by(self.page()),
            KeyCode::PageDown => return self.scroll_by(-self.page()),
            _ => {}
        }
        let Some(search) = &mut self.search else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter if shift => search.step(1),
            KeyCode::Down => search.step(1),
            KeyCode::Enter | KeyCode::Up => search.step(-1),
            KeyCode::Backspace => {
                search.query.pop();
                search.fresh = true;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                search.query.push(c);
                search.fresh = true;
            }
            _ => {}
        }
    }

    fn scroll_by(&mut self, lines: isize) {
        let most = self.stream_lines.saturating_sub(self.stream_height);
        self.scroll = self.scroll.saturating_add_signed(lines).min(most);
    }
}

/// What to do with a step of `/steps`.
fn step_do(run: String, turn: u32) -> Picker {
    let items = vec![
        Item::plain(
            tr!(step_show),
            tr!(step_show_detail),
            Pick::StepDo {
                run: run.clone(),
                turn,
                restore: false,
            },
        ),
        Item::plain(
            tr!(step_restore),
            tr!(step_restore_detail),
            Pick::StepDo {
                run,
                turn,
                restore: true,
            },
        ),
    ];
    Picker::new(tr!(step_title, turn = turn), items, None)
}

/// How soon a second Esc opens rewind.
const DOUBLE_ESC: Duration = Duration::from_millis(600);

/// How long a walk of the folder stands for its files.
const FILES_FRESH: Duration = Duration::from_secs(60);

/// The owner's command in `text` — `!cmd` or `!!cmd` as its whole line, the command right after the marks — and
/// whether it is for the owner alone; `None` for any other message ("! wow", "!!!", two lines).
pub fn owners_command(text: &str) -> Option<(bool, String)> {
    if text.contains('\n') {
        return None;
    }
    let (alone, rest) = match text.strip_prefix("!!") {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('!')?),
    };
    let first = rest.chars().next()?;
    if first.is_whitespace() || first == '!' {
        return None;
    }
    Some((alone, rest.trim_end().to_owned()))
}

/// How long a toast stays in the key line.
const TOAST_FOR: Duration = Duration::from_secs(3);

/// The reading mode's choice: a block of the stream, and whether the stream still has to scroll to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    pub block: usize,
    pub jump: bool,
}

/// The search in the stream (Ctrl+F): what is looked for, where it stands, which match is shown.

#[derive(Debug, Default)]
pub struct Search {
    pub query: String,
    /// Each match: the stream's line and its columns; made by the frame, which has the stream's lines.
    pub matches: Vec<(usize, usize, usize)>,
    /// The match shown, an index into `matches`.
    pub current: Option<usize>,
    /// The query changed: the frame finds the matches anew and goes to the last one (the newest).
    pub fresh: bool,
    /// The frame scrolls the shown match into view.
    pub jump: bool,
    /// What the matches were found for: the query, and how many layouts of blocks there had been
    /// ([`crate::transcript::layouts`]) — found again only when one of them changed (T47), not at every frame.
    pub searched: Option<(String, u64)>,
}

impl Search {
    /// One match up (`-1`, older) or down (`1`, newer), going round at the ends.
    fn step(&mut self, by: isize) {
        if self.matches.is_empty() {
            return;
        }
        let count = self.matches.len() as isize;
        let at = self.current.map_or(count - 1, |at| at as isize);
        self.current = Some((at + by).rem_euclid(count) as usize);
        self.jump = true;
    }
}

/// Glut's own colours and the app's themes, the one in use chosen; a light theme says it wants a light terminal.
fn theme_picker() -> Picker {
    let items = crate::theme::all()
        .map(|palette| {
            // Glut colours the text, never the terminal's background: a light theme reads on a light terminal.
            let detail = match palette.name {
                "glut" => tr!(source_glut),
                "catppuccin-latte" | "github-light" | "paper" => tr!(theme_light),
                _ => tr!(theme_axiomata),
            };
            Item::plain(palette.name, detail, Pick::Theme(palette.name.to_owned()))
        })
        .collect();
    let current = Pick::Theme(crate::theme::c().name.to_owned());
    Picker::new(tr!(theme_title), items, Some(&current))
}

/// Glut's languages, each by its own name, the one read now marked (T48).
fn language_picker() -> Picker {
    let now = glut::lang::current();
    let items = glut::lang::Lang::ALL
        .into_iter()
        .map(|lang| {
            let detail = if lang == now {
                format!("{} · {}", lang.code(), tr!(language_now))
            } else {
                lang.code().to_owned()
            };
            Item::plain(lang.name(), detail, Pick::Language(lang))
        })
        .collect();
    Picker::new(tr!(language_title), items, Some(&Pick::Language(now)))
}

/// The effort levels, and "the provider's default".
fn effort_picker(current: Option<Effort>) -> Picker {
    let mut items = vec![Item::plain(
        tr!(effort_default),
        tr!(effort_default_detail),
        Pick::Effort(None),
    )];
    items.extend(
        Effort::ALL
            .iter()
            .map(|effort| Item::plain(effort.to_string(), "", Pick::Effort(Some(*effort)))),
    );
    Picker::new(tr!(effort_title), items, Some(&Pick::Effort(current)))
}

/// The editors on the `PATH`, the one in use named in the title.
fn editor_picker(current: Option<String>) -> Picker {
    let items = prefs::installed_editors()
        .into_iter()
        .map(|(program, command)| Item::plain(program, command, Pick::Editor(command.into())))
        .collect();
    let now = current.as_deref().unwrap_or(tr!(meter_none_yet));
    // `{file}` and `{line}` stand in the title as `/editor` takes them: filled with themselves.
    let title = tr!(editor_title, now = now, file = "{file}", line = "{line}");
    Picker::new(title, items, current.map(Pick::Editor).as_ref())
}

/// A model's window, price and whether it reasons, for the picker.
fn model_detail(model: &models::ModelInfo) -> String {
    let mut parts = Vec::new();
    if let Some(window) = model.context_window {
        parts.push(format!("{}k", window / 1024));
    }
    if let Some(prices) = model.prices {
        parts.push(format!(
            "{}/{}",
            glut::lang::usd(prices.input_per_m, 2),
            glut::lang::usd(prices.output_per_m, 2)
        ));
    }
    if model.reasoning == Some(true) {
        parts.push(tr!(model_reasons).into());
    }
    parts.join(" · ")
}

/// The largest window `/context` takes: far above any model's today, a stop for a slip of the finger.
const MAX_WINDOW: u32 = 64 * 1024 * 1024;

/// A token count as the owner types it: `65536`, `64k`, `1.5M` — digits and one point, a k or an M.
fn parse_tokens(text: &str) -> Option<u32> {
    let text = text.trim().to_lowercase();
    let (number, factor) = match text.strip_suffix('k') {
        Some(number) => (number, 1024.0),
        None => match text.strip_suffix('m') {
            Some(number) => (number, 1024.0 * 1024.0),
            None => (text.as_str(), 1.0),
        },
    };
    let plain = !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit() || c == '.')
        && number.chars().filter(|c| *c == '.').count() <= 1;
    if !plain {
        return None;
    }
    let tokens = (number.parse::<f64>().ok()? * factor).round();
    (tokens >= 1.0 && tokens <= f64::from(MAX_WINDOW)).then_some(tokens as u32)
}

/// Whether `prompt` is one of the owner's messages of `conversation` already.
fn told(conversation: &[Message], prompt: &str) -> bool {
    conversation.iter().any(|message| {
        message.role == glut::model::Role::User && message.content.as_deref() == Some(prompt)
    })
}

/// Whether command `name` (with `rest`) waits while Glut works: what changes the model, the conversation or the
/// window the turn counts with.
fn waits(name: &str, rest: &str) -> bool {
    matches!(
        name,
        "model"
            | "provider"
            | "effort"
            | "compact"
            | "clear"
            | "handoff"
            | "resume"
            | "init"
            | "skill"
    ) || (name == "context" && !rest.is_empty())
}

/// A finish reason in words.
fn reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Answered => tr!(ended_answered),
        FinishReason::MaxTurns => tr!(ended_steps),
        FinishReason::Timeout => tr!(ended_time),
        FinishReason::Cancelled => tr!(stopped),
        FinishReason::Failed => tr!(ended_failed),
        FinishReason::Refused => tr!(ended_refused),
        FinishReason::Budget => tr!(ended_budget),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use glut::RunOutcome;

    fn app() -> (App, mpsc::UnboundedReceiver<Report>) {
        let (parts, engine) = session::for_tests();
        let (send, receive) = mpsc::unbounded_channel();
        (App::new(Arc::new(parts), engine, send), receive)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> TermEvent {
        TermEvent::Key(KeyEvent::new(code, modifiers))
    }

    /// `⌃X A`: the Agents tab (TUI9a; ⌃A is the line's start).
    fn agents_key(app: &mut App) {
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::NONE));
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> TermEvent {
        TermEvent::Mouse(crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    /// One frame on an 80 × 24 screen, as the terminal would draw it.
    fn drawn(app: &mut App) -> ratatui::Terminal<ratatui::backend::TestBackend> {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| crate::view::draw(frame, app))
            .unwrap();
        terminal
    }

    #[test]
    fn the_footer_carries_the_status_and_the_keys_tab_reads_with_its_legend() {
        let (mut app, _reports) = app();
        app.separators = crate::footer::Separators::Plain;
        app.cost = Some(0.0123);
        app.panel = Some(true);
        app.tab = Tab::Keys;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(130, 30)).unwrap();
        terminal
            .draw(|frame| crate::view::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol().to_owned())
                .collect::<String>()
        };
        let screen: Vec<String> = (0..buffer.area.height).map(row).collect();
        if std::env::var_os("GLUT_SHOW_SCREEN").is_some() {
            println!("{}", screen.join("\n"));
        }
        let footer = screen.last().unwrap();
        assert!(
            footer.contains(app.engine.model_name())
                && footer.contains("ctx ")
                && footer.contains("$0.0123"),
            "{footer}"
        );
        assert!(!screen[0].contains("ctx "), "the header no longer counts");
        assert!(
            screen.iter().any(|row| row.contains("⌃ Ctrl · ⇧ Shift")),
            "the legend heads the sheet"
        );
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn a_drag_selects_the_text_under_it_and_lets_go_on_esc() {
        let (mut app, _reports) = app();
        app.transcript
            .push(crate::transcript::Kind::Owner("hello".into()));
        app.transcript.note("alpha beta gamma", Tone::Quiet);
        app.transcript.note("second line", Tone::Quiet);
        drawn(&mut app);
        let (row, _, _) = *app.rows.iter().rev().nth(1).unwrap();
        app.on_terminal(mouse(MouseEventKind::Down(MouseButton::Left), 2, row));
        app.on_terminal(mouse(MouseEventKind::Drag(MouseButton::Left), 6, row + 1));
        drawn(&mut app);
        let selection = app.selection.unwrap();
        assert_eq!(
            select::selected_text(&selection, &app.stream_lines_text()),
            "alpha beta gamma\n· secon"
        );
        let buffer = drawn(&mut app).backend().buffer().clone();
        let reversed = ratatui::style::Modifier::REVERSED;
        assert!(
            buffer[(3, row)].modifier.contains(reversed),
            "the selection shows"
        );
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.selection.is_none());
    }

    #[test]
    fn search_finds_the_newest_match_first_and_steps_up_and_down() {
        let (mut app, _reports) = app();
        for at in 0..3 {
            app.transcript.note(format!("needle {at}"), Tone::Quiet);
        }
        drawn(&mut app);
        app.on_terminal(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        for c in "NEEDLE".chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::SHIFT));
        }
        drawn(&mut app);
        let search = app.search.as_ref().unwrap();
        assert_eq!(search.matches.len(), 3, "in any case");
        assert_eq!(search.current, Some(2), "the newest first");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.search.as_ref().unwrap().current,
            Some(1),
            "Enter: the one above"
        );
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            app.search.as_ref().unwrap().current,
            Some(0),
            "round at the end"
        );
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.search.is_none());
        assert!(
            app.input.is_empty(),
            "the search typed nothing into the input"
        );
    }

    #[test]
    fn the_search_jumps_to_a_match_far_above_and_shows_it() {
        let (mut app, _reports) = app();
        for at in 0..200 {
            let text = if at == 5 {
                "the needle".to_owned()
            } else {
                format!("line {at}")
            };
            app.transcript.note(text, Tone::Quiet);
        }
        drawn(&mut app);
        app.on_terminal(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        for c in "needle".chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        drawn(&mut app);
        drawn(&mut app);
        assert!(!app.rows.is_empty(), "the stream is drawn, not blank");
        assert!(
            app.rows.iter().any(|(_, _, line)| *line == 5),
            "the match is on the screen"
        );
        app.on_terminal(TermEvent::Paste("x".into()));
        assert_eq!(
            app.search.as_ref().unwrap().query,
            "needlex",
            "a paste goes to the search"
        );
        assert!(app.input.is_empty());
    }

    #[test]
    fn ctrl_k_keeps_the_keys_in_the_panel_or_shows_them_over_a_narrow_stream_only_to_read() {
        let (mut app, _reports) = app();
        app.panel_fits = true;
        app.on_terminal(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!((app.panel, app.tab), (Some(true), Tab::Keys));
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(
            app.input.text(),
            "x",
            "the tab takes no key: the input types"
        );
        app.on_terminal(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.panel, Some(false), "Ctrl+K again hides the panel");
        app.panel_fits = false;
        app.on_terminal(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert_eq!(app.keys_sheet, Some(0));
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(app.keys_sheet, Some(1));
        assert_eq!(app.input.text(), "x", "nothing typed through the sheet");
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.keys_sheet, None);
    }

    #[tokio::test]
    async fn a_resting_session_keeps_its_cache_warm_until_a_turn_or_off_and_the_reads_are_counted()
    {
        let (mut app, _reports) = app();
        let done = |warm: Option<glut::Warm>| {
            Report::Done(
                RunOutcome {
                    reason: FinishReason::Answered,
                    answer: Some("ok".into()),
                    turns: 1,
                    usage: Usage::default(),
                    refusals: 0,
                    warm,
                },
                vec![glut::Message::user("hi"), glut::Message::user("ok")],
            )
        };
        let warm = glut::Warm {
            length: 1,
            tools: Vec::new(),
        };
        app.on_report(done(Some(warm.clone())));
        assert!(app.keeper.is_some(), "pinging by default");
        assert!(
            app.warm_until.is_none() && !app.cache_seen,
            "no mark before the provider said it caches"
        );
        app.on_report(Report::Warmed(Ok(Usage {
            input_tokens: 9000,
            cached_input_tokens: 8900,
            output_tokens: 0,
        })));
        assert!(app.cache_seen && app.warm_until.is_some_and(|until| until > Instant::now()));
        assert_eq!(app.usage.input_tokens, 9000, "the reads are counted");
        app.send("next".into());
        assert!(app.keeper.is_none(), "a turn reads the cache itself");
        app.busy = None;
        app.on_report(done(Some(warm.clone())));
        typed(&mut app, "/cache off");
        assert!(app.keeper.is_none());
        assert_eq!(
            Prefs::load(&app.parts.home).cache,
            Some(crate::warm::CacheMode::Off)
        );
        app.on_report(done(Some(warm)));
        assert!(app.keeper.is_none(), "off keeps nothing");
        app.on_report(Report::Warmed(Err("401".into())));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[tokio::test]
    async fn an_at_sign_at_a_word_start_opens_the_files_and_esc_keeps_it() {
        let (mut app, mut reports) = app();
        app.input.insert_text("look at");
        app.on_terminal(key(KeyCode::Char('@'), KeyModifiers::NONE));
        assert!(app.picker.is_none(), "inside a word it is only a character");
        app.on_terminal(key(KeyCode::Char(' '), KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('@'), KeyModifiers::NONE));
        assert!(app.picker.is_some(), "after a space the files open");
        assert!(
            app.picker.as_ref().unwrap().title.contains("looking"),
            "the walk runs beside the screen"
        );
        let walked = reports.recv().await.unwrap();
        app.on_report(walked);
        assert!(
            !app.picker.as_ref().unwrap().title.contains("looking"),
            "the files came"
        );
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "look at@ @");
    }

    #[tokio::test]
    async fn an_owners_command_runs_and_only_a_single_bang_reaches_the_model() {
        let (mut app, mut reports) = app();
        typed(&mut app, "!!echo alone");
        typed(&mut app, "!echo shared");
        for _ in 0..2 {
            let report = reports.recv().await.unwrap();
            app.on_report(report);
        }
        assert_eq!(
            app.conversation.len(),
            1,
            "only !cmd joins the conversation"
        );
        let told = format!("{:?}", app.conversation[0]);
        assert!(
            told.contains("echo shared") && told.contains("shared\\n"),
            "{told}"
        );
        let done = app
            .transcript
            .blocks
            .iter()
            .filter(|block| {
                matches!(&block.kind, crate::transcript::Kind::Tool { state, .. }
                if *state == crate::transcript::ToolState::Done)
            })
            .count();
        assert_eq!(done, 2);
    }

    #[test]
    fn enter_while_working_steers_tab_queues_and_ctrl_up_takes_back() {
        let (mut app, _reports) = app();
        let inbox = glut::Inbox::default();
        app.steering = Some(inbox.clone());
        app.busy = Some(Busy::Turn);
        app.running = Some(Cancel::new());
        typed(&mut app, "also check the tests");
        assert_eq!(inbox.waiting(), ["also check the tests"], "Enter steers");
        app.input.insert_text("then write the docs");
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.queue, ["then write the docs"], "Tab queues");
        assert!(app.input.is_empty());
        app.on_terminal(key(KeyCode::Up, KeyModifiers::CONTROL));
        assert_eq!(
            app.input.text(),
            "then write the docs",
            "the last waiting comes back first"
        );
        app.input.clear();
        app.on_terminal(key(KeyCode::Up, KeyModifiers::CONTROL));
        assert_eq!(
            app.input.text(),
            "also check the tests",
            "then the one sent to steer"
        );
        assert!(inbox.is_empty());
    }

    #[tokio::test]
    async fn a_failed_turn_keeps_the_queue_for_enter() {
        let (mut app, _reports) = app();
        app.busy = Some(Busy::Turn);
        app.running = Some(Cancel::new());
        app.queue.push("queued".into());
        app.on_report(Report::Done(
            RunOutcome {
                reason: FinishReason::Failed,
                answer: None,
                turns: 1,
                usage: Usage::default(),
                refusals: 0,
                warm: None,
            },
            Vec::new(),
        ));
        assert!(!app.working(), "nothing ran into the same wall");
        assert_eq!(app.queue, ["queued"]);
        typed(&mut app, "");
        assert!(app.working(), "Enter on an empty input sends it");
        assert!(app.queue.is_empty());
    }

    #[tokio::test]
    async fn a_turn_that_ends_sends_the_queue_and_what_was_never_taken_in() {
        let (mut app, _reports) = app();
        let inbox = glut::Inbox::default();
        inbox.send("late steer");
        app.steering = Some(inbox);
        app.busy = Some(Busy::Turn);
        app.running = Some(Cancel::new());
        app.queue.push("queued".into());
        app.on_report(Report::Done(
            RunOutcome {
                reason: FinishReason::Answered,
                answer: None,
                turns: 1,
                usage: Usage::default(),
                refusals: 0,
                warm: None,
            },
            Vec::new(),
        ));
        assert!(app.working(), "the next message went out");
        assert_eq!(
            app.queue,
            ["queued"],
            "the late steer first, the queued one after it"
        );
    }

    #[test]
    fn esc_esc_rewinds_the_conversation_to_before_a_message_and_gives_it_back() {
        let (mut app, _reports) = app();
        app.conversation = vec![
            Message::user("first"),
            Message::system("one"),
            Message::user("second"),
            Message::system("two"),
        ];
        app.session.marks = vec![
            crate::store::Mark {
                at: Some(0),
                prompt: "first".into(),
                run: None,
            },
            crate::store::Mark {
                at: Some(2),
                prompt: "second".into(),
                run: None,
            },
        ];
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picker.is_none(), "one Esc is no rewind");
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        let picker = app.picker.as_ref().expect("Esc Esc opens rewind");
        assert_eq!(
            picker.chosen(),
            Some(Pick::RewindTo(1)),
            "the newest message first"
        );
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        let how = app.picker.as_ref().expect("then how far");
        assert_eq!(
            how.chosen(),
            Some(Pick::Rewind {
                mark: 1,
                conversation: true,
                files: false
            }),
            "no run changed a file: the conversation only"
        );
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.conversation.len(), 2);
        assert_eq!(app.session.marks.len(), 1);
        assert_eq!(app.input.text(), "second", "the message comes back to edit");
    }

    #[test]
    fn after_a_compaction_the_conversation_cannot_go_back_and_a_snapshot_names_the_run() {
        let (mut app, _reports) = app();
        app.conversation = vec![Message::user("first"), Message::system("one")];
        app.session.marks = vec![crate::store::Mark {
            at: Some(0),
            prompt: "first".into(),
            run: None,
        }];
        app.on_report(Report::Event(Event::Snapshot {
            turn: 1,
            run: "run-a".into(),
            commit: "0".repeat(40),
        }));
        app.on_report(Report::Event(Event::Snapshot {
            turn: 2,
            run: "run-b".into(),
            commit: "1".repeat(40),
        }));
        assert_eq!(
            app.session.marks[0].run.as_deref(),
            Some("run-a"),
            "the first snapshot names the run"
        );
        app.on_report(Report::Event(Event::Compacted {
            turn: 1,
            before_tokens: 10_000,
            after_tokens: 1_000,
            replaced: 2,
        }));
        assert_eq!(app.session.marks[0].at, None);
        app.rewind(0, true, false);
        assert_eq!(
            app.conversation.len(),
            2,
            "nothing is cut at a place that no longer holds"
        );
    }

    #[test]
    fn a_fork_is_a_new_session_from_before_the_message_and_the_old_one_stays() {
        let (mut app, _reports) = app();
        app.session.title = "work".into();
        app.conversation = vec![
            Message::user("first"),
            Message::system("one"),
            Message::user("second"),
        ];
        app.session.marks = vec![
            crate::store::Mark {
                at: Some(0),
                prompt: "first".into(),
                run: None,
            },
            crate::store::Mark {
                at: Some(2),
                prompt: "second".into(),
                run: None,
            },
        ];
        let old = app.session.id.clone();
        app.fork(1);
        assert_ne!(app.session.id, old, "a session of its own");
        assert_eq!(app.session.parent.as_deref(), Some(old.as_str()));
        assert_eq!(app.conversation.len(), 2);
        assert_eq!(app.session.marks.len(), 1);
        assert_eq!(app.input.text(), "second");
        let kept = Session::load(&app.parts.home, &old).unwrap();
        assert_eq!(kept.conversation.len(), 3, "the session left is kept whole");
        // In /resume the fork hangs under its session, drawn as one and marked as the session in use.
        let picker = app.session_picker();
        let labels: Vec<&str> = picker
            .shown()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["work", "└─⑂ ● work (fork)"], "{labels:?}");
        assert_eq!(picker.chosen(), Some(Pick::Session(app.session.id.clone())));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[tokio::test]
    async fn the_gate_tab_follows_the_judgements_and_gate_takes_an_always_back() {
        /// Says "always" to every ask.
        struct Yes;
        impl glut::permissions::Asker for Yes {
            fn ask<'a>(&'a self, _request: &'a AskRequest) -> glut::permissions::AnswerFuture<'a> {
                Box::pin(async { Answer::AllowAlways })
            }
        }
        let (mut parts, engine) = session::for_tests();
        let rules = glut::RuleSet {
            deny: vec![glut::Rule::parse("Bash(rm:*)").unwrap()],
            ..glut::RuleSet::default()
        };
        let gate = Arc::new(glut::Gate::new(rules, glut::Mode::Default, Arc::new(Yes)));
        parts.tools.set_gate(gate.clone());
        let (send, _reports) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        let make = glut::Access::Bash {
            command: "make build".into(),
            folder: "/tmp".into(),
            unsandboxed: false,
            sandboxed: false,
        };
        assert!(gate.judge("bash", &make).await.is_ok());
        for judged in gate.take_log() {
            app.on_report(Report::Event(Event::permission(1, "c1", &judged)));
        }
        assert_eq!(app.watch.seen.len(), 1);

        app.on_terminal(key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert_eq!((app.panel, app.tab), (Some(true), Tab::Gate));
        let (_, _, granted, rules) = app.guard_facts();
        assert_eq!(granted, ["Bash(make build)"]);
        assert_eq!(rules, (0, 0, 1), "the always is no settings rule");

        let picker = app.gate_picker();
        let labels: Vec<&str> = picker
            .shown()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["allow Bash(make build)", "deny Bash(rm:*)"]);
        app.picker = Some(picker);
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(gate.granted().is_empty(), "taken back");

        app.on_terminal(key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        assert_eq!(app.panel, Some(false), "Ctrl+G on the gate hides the panel");
        app.on_terminal(key(KeyCode::Char('g'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(
            (app.panel, app.tab),
            (Some(true), Tab::Plan),
            "Ctrl+B turns an open panel to the plan"
        );
        app.on_terminal(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(app.panel, Some(false));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[tokio::test]
    async fn a_rejected_hunk_goes_back_on_disk_and_the_model_is_told_why() {
        let folder = std::env::temp_dir().join(format!("glut-review-{}", std::process::id()));
        let root = folder.with_extension("snapshots");
        std::fs::create_dir_all(&folder).unwrap();
        let folder = folder.canonicalize().unwrap();
        std::fs::write(folder.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        glut::snapshot::Snapshots::open(&root, &folder, "run1")
            .unwrap()
            .take(1)
            .unwrap();
        std::fs::write(folder.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(folder.join("made.txt"), "new\n").unwrap();

        let (mut parts, engine) = session::for_tests();
        parts.config.cwd = folder.clone();
        parts.config.snapshots = Some(root.clone());
        let (send, mut reports) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        app.session.marks.push(crate::store::Mark {
            at: Some(0),
            prompt: "make it loud".into(),
            run: Some("run1".into()),
        });
        app.on_terminal(key(KeyCode::Char('r'), KeyModifiers::CONTROL));
        let report = reports.recv().await.unwrap();
        app.on_report(report);
        assert!(app.review.open);
        assert_eq!(app.review.files.len(), 2);

        // a.txt: reject its hunk, saying why.
        app.on_terminal(key(KeyCode::Char('r'), KeyModifiers::NONE));
        for c in "keep it".chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            std::fs::read_to_string(folder.join("a.txt")).unwrap(),
            "one\ntwo\nthree\n"
        );
        let told = app
            .pending
            .last()
            .and_then(|message| message.content.clone())
            .unwrap();
        assert!(
            told.contains("a.txt") && told.contains("Why: keep it"),
            "{told}"
        );
        let report = reports.recv().await.unwrap();
        app.on_report(report);
        assert_eq!(
            app.review.files.len(),
            1,
            "what was put back is no change any more"
        );

        // made.txt: a file the change made goes when its only hunk is rejected.
        app.on_terminal(key(KeyCode::Char('r'), KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!folder.join("made.txt").exists());
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.review.open);
        let _ = std::fs::remove_dir_all(&folder);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn only_a_first_bang_right_before_a_command_makes_one() {
        assert_eq!(owners_command("!ls -la"), Some((false, "ls -la".into())));
        assert_eq!(
            owners_command("!!git status"),
            Some((true, "git status".into()))
        );
        for message in ["! wow", "!!!", "!", "!!", "hi !ls", " !ls", "!ls\nmore"] {
            assert_eq!(owners_command(message), None, "{message:?}");
        }
    }

    #[tokio::test]
    async fn a_commands_output_that_comes_during_a_turn_waits_for_the_conversation() {
        let (mut app, _reports) = app();
        app.busy = Some(Busy::Switching);
        app.on_report(Report::Shell {
            id: "shell-1".into(),
            command: "echo x".into(),
            ran: crate::shell::Ran {
                ok: true,
                output: "x".into(),
            },
            to_model: true,
        });
        assert!(
            app.conversation.is_empty(),
            "held while the conversation is not the session's"
        );
        app.on_report(Report::Switched(Err("no".into())));
        assert_eq!(app.conversation.len(), 1, "it joins once it is");
    }

    #[test]
    fn a_click_without_a_drag_still_unfolds_a_tool_line() {
        let (mut app, _reports) = app();
        app.transcript.take(&glut::Event::ToolCall {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"a.rs"}"#.into(),
        });
        app.transcript.take(&glut::Event::ToolResult {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            ok: true,
            output: "fn a() {}".into(),
            truncated: false,
        });
        drawn(&mut app);
        let (row, _, _) = *app.rows.last().unwrap();
        app.on_terminal(mouse(MouseEventKind::Down(MouseButton::Left), 4, row));
        app.on_terminal(mouse(MouseEventKind::Up(MouseButton::Left), 4, row));
        assert!(matches!(
            app.transcript.blocks.last().unwrap().kind,
            crate::transcript::Kind::Tool { unfolded: true, .. }
        ));
    }

    /// A subagent's run as the harness reports it: the parent's `task` call, the subagent's steps wrapped once — and,
    /// when `nested`, a subagent of its own inside — then the parent's result.
    fn subagent_events(nested: bool) -> Vec<glut::Event> {
        use glut::Event;
        let sub = |event: Event| Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(event),
        };
        let mut events = vec![Event::ToolCall {
            turn: 1,
            id: "t1".into(),
            name: "task".into(),
            arguments: r#"{"description":"find the callers","prompt":"look"}"#.into(),
        }];
        events.push(sub(Event::ToolCall {
            turn: 1,
            id: "s1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"run.rs"}"#.into(),
        }));
        events.push(sub(Event::Usage {
            turn: 1,
            input_tokens: 1200,
            cached_input_tokens: 0,
            output_tokens: 300,
        }));
        if nested {
            events.push(sub(Event::ToolCall {
                turn: 2,
                id: "t2".into(),
                name: "task".into(),
                arguments: r#"{"description":"deeper","prompt":"look deeper"}"#.into(),
            }));
            events.push(sub(Event::Sub {
                depth: 2,
                task: None,
                event: Box::new(Event::AssistantText {
                    turn: 1,
                    text: "found it deep".into(),
                }),
            }));
            events.push(sub(Event::ToolResult {
                turn: 2,
                id: "t2".into(),
                name: "task".into(),
                ok: true,
                output: "found it deep".into(),
                truncated: false,
            }));
        }
        events.push(sub(Event::AssistantText {
            turn: 3,
            text: "two callers".into(),
        }));
        events.push(Event::ToolResult {
            turn: 1,
            id: "t1".into(),
            name: "task".into(),
            ok: true,
            output: "two callers".into(),
            truncated: false,
        });
        events
    }

    /// A task call and its answer at depth 0: a subagent that came and went.
    fn task(id: &str, label: &str, ended: bool) -> Vec<Event> {
        let mut events = vec![Event::ToolCall {
            turn: 1,
            id: id.into(),
            name: "task".into(),
            arguments: format!(r#"{{"description":"{label}","prompt":"go"}}"#),
        }];
        if ended {
            events.push(Event::ToolResult {
                turn: 1,
                id: id.into(),
                name: "task".into(),
                ok: true,
                output: "ok".into(),
                truncated: false,
            });
        }
        events
    }

    #[test]
    fn skill_alone_lists_the_skills_and_an_unknown_one_is_said() {
        let (mut parts, engine) = session::for_tests();
        parts.skills = vec![("make-agent".into(), "Makes a subagent.".into())];
        let (send, _reports) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        typed(&mut app, "/skill");
        assert!(app.picker.is_some());
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "/skill make-agent ");
        app.input.clear();
        typed(&mut app, "/skill nothing");
        let said = app.transcript.blocks.iter().any(|block| {
            matches!(&block.kind, crate::transcript::Kind::Note { text, .. } if text == "no skill nothing: make-agent")
        });
        assert!(said);
    }

    #[test]
    fn pick_model_is_answered_by_the_owners_pick_or_by_none() {
        let (mut app, _reports) = app();
        let (reply, mut answer) = oneshot::channel();
        app.on_report(Report::PickModel(reply));
        assert!(
            app.picker
                .as_ref()
                .is_some_and(|picker| picker.title.starts_with("Glut asks: the model"))
        );
        // A key typed before the list showed picks nothing.
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picker.is_some());
        app.model_pick_opened = Instant::now() - ASK_SETTLE;
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(answer.try_recv().unwrap(), None);
        assert!(app.picker.is_none() && app.model_pick.is_none());
        // Another list in its place answers it with none: the tool never waits on a list gone.
        let (reply, mut answer) = oneshot::channel();
        app.on_report(Report::PickModel(reply));
        app.picker = Some(Picker::new("Theme", Vec::new(), None));
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(answer.try_recv().unwrap(), None);
    }

    #[test]
    fn a_guide_too_large_is_said_in_orange_at_the_start_and_quiet_once_init_checked_it() {
        let dir = std::env::temp_dir().join(format!("glut-tui-app-guide-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let guide = dir.join("AGENTS.md");
        std::fs::write(&guide, "x".repeat(9 * 1024)).unwrap();
        let (mut parts, engine) = session::for_tests();
        parts.config.cwd = dir.clone();
        parts.guide_paths = vec![guide.clone()];
        let (send, _reports) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        let heeded = |app: &App| {
            app.transcript.blocks.iter().any(|block| {
                matches!(&block.kind, crate::transcript::Kind::Note { text, tone: Tone::Heed }
                    if text.starts_with("AGENTS.md has 9 KB"))
            })
        };
        assert!(heeded(&app));
        assert_eq!(app.guide_warnings().len(), 1);
        // The /init turn answered: its guide's size is kept, and the warning goes.
        app.init_guide = Some(guide.clone());
        app.on_report(Report::Done(
            RunOutcome {
                reason: FinishReason::Answered,
                answer: None,
                turns: 1,
                usage: Usage::default(),
                refusals: 0,
                warm: None,
            },
            Vec::new(),
        ));
        assert!(app.guide_warnings().is_empty());
        assert_eq!(
            Prefs::load(&app.parts.home)
                .guide_checked
                .get(&guide.display().to_string()),
            Some(&(9 * 1024))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn todo_adds_at_once_and_ctrl_y_shows_the_list_space_ticks_and_esc_gives_the_keys_back() {
        let dir = std::env::temp_dir().join(format!("glut-tui-app-todos-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (mut parts, engine) = session::for_tests();
        parts.config.cwd = dir.clone();
        let (send, _reports) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        let wide = |app: &mut App| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
            terminal
                .draw(|frame| crate::view::draw(frame, app))
                .unwrap();
            format!("{:?}", terminal.backend().buffer())
        };
        typed(&mut app, "/todo the first idea");
        assert_eq!(
            std::fs::read_to_string(dir.join("TODO.md")).unwrap(),
            "- [ ] the first idea\n"
        );
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!((app.panel, app.tab), (Some(true), Tab::Todos));
        let screen = wide(&mut app);
        assert!(
            screen.contains("To-dos 1") && screen.contains("the first idea"),
            "{screen}"
        );
        app.on_terminal(key(KeyCode::Char(' '), KeyModifiers::NONE));
        assert_eq!(
            std::fs::read_to_string(dir.join("TODO.md")).unwrap(),
            "- [x] the first idea\n"
        );
        assert!(wide(&mut app).contains("1 done"));
        // a hands the input `/todo `, and the keys with it.
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(
            (app.input.text(), app.project_todos.keys),
            ("/todo ".to_owned(), false)
        );
        app.input.clear();
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.project_todos.keys);
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(app.input.text(), "x");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn type_text(app: &mut App, text: &str) {
        for c in text.chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
        }
    }

    fn wide_screen(app: &mut App) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
        terminal
            .draw(|frame| crate::view::draw(frame, app))
            .unwrap();
        format!("{:?}", terminal.backend().buffer())
    }

    #[test]
    fn each_change_of_state_is_a_line_for_the_reader_and_the_work_is_timed() {
        let (mut app, _reports) = app();
        let notes = |app: &App| -> Vec<String> {
            app.transcript
                .blocks
                .iter()
                .filter_map(|block| match &block.kind {
                    crate::transcript::Kind::Note { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        };
        app.track_state_saying(true);
        let before = notes(&app).len();
        assert!(
            app.busy_since.is_none(),
            "the first look says nothing and times nothing"
        );
        app.working_for_tests();
        app.track_state_saying(true);
        assert!(app.busy_since.is_some());
        let (reply, _answer) = oneshot::channel();
        app.on_report(Report::Ask(
            AskRequest {
                tool: "bash".into(),
                target: "ls".into(),
                scopes: Vec::new(),
                from: None,
            },
            reply,
        ));
        let since = app.busy_since;
        app.track_state_saying(true);
        app.track_state_saying(true);
        assert_eq!(app.busy_since, since, "a question keeps the time running");
        app.ask = None;
        app.track_state_saying(true);
        app.busy = None;
        app.last_turn = Some(true);
        app.track_state_saying(true);
        assert!(app.busy_since.is_none());
        let said = notes(&app)[before..].to_vec();
        assert_eq!(said.len(), 4, "{said:?}");
        assert!(said[0] == "Glut works" && said[1].starts_with("Glut asks: bash ls"));
        assert!(said[2] == "Glut works on" && said[3] == "Glut is done");
        // The next turn of a queued message, with no rest between, is said and timed anew.
        app.working_for_tests();
        app.track_state_saying(true);
        let first = app.busy_since;
        app.meter.turns.push(Default::default());
        app.track_state_saying(true);
        assert!(app.busy_since >= first);
        assert!(notes(&app).last().unwrap().contains("next message"));
        app.busy = None;
        app.track_state_saying(false);
        let before = notes(&app).len() - 4;
        // Without the reader mode the times run all the same, and nothing is said.
        app.working_for_tests();
        app.track_state_saying(false);
        assert!(app.busy_since.is_some() && notes(&app).len() == before + 4);
    }

    #[test]
    fn the_first_start_welcomes_with_the_first_keys_until_something_is_sent() {
        let (mut app, _reports) = app();
        assert!(
            app.first_start,
            "a home without prefs starts for the first time"
        );
        let screen = wide_screen(&mut app);
        assert!(
            screen.contains("all the keys") && screen.contains("plan first"),
            "{screen}"
        );
        type_text(&mut app, "/motion");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        // Kept: the next start of that home is no first one.
        assert!(!app.first_start && Prefs::kept(&app.parts.home));
        assert!(!wide_screen(&mut app).contains("all the keys"));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    /// The resident memory of this process, in MB, as `ps` says it.
    fn resident_mb() -> u64 {
        let out = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u64>()
            .unwrap_or(0)
            / 1024
    }

    /// TUI9f: a session of 10,000 steps, measured — `cargo test --release -p glut-tui long_sessions -- --ignored
    /// --nocapture`. Each step is a message of the owner's, a tool call with its output (4 kB, every tenth the most a
    /// result keeps in the conversation, 30,000 characters) and an answer.
    #[test]
    #[ignore = "a measurement: run it in release"]
    fn long_sessions_open_draw_scroll_and_search_in_time() {
        const STEPS: usize = 10_000;
        let (mut app, _reports) = app();
        let small = "a line of a tool's output, as a build or a grep prints it\n".repeat(70);
        let large = "x".repeat(glut::tools::MAX_TOOL_OUTPUT_CHARS);
        let mut conversation = Vec::new();
        for step in 0..STEPS {
            conversation.push(glut::Message::user(format!(
                "step {step}: look at file {step}.rs"
            )));
            let mut call = glut::Message::user("");
            call.role = glut::model::Role::Assistant;
            call.content = None;
            call.tool_calls = vec![glut::ToolCall {
                id: format!("c{step}"),
                kind: "function".into(),
                function: glut::model::FunctionCall {
                    name: "read_file".into(),
                    arguments: format!("{{\"path\":\"src/{step}.rs\"}}"),
                },
            }];
            conversation.push(call);
            let output = if step % 10 == 0 { &large } else { &small };
            conversation.push(glut::Message::tool(format!("c{step}"), output.clone()));
            let mut answer = glut::Message::user(format!(
                "Step {step} done: **{step}.rs** reads fine, see [the docs](https://example.com/{step})."
            ));
            answer.role = glut::model::Role::Assistant;
            conversation.push(answer);
        }
        let mut session = crate::store::Session::new(std::path::Path::new("/tmp"), "p", "m");
        session.conversation = conversation;
        let saved = Instant::now();
        let written = session.save(&app.parts.home);
        let bytes = std::fs::metadata(
            crate::store::Session::dir(&app.parts.home).join(format!("{}.json", session.id)),
        )
        .map(|meta| meta.len())
        .unwrap_or(0);
        println!(
            "session file: {} MB, written in {:?} ({written:?})",
            bytes / 1_000_000,
            saved.elapsed()
        );
        let loaded = Instant::now();
        // `/resume`: the file read and the session opened.
        let read = crate::store::Session::load(&app.parts.home, &session.id)
            .expect("a saved session opens again");
        let read_in = loaded.elapsed();
        drop(session);
        let opened = Instant::now();
        app.open(read);
        let resumed = read_in + opened.elapsed();
        println!(
            "resumed ({} blocks): read {read_in:?}, all {resumed:?}",
            app.transcript.blocks.len()
        );
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 50)).unwrap();
        let mut frame = |app: &mut App| {
            let at = Instant::now();
            terminal
                .draw(|frame| crate::view::draw(frame, app))
                .unwrap();
            at.elapsed()
        };
        // The first frame lays every block out once, to count the stream; later ones only what is in view.
        println!("first frame: {:?}", frame(&mut app));
        let steady = (0..20).map(|_| frame(&mut app)).max().unwrap();
        println!("a frame after: {steady:?}");
        let mut most = std::time::Duration::ZERO;
        for _ in 0..200 {
            app.on_terminal(key(KeyCode::PageUp, KeyModifiers::NONE));
            most = most.max(frame(&mut app));
        }
        println!("scrolling 200 pages up, the slowest frame: {most:?}");
        app.on_terminal(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        type_text(&mut app, "step 4242");
        let searched = frame(&mut app);
        let typed = frame(&mut app);
        println!(
            "search: the frame finding it {searched:?}, a frame while it stays open {typed:?}"
        );
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Up, KeyModifiers::NONE));
        println!(
            "reading mode, the frame scrolling to the last block: {:?}",
            frame(&mut app)
        );
        let memory = resident_mb();
        println!("resident memory: {memory} MB");
        let _ = std::fs::remove_dir_all(&app.parts.home);
        let frame_budget = std::time::Duration::from_millis(16);
        assert!(
            resumed < std::time::Duration::from_secs(1),
            "/resume under a second"
        );
        assert!(
            steady < frame_budget && most < frame_budget,
            "a frame under 16 ms, scrolling too"
        );
        assert!(
            searched.max(typed) < std::time::Duration::from_millis(50),
            "the search without a felt pause"
        );
        assert!(memory < 500, "under 500 MB");
    }

    #[test]
    fn switches_gone_round_one_after_another_leave_one_line() {
        let (mut app, _reports) = app();
        let notes = |app: &App| {
            app.transcript
                .blocks
                .iter()
                .filter(|block| matches!(block.kind, crate::transcript::Kind::Note { .. }))
                .count()
        };
        let before = notes(&app);
        for mode in ["plan", "ask", "build", "auto", "plan", "ask"] {
            app.say_switch(Switch::Mode, format!("{mode}: …"), Tone::Quiet);
        }
        assert_eq!(notes(&app), before + 1);
        app.transcript.note("something else", Tone::Quiet);
        app.say_switch(Switch::Mode, "build: …", Tone::Quiet);
        assert_eq!(
            notes(&app),
            before + 3,
            "a mode note after another note stands on its own"
        );
        // ⌃T gone round leaves one line too, and never takes the mode's.
        for _ in 0..4 {
            app.on_terminal(key(KeyCode::Char('t'), KeyModifiers::CONTROL));
        }
        assert_eq!(notes(&app), before + 4);
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn tab_completes_a_commands_word_begun() {
        let (mut app, _reports) = app();
        type_text(&mut app, "/motion c");
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "/motion calm");
    }

    /// A PNG of 2 × 1 pixels, red and blue.
    pub(crate) const TINY_PNG: [u8; 70] = [
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1, 8, 2,
        0, 0, 0, 123, 64, 232, 221, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207, 0, 4, 255,
        1, 7, 0, 1, 255, 226, 35, 158, 89, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
    ];

    #[tokio::test]
    async fn a_dropped_image_file_goes_with_the_message_numbered_on_through_the_session() {
        let (mut app, mut reports) = app();
        let dir = app.parts.home.join("drop");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a shot.png");
        std::fs::write(&file, TINY_PNG).unwrap();
        type_text(&mut app, "what is");
        app.on_terminal(TermEvent::Paste(
            file.display().to_string().replace(' ', "\\ "),
        ));
        // Read beside the screen (T47): a message waits until it is in.
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.working() && app.input.images().is_empty());
        app.on_report(reports.recv().await.unwrap());
        assert_eq!(app.input.text(), "what is [image #1] ");
        assert_eq!(app.input.images().len(), 1);
        // A model its list says sees none: the message waits, said why.
        app.engine.images = Some(false);
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.input.is_empty() && !app.working());
        app.engine.images = None;
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        let shown = app
            .transcript
            .blocks
            .iter()
            .find_map(|block| match &block.kind {
                crate::transcript::Kind::Picture(image) => Some(image.label()),
                _ => None,
            });
        assert_eq!(shown.unwrap(), "[image #1 · 2×1 · 1 kB]");
        // The picture stands under the message and opens whole on ⏎ in the reading mode (T46).
        let picture = app
            .transcript
            .blocks
            .iter()
            .position(|block| matches!(block.kind, crate::transcript::Kind::Picture(_)))
            .unwrap();
        assert_eq!(app.image_of(picture).map(|image| image.number), Some(1));
        // Drawn in half blocks under the message; whole over the stream on a click or ⏎, ⎋ closes it.
        let screen = wide_screen(&mut app);
        assert!(
            screen.contains("▀") && screen.contains("[image #1 · 2×1 · 1 kB]"),
            "{screen}"
        );
        app.picture_view = app.image_of(picture);
        assert!(wide_screen(&mut app).contains("⎋ closes · o opens it"));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picture_view.is_none());
        // The next image numbers on, and a mark deleted takes its image out.
        let mut sent = glut::Message::user("earlier");
        sent.images = vec![glut::images::keep(&app.parts.home, &TINY_PNG, 1).unwrap()];
        app.conversation = vec![sent];
        app.input.clear();
        app.on_terminal(TermEvent::Paste(
            file.display().to_string().replace(' ', "\\ "),
        ));
        app.on_report(reports.recv().await.unwrap());
        assert_eq!(app.input.text(), "[image #2] ");
        app.on_terminal(key(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(app.input.images().is_empty());
        // A paste naming anything else is text.
        app.on_terminal(TermEvent::Paste(format!("see {}", file.display())));
        assert!(app.input.text().starts_with("see "));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn the_search_finds_what_an_answer_streams_onto_the_line_it_stands_on() {
        let (mut app, _reports) = app();
        let delta = |text: &str| {
            Report::Event(Event::TextDelta {
                turn: 1,
                text: text.into(),
            })
        };
        app.on_report(delta("alpha"));
        wide_screen(&mut app);
        app.on_terminal(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        type_text(&mut app, "omega");
        wide_screen(&mut app);
        assert!(app.search.as_ref().unwrap().matches.is_empty());
        // Same blocks, same lines: only the line grew (T47).
        app.on_report(delta(" omega"));
        wide_screen(&mut app);
        assert_eq!(app.search.as_ref().unwrap().matches.len(), 1);
        // Closed, the search gives its texts up.
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        wide_screen(&mut app);
        assert!(!app.search_texts);
    }

    /// A session of `cwd` kept below `home`, saved `minutes_ago`, as `Session::save` writes it.
    fn kept_session(home: &std::path::Path, cwd: &std::path::Path, id: &str, minutes_ago: i64) {
        let mut session = Session::new(cwd, "p", "m");
        session.id = id.into();
        session.updated = chrono::Utc::now() - chrono::Duration::minutes(minutes_ago);
        let dir = home.join(".glut/sessions");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{id}.json")),
            serde_json::to_string(&session).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn projects_lists_the_folders_worked_in_pins_one_and_switches_to_the_chosen() {
        let (mut app, _reports) = app();
        let home = app.parts.home.clone();
        let (older, newer) = (home.join("older"), home.join("newer"));
        std::fs::create_dir_all(&older).unwrap();
        std::fs::create_dir_all(&newer).unwrap();
        kept_session(&home, &older, "s1", 60);
        kept_session(&home, &newer, "s2", 5);
        // The welcome of a folder without sessions shows them, newest first, and says how to pick one.
        let screen = wide_screen(&mut app);
        assert!(
            screen.find("~/newer").unwrap() < screen.find("~/older").unwrap()
                && screen.contains("/projects picks one"),
            "{screen}"
        );
        type_text(&mut app, "/projects");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        let picker = app.picker.as_ref().unwrap();
        let labels: Vec<&str> = picker
            .shown()
            .iter()
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["newer", "older", "another folder…"]);
        // Letters are keys here: `p` pins the chosen one, the list shown anew with it on top and still chosen.
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('p'), KeyModifiers::NONE));
        let picker = app.picker.as_ref().unwrap();
        assert!(picker.query.is_empty());
        assert_eq!(picker.shown()[0].label, "★ older");
        assert_eq!(picker.chosen(), Some(Pick::Project(older.clone())));
        // The open project is not hidden from itself.
        app.project = older.clone();
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(crate::projects::Picks::load(&home).hidden.is_empty());
        app.project = app.parts.config.cwd.clone();
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.pane_request,
            Some(crate::panes::Request::Switch(older.clone()))
        );
        // By its path, `~` the owner's home; ⇥ completes a folder's name.
        app.pane_request = None;
        type_text(&mut app, "/projects ~/ne");
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "/projects ~/newer/");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        // A typed path is the folder itself, links resolved — as a session's folder is.
        assert_eq!(
            app.pane_request,
            Some(crate::panes::Request::Switch(newer.canonicalize().unwrap()))
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_host_that_picks_the_folder_offers_no_projects() {
        let (mut parts, engine) = session::for_tests();
        parts.projects = false;
        let (send, _receive) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        type_text(&mut app, "/proj");
        assert!(
            app.suggestions()
                .iter()
                .all(|command| command.name != "projects")
        );
        app.input.clear();
        type_text(&mut app, "/projects ~");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.pane_request.is_none() && app.picker.is_none());
    }

    #[test]
    fn resume_offers_the_panes_kept_at_the_last_end_first() {
        let (app, _reports) = app();
        let kept = |session: &str| crate::layout::Kept {
            session: session.into(),
            role: None,
            agent: None,
            branch: None,
            into: None,
        };
        let layout = crate::layout::Layout {
            panes: vec![kept("s1"), kept("s2")],
            share: crate::panes::SHARE,
        };
        crate::layout::save(&app.parts.home, &app.parts.config.cwd, &layout).unwrap();
        let picker = app.session_picker();
        let first = &picker.shown()[0];
        assert_eq!(first.label, "as it stood: 2 panes");
        assert_eq!(first.pick, Pick::Pane(crate::panes::Request::Restore));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn ctrl_a_is_the_lines_start_as_in_the_shell() {
        let (mut app, _reports) = app();
        type_text(&mut app, "abc");
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::CONTROL));
        type_text(&mut app, "x");
        assert_eq!(app.input.text(), "xabc");
        assert_ne!(app.panel, Some(true), "no tab opens");
    }

    #[test]
    fn the_reading_mode_walks_the_blocks_folds_one_and_types_nothing() {
        let (mut app, _reports) = app();
        app.transcript
            .push(crate::transcript::Kind::Owner("look at a.rs".into()));
        app.transcript.push(crate::transcript::Kind::Tool {
            id: "t1".into(),
            name: "read_file".into(),
            target: "a.rs".into(),
            state: crate::transcript::ToolState::Done,
            gate: None,
            output: "fn main() {}\n".into(),
            unfolded: false,
            depth: 0,
        });
        app.transcript.push(crate::transcript::Kind::Answer {
            text: "a.rs is empty".into(),
            streaming: false,
        });
        wide_screen(&mut app);
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.reading.map(|reading| reading.block), Some(2));
        let screen = wide_screen(&mut app);
        assert!(
            screen.contains("▐") && screen.contains("c copies"),
            "{screen}"
        );
        // The chosen block's rows are underlaid, so it is seen where its text is.
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
        terminal
            .draw(|frame| crate::view::draw(frame, &mut app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let answer_row = (0..30)
            .find(|y| {
                (0..140)
                    .map(|x| buffer[(x, *y)].symbol())
                    .collect::<String>()
                    .contains("a.rs is empty")
            })
            .unwrap();
        assert_ne!(buffer[(0, answer_row)].bg, ratatui::style::Color::Reset);
        app.on_terminal(key(KeyCode::Up, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(
            app.transcript.blocks[1].kind,
            crate::transcript::Kind::Tool { unfolded: true, .. }
        ));
        type_text(&mut app, "kq");
        assert_eq!(
            app.reading.map(|reading| reading.block),
            Some(0),
            "k went up, q did nothing"
        );
        assert!(app.input.is_empty());
        app.on_terminal(key(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(app.reading.map(|reading| reading.block), Some(2));
        app.on_terminal(key(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(app.reading.is_none() && app.search.is_some());
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Up, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.reading.is_none());
        type_text(&mut app, "ok");
        assert_eq!(app.input.text(), "ok");
        // A tool's whole output is what c copies.
        assert_eq!(app.transcript.blocks[1].kind.copied(), "fn main() {}\n");
    }

    #[test]
    fn reading_steps_over_what_draws_nothing_and_a_question_takes_the_keys_from_it() {
        let (mut app, _reports) = app();
        app.transcript
            .push(crate::transcript::Kind::Owner("first".into()));
        app.transcript.push(crate::transcript::Kind::Thinking {
            text: "hidden".into(),
            streaming: false,
            flipped: false,
        });
        app.transcript.thinking = Thinking::Off;
        wide_screen(&mut app);
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(
            app.reading.map(|reading| reading.block),
            Some(0),
            "the thought draws nothing"
        );
        // A question arriving meanwhile is answered only by its own keys, after its moment to be read.
        let (reply, mut answer) = oneshot::channel();
        app.on_report(Report::Ask(
            AskRequest {
                tool: "bash".into(),
                target: "ls".into(),
                scopes: Vec::new(),
                from: None,
            },
            reply,
        ));
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(answer.try_recv().is_err(), "too soon to be read");
        if let Some(open) = &mut app.ask {
            open.opened = Instant::now() - ASK_SETTLE * 2;
        }
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(answer.try_recv().is_ok());
        assert!(app.reading.is_some(), "the reading mode waits behind it");
        // A paste types nothing while reading.
        app.on_terminal(TermEvent::Paste("pasted".into()));
        assert!(app.input.is_empty());
    }

    #[test]
    fn a_to_do_added_from_the_panels_focus_is_typed_into_the_input() {
        let (mut app, _reports) = app();
        wide_screen(&mut app);
        app.tab = Tab::Todos;
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        wide_screen(&mut app);
        assert!(app.panel_focus && app.project_todos.keys);
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(
            !app.panel_focus,
            "the panel gives the keys away with the list"
        );
        type_text(&mut app, "next");
        assert_eq!(app.input.text(), "/todo next");
        // ⌃F under the panel's focus is typed into.
        app.input.clear();
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('f'), KeyModifiers::CONTROL));
        type_text(&mut app, "ab");
        assert_eq!(
            app.search.as_ref().map(|search| search.query.as_str()),
            Some("ab")
        );
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn the_panel_takes_the_keys_goes_round_its_tabs_and_gives_them_back() {
        let (mut app, _reports) = app();
        wide_screen(&mut app);
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        wide_screen(&mut app);
        assert!(app.panel_focus && app.panel == Some(true));
        let first = app.tab;
        app.on_terminal(key(KeyCode::Right, KeyModifiers::NONE));
        assert_ne!(app.tab, first);
        app.on_terminal(key(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(app.tab, first);
        type_text(&mut app, "q");
        assert!(app.input.is_empty(), "the panel's keys type nothing");
        assert!(wide_screen(&mut app).contains("←→ tabs"));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.panel_focus && app.panel == Some(true));
        type_text(&mut app, "q");
        assert_eq!(app.input.text(), "q");
        // ⌃X ⇥ again gives the keys back as well.
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(app.panel_focus);
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert!(!app.panel_focus);
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn ctrl_x_a_shows_the_agents_tab_its_list_takes_the_keys_and_enter_opens_one() {
        let (mut app, _reports) = app();
        let wide = |app: &mut App| {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
            terminal
                .draw(|frame| crate::view::draw(frame, app))
                .unwrap();
            format!("{:?}", terminal.backend().buffer())
        };
        // Before any subagent the tab opens with its hint and leaves the keys to the input.
        agents_key(&mut app);
        assert_eq!(
            (app.panel, app.tab, app.agents_list.clone()),
            (Some(true), Tab::Agents, None)
        );
        assert!(wide(&mut app).contains("No subagent yet"));
        // An empty list never takes the keys: ⌃X A hides at once, and shows again.
        agents_key(&mut app);
        assert_eq!(app.panel, Some(false));
        agents_key(&mut app);
        assert_eq!(app.panel, Some(true));
        for event in subagent_events(true) {
            app.on_report(Report::Event(event));
        }
        // Shown already, ⌃X A hands the list the keys; the nested one stands under its parent.
        agents_key(&mut app);
        // A note may stand before it in the stream: its path is its block's.
        let tile = app.transcript.agent_blocks()[0];
        assert_eq!(app.agents_list, Some(vec![tile]));
        let screen = wide(&mut app);
        assert!(
            screen.contains("find the callers") && screen.contains("deeper"),
            "{screen}"
        );
        assert!(screen.contains("⏎ opens"));
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            app.agents_chosen(),
            Some(1),
            "the list ends at its last row"
        );
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.agent_view.as_ref().unwrap().path.len(), 2);
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.agent_view.is_none());
        // A new one running goes on top; the choice stays on its subagent, not on its row.
        app.on_terminal(key(KeyCode::Up, KeyModifiers::NONE));
        for event in task("t9", "newer", false) {
            app.on_report(Report::Event(event));
        }
        assert_eq!(app.agents_chosen(), Some(1));
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.agent_view.as_ref().unwrap().path, [tile]);
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        // Esc gives the keys back; an Alt key is the input's too.
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.agents_list, None);
        agents_key(&mut app);
        app.on_terminal(key(KeyCode::Char('j'), KeyModifiers::ALT));
        assert_eq!(app.agents_list, None, "Alt+j is the input's");
        // A key of the input's gives it the keys back and is typed.
        agents_key(&mut app);
        assert!(app.agents_list.is_some());
        app.on_terminal(key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(
            (app.agents_list.clone(), app.input.text()),
            (None, "jq".to_owned())
        );
        // ⌃X A takes the keys again, and once more hides the panel.
        agents_key(&mut app);
        wide(&mut app);
        agents_key(&mut app);
        assert_eq!((app.panel, app.agents_list), (Some(false), None));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn x_in_the_agents_tab_stops_only_a_running_subagent_of_the_turn() {
        let (mut app, _reports) = app();
        app.running = Some(Cancel::new());
        for event in [task("t1", "ended", true), task("t2", "running", false)].concat() {
            app.on_report(Report::Event(event));
        }
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(140, 30)).unwrap();
        agents_key(&mut app);
        terminal
            .draw(|frame| crate::view::draw(frame, &mut app))
            .unwrap();
        let notes = |app: &App| format!("{:?}", app.transcript.blocks.last().map(|b| &b.kind));
        // The running one is listed first; this cancel lists no subagent of that call, so it cannot stop it alone.
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(notes(&app).contains("⎋ stops the turn"), "{}", notes(&app));
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::NONE));
        assert!(notes(&app).contains("ended already"), "{}", notes(&app));
        assert!(
            !app.running.as_ref().unwrap().is_cancelled(),
            "the turn goes on"
        );
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn the_turns_second_subagent_opens_the_agents_tab_unless_the_panel_was_closed() {
        let (mut app, _reports) = app();
        for event in task("t1", "one", true) {
            app.on_report(Report::Event(event));
        }
        assert_eq!(app.panel, None, "one subagent is its tile in the stream");
        for event in task("t2", "two", false) {
            app.on_report(Report::Event(event));
        }
        assert_eq!(
            (app.panel, app.tab, app.agents_list.clone()),
            (Some(true), Tab::Agents, None)
        );
        // Closed by the owner, it stays closed.
        let (mut app, _reports) = self::app();
        app.panel = Some(false);
        for event in [task("t1", "one", true), task("t2", "two", false)].concat() {
            app.on_report(Report::Event(event));
        }
        assert_eq!(app.panel, Some(false));
        let _ = std::fs::remove_dir_all(&app.parts.home);
    }

    #[test]
    fn a_click_on_a_subagents_tile_opens_its_stream_and_a_number_its_own_subagent() {
        let (mut app, _reports) = app();
        for event in subagent_events(true) {
            app.on_report(Report::Event(event));
        }
        assert_eq!(
            app.transcript.blocks.len(),
            1,
            "the subagent's steps are its own, not the stream's"
        );
        drawn(&mut app);
        let (row, _, _) = *app.rows.last().unwrap();
        app.on_terminal(mouse(MouseEventKind::Down(MouseButton::Left), 4, row));
        app.on_terminal(mouse(MouseEventKind::Up(MouseButton::Left), 4, row));
        assert_eq!(app.agent_view.as_ref().unwrap().path, [0]);
        let screen = format!("{:?}", drawn(&mut app).backend().buffer());
        assert!(screen.contains("Subagent · find the callers · done"));
        assert!(screen.contains("read_file"));
        app.on_terminal(key(KeyCode::Char('1'), KeyModifiers::NONE));
        assert_eq!(app.agent_view.as_ref().unwrap().path.len(), 2);
        let screen = format!("{:?}", drawn(&mut app).backend().buffer());
        assert!(screen.contains("found it deep"));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.agent_view.as_ref().unwrap().path, [0]);
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.agent_view.is_none());
        // `/agents` lists both, the nested one under its parent.
        typed(&mut app, "/agents");
        let shown: Vec<String> = app
            .picker
            .as_ref()
            .unwrap()
            .shown()
            .iter()
            .map(|item| item.label.clone())
            .collect();
        assert_eq!(shown, ["⬡ find the callers", "  ⬡ deeper"]);
    }

    /// The plan's ask as the harness sends it, after the call that carried the whole plan.
    fn plan_asked(app: &mut App) -> oneshot::Receiver<Answer> {
        app.on_report(Report::Event(glut::Event::ToolCall {
            turn: 1,
            id: "p1".into(),
            name: glut::planning::EXIT_PLAN.into(),
            arguments: r#"{"plan":"1. change a.rs\n2. test it"}"#.into(),
        }));
        let (reply, answer) = oneshot::channel();
        app.on_report(Report::Ask(
            AskRequest {
                tool: glut::planning::EXIT_PLAN.into(),
                target: "leave plan mode and carry out: 1. change a.rs …".into(),
                scopes: vec!["accept_edits".into(), "auto".into(), "default".into()],
                from: None,
            },
            reply,
        ));
        if let Some(open) = &mut app.ask {
            open.opened = Instant::now() - ASK_SETTLE * 2;
        }
        answer
    }

    #[test]
    fn while_split_the_plan_can_be_built_in_the_other_pane_and_this_one_stays_in_plan_mode() {
        let (mut app, _reports) = app();
        let mut answer = plan_asked(&mut app);
        app.on_terminal(key(KeyCode::Char('b'), KeyModifiers::NONE));
        assert!(answer.try_recv().is_err(), "alone, b is no choice");
        app.others = vec![2];
        app.on_terminal(key(KeyCode::Char('b'), KeyModifiers::NONE));
        assert_eq!(answer.try_recv().unwrap(), Answer::Deny);
        assert_eq!(
            app.pane_request,
            Some(crate::panes::Request::SendPlan {
                to: Some("2".to_owned())
            })
        );
        let plan = app.plan_to_build().unwrap();
        assert!(
            plan.starts_with("Build this plan:") && plan.contains("2. test it"),
            "{plan}"
        );
    }

    #[test]
    fn the_plans_dialog_shows_the_whole_plan_and_approves_into_the_mode_picked() {
        let (mut app, _reports) = app();
        let mut answer = plan_asked(&mut app);
        assert!(app.plan_view.as_ref().is_some_and(|view| view.asking));
        assert_eq!(app.plan_modes(), ["build", "auto", "ask"]);
        let screen = format!("{:?}", drawn(&mut app).backend().buffer());
        assert!(
            screen.contains("2. test it"),
            "the whole plan, not the ask's line"
        );
        assert!(!screen.contains("Glut asks"), "no card below the dialog");
        app.on_terminal(key(KeyCode::Char('2'), KeyModifiers::NONE));
        assert_eq!(answer.try_recv().unwrap(), Answer::AllowWithin(1));
        assert!(app.ask.is_none() && app.plan_view.is_none());
        assert_eq!(
            app.plan.as_ref().unwrap().state,
            crate::plan::State::Approved("auto".into())
        );
        // `/plan` reads it again; it answers nothing.
        typed(&mut app, "/plan");
        assert!(app.plan_view.as_ref().is_some_and(|view| !view.asking));
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.plan_view.is_none());
    }

    #[test]
    fn ctrl_c_on_the_plan_refuses_it_and_the_next_question_is_a_card_again() {
        let (mut app, _reports) = app();
        let mut answer = plan_asked(&mut app);
        app.on_terminal(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(answer.try_recv().unwrap(), Answer::Deny);
        assert!(app.plan_view.is_none());
        assert_eq!(
            app.plan.as_ref().unwrap().state,
            crate::plan::State::SentBack(None)
        );
        // The next ordinary question is answered on its own card, never through a plan dialog.
        let mut next = asked(&mut app);
        assert!(!app.plan_asks());
        app.on_terminal(key(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(next.try_recv().unwrap(), Answer::Deny);
    }

    #[test]
    fn the_plans_ask_closes_what_was_open_and_takes_no_ctrl_or_alt_key() {
        let (mut app, _reports) = app();
        app.picker = Some(Picker::new("Theme", Vec::new(), None));
        app.review.open = true;
        app.keys_sheet = Some(0);
        let mut answer = plan_asked(&mut app);
        assert!(
            app.picker.is_none() && !app.review.open && app.keys_sheet.is_none(),
            "no key meant for them answers the plan"
        );
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Char('2'), KeyModifiers::ALT));
        assert!(answer.try_recv().is_err());
        assert!(app.plan_asks());
    }

    #[test]
    fn a_plan_its_event_could_not_carry_whole_is_the_asks_line_said_so() {
        let (mut app, _reports) = app();
        app.on_report(Report::Event(glut::Event::ToolCall {
            turn: 1,
            id: "p1".into(),
            name: glut::planning::EXIT_PLAN.into(),
            arguments: r#"{"plan":"cut of"#.into(),
        }));
        assert!(app.plan.is_none(), "no plan before its ask");
        let (reply, _answer) = oneshot::channel();
        app.on_report(Report::Ask(
            AskRequest {
                tool: glut::planning::EXIT_PLAN.into(),
                target: "leave plan mode and carry out: cut of".into(),
                scopes: vec!["accept_edits".into()],
                from: None,
            },
            reply,
        ));
        let plan = app.plan.as_ref().unwrap();
        assert!(
            plan.text
                .starts_with("leave plan mode and carry out: cut of")
        );
        assert!(plan.text.contains("Only the head of the plan"));
    }

    #[test]
    fn a_plan_sent_back_takes_the_owners_note_to_glut_before_its_next_step() {
        let (mut app, _reports) = app();
        let inbox = glut::Inbox::default();
        app.steering = Some(inbox.clone());
        let mut answer = plan_asked(&mut app);
        app.on_terminal(key(KeyCode::Char('n'), KeyModifiers::NONE));
        assert!(
            app.ask.is_some(),
            "n opens the note, it answers nothing yet"
        );
        for c in "add a test first".chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(answer.try_recv().unwrap(), Answer::Deny);
        assert_eq!(inbox.waiting(), ["About your plan: add a test first"]);
        assert_eq!(
            app.plan.as_ref().unwrap().state,
            crate::plan::State::SentBack(Some("add a test first".into()))
        );
    }

    fn typed(app: &mut App, text: &str) {
        app.input.insert_text(text);
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
    }

    /// Opens a question as the gate would, already on the screen long enough to be answered.
    fn asked(app: &mut App) -> oneshot::Receiver<Answer> {
        let (reply, answer) = oneshot::channel();
        app.on_report(Report::Ask(
            AskRequest {
                tool: "run_command".into(),
                target: "git status --short".into(),
                scopes: vec![
                    "git status --short".into(),
                    "git status *".into(),
                    "git *".into(),
                ],
                from: None,
            },
            reply,
        ));
        if let Some(open) = &mut app.ask {
            open.opened = Instant::now() - ASK_SETTLE * 2;
        }
        answer
    }

    #[test]
    fn a_question_takes_only_a_plain_key_and_not_one_typed_before_it_was_read() {
        let (mut app, _reports) = app();
        let mut answer = asked(&mut app);
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::CONTROL));
        assert!(app.ask.is_some(), "Ctrl+A does not allow for the session");
        app.on_terminal(TermEvent::Key(KeyEvent {
            kind: KeyEventKind::Repeat,
            ..KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE)
        }));
        assert!(app.ask.is_some(), "a held key does not answer");
        app.on_terminal(key(KeyCode::Char('4'), KeyModifiers::NONE));
        assert!(app.ask.is_some(), "there is no fourth scope");
        app.on_terminal(key(KeyCode::Char('2'), KeyModifiers::NONE));
        assert!(app.ask.is_none());
        assert_eq!(answer.try_recv().unwrap(), Answer::AllowWithin(1));
        let mut answer = asked(&mut app);
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(answer.try_recv().unwrap(), Answer::AllowAlways);

        let mut fresh = asked(&mut app);
        if let Some(open) = &mut app.ask {
            open.opened = Instant::now();
        }
        app.on_terminal(key(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(
            app.ask.is_some(),
            "a key in the first moment is typed ahead, not an answer"
        );
        assert!(fresh.try_recv().is_err());
    }

    #[test]
    fn glimm_rests_works_waits_and_is_pleased_or_gone_out_after_a_turn() {
        use crate::glimm::Mood;
        let (mut app, _reports) = app();
        assert_eq!(app.mood(), Mood::Resting);
        let _answer = asked(&mut app);
        assert_eq!(app.mood(), Mood::Waiting);
        let ended = |reason| {
            Report::Done(
                RunOutcome {
                    reason,
                    answer: None,
                    turns: 1,
                    usage: Usage::default(),
                    refusals: 0,
                    warm: None,
                },
                Vec::new(),
            )
        };
        app.on_report(ended(FinishReason::Answered));
        assert_eq!(app.mood(), Mood::Done);
        app.on_report(ended(FinishReason::Failed));
        assert_eq!(app.mood(), Mood::Failed);
        app.on_report(ended(FinishReason::Cancelled));
        assert_eq!(
            app.mood(),
            Mood::Resting,
            "a turn the owner stopped is no failure"
        );
    }

    #[test]
    fn a_turn_that_ends_takes_its_open_question_away() {
        let (mut app, _reports) = app();
        let _answer = asked(&mut app);
        app.on_report(Report::Done(
            RunOutcome {
                reason: FinishReason::Cancelled,
                answer: None,
                turns: 1,
                usage: Usage::default(),
                refusals: 0,
                warm: None,
            },
            Vec::new(),
        ));
        assert!(app.ask.is_none());
    }

    #[test]
    fn ctrl_c_clears_then_quits_on_a_second_press_and_another_key_disarms_it() {
        let (mut app, _reports) = app();
        let ctrl_c = || key(KeyCode::Char('c'), KeyModifiers::CONTROL);
        app.input.insert_text("half a thought");
        app.on_terminal(ctrl_c());
        assert!(
            app.input.is_empty() && !app.quit,
            "the first press clears the input"
        );
        app.on_terminal(ctrl_c());
        assert!(!app.quit, "an empty input arms the quit");
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        app.on_terminal(ctrl_c());
        assert!(!app.quit, "a key in between disarmed it");
        app.on_terminal(ctrl_c());
        assert!(app.quit);
    }

    #[test]
    fn commands_are_the_sessions_own_and_never_reach_the_model() {
        let (mut app, _reports) = app();
        typed(&mut app, "/help");
        assert!(!app.working(), "a command starts no turn");
        typed(&mut app, "/nonesuch");
        let notes: Vec<String> = app
            .transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Note { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert!(notes[0].starts_with("Commands\n"));
        assert!(notes[1].contains("no command /nonesuch"));
        typed(&mut app, "/effort");
        assert_eq!(
            app.picker.as_ref().map(|picker| picker.title.as_str()),
            Some("Effort")
        );
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.picker.is_none());
        typed(&mut app, "/quit");
        assert!(app.quit);
    }

    #[test]
    fn a_slash_lists_the_commands_and_tab_or_enter_takes_the_chosen_one() {
        let (mut app, _reports) = app();
        app.on_terminal(key(KeyCode::Char('/'), KeyModifiers::NONE));
        assert_eq!(
            app.suggestions().len(),
            crate::commands::COMMANDS.len(),
            "a lone slash shows them all"
        );
        app.on_terminal(key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(
            app.suggestions()
                .iter()
                .map(|command| command.name)
                .collect::<Vec<_>>(),
            ["clear", "compact", "context", "close", "cache"]
        );
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "/compact ");
        assert!(app.suggestions().is_empty(), "a space ends the list");
        app.input.clear();
        app.input.insert_text("/hel");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            app.transcript.blocks.iter().any(|block| matches!(
                &block.kind,
                crate::transcript::Kind::Note { text, .. } if text.starts_with("Commands\n")
            )),
            "Enter on a part of a name ran /help"
        );
        app.input.clear();
        app.input.insert_text("/");
        app.suggested = 0;
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "/clear ", "a lone slash runs nothing");
        app.input.clear();
        app.input.insert_text("/session");
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.input.text(),
            "/clear ",
            "nor does a word of what a command does"
        );
    }

    #[tokio::test]
    async fn a_command_that_must_wait_stays_in_the_input_and_a_path_is_a_message() {
        let (mut app, _reports) = app();
        app.busy = Some(Busy::Turn);
        typed(&mut app, "/model other");
        assert_eq!(app.input.text(), "/model other", "kept, not thrown away");
        assert!(app.picker.is_none());
        app.input.clear();
        typed(&mut app, "/context");
        assert!(app.input.is_empty(), "showing the window does not wait");
        app.busy = None;
        typed(&mut app, "/usr/bin is missing");
        assert!(
            app.working(),
            "a path at the start is a message: a turn started"
        );
    }

    #[test]
    fn a_failed_compaction_gives_the_conversation_back_and_a_good_one_replaces_it() {
        let (mut app, _reports) = app();
        let before = vec![Message::user("one"), Message::user("two")];
        app.busy = Some(Busy::Compacting);
        app.on_report(Report::Compacted(Err("no".into()), before.clone()));
        assert_eq!(app.conversation, before);
        assert!(!app.working());
        app.on_report(Report::Compacted(
            Ok(glut::compact::Compacted {
                conversation: vec![Message::user("summary")],
                usage: Usage::default(),
                replaced: 2,
                older: before.clone(),
            }),
            before,
        ));
        assert_eq!(app.conversation, vec![Message::user("summary")]);
    }

    #[tokio::test]
    async fn a_handoff_is_read_into_the_conversation_and_a_missing_one_is_said() {
        let (mut app, _reports) = app();
        typed(&mut app, "/handoff read");
        assert!(app.conversation.is_empty());
        let path = session::handoff_path(&app.parts.home, &app.parts.config.cwd);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "## Done\n- the parser\n## Next\n- tests").unwrap();
        typed(&mut app, "/handoff read");
        assert_eq!(app.conversation.len(), 1);
        let said = app.conversation[0].content.clone().unwrap_or_default();
        assert!(
            said.contains("the parser") && said.contains("no instruction of the owner"),
            "{said}"
        );
        assert!(!path.exists(), "a handoff read is moved away");
        let moved: Vec<_> = std::fs::read_dir(path.parent().unwrap().join("read"))
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(moved.len(), 1, "into handoffs/read/");
        typed(&mut app, "/handoff read");
        assert_eq!(app.conversation.len(), 1, "read once, not twice");
        typed(&mut app, "/handoff");
        assert!(
            !app.working(),
            "a bare /handoff writes nothing: it says how"
        );
        typed(&mut app, "/handoff write");
        assert!(app.working(), "writing runs in the background");
    }

    #[test]
    fn a_plan_opens_the_panel_once_and_ctrl_x_e_hands_the_input_to_the_editor() {
        let (mut app, _reports) = app();
        let todo = |text: &str| glut::planning::TodoItem {
            text: text.into(),
            status: glut::planning::TodoStatus::Pending,
        };
        app.on_report(Report::Event(Event::Todo {
            turn: 1,
            items: vec![todo("read"), todo("fix")],
        }));
        assert_eq!(app.todos.len(), 2);
        assert_eq!(app.panel, Some(true), "the first plan opens the panel");
        app.on_terminal(key(KeyCode::Char('b'), KeyModifiers::CONTROL));
        assert_eq!(app.panel, Some(false));
        app.input.insert_text("a long thought");
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Char('e'), KeyModifiers::NONE));
        assert_eq!(app.edit_request.as_deref(), Some("a long thought"));
        app.edited(Ok("an edited thought\n".into()));
        assert_eq!(app.input.text(), "an edited thought");
        app.on_terminal(key(KeyCode::Char('x'), KeyModifiers::CONTROL));
        app.on_terminal(key(KeyCode::Char('q'), KeyModifiers::NONE));
        assert_eq!(
            app.input.text(),
            "an edited thought",
            "another key ends the chord, typing nothing"
        );
    }

    #[test]
    fn a_token_count_reads_as_typed() {
        assert_eq!(parse_tokens("65536"), Some(65_536));
        assert_eq!(parse_tokens("64k"), Some(65_536));
        assert_eq!(parse_tokens("1M"), Some(1_048_576));
        assert_eq!(parse_tokens("1.5k"), Some(1_536));
        assert_eq!(parse_tokens("lots"), None);
        assert_eq!(parse_tokens("0"), None);
        for bad in ["inf", "nan", "-1", "1e3", "64 k", "4000m", "1.2.3"] {
            assert_eq!(parse_tokens(bad), None, "{bad}");
        }
    }

    /// The stream's notes, one after the other.
    fn text_of(app: &App) -> String {
        app.transcript
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                crate::transcript::Kind::Note { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A host that refuses every turn and keeps what the screen hands it.
    #[derive(Default)]
    struct Keeping {
        kept: std::sync::Mutex<Vec<usize>>,
    }

    impl crate::host::Host for Keeping {
        fn before_turn<'a>(
            &'a self,
            _config: &'a mut glut::RunConfig,
            _turn: crate::host::Turn,
        ) -> crate::host::HostFuture<'a, Result<(), String>> {
            Box::pin(async { Err("the session is at its limit".to_owned()) })
        }

        fn kept(&self, conversation: &[Message]) {
            self.kept.lock().unwrap().push(conversation.len());
        }
    }

    /// A screen whose session a [`Keeping`] host runs, with the model fixed and nothing kept under `~/.glut`.
    fn hosted() -> (App, mpsc::UnboundedReceiver<Report>, Arc<Keeping>) {
        let (mut parts, engine) = session::for_tests();
        let host = Arc::new(Keeping::default());
        parts.host = host.clone();
        parts.fixed_engine = true;
        parts.keep_sessions = false;
        let (send, receive) = mpsc::unbounded_channel();
        (App::new(Arc::new(parts), engine, send), receive, host)
    }

    #[test]
    fn a_hosted_session_offers_no_switch_of_model_and_keeps_its_conversation_with_the_host() {
        let (mut app, _reports, host) = hosted();
        typed(&mut app, "/model other/model");
        assert!(!app.working(), "no switch started");
        assert!(
            text_of(&app).contains("no switch here"),
            "{}",
            text_of(&app)
        );
        app.go_on_from(vec![Message::user("Fix the parser")]);
        assert_eq!(app.session.title, "Fix the parser");
        app.keep();
        assert_eq!(*host.kept.lock().unwrap(), [1]);
        assert!(
            !app.parts.home.join(".glut").join("sessions").exists(),
            "a hosted session is not kept under ~/.glut"
        );
    }

    #[tokio::test]
    async fn a_path_typed_lists_its_folder_tab_completes_enter_takes_and_a_file_ends_the_list() {
        let root = std::env::temp_dir().join(format!("glut-app-paths-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("crates/glut")).unwrap();
        std::fs::create_dir_all(root.join("crates/glut-tui")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "").unwrap();
        let mut app = app_in(&root, &[], &root);
        let typing = |app: &mut App, text: &str| {
            for c in text.chars() {
                app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
            }
        };
        typing(&mut app, "look at ./cr");
        let listing = app.paths.clone().expect("a list");
        assert_eq!(listing.entries.len(), 1);
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.input.text(),
            "look at ./crates/",
            "the only one, with its slash"
        );
        assert_eq!(
            app.paths.as_ref().map(|listing| listing.entries.len()),
            Some(2),
            "the list goes on in it"
        );
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(
            app.input.text(),
            "look at ./crates/glut",
            "as far as both fit"
        );
        app.on_terminal(key(KeyCode::Down, KeyModifiers::NONE));
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.input.text(),
            "look at ./crates/glut-tui/",
            "Enter takes the chosen one, not sends"
        );
        assert!(app.paths.is_none(), "an empty folder lists nothing");
        // A file taken ends the list; Enter without a pick sends what is typed.
        app.input.clear();
        typing(&mut app, "see ./Car");
        app.on_terminal(key(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(app.input.text(), "see ./Cargo.toml");
        assert!(app.paths.is_none());
        app.input.clear();
        typing(&mut app, "see ./crates/glu");
        assert!(app.paths.is_some());
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.input.is_empty(), "sent, no pick taken");
        if let Some(cancel) = &app.running {
            cancel.cancel();
        }
        // Esc closes the list for what stands; typing on opens it again.
        app.input.clear();
        typing(&mut app, "/projects ./c");
        assert!(app.paths.is_some(), "a command that takes a path");
        app.on_terminal(key(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.paths.is_none());
        assert_eq!(app.input.text(), "/projects ./c", "Esc kept the input");
        typing(&mut app, "r");
        assert!(app.paths.is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An app working in `cwd` that read `guides`, keeping its prefs in `home`.
    fn app_in(cwd: &std::path::Path, guides: &[&str], home: &std::path::Path) -> App {
        let (mut parts, engine) = session::for_tests();
        parts.config.cwd = cwd.to_path_buf();
        parts.guide_paths = guides.iter().map(|name| cwd.join(name)).collect();
        parts.home = home.to_path_buf();
        let (send, _receive) = mpsc::unbounded_channel();
        App::new(Arc::new(parts), engine, send)
    }

    /// The owner's last message in the stream; the turn it started is called off.
    fn sent(app: &App) -> String {
        if let Some(cancel) = &app.running {
            cancel.cancel();
        }
        app.transcript
            .blocks
            .iter()
            .rev()
            .find_map(|block| match &block.kind {
                crate::transcript::Kind::Owner(text) => Some(text.clone()),
                _ => None,
            })
            .unwrap_or_default()
    }

    #[tokio::test]
    async fn init_writes_checks_or_asks_once_about_the_guide_the_session_read() {
        let base = std::env::temp_dir().join(format!("glut-init-{}", std::process::id()));
        let (dir, home) = (base.join("repo"), base.join("home"));
        std::fs::create_dir_all(&dir).unwrap();
        // No guide: a new one.
        let mut app = app_in(&dir, &[], &home);
        app.init("");
        assert!(sent(&app).contains("write a short AGENTS.md"));
        // An AGENTS.md: checked.
        let mut app = app_in(&dir, &["AGENTS.md"], &home);
        app.init("");
        assert!(sent(&app).contains("check the guide AGENTS.md"));
        // A bad argument sends nothing.
        let mut app = app_in(&dir, &[], &home);
        app.init("--wat");
        assert!(sent(&app).is_empty());
        // A CLAUDE.md: asked; yes moves it.
        let mut app = app_in(&dir, &["CLAUDE.md"], &home);
        app.init("");
        assert!(app.picker.is_some(), "asked about CLAUDE.md");
        app.picker = None;
        app.init_with(true);
        assert!(sent(&app).contains("move CLAUDE.md into an AGENTS.md"));
        // No keeps it, checks it, and is not asked again — until --migrate.
        let mut app = app_in(&dir, &["CLAUDE.md"], &home);
        app.init("");
        app.picker = None;
        app.init_with(false);
        assert!(sent(&app).contains("check the guide CLAUDE.md"));
        let mut app = app_in(&dir, &["CLAUDE.md"], &home);
        app.init("");
        assert!(app.picker.is_none(), "not asked again");
        assert!(sent(&app).contains("check the guide CLAUDE.md"));
        let mut app = app_in(&dir, &["CLAUDE.md"], &home);
        app.init("--migrate");
        assert!(app.picker.is_some(), "asked again");
        assert!(
            Prefs::load(&home).keep_claude.is_empty(),
            "the no is taken back"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[tokio::test]
    async fn the_hosts_refusal_before_a_turn_sends_nothing_and_says_why() {
        let (mut app, mut reports, _host) = hosted();
        app.send("go on".into());
        while let Some(report) = reports.recv().await {
            let refused = matches!(report, Report::Refused { .. });
            app.on_report(report);
            if refused {
                break;
            }
        }
        assert!(text_of(&app).contains("at its limit"), "{}", text_of(&app));
        assert_eq!(
            app.input.text(),
            "go on",
            "the message is back in the input"
        );
        assert!(app.session.title.is_empty());
        assert!(!app.working());
        assert!(app.conversation.is_empty(), "the message never joined");
        assert!(app.session.marks.is_empty(), "nothing to rewind to");
    }

    #[test]
    fn the_hosts_stop_ends_the_turn_and_its_theme_gives_way_to_the_owners_own_for_the_session() {
        let (mut app, _reports, _host) = hosted();
        let cancel = Cancel::new();
        app.running = Some(cancel.clone());
        app.on_report(Report::Host(Signal::Stop(
            "the session's money is spent".into(),
        )));
        assert!(cancel.is_cancelled());
        assert!(text_of(&app).contains("money is spent"));
        // Glut's own palette, so no other test sees other colours.
        let own = crate::theme::all().next().unwrap().name;
        app.on_report(Report::Host(Signal::Theme("no-such-theme".into())));
        assert_eq!(
            app.theme_from,
            ThemeFrom::Owner,
            "an unknown theme changes nothing"
        );
        app.on_report(Report::Host(Signal::Theme(own.into())));
        assert_eq!(app.theme_from, ThemeFrom::Host);
        typed(&mut app, &format!("/theme {own}"));
        assert_eq!(app.theme_from, ThemeFrom::Pinned);
        assert!(
            Prefs::load(&app.parts.home).theme.is_none(),
            "not kept as the owner's own"
        );
        app.on_report(Report::Host(Signal::Theme(own.into())));
        assert_eq!(
            app.theme_from,
            ThemeFrom::Pinned,
            "the host no longer reaches it"
        );
    }

    #[test]
    fn a_question_answered_elsewhere_closes_on_the_screen_and_one_of_a_turn_called_off_says_nothing()
     {
        let (mut app, _reports) = app();
        let cancel = Cancel::new();
        app.running = Some(cancel.clone());
        let answer = asked(&mut app);
        app.drop_answered_ask();
        assert!(app.ask.is_some(), "still awaited");
        drop(answer);
        app.drop_answered_ask();
        assert!(app.ask.is_none());
        assert_eq!(app.toast(), Some("answered elsewhere"));
        app.toast = None;
        let answer = asked(&mut app);
        cancel.cancel();
        drop(answer);
        app.drop_answered_ask();
        assert!(app.ask.is_none());
        assert_eq!(
            app.toast(),
            None,
            "a turn called off is no answer from elsewhere"
        );
    }

    #[tokio::test]
    async fn a_cut_off_turn_the_host_refuses_leaves_its_first_message_queued() {
        let (mut app, mut reports, _host) = hosted();
        app.resume("turn-1".into(), Some("start the card".into()));
        assert!(app.working());
        while let Some(report) = reports.recv().await {
            let refused = matches!(report, Report::Refused { .. });
            app.on_report(report);
            if refused {
                break;
            }
        }
        assert!(!app.working());
        assert!(
            app.input.is_empty(),
            "no message of the owner's to give back"
        );
        assert_eq!(app.queue, ["start the card"]);
    }

    #[tokio::test]
    async fn the_first_message_follows_a_resumed_turn_unless_it_had_it() {
        let (mut app, _reports) = app();
        app.busy = Some(Busy::Turn);
        app.after_resume = Some("start the card".into());
        let outcome = RunOutcome {
            reason: FinishReason::Cancelled,
            answer: None,
            turns: 1,
            usage: Usage::default(),
            refusals: 0,
            warm: None,
        };
        app.on_report(Report::Done(
            outcome.clone(),
            vec![Message::user("start the card")],
        ));
        assert!(app.queue.is_empty(), "the lost turn had it");
        app.busy = Some(Busy::Turn);
        app.after_resume = Some("start the card".into());
        app.on_report(Report::Done(outcome, vec![Message::user("something else")]));
        assert!(
            app.working(),
            "sent at once, however the cut-off turn ended"
        );
        assert!(app.queue.is_empty());
    }

    #[test]
    fn a_host_can_lock_the_mode_keep_the_shell_off_and_refuse_paid_calls_outside_a_turn() {
        let (mut parts, engine) = session::for_tests();
        /// A host whose session is at its limit.
        struct Spent;
        impl crate::host::Host for Spent {
            fn may_spend(&self) -> Result<(), String> {
                Err("the session's money is spent".into())
            }
        }
        parts.host = Arc::new(Spent);
        parts.mode_locked = true;
        parts.owner_shell = false;
        let (send, _receive) = mpsc::unbounded_channel();
        let mut app = App::new(Arc::new(parts), engine, send);
        app.cycle_mode();
        typed(&mut app, "!ls");
        typed(&mut app, "/compact");
        typed(&mut app, "/handoff write");
        assert!(!app.working(), "nothing started");
        let text = text_of(&app);
        assert!(text.contains("mode is set by the program"), "{text}");
        assert!(text.contains("no !commands"), "{text}");
        assert_eq!(text.matches("money is spent").count(), 2, "{text}");
    }
}
