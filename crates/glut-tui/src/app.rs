//! The session on the screen: what is shown, and what a key, a click or a step of the run changes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use glut::permissions::{Answer, AskRequest};
use glut::{Cancel, Event, FinishReason, Message, Usage};
use tokio::sync::{mpsc, oneshot};

use crate::input::Input;
use crate::session::{self, Parts, Report};
use crate::transcript::{Tone, Transcript};

/// How long a second Ctrl+C has to come to quit.
const QUIT_WINDOW: Duration = Duration::from_millis(1500);
/// How many lines the mouse wheel scrolls.
const WHEEL_LINES: usize = 3;
/// How long a question is on the screen before a key answers it.
const ASK_SETTLE: Duration = Duration::from_millis(400);

/// A question of the gate on the screen.
pub struct OpenAsk {
    pub request: AskRequest,
    reply: oneshot::Sender<Answer>,
    /// When the card came up: keys before [`ASK_SETTLE`] has passed do not answer it.
    opened: Instant,
}

pub struct App {
    pub parts: Arc<Parts>,
    pub folder: String,
    pub transcript: Transcript,
    pub input: Input,
    /// The conversation between turns; taken while a turn runs.
    conversation: Vec<Message>,
    /// The turn under way: its cancel.
    running: Option<Cancel>,
    pub ask: Option<OpenAsk>,
    pub usage: Usage,
    pub cost: Option<f64>,
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
    pub fn new(parts: Arc<Parts>, reports: mpsc::UnboundedSender<Report>) -> Self {
        let folder = session::shown_folder(&parts.config.cwd);
        let mut transcript = Transcript::default();
        for note in &parts.notes {
            transcript.note(note.clone(), Tone::Quiet);
        }
        App {
            parts,
            folder,
            transcript,
            input: Input::new(),
            conversation: Vec::new(),
            running: None,
            ask: None,
            usage: Usage::default(),
            cost: None,
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

    pub fn working(&self) -> bool {
        self.running.is_some()
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
        session::turn(
            Arc::clone(&self.parts),
            std::mem::take(&mut self.conversation),
            prompt,
            cancel,
            self.reports.clone(),
        );
    }

    /// A step of the run, a question, or the turn's end.
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
                    if let Some(prices) = self.parts.prices {
                        *self.cost.get_or_insert(0.0) += prices.cost_usd(&call);
                    }
                    self.usage += call;
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
        }
    }

    /// A key, a paste, a click or the wheel.
    pub fn on_terminal(&mut self, event: TermEvent) {
        match event {
            TermEvent::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            TermEvent::Paste(text) if self.ask.is_none() => self.input.insert_text(&text),
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
        match key.code {
            KeyCode::Enter if shift || alt => self.input.newline(),
            KeyCode::Char('j') if ctrl => self.input.newline(),
            KeyCode::Enter => {
                if self.working() {
                    self.transcript.note(
                        "Glut is working — Esc stops it; the message waits in the input",
                        Tone::Quiet,
                    );
                } else {
                    let prompt = self.input.take();
                    self.send(prompt);
                }
            }
            KeyCode::Esc => {
                if let Some(cancel) = &self.running {
                    cancel.cancel();
                }
            }
            KeyCode::Char('d') if ctrl && self.input.is_empty() && !self.working() => {
                self.quit = true
            }
            KeyCode::Char('o') if ctrl => {
                self.transcript.unfold_all();
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
            KeyCode::Up if shift => self.scroll_by(1),
            KeyCode::Down if shift => self.scroll_by(-1),
            KeyCode::Up => self.input.up(),
            KeyCode::Down => self.input.down(),
            KeyCode::PageUp => self.scroll_by(self.page()),
            KeyCode::PageDown => self.scroll_by(-self.page()),
            KeyCode::Tab => self.input.insert(' '),
            KeyCode::Char(c) if !ctrl => self.input.insert(c),
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

    /// Ctrl+C: clears the input, else stops the turn, else quits on a second press.
    fn interrupt(&mut self) {
        if let Some(open) = self.ask.take() {
            let _ = open.reply.send(Answer::Deny);
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
    use glut::{ModelEndpoint, OpenAiCompat, RunConfig, RunOutcome, SharedSink, Toolbox};

    /// Swallows the events of a test session.
    struct Nowhere;

    impl glut::EventSink for Nowhere {
        fn emit(&mut self, _event: Event) {}
    }

    fn app() -> (App, mpsc::UnboundedReceiver<Report>) {
        let parts = Parts {
            config: RunConfig::new("/tmp"),
            model: OpenAiCompat::new(ModelEndpoint {
                base_url: "http://localhost:9".into(),
                api_key: None,
                model: "m".into(),
                effort: None,
                versioned: true,
            })
            .unwrap(),
            tools: Toolbox::new(),
            sink: SharedSink::new(Box::new(Nowhere)),
            provider: "test".into(),
            model_name: "m".into(),
            prices: None,
            notes: Vec::new(),
        };
        let (send, receive) = mpsc::unbounded_channel();
        (App::new(Arc::new(parts), send), receive)
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> TermEvent {
        TermEvent::Key(KeyEvent::new(code, modifiers))
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
}
