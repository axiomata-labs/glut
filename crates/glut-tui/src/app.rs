//! The session on the screen: what is shown, and what a key, a click, a command or a step of the run changes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use glut::permissions::{Answer, AskRequest, Mode};
use glut::providers::{Providers, models};
use glut::{Cancel, Effort, Event, FinishReason, Message, Usage};
use tokio::sync::{mpsc, oneshot};

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

/// What `/help` says.
const HELP: &str = "Commands: /model [id] · /provider [id] · /effort [level] · /context [tokens] · /compact · \
                    /editor [command] · /thinking off|folded|open · /theme [name] · /vim on|off · /scrollback · \
                    /clear · /help · /quit\n\
                    Keys: Enter send · Shift+Enter or Alt+Enter new line · ↑ history · Shift+Tab plan → ask → \
                    build → auto · Esc stop · Ctrl+O tool results · Ctrl+T thinking · Ctrl+B panel · \
                    Ctrl+X E editor · PgUp/PgDn scroll · Ctrl+F search · drag to copy · F2 mouse to the terminal \
                    and back · F3 scrollback · Ctrl+C twice quit\n\
                    Also: Ctrl+K every key · @ mentions a file · !cmd runs a command and shows Glut \
                    its output · !!cmd runs it for you alone · a large paste stands as [pasted N lines]\n\
                    While Glut works: Enter steers it (taken in before its next step) · Tab queues for after the \
                    turn · Alt+↑ takes the last back\n\
                    /vim on: Esc to Normal mode (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y with a motion \
                    or doubled · counts · p P · u Ctrl+R), i a I A o O to Insert; Esc in Normal mode stops Glut";

/// The session's commands with what each does, as the list under a typed `/` shows them.
pub const COMMANDS: [(&str, &str); 17] = [
    (
        "model",
        "pick a model of the provider in use, or /model <id>",
    ),
    ("provider", "switch the provider, or /provider <id>"),
    ("effort", "how hard the model thinks, or /effort <level>"),
    ("context", "how full the window is; /context 64k sets it"),
    ("compact", "summarise the conversation so far"),
    (
        "handoff",
        "/handoff write: what was done and what comes next; /handoff read in a new session",
    ),
    ("editor", "pick your editor, or /editor <command>"),
    (
        "thinking",
        "the model's thinking: /thinking off, folded or open",
    ),
    (
        "panel",
        "show or hide the side panel with the plan (Ctrl+B)",
    ),
    ("resume", "open an earlier session of this folder"),
    (
        "theme",
        "Glut's colours, or an Axiomata theme's: /theme <name>",
    ),
    ("vim", "edit the input the vi way: /vim on or off"),
    (
        "scrollback",
        "the conversation into the terminal's own scrollback (F3)",
    ),
    ("clear", "start a new session"),
    ("help", "commands and keys"),
    ("quit", "end the session"),
    ("exit", "end the session"),
];

/// A question of the gate on the screen.
pub struct OpenAsk {
    pub request: AskRequest,
    reply: oneshot::Sender<Answer>,
    /// When the card came up: keys before [`ASK_SETTLE`] has passed do not answer it.
    opened: Instant,
}

/// What runs in the background besides the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Turn,
    Compacting,
    Switching,
    HandingOff,
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
    /// The conversation between turns; taken while a turn runs.
    conversation: Vec<Message>,
    /// The session as it is kept (T25).
    pub session: Session,
    /// The run's task list as `todo_write` last sent it (the Plan tab, T28).
    pub todos: Vec<glut::planning::TodoItem>,
    /// How the last turn ended, for Glimm's mood: answered, or ended by an error or a limit; `None` before the first
    /// and after one the owner stopped.
    pub last_turn: Option<bool>,
    /// Whether the side panel is shown; `None` until the owner or a first plan decides.
    pub panel: Option<bool>,
    /// Ctrl+X was pressed: the next key finishes a chord (Ctrl+X E: the editor).
    chord: bool,
    /// The input, handed to the editor by the main loop (T29), which owns the terminal.
    pub edit_request: Option<String>,
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
    pub frame: usize,
    quit_armed: Option<Instant>,
    pub quit: bool,
    reports: mpsc::UnboundedSender<Report>,
}

impl App {
    pub fn new(parts: Arc<Parts>, engine: Engine, reports: mpsc::UnboundedSender<Report>) -> Self {
        let folder = session::shown_folder(&parts.config.cwd);
        let session = Session::new(&parts.config.cwd, &engine.provider, engine.model_name());
        let prefs = Prefs::load(&parts.home);
        let panel = prefs.panel;
        let vi = prefs.vi.unwrap_or(false);
        // The theme kept from an earlier session; an unknown name (a theme dropped since) keeps Glut's own.
        if let Some(name) = &prefs.theme {
            crate::theme::set(name);
        }
        let mut transcript = Transcript {
            thinking: Prefs::load(&parts.home).thinking.unwrap_or_default(),
            ..Transcript::default()
        };
        for note in &parts.notes {
            transcript.note(note.clone(), Tone::Quiet);
        }
        App {
            parts,
            engine,
            folder,
            transcript,
            input: Input::new(),
            picker: None,
            suggested: 0,
            conversation: Vec::new(),
            session,
            todos: Vec::new(),
            last_turn: None,
            panel,
            chord: false,
            edit_request: None,
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
            vi: vi.then(crate::vi::Vi::default),
            steering: None,
            queue: Vec::new(),
            send_when_free: false,
            pending: Vec::new(),
            shells: 0,
            files: None,
            stream_width: u16::MAX,
            frame: 0,
            quit_armed: None,
            quit: false,
            reports,
        }
    }

    /// Keeps the session as it stands now; a failure is said, the session goes on.
    fn keep(&mut self) {
        self.session.conversation.clone_from(&self.conversation);
        self.session.provider.clone_from(&self.engine.provider);
        self.engine.model_name().clone_into(&mut self.session.model);
        self.session.cost = self.cost;
        if let Err(reason) = self.session.save(&self.parts.home) {
            self.transcript.note(
                format!("the session could not be kept: {reason}"),
                Tone::Fault,
            );
        }
    }

    /// Opens session `session`: its conversation goes on, its stream is shown again.
    pub fn open(&mut self, session: Session) {
        // What waited for the other session is not this one's.
        self.queue.clear();
        self.pending.clear();
        self.transcript = Transcript {
            thinking: self.transcript.thinking,
            ..Transcript::default()
        };
        session.transcript(&mut self.transcript);
        let title = if session.title.is_empty() {
            "it".to_owned()
        } else {
            crate::transcript::clean(&session.title)
        };
        self.transcript.note(
            format!(
                "session of {} opened — {title} goes on from here",
                session
                    .updated
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M"),
            ),
            Tone::Fine,
        );
        self.conversation.clone_from(&session.conversation);
        self.todos.clear();
        self.cost = session.cost;
        self.usage = Usage::default();
        self.context_used = 0;
        self.scroll = 0;
        self.session = session;
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
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Says where the mouse went.
    pub fn mouse_said(&mut self, held: bool) {
        self.say(if held {
            "the mouse is Glut's again: drag to copy, click to unfold"
        } else {
            "the mouse is the terminal's: its own selection works — F2 takes it back"
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

    /// Stops the turn under way, if there is one (the session is ending).
    pub fn stop_turn(&mut self) {
        if let Some(cancel) = &self.running {
            cancel.cancel();
        }
    }

    /// Sends `prompt` as the next turn.
    pub fn send(&mut self, prompt: String) {
        if prompt.trim().is_empty() || self.working() {
            return;
        }
        // A picker left open (glut -r with a first message) is no place to choose from while the turn runs.
        self.picker = None;
        self.transcript
            .push(crate::transcript::Kind::Owner(prompt.clone()));
        // The session is named by the owner's first own message, not by a handoff read in front of it.
        if self.session.title.is_empty() {
            self.session.title = crate::store::title_of(&prompt);
        }
        self.scroll = 0;
        let cancel = Cancel::new();
        self.running = Some(cancel.clone());
        self.busy = Some(Busy::Turn);
        let inbox = glut::Inbox::default();
        self.steering = Some(inbox.clone());
        session::turn(
            Arc::clone(&self.parts),
            self.engine.clone(),
            std::mem::take(&mut self.conversation),
            prompt,
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
                }
                if let Event::Todo { items, .. } = &event {
                    self.todos.clone_from(items);
                    // The first plan opens the panel, unless the owner closed it once.
                    if self.panel.is_none() {
                        self.panel = Some(true);
                    }
                }
                self.transcript.take(&event);
            }
            Report::Ask(request, reply) => {
                // A chord begun before the question came does not take the first key after it.
                self.chord = false;
                self.ask = Some(OpenAsk {
                    request,
                    reply,
                    opened: Instant::now(),
                });
                self.scroll = 0;
            }
            Report::Done(outcome, conversation) => {
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
                self.transcript.settle();
                self.last_turn = match outcome.reason {
                    FinishReason::Answered => Some(true),
                    FinishReason::Cancelled => None,
                    _ => Some(false),
                };
                match outcome.reason {
                    FinishReason::Answered => {}
                    FinishReason::Cancelled => self.transcript.note("stopped", Tone::Quiet),
                    other => self
                        .transcript
                        .note(format!("the turn ended: {}", reason(other)), Tone::Fault),
                }
                // The next queued message follows a turn that answered; after a stop, a failure or a limit it waits
                // for Enter, or it would run into the same wall.
                if outcome.reason == FinishReason::Answered && !self.queue.is_empty() {
                    let next = self.queue.remove(0);
                    self.send(next);
                }
            }
            Report::Compacted(result, before) => {
                self.busy = None;
                match result {
                    Ok(compacted) => {
                        if let Some(prices) = self.engine.prices {
                            *self.cost.get_or_insert(0.0) += prices.cost_usd(&compacted.usage);
                        }
                        self.usage += compacted.usage;
                        self.conversation = compacted.conversation;
                        self.context_used = 0;
                        self.keep();
                        self.transcript.note(
                            format!(
                                "compacted: {} messages became a summary",
                                compacted.replaced
                            ),
                            Tone::Fine,
                        );
                    }
                    Err(reason) => {
                        self.conversation = before;
                        self.transcript
                            .note(format!("not compacted: {reason}"), Tone::Fault);
                    }
                }
            }
            Report::HandedOff(result, conversation) => {
                self.busy = None;
                self.conversation = conversation;
                match result {
                    Ok((path, usage)) => {
                        if let Some(prices) = self.engine.prices {
                            *self.cost.get_or_insert(0.0) += prices.cost_usd(&usage);
                        }
                        self.usage += usage;
                        self.transcript.note(
                            format!(
                                "handoff written to {} — /handoff read in the next session",
                                session::shown_folder(&path)
                            ),
                            Tone::Fine,
                        );
                    }
                    Err(reason) => self
                        .transcript
                        .note(format!("no handoff: {reason}"), Tone::Fault),
                }
            }
            Report::ChooseModel(provider) => {
                self.busy = None;
                self.transcript.note(
                    format!("{provider} has no model yet: pick the one it takes"),
                    Tone::Quiet,
                );
                self.picker = Some(self.model_picker(&provider));
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
                    self.say("Glut reads this output with your next message");
                }
            }
            Report::Files(found) => {
                let (files, truncated) = found;
                self.files = Some((Instant::now(), files, truncated));
                // A file picker still waiting for the walk gets its files, with what was typed in it kept.
                if let Some(picker) = &self.picker
                    && picker.title.starts_with(FILE_PICKER)
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
                            self.transcript.note(
                                format!(
                                    "no prices for {}: the cost shown counts no further",
                                    engine.model_name()
                                ),
                                Tone::Quiet,
                            );
                        }
                        self.engine = engine;
                        // The next start begins where the owner switched to.
                        if let Err(reason) = session::keep_choice(&self.parts.home, &self.engine) {
                            self.transcript.note(
                                format!("not kept for the next start: {reason}"),
                                Tone::Quiet,
                            );
                        }
                        // The new model has read nothing of this session yet: the window fills on its next call.
                        self.context_used = 0;
                        for note in notes {
                            self.transcript.note(note, Tone::Quiet);
                        }
                        self.transcript.note(
                            format!(
                                "now {} · {}",
                                self.engine.provider,
                                self.engine.model_name()
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
            TermEvent::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            TermEvent::Paste(text) if self.ask.is_none() && self.picker.is_none() => {
                match &mut self.search {
                    // A paste while searching is what to search for.
                    Some(search) => {
                        search.query.push_str(&text.replace(['\n', '\r'], " "));
                        search.fresh = true;
                    }
                    None => self.input.paste(&text),
                }
            }
            TermEvent::Mouse(mouse) => match mouse.kind {
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
        self.quit_armed = None;
        if self.ask.is_some() {
            return self.on_ask_key(key);
        }
        if self.picker.is_some() {
            return self.on_picker_key(key);
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
            self.picker = Some(palette());
            return;
        }
        // `@` at a word's start opens the folder's files to mention; Esc keeps the `@` as typed.
        if key.code == KeyCode::Char('@') && self.input.at_word_start() {
            self.chord = false;
            return self.mention();
        }
        let suggestions = self.suggestions();
        if !suggestions.is_empty() {
            let chosen = suggestions[self.suggested.min(suggestions.len() - 1)].0;
            match key.code {
                KeyCode::Up => return self.suggested = self.suggested.saturating_sub(1),
                KeyCode::Down => {
                    return self.suggested = (self.suggested + 1).min(suggestions.len() - 1);
                }
                KeyCode::Tab => {
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
        if self.picker.is_some() {
            self.chord = false;
        }
        if std::mem::take(&mut self.chord) {
            if matches!(key.code, KeyCode::Char('e' | 'E')) {
                self.edit_request = Some(self.input.expanded());
            }
            return;
        }
        match key.code {
            KeyCode::Char('x') if ctrl => self.chord = true,
            KeyCode::Char('b') if ctrl => self.toggle_panel(),
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
            KeyCode::Char('a') if ctrl => self.input.home(),
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
            KeyCode::Tab if self.working() => self.queue_next(),
            KeyCode::Tab => self.input.insert(' '),
            KeyCode::Char(c) if !ctrl => {
                self.input.insert(c);
                self.suggested = 0;
            }
            _ => {}
        }
    }

    /// The commands that fit what is typed: the input is a `/` and part of a name, nothing after it yet.
    pub fn suggestions(&self) -> Vec<(&'static str, &'static str)> {
        let text = self.input.text();
        let Some(typed) = text.strip_prefix('/') else {
            return Vec::new();
        };
        if typed.contains([' ', '\n', '/']) {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(typed))
            .copied()
            .collect()
    }

    /// Enter: a command when the input is one, else the next message. A command that has to wait while Glut works
    /// stays in the input, like a message; a first word with a slash in it (`/usr/bin is missing`) is a message.
    fn submit(&mut self) {
        let text = self.input.text();
        if self.input.broken_pastes() > 0 {
            self.transcript.note(
                "a paste lost its mark to an edit and is left out; Ctrl+U clears the line, paste it again",
                Tone::Fault,
            );
            return;
        }
        if let Some(command) = text.trim().strip_prefix('/') {
            let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
            if !name.is_empty() && !name.contains('/') {
                if self.working() && waits(name, rest.trim()) {
                    self.transcript.note(
                        format!("/{name} waits until Glut is done — it stays in the input; Esc stops Glut"),
                        Tone::Quiet,
                    );
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
                self.transcript.note(
                    "!cmd hands its output to Glut: it waits until Glut is done (!!cmd runs now, for you alone)",
                    Tone::Quiet,
                );
                return;
            }
            // The command as sent: a paste in it given back.
            let expanded = self.input.take();
            let command = owners_command(&expanded).map_or(command, |(_, command)| command);
            return self.shell(command, !alone);
        }
        if self.working() {
            // While a turn runs, Enter steers it: the message joins before its next step. While Glut compacts or
            // switches, it waits as the next message.
            if self.input.is_empty() {
                return;
            }
            let text = self.input.take();
            match &self.steering {
                Some(inbox) => {
                    inbox.send(text);
                    self.say("Glut takes this in before its next step — Alt+↑ takes it back");
                }
                None => {
                    self.queue.push(text);
                    self.send_when_free = true;
                    self.say("queued: it goes when Glut is free — Alt+↑ takes it back");
                }
            }
            return;
        }
        // Enter on an empty input sends what waits in the queue (after a turn the owner stopped).
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
        let text = self.input.take();
        self.queue.push(text);
        self.say("queued for after this turn — Alt+↑ takes it back");
    }

    /// Alt+↑ (or Ctrl+↑, which macOS keeps for Mission Control): the last message waiting — queued, or sent to steer
    /// and not yet taken — back into the input.
    fn take_back(&mut self) {
        if !self.input.is_empty() {
            return self.say("the input holds a message: send or clear it first");
        }
        let back = self
            .queue
            .pop()
            .or_else(|| self.steering.as_ref().and_then(glut::Inbox::take_back));
        match back {
            Some(text) => self.input.insert_text(&text),
            None => self.say("nothing waits to be taken back"),
        }
    }

    /// Runs the owner's own command in the folder, beside the session; its output comes back as a report.
    fn shell(&mut self, command: String, to_model: bool) {
        self.shells += 1;
        let id = format!("shell-{}", self.shells);
        self.transcript.push(crate::transcript::Kind::Tool {
            id: id.clone(),
            name: if to_model { "!" } else { "!!" }.into(),
            target: command.clone(),
            state: crate::transcript::ToolState::Running,
            gate: Some(
                if to_model {
                    "yours, for Glut"
                } else {
                    "yours alone"
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
            return Picker::new(format!("{FILE_PICKER} (looking …)"), Vec::new(), None);
        };
        let items = files
            .iter()
            .filter(|path| crate::transcript::clean(path) == **path)
            .map(|path| Item::plain(path.clone(), "", Pick::File(path.clone())))
            .collect();
        let title = if *truncated {
            format!("{FILE_PICKER} (more than 50,000 files: not all of them)")
        } else {
            FILE_PICKER.to_owned()
        };
        Picker::new(title, items, None)
    }

    /// Shift+Tab: plan → ask → build → auto → plan, from the next call on.
    fn cycle_mode(&mut self) {
        let Some(gate) = self.parts.tools.gate() else {
            return;
        };
        let next = session::next_mode(gate.mode());
        gate.set_mode(next);
        let mut what = match next {
            Mode::Plan => "plan: Glut reads and plans, it changes nothing".to_owned(),
            Mode::Default => {
                "ask: Glut reads freely, it asks before it changes a file or runs a command".to_owned()
            }
            Mode::AcceptEdits => {
                "build: Glut reads and edits freely, it asks before a command that does more than read".to_owned()
            }
            // Auto leans on the sandbox; where there is none, the owner hears it before Glut does anything.
            Mode::Auto if session::sandboxed(&self.parts) => {
                "auto: Glut does what no rule forbids, in the sandbox — asked only before a heavy command (rm -rf, \
                 git push, sudo, …)"
                    .to_owned()
            }
            Mode::Auto => "auto: Glut does what no rule forbids — and commands run WITHOUT a sandbox here; only \
                           the rules and the question before a heavy command hold them"
                .to_owned(),
            Mode::DontAsk => "don't ask: what no rule allows is refused".to_owned(),
        };
        if self.running.is_some() {
            what.push_str(" (the tools on offer change with the next message)");
        }
        let tone = if next == Mode::Auto && !session::sandboxed(&self.parts) {
            Tone::Fault
        } else {
            Tone::Quiet
        };
        self.transcript.note(what, tone);
    }

    /// One of the session's own commands (`/model`, `/context` …).
    fn command(&mut self, line: &str) {
        let (name, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        match name {
            "help" | "?" => self.transcript.note(HELP, Tone::Quiet),
            "quit" | "exit" | "q" => self.quit = true,
            "clear" => {
                self.selection = None;
                self.search = None;
                self.pending.clear();
                self.queue.clear();
                self.conversation.clear();
                self.todos.clear();
                self.last_turn = None;
                // A new session counts from nothing.
                self.cost = None;
                self.usage = Usage::default();
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
                self.picker = Some(self.model_picker(&provider));
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
            "compact" => {
                self.busy = Some(Busy::Compacting);
                self.transcript
                    .note("compacting the conversation…", Tone::Quiet);
                session::compact(
                    self.engine.clone(),
                    std::mem::take(&mut self.conversation),
                    self.reports.clone(),
                );
            }
            "editor" if rest.is_empty() => {
                self.picker = Some(editor_picker(Prefs::load(&self.parts.home).editor()))
            }
            "handoff" => match rest {
                // A paid call: written only when asked by name.
                "" => self.transcript.note(
                    "/handoff write — what was done and what comes next, for the next session; /handoff read — \
                     take the folder's handoff into this one",
                    Tone::Quiet,
                ),
                "write" => {
                    self.busy = Some(Busy::HandingOff);
                    self.transcript.note("writing the handoff…", Tone::Quiet);
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
                                "handoff read — Glut knows it from the next message on; it moved to {} so no later \
                                 session reads it again:\n{}",
                                session::shown_folder(&moved),
                                text.trim()
                            ),
                            Tone::Fine,
                        );
                    }
                    Err(reason) => self.transcript.note(reason, Tone::Fault),
                },
                other => self
                    .transcript
                    .note(format!("/handoff {other}: write or read"), Tone::Fault),
            },
            "editor" => self.set_editor(rest),
            "resume" => self.picker = Some(self.session_picker()),
            "panel" => self.toggle_panel(),
            "scrollback" => self.scrollback_request = true,
            "vim" => match rest {
                "on" | "" => self.set_vi(true),
                "off" => self.set_vi(false),
                other => self.transcript.note(format!("/vim {other}: on or off"), Tone::Fault),
            },
            "theme" if rest.is_empty() => self.picker = Some(theme_picker()),
            "theme" => self.set_theme(rest),
            "thinking" => match rest {
                "off" => self.set_thinking(Thinking::Off),
                "folded" | "fold" | "" => self.set_thinking(Thinking::Folded),
                "open" | "on" => self.set_thinking(Thinking::Open),
                other => self.transcript.note(
                    format!("/thinking {other}: off, folded or open"),
                    Tone::Fault,
                ),
            },
            other => self.transcript.note(
                format!("no command /{other} — /help lists them"),
                Tone::Fault,
            ),
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
                Some((window, from)) => format!(
                    "context: {} of {window} tokens used ({}%), the window from {from}; Glut compacts shortly \
                     before it is full — /context <tokens> sets another",
                    self.context_used,
                    self.context_used * 100 / u64::from(window.max(1))
                ),
                None => format!(
                    "context: {} tokens used; the window of {} is unknown — /context <tokens> sets it",
                    self.context_used,
                    self.engine.model_name()
                ),
            };
            self.transcript.note(text, Tone::Quiet);
            return;
        }
        let Some(tokens) = parse_tokens(rest) else {
            self.transcript.note(
                format!("/context {rest}: a number of tokens (65536, 64k, 1M)"),
                Tone::Fault,
            );
            return;
        };
        match session::keep_window(&self.parts.home, &self.engine, tokens) {
            Ok(()) => {
                self.engine.window = Some((tokens, "set in Glut"));
                self.transcript.note(
                    format!(
                        "{} counts with {tokens} tokens from now on",
                        self.engine.model_name()
                    ),
                    Tone::Fine,
                );
            }
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Shows or hides the side panel, and keeps the choice.
    pub fn toggle_panel(&mut self) {
        let shown = !self.panel.unwrap_or(false);
        self.panel = Some(shown);
        if shown && self.stream_width < crate::view::PANEL_MIN_SCREEN {
            self.transcript.note(
                "the panel shows when the window is at least 100 columns wide",
                Tone::Quiet,
            );
        }
        if let Err(reason) = prefs::set_panel(&self.parts.home, shown) {
            self.transcript.note(reason, Tone::Fault);
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
            Thinking::Off => "thinking: hidden",
            Thinking::Folded => "thinking: folded — a click opens one, Ctrl+T all",
            Thinking::Open => "thinking: open — a click folds one, Ctrl+T folds all",
        };
        match prefs::set_thinking(&self.parts.home, thinking) {
            Ok(()) => self.transcript.note(said, Tone::Quiet),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Turns the vi mode on (in Insert mode, as a message starts) or off, and keeps it.
    fn set_vi(&mut self, on: bool) {
        self.vi = on.then(crate::vi::Vi::default);
        let said = if on {
            "vim: on — Esc to Normal mode, i a o to Insert; Esc in Normal mode stops Glut"
        } else {
            "vim: off"
        };
        match prefs::set_vi(&self.parts.home, on) {
            Ok(()) => self.transcript.note(said, Tone::Quiet),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Uses and keeps the theme `name`; every block takes its colours at the next frame.
    fn set_theme(&mut self, name: &str) {
        if !crate::theme::set(name) {
            let names: Vec<&str> = crate::theme::all().map(|palette| palette.name).collect();
            return self.transcript.note(
                format!("no theme {name} — {}", names.join(", ")),
                Tone::Fault,
            );
        }
        self.transcript.touch_all();
        match prefs::set_theme(&self.parts.home, name) {
            Ok(()) => self.say(format!("theme: {name}")),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    fn set_editor(&mut self, command: &str) {
        match prefs::set_editor(&self.parts.home, command) {
            Ok(command) => self
                .transcript
                .note(format!("editor: {command}"), Tone::Fine),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// The models of provider `only` from the list Glut keeps, the current one selected.
    fn model_picker(&self, only: &str) -> Picker {
        let mut items = Vec::new();
        let mut missing = Vec::new();
        if let Ok(list) = Providers::load(&self.parts.home) {
            for (id, entry) in &list.providers {
                if id != only {
                    continue;
                }
                match models::cached(&self.parts.home, id, entry.root().trim_end_matches('/')) {
                    Some(list) => items.extend(list.models.iter().map(|model| {
                        Item {
                            label: crate::transcript::clean(&model.id).to_string(),
                            detail: model_detail(model),
                            pick: Pick::Model {
                                provider: id.clone(),
                                model: model.id.clone(),
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
        let mut title = format!("Model of {}", crate::transcript::clean(only));
        if !missing.is_empty() {
            title.push_str(&format!(
                " (no list yet: {} — axh models refresh)",
                missing.join(", ")
            ));
        }
        let current = Pick::Model {
            provider: self.engine.provider.clone(),
            model: self.engine.model_name().to_owned(),
        };
        Picker::new(title, items, Some(&current)).sorting()
    }

    /// The folder's sessions, newest first.
    pub fn session_picker(&self) -> Picker {
        let items: Vec<Item> = Session::list(&self.parts.home, &self.parts.config.cwd)
            .into_iter()
            .filter(|session| session.id != self.session.id)
            .map(|session| {
                let when = session
                    .updated
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M");
                let cost = session
                    .cost
                    .map(|cost| format!(" · ${cost:.4}"))
                    .unwrap_or_default();
                Item::plain(
                    crate::transcript::clean(&session.title),
                    format!("{when} · {} · {}{cost}", session.provider, session.model),
                    Pick::Session(session.id),
                )
            })
            .collect();
        let title = if items.is_empty() {
            "Sessions (none kept for this folder yet)"
        } else {
            "Sessions of this folder"
        };
        Picker::new(title, items, None)
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
                                entry.model.as_deref().unwrap_or("no default model")
                            ),
                            Pick::Provider(id.clone()),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        Picker::new(
            "Provider",
            items,
            Some(&Pick::Provider(self.engine.provider.clone())),
        )
    }

    fn on_picker_key(&mut self, key: KeyEvent) {
        let Some(picker) = &mut self.picker else {
            return;
        };
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Char('s') if ctrl => picker.next_sort(),
            KeyCode::Char('f') if ctrl => picker.toggle_free(),
            KeyCode::Up => picker.up(),
            KeyCode::Down | KeyCode::Tab => picker.down(),
            KeyCode::Backspace => picker.backspace(),
            KeyCode::Enter => {
                let chosen = picker.chosen();
                self.picker = None;
                match chosen {
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
                    Some(Pick::Session(_)) if self.working() => self.transcript.note(
                        "a session opens when Glut is done — /resume again then",
                        Tone::Quiet,
                    ),
                    Some(Pick::Session(id)) => match Session::load(&self.parts.home, &id) {
                        Ok(session) => self.open(session),
                        Err(reason) => self.transcript.note(reason, Tone::Fault),
                    },
                    Some(Pick::Run(name)) if self.working() && waits(&name, "") => {
                        self.transcript.note(
                            format!("/{name} waits until Glut is done; Esc stops Glut"),
                            Tone::Quiet,
                        )
                    }
                    Some(Pick::Run(name)) => self.command(&name),
                    Some(Pick::Keys(keys)) => {
                        for key in keys {
                            self.on_key(key);
                        }
                    }
                    // The path goes after the `@` typed before the picker opened, in quotes when it has a space.
                    Some(Pick::File(path)) if path.contains(' ') => {
                        self.input.insert_text(&format!("\"{path}\" "))
                    }
                    Some(Pick::File(path)) => self.input.insert_text(&format!("{path} ")),
                    Some(Pick::Mention) => self.mention(),
                    Some(Pick::Theme(name)) => self.set_theme(&name),
                    None => {}
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.type_char(c)
            }
            _ => {}
        }
    }

    /// y allows once; 1, 2, 3 for the rest of the session, as far as the card's scope with that number reaches
    /// (`git status --short`, `git status *`, `git *`), a the first of them; n or Esc refuses — plain keys only,
    /// pressed once, and not in the first moment after the card came up: a key typed (or held) for the input must
    /// not answer a question the owner has not read yet, nor Ctrl+A ("home") allow for the whole session.
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
                self.transcript.note("Ctrl+C again to quit", Tone::Quiet);
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
                let count = text.chars().count();
                match select::copy(&text) {
                    Ok(Copied::Pbcopy) => self.say(format!("copied {count} characters")),
                    Ok(Copied::Terminal) => {
                        self.say(format!("copied {count} characters (through the terminal)"))
                    }
                    Err(reason) => self.say(reason),
                }
            }
            _ => {
                self.selection = None;
                if let Some((_, block, _)) = self
                    .rows
                    .iter()
                    .find(|(at, _, line)| *at == row && *line == press.line)
                {
                    self.transcript.toggle(*block);
                }
            }
        }
    }

    /// The whole stream laid out at `width`, as the terminal's scrollback gets it: folded as the screen shows it.
    pub fn stream_at(&mut self, width: u16) -> Vec<ratatui::text::Line<'static>> {
        let thinking = self.transcript.thinking;
        self.transcript
            .blocks
            .iter_mut()
            .fold(Vec::new(), |mut lines, block| {
                lines.extend_from_slice(block.lines(width, thinking, 0));
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
                block
                    .lines(width, thinking, frame)
                    .iter()
                    .map(select::text_of)
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

/// The keys (Ctrl+K, T12): every action Glut has a key for, the key first, the `/` command beside it where there is
/// one — found by typing, done on Enter. A command without a key is in the `/` list, not here (owner, 2026-10-22).
fn palette() -> Picker {
    let press = |code: KeyCode, modifiers: KeyModifiers| KeyEvent::new(code, modifiers);
    let ctrl = KeyModifiers::CONTROL;
    let none = KeyModifiers::NONE;
    let keys: Vec<(&str, &str, &str, Pick)> = vec![
        (
            "/",
            "the commands",
            "",
            Pick::Keys(vec![press(KeyCode::Char('/'), none)]),
        ),
        ("@", "mention a file", "", Pick::Mention),
        (
            "Ctrl+F",
            "search the stream",
            "",
            Pick::Keys(vec![press(KeyCode::Char('f'), ctrl)]),
        ),
        (
            "Ctrl+O",
            "unfold or fold every tool result",
            "",
            Pick::Keys(vec![press(KeyCode::Char('o'), ctrl)]),
        ),
        (
            "Ctrl+T",
            "open or fold every thought",
            "/thinking",
            Pick::Keys(vec![press(KeyCode::Char('t'), ctrl)]),
        ),
        (
            "Ctrl+B",
            "show or hide the side panel",
            "/panel",
            Pick::Keys(vec![press(KeyCode::Char('b'), ctrl)]),
        ),
        (
            "Shift+Tab",
            "the next mode: plan → ask → build → auto",
            "",
            Pick::Keys(vec![press(KeyCode::BackTab, KeyModifiers::SHIFT)]),
        ),
        (
            "Ctrl+X E",
            "edit the input in your editor",
            "/editor picks it",
            Pick::Keys(vec![
                press(KeyCode::Char('x'), ctrl),
                press(KeyCode::Char('e'), none),
            ]),
        ),
        (
            "Shift+Enter",
            "a new line in the input",
            "",
            Pick::Keys(vec![press(KeyCode::Enter, KeyModifiers::SHIFT)]),
        ),
        (
            "PgUp",
            "scroll the stream up",
            "",
            Pick::Keys(vec![press(KeyCode::PageUp, none)]),
        ),
        (
            "PgDn",
            "scroll the stream down",
            "",
            Pick::Keys(vec![press(KeyCode::PageDown, none)]),
        ),
        (
            "F2",
            "hand the mouse to the terminal and back",
            "",
            Pick::Keys(vec![press(KeyCode::F(2), none)]),
        ),
        (
            "F3",
            "the conversation into the terminal's scrollback",
            "/scrollback",
            Pick::Keys(vec![press(KeyCode::F(3), none)]),
        ),
        (
            "Tab",
            "while Glut works: queue the message for after",
            "",
            Pick::Keys(vec![press(KeyCode::Tab, none)]),
        ),
        (
            "Alt+↑",
            "take the last waiting message back",
            "",
            Pick::Keys(vec![press(KeyCode::Up, KeyModifiers::ALT)]),
        ),
        (
            "Esc",
            "stop Glut",
            "",
            Pick::Keys(vec![press(KeyCode::Esc, none)]),
        ),
        (
            "Ctrl+C twice",
            "quit",
            "/quit",
            Pick::Run("quit".to_owned()),
        ),
    ];
    let items = keys
        .into_iter()
        .map(|(key, what, command, pick)| Item::plain(format!("{key:<13}{what}"), command, pick))
        .collect();
    Picker::new("Keys", items, None)
}

/// The file picker's title, by which a finished walk finds it open.
const FILE_PICKER: &str = "File to mention";
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
                "glut" => "Glut's own",
                "catppuccin-latte" | "github-light" | "paper" => {
                    "Axiomata's theme · for a light terminal"
                }
                _ => "Axiomata's theme",
            };
            Item::plain(palette.name, detail, Pick::Theme(palette.name.to_owned()))
        })
        .collect();
    let current = Pick::Theme(crate::theme::c().name.to_owned());
    Picker::new("Theme", items, Some(&current))
}

/// The effort levels, and "the provider's default".
fn effort_picker(current: Option<Effort>) -> Picker {
    let mut items = vec![Item::plain(
        "default",
        "the provider decides",
        Pick::Effort(None),
    )];
    items.extend(
        Effort::ALL
            .iter()
            .map(|effort| Item::plain(effort.to_string(), "", Pick::Effort(Some(*effort)))),
    );
    Picker::new("Effort", items, Some(&Pick::Effort(current)))
}

/// The editors on the `PATH`, the one in use named in the title.
fn editor_picker(current: Option<String>) -> Picker {
    let items = prefs::installed_editors()
        .into_iter()
        .map(|(program, command)| Item::plain(program, command, Pick::Editor(command.into())))
        .collect();
    let now = current.as_deref().unwrap_or("none yet");
    let title = format!("Editor — now {now} (or /editor <command with {{file}} and {{line}}>)");
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
            "${:.2}/${:.2}",
            prices.input_per_m, prices.output_per_m
        ));
    }
    if model.reasoning == Some(true) {
        parts.push("reasons".into());
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

/// Whether command `name` (with `rest`) waits while Glut works: what changes the model, the conversation or the
/// window the turn counts with.
fn waits(name: &str, rest: &str) -> bool {
    matches!(
        name,
        "model" | "provider" | "effort" | "compact" | "clear" | "handoff" | "resume"
    ) || (name == "context" && !rest.is_empty())
}

/// A finish reason in words.
fn reason(reason: FinishReason) -> &'static str {
    match reason {
        FinishReason::Answered => "answered",
        FinishReason::MaxTurns => "the step limit was reached",
        FinishReason::Timeout => "the time limit passed",
        FinishReason::Cancelled => "stopped",
        FinishReason::Failed => "the model API failed",
        FinishReason::Refused => "too many refused calls",
        FinishReason::Budget => "the budget is spent",
    }
}

#[cfg(test)]
mod tests {
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
    fn the_palette_finds_an_action_by_its_words_and_does_it() {
        let (mut app, _reports) = app();
        app.on_terminal(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
        assert!(app.picker.is_some());
        for c in "scrollback".chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            app.scrollback_request,
            "the action ran, found by its command's name"
        );
        app.scrollback_request = false;
        app.on_terminal(key(KeyCode::Char('k'), KeyModifiers::CONTROL));
        for c in "search the".chars() {
            app.on_terminal(key(KeyCode::Char(c), KeyModifiers::NONE));
        }
        app.on_terminal(key(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            app.search.is_some(),
            "a key's action is pressed for the owner"
        );
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
        assert!(
            app.ask.is_some(),
            "Ctrl+A (home) does not allow for the session"
        );
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
        assert!(notes[0].starts_with("Commands:"));
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
            COMMANDS.len(),
            "a lone slash shows them all"
        );
        app.on_terminal(key(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(
            app.suggestions()
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            ["context", "compact", "clear"]
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
                crate::transcript::Kind::Note { text, .. } if text.starts_with("Commands:")
            )),
            "Enter on a part of a name ran /help"
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
}
