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
use crate::picker::{Item, Pick, Picker};
use crate::prefs::{self, Prefs};
use crate::session::{self, Engine, Parts, Report};
use crate::transcript::{Tone, Transcript};

/// How long a second Ctrl+C has to come to quit.
const QUIT_WINDOW: Duration = Duration::from_millis(1500);
/// How many lines the mouse wheel scrolls.
const WHEEL_LINES: usize = 3;
/// How long a question is on the screen before a key answers it.
const ASK_SETTLE: Duration = Duration::from_millis(400);

/// What `/help` says.
const HELP: &str = "Commands: /model [id] · /provider [id] · /effort [level] · /context [tokens] · /compact · \
                    /editor [command] · /clear · /help · /quit\n\
                    Keys: Enter send · Shift+Enter or Alt+Enter new line · ↑ history · Shift+Tab plan → ask → edits \
                    → auto · Esc stop · Ctrl+O tool results · PgUp/PgDn scroll · Ctrl+C twice quit";

/// The session's commands with what each does, as the list under a typed `/` shows them.
pub const COMMANDS: [(&str, &str); 10] = [
    ("model", "pick the model, or /model <id>"),
    ("provider", "switch the provider, or /provider <id>"),
    ("effort", "how hard the model thinks, or /effort <level>"),
    ("context", "how full the window is; /context 64k sets it"),
    ("compact", "summarise the conversation so far"),
    ("editor", "pick your editor, or /editor <command>"),
    ("clear", "start the conversation afresh"),
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
    /// Which block each row of the stream showed at the last frame (for a click).
    pub rows: Vec<(u16, usize)>,
    pub frame: usize,
    quit_armed: Option<Instant>,
    pub quit: bool,
    reports: mpsc::UnboundedSender<Report>,
}

impl App {
    pub fn new(parts: Arc<Parts>, engine: Engine, reports: mpsc::UnboundedSender<Report>) -> Self {
        let folder = session::shown_folder(&parts.config.cwd);
        let mut transcript = Transcript::default();
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
            frame: 0,
            quit_armed: None,
            quit: false,
            reports,
        }
    }

    /// Whether Glut works on something: a turn, a summary, a switch of model.
    pub fn working(&self) -> bool {
        self.busy.is_some()
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
        self.transcript
            .push(crate::transcript::Kind::Owner(prompt.clone()));
        self.scroll = 0;
        let cancel = Cancel::new();
        self.running = Some(cancel.clone());
        self.busy = Some(Busy::Turn);
        session::turn(
            Arc::clone(&self.parts),
            self.engine.clone(),
            std::mem::take(&mut self.conversation),
            prompt,
            cancel,
            self.reports.clone(),
        );
    }

    /// A step of the run, a question, or the end of something that ran in the background.
    pub fn on_report(&mut self, report: Report) {
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
                self.transcript.take(&event);
            }
            Report::Ask(request, reply) => {
                self.ask = Some(OpenAsk {
                    request,
                    reply,
                    opened: Instant::now(),
                });
                self.scroll = 0;
            }
            Report::Done(outcome, conversation) => {
                self.conversation = conversation;
                self.running = None;
                self.busy = None;
                // A turn that ended (cancelled, timed out) while asking dropped the question: its card goes too.
                self.ask = None;
                self.transcript.settle();
                match outcome.reason {
                    FinishReason::Answered => {}
                    FinishReason::Cancelled => self.transcript.note("stopped", Tone::Quiet),
                    other => self
                        .transcript
                        .note(format!("the turn ended: {}", reason(other)), Tone::Fault),
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
                self.input.insert_text(&text)
            }
            TermEvent::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => self.scroll_by(WHEEL_LINES as isize),
                MouseEventKind::ScrollDown => self.scroll_by(-(WHEEL_LINES as isize)),
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some((_, block)) = self.rows.iter().find(|(row, _)| *row == mouse.row) {
                        self.transcript.toggle(*block);
                    }
                }
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
        match key.code {
            KeyCode::Enter if shift || alt => self.input.newline(),
            KeyCode::Char('j') if ctrl => self.input.newline(),
            KeyCode::Enter => self.submit(),
            KeyCode::BackTab => self.cycle_mode(),
            KeyCode::Esc => {
                if let Some(cancel) = &self.running {
                    cancel.cancel();
                }
            }
            KeyCode::Char('d') if ctrl && self.input.is_empty() && !self.working() => {
                self.quit = true
            }
            KeyCode::Char('o') if ctrl => self.transcript.unfold_all(),
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
            KeyCode::Up if shift => self.scroll_by(1),
            KeyCode::Down if shift => self.scroll_by(-1),
            KeyCode::Up => self.input.up(),
            KeyCode::Down => self.input.down(),
            KeyCode::PageUp => self.scroll_by(self.page()),
            KeyCode::PageDown => self.scroll_by(-self.page()),
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
        if self.working() {
            self.transcript.note(
                "Glut is working — Esc stops it; the message waits in the input",
                Tone::Quiet,
            );
            return;
        }
        let prompt = self.input.take();
        self.send(prompt);
    }

    /// Shift+Tab: plan → ask → edits → auto → plan, from the next call on.
    fn cycle_mode(&mut self) {
        let Some(gate) = self.parts.tools.gate() else {
            return;
        };
        let next = session::next_mode(gate.mode());
        gate.set_mode(next);
        let mut what = match next {
            Mode::Plan => "plan: Glut reads and plans, it changes nothing".to_owned(),
            Mode::Default => "ask: Glut asks before it changes a file or runs a command".to_owned(),
            Mode::AcceptEdits => "edits: Glut edits files freely, it asks before a command".to_owned(),
            // Auto leans on the sandbox; where there is none, the owner hears it before Glut does anything.
            Mode::Auto if session::sandboxed(&self.parts) => {
                "auto: Glut does what no rule forbids, in the sandbox — asked only what no rule can read".to_owned()
            }
            Mode::Auto => "auto: Glut does what no rule forbids — and commands run WITHOUT a sandbox here; only \
                           the rules hold them"
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
                self.conversation.clear();
                self.transcript = Transcript::default();
                self.context_used = 0;
                self.scroll = 0;
            }
            "model" if rest.is_empty() => self.picker = Some(self.model_picker()),
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
            "editor" => self.set_editor(rest),
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

    fn set_editor(&mut self, command: &str) {
        match prefs::set_editor(&self.parts.home, command) {
            Ok(command) => self
                .transcript
                .note(format!("editor: {command}"), Tone::Fine),
            Err(reason) => self.transcript.note(reason, Tone::Fault),
        }
    }

    /// Every model of every provider whose list Glut keeps, the current one selected.
    fn model_picker(&self) -> Picker {
        let mut items = Vec::new();
        let mut missing = Vec::new();
        if let Ok(list) = Providers::load(&self.parts.home) {
            for (id, entry) in &list.providers {
                match models::cached(&self.parts.home, id, entry.root().trim_end_matches('/')) {
                    Some(models) => items.extend(models.models.iter().map(|model| Item {
                        label: format!("{id} · {}", crate::transcript::clean(&model.id)),
                        detail: model_detail(model),
                        pick: Pick::Model {
                            provider: id.clone(),
                            model: model.id.clone(),
                        },
                    })),
                    None => missing.push(id.clone()),
                }
            }
        }
        let mut title = "Model".to_owned();
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
        Picker::new(title, items, Some(&current))
    }

    fn provider_picker(&self) -> Picker {
        let items = Providers::load(&self.parts.home)
            .map(|list| {
                list.providers
                    .iter()
                    .map(|(id, entry)| Item {
                        label: id.clone(),
                        detail: format!(
                            "{} · {}",
                            entry.kind,
                            entry.model.as_deref().unwrap_or("no default model")
                        ),
                        pick: Pick::Provider(id.clone()),
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
        match key.code {
            KeyCode::Esc => self.picker = None,
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
                    None => {}
                }
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                picker.type_char(c)
            }
            _ => {}
        }
    }

    /// y allows once, a for the rest of the session, n or Esc refuses — plain keys only, pressed once, and not in
    /// the first moment after the card came up: a key typed (or held) for the input must not answer a question the
    /// owner has not read yet, nor Ctrl+A ("home") allow for the whole session.
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
            KeyCode::Char('a') => Answer::AllowAlways,
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

    fn scroll_by(&mut self, lines: isize) {
        let most = self.stream_lines.saturating_sub(self.stream_height);
        self.scroll = self.scroll.saturating_add_signed(lines).min(most);
    }
}

/// The effort levels, and "the provider's default".
fn effort_picker(current: Option<Effort>) -> Picker {
    let mut items = vec![Item {
        label: "default".into(),
        detail: "the provider decides".into(),
        pick: Pick::Effort(None),
    }];
    items.extend(Effort::ALL.iter().map(|effort| Item {
        label: effort.to_string(),
        detail: String::new(),
        pick: Pick::Effort(Some(*effort)),
    }));
    Picker::new("Effort", items, Some(&Pick::Effort(current)))
}

/// The editors on the `PATH`, the one in use named in the title.
fn editor_picker(current: Option<String>) -> Picker {
    let items = prefs::installed_editors()
        .into_iter()
        .map(|(program, command)| Item {
            label: program.into(),
            detail: command.into(),
            pick: Pick::Editor(command.into()),
        })
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
    matches!(name, "model" | "provider" | "effort" | "compact" | "clear")
        || (name == "context" && !rest.is_empty())
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
                target: "ls".into(),
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
        app.on_terminal(key(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(app.ask.is_none());
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
