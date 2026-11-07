//! A side question (`/btw`, T57): the owner asks something briefly — about the pane's own
//! session or another's, a background agent's — without disturbing the turn under way or filling any conversation.
//! It goes to the asked session's own model as the real messages with the same tools on offer, so the provider's
//! cache of them answers the most of it; no tool is ever run. The answer streams into a card over the pane that
//! asked; closed, a folded line of it stays in that pane's stream.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use glut::model::Role;
use glut::{ChatModel, Message, Model, Piece, Prices, ToolSpec, Usage};
use ratatui::layout::Rect;
use tokio::sync::mpsc;

use crate::session::Report;
use crate::transcript::{Block, Kind, Tone, Transcript};

/// How much of the turn under way the question carries, its newest part.
const UNDER_WAY_CHARS: usize = 60_000;
/// How long one step of it may be.
const STEP_CHARS: usize = 600;
/// How much of a conversation sent as text the question carries ([`crate::session::as_text`]).
const AS_TEXT_CHARS: usize = 200_000;
/// How much of the owner's message under way the question carries.
const PROMPT_CHARS: usize = 8_000;
/// The longest answer, in tokens: a side question is answered briefly.
const SIDE_TOKENS: u32 = 4_000;

/// The side questions asked in this process: a late piece of one replaced or closed finds no card.
static ASKED: AtomicU64 = AtomicU64::new(0);

/// What the asked session lends a side question: its model, and the conversation as its next call would begin.
pub struct Asked {
    pub model: Arc<Model>,
    pub prices: Option<Prices>,
    /// Whether its model sees images, as its provider's list says; `None`: unknown.
    pub sees_images: Option<bool>,
    /// Its conversation: between turns the session's, while one runs the one it began with.
    pub conversation: Vec<Message>,
    /// The tools its last call offered; `None` before it called a model in this window (a session opened again).
    pub tools: Option<Vec<ToolSpec>>,
    /// While a turn runs: the owner's message it works on, and what it did of it so far.
    pub under_way: Option<UnderWay>,
}

/// The turn a side question comes into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnderWay {
    pub prompt: String,
    /// The images that message carries: the conversation the turn began with holds them not yet.
    pub images: Vec<glut::images::Image>,
    /// Its steps as text, a line each.
    pub steps: String,
}

/// What the call sends: the messages and the tools on offer. Between turns the conversation is the one the last call
/// began with, so the provider's cache holds its head; while a turn runs it is the one that turn began with, whose
/// system prompt the turn wrote anew — the same but for a day or a role changed since. Older thinking is left out as
/// the loop leaves it out, its images as the loop sends them.
pub fn request(asked: &Asked, question: &str) -> (Vec<Message>, Vec<ToolSpec>) {
    let asked_before = asked
        .conversation
        .iter()
        .any(|message| message.role == Role::User);
    let ask = framed(question, asked.under_way.as_ref(), asked_before);
    let called_tools = asked
        .conversation
        .iter()
        .any(|message| !message.tool_calls.is_empty() || message.role == Role::Tool);
    match &asked.tools {
        // Tool calls without the tools that made them: a model then tends to go on calling them in its raw markup
        // (as a handoff found) — the conversation goes as text, which no cache holds anyway.
        None if called_tools => {
            let text = crate::session::as_text(&asked.conversation, AS_TEXT_CHARS);
            let messages = vec![
                Message::system(SYSTEM),
                with_turn_images(
                    Message::user(format!(
                        "The conversation so far, between the lines:\n----\n{text}\n----\n\n{ask}"
                    )),
                    asked.under_way.as_ref(),
                ),
            ];
            let messages = glut::run::sendable(&messages, asked.sees_images).into_owned();
            (messages, Vec::new())
        }
        tools => {
            let mut messages = asked.conversation.clone();
            if messages
                .first()
                .is_none_or(|first| first.role != Role::System)
            {
                messages.insert(0, Message::system(SYSTEM));
            }
            messages.push(with_turn_images(
                Message::user(ask),
                asked.under_way.as_ref(),
            ));
            let messages = glut::run::sendable(&messages, asked.sees_images).into_owned();
            (messages, tools.clone().unwrap_or_default())
        }
    }
}

/// The question with the images of the message under way: the model sees what the owner showed it, as the turn does.
fn with_turn_images(mut message: Message, under_way: Option<&UnderWay>) -> Message {
    if let Some(turn) = under_way {
        message.images = turn.images.clone();
    }
    message
}

/// The system prompt of a side question that has no session's own to go after.
const SYSTEM: &str = "You answer the owner's side questions about a coding session. You have no tools; answer briefly.";

/// The owner's question, framed so the model answers it from what it knows and leaves its work alone.
/// `asked_before`: whether the session was ever given a message — an agent at rest says so, or a model asked what it
/// does makes up a task (the owner saw one invent an article it was never asked for).
fn framed(question: &str, under_way: Option<&UnderWay>, asked_before: bool) -> String {
    let mut text = String::from(
        "[A side question from the owner. Answer it briefly from what you know and what the conversation shows. Do \
         not call any tool and do not go on with the work: this answer goes to the owner alone and is not part of \
         it.]\n\n",
    );
    match under_way {
        None if asked_before => text.push_str("You are not working on anything right now.\n\n"),
        None => text.push_str(
            "You are not working on anything right now: nothing has been asked of you in this session yet.\n\n",
        ),
        Some(_) => {}
    }
    if let Some(turn) = under_way {
        text.push_str(&format!(
            "You are working on this message of the owner's right now:\n----\n{}\n----\n",
            piece_of(turn.prompt.trim(), PROMPT_CHARS)
        ));
        if turn.steps.trim().is_empty() {
            text.push_str("You have done nothing of it yet.\n\n");
        } else {
            text.push_str(&format!(
                "What you did of it so far:\n----\n{}\n----\n\n",
                turn.steps.trim_end()
            ));
        }
    }
    text.push_str(&format!("The side question:\n{}", question.trim()));
    text
}

/// The turn under way on `prompt`, as its stream shows it in `blocks` (those after the owner's message): the owner's
/// words sent while it worked, Glut's, each tool call and subagent with how it stands — the newest part where it is
/// long.
pub fn under_way(prompt: &str, images: &[glut::images::Image], blocks: &[Block]) -> UnderWay {
    let mut steps = String::new();
    for block in blocks {
        let line = match &block.kind {
            Kind::Owner(text) => format!("[owner, while you worked] {}", piece(text)),
            Kind::Answer { text, .. } if !text.trim().is_empty() => {
                format!("[you] {}", piece(text))
            }
            Kind::Tool {
                name,
                target,
                state,
                depth: 0,
                ..
            } => format!("[tool {name}] {} ({})", piece(target), stood(*state)),
            Kind::Agent(agent) => {
                format!("[subagent] {} ({})", piece(&agent.task), stood(agent.state))
            }
            _ => continue,
        };
        steps.push_str(&line);
        steps.push('\n');
    }
    let count = steps.chars().count();
    if count > UNDER_WAY_CHARS {
        steps = steps.chars().skip(count - UNDER_WAY_CHARS).collect();
    }
    UnderWay {
        prompt: prompt.to_owned(),
        images: images.to_vec(),
        steps,
    }
}

/// One step's text on one line, cut short.
fn piece(text: &str) -> String {
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    piece_of(&line, STEP_CHARS)
}

/// `text`'s first `most` characters, and how many more there were.
fn piece_of(text: &str, most: usize) -> String {
    let count = text.chars().count();
    if count <= most {
        return text.to_owned();
    }
    let kept: String = text.chars().take(most).collect();
    format!("{kept} … ({} more characters)", count - most)
}

/// How a tool call stands, in words for the model.
fn stood(state: crate::transcript::ToolState) -> &'static str {
    use crate::transcript::ToolState;
    match state {
        ToolState::Running => "running",
        ToolState::Done => "done",
        ToolState::Failed => "failed",
        ToolState::Refused => "refused",
    }
}

/// A piece of a side question's answer, or its end.
#[derive(Debug, Clone, PartialEq)]
pub enum Came {
    Text(String),
    Thinking(String),
    Done(Result<Answered, String>),
}

/// How a side question's call ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub usage: Usage,
    /// The whole text, for a model that did not stream it.
    pub text: Option<String>,
    /// The model called a tool (or wrote one in its raw markup): dropped, never run.
    pub wanted_tools: bool,
}

/// A side question on its way: dropping it ends its call.
pub struct Call(tokio::task::JoinHandle<()>);

impl Call {
    /// A call that asks nothing, for the screen's tests.
    #[cfg(test)]
    pub(crate) fn idle() -> Self {
        Call(tokio::spawn(async {}))
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Sends the side question in the background, each piece of its answer — and its end — reported as
/// [`Report::Side`] under `id`.
pub fn ask(
    model: Arc<Model>,
    messages: Vec<Message>,
    tools: Vec<ToolSpec>,
    id: u64,
    reports: mpsc::UnboundedSender<Report>,
) -> Call {
    Call(tokio::spawn(async move {
        let pieces = reports.clone();
        let mut on_piece = move |piece: Piece<'_>| {
            let came = match piece {
                Piece::Text(text) => Came::Text(text.to_owned()),
                Piece::Reasoning(text) => Came::Thinking(text.to_owned()),
            };
            let _ = pieces.send(Report::Side(id, came));
        };
        let reply = model
            .complete_streaming(&messages, &tools, Some(SIDE_TOKENS), &mut on_piece)
            .await;
        let done = reply
            .map(|reply| {
                let text = reply.message.content.filter(|text| !text.trim().is_empty());
                let wanted_tools = !reply.message.tool_calls.is_empty()
                    || text.as_deref().is_some_and(crate::session::calls_tools);
                Answered {
                    usage: reply.usage,
                    text,
                    wanted_tools,
                }
            })
            .map_err(|err| err.to_string());
        let _ = reports.send(Report::Side(id, Came::Done(done)));
    }))
}

/// How a side question stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Asking,
    Answered,
    /// It wanted to look something up: its call dropped.
    WantedTools,
    Failed,
}

/// The card of the side question asked last.
pub struct Side {
    pub id: u64,
    /// The pane or background agent asked, as the owner knows it; `None`: this pane's own session.
    pub about: Option<String>,
    pub question: String,
    /// The card's stream: the question, the thinking, the answer.
    pub stream: Transcript,
    pub state: State,
    pub prices: Option<Prices>,
    /// The first of the card's rows shown, and whether it follows the answer's end as it grows.
    pub scroll: usize,
    pub follow: bool,
    /// How many rows the card had and showed when last drawn, and where it stood.
    pub rows: usize,
    pub room: usize,
    pub area: Rect,
    _call: Option<Call>,
}

impl Side {
    /// A side question's card, its call on its way.
    pub fn new(
        about: Option<String>,
        question: String,
        prices: Option<Prices>,
        call: impl FnOnce(u64) -> Call,
    ) -> Self {
        let id = ASKED.fetch_add(1, Ordering::Relaxed);
        let mut stream = Transcript::default();
        stream.push(Kind::Owner(question.clone()));
        Side {
            id,
            about,
            question,
            stream,
            state: State::Asking,
            prices,
            scroll: 0,
            follow: true,
            rows: 0,
            room: 0,
            area: Rect::default(),
            _call: Some(call(id)),
        }
    }

    /// Takes in a piece of the answer; at its end, what the call used — when it ended answered.
    pub fn take(&mut self, came: Came) -> Option<Usage> {
        if self.state != State::Asking {
            return None;
        }
        match came {
            Came::Text(text) => self.stream.take(&glut::Event::TextDelta { turn: 0, text }),
            Came::Thinking(text) => self
                .stream
                .take(&glut::Event::ReasoningDelta { turn: 0, text }),
            Came::Done(Ok(answered)) => {
                // A model that does not stream hands its whole text over at the end.
                if self.answer().is_empty()
                    && let Some(text) = &answered.text
                {
                    self.stream.take(&glut::Event::TextDelta {
                        turn: 0,
                        text: text.clone(),
                    });
                }
                self.stream.settle();
                self._call = None;
                if answered.wanted_tools {
                    self.state = State::WantedTools;
                    self.stream.note(tr!(btw_wanted_tools), Tone::Heed);
                } else {
                    self.state = State::Answered;
                }
                return Some(answered.usage);
            }
            Came::Done(Err(reason)) => {
                self.stream.settle();
                self._call = None;
                self.state = State::Failed;
                self.stream.note(
                    tr!(btw_failed, reason = crate::transcript::clean(&reason)),
                    Tone::Fault,
                );
            }
        }
        None
    }

    /// The answer written so far.
    pub fn answer(&self) -> String {
        self.stream
            .blocks
            .iter()
            .filter_map(|block| match &block.kind {
                Kind::Answer { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Whether nothing of the answer came yet: the card says it waits.
    pub fn waiting(&self) -> bool {
        self.state == State::Asking && self.stream.blocks.len() == 1
    }

    /// Rolls the card by `rows` (up when negative); at its end it follows the answer again.
    pub fn roll(&mut self, rows: isize) {
        let last = self.rows.saturating_sub(self.room);
        let at = self.scroll.saturating_add_signed(rows).min(last);
        self.scroll = at;
        self.follow = at >= last;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asked(
        conversation: Vec<Message>,
        tools: Option<Vec<ToolSpec>>,
        under_way: Option<UnderWay>,
    ) -> Asked {
        let (_, engine) = crate::session::for_tests();
        Asked {
            model: engine.model,
            prices: None,
            sees_images: None,
            conversation,
            tools,
            under_way,
        }
    }

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            description: String::new(),
            parameters: serde_json::json!({"type": "object"}),
        }
    }

    fn called() -> Vec<Message> {
        let call: glut::ToolCall = serde_json::from_value(serde_json::json!({
            "id": "c1", "type": "function", "function": {"name": "bash", "arguments": "{\"command\":\"ls\"}"}
        }))
        .unwrap();
        let mut answer = Message::user("Looking.");
        answer.role = Role::Assistant;
        answer.tool_calls = vec![call];
        let mut result = Message::user("a.rs");
        result.role = Role::Tool;
        result.tool_call_id = Some("c1".into());
        let mut done = Message::user("Done.");
        done.role = Role::Assistant;
        vec![
            Message::system("You are Glut."),
            Message::user("List it"),
            answer,
            result,
            done,
        ]
    }

    #[test]
    fn an_agent_at_rest_is_told_so_and_one_never_asked_that_it_has_no_task() {
        let asked_text = |conversation: Vec<Message>, under_way: Option<UnderWay>| {
            let (messages, _) =
                request(&asked(conversation, None, under_way), "what are you doing?");
            messages
                .last()
                .and_then(|m| m.content.clone())
                .unwrap_or_default()
        };
        let fresh = asked_text(vec![Message::system("You are a reviewer.")], None);
        assert!(fresh.contains("nothing has been asked of you"), "{fresh}");
        let resting = asked_text(called(), None);
        assert!(
            resting.contains("not working on anything")
                && !resting.contains("nothing has been asked"),
            "{resting}"
        );
        let working = asked_text(
            vec![
                Message::system("You are a reviewer."),
                Message::user("Review it"),
            ],
            Some(under_way("Review it", &[], &[])),
        );
        assert!(!working.contains("not working on anything"), "{working}");
    }

    #[test]
    fn a_side_question_goes_after_the_conversation_as_it_is_with_its_tools_so_the_cache_holds() {
        let conversation = called();
        let (messages, tools) = request(
            &asked(conversation.clone(), Some(vec![tool("bash")]), None),
            "why ls?",
        );
        assert_eq!(
            messages[..conversation.len()],
            conversation[..],
            "the head unchanged"
        );
        assert_eq!(messages.len(), conversation.len() + 1);
        let last = messages.last().unwrap();
        assert_eq!(last.role, Role::User);
        let text = last.content.as_deref().unwrap();
        assert!(text.contains("Do not call any tool"), "{text}");
        assert!(text.ends_with("The side question:\nwhy ls?"), "{text}");
        assert!(
            !text.contains("You are working on") && text.contains("not working on anything"),
            "no turn under way: {text}"
        );
        assert_eq!(tools, vec![tool("bash")]);
    }

    #[test]
    fn the_images_of_the_message_under_way_go_with_the_question() {
        let image = glut::images::Image {
            number: 1,
            path: "/kept/receipt".into(),
            media: "image/png".into(),
            width: 420,
            height: 440,
            bytes: 24_911,
        };
        let turn = under_way(
            "does total() agree with [image #1]?",
            std::slice::from_ref(&image),
            &[],
        );
        for tools in [Some(vec![tool("bash")]), None] {
            let (messages, _) = request(
                &asked(called(), tools, Some(turn.clone())),
                "what is on it?",
            );
            let last = messages.last().unwrap();
            assert_eq!(
                last.images,
                std::slice::from_ref(&image),
                "the question shows the image the turn was given"
            );
        }
        let (messages, _) = request(&asked(called(), None, None), "what is on it?");
        assert!(
            messages.iter().all(|message| message.images.is_empty()),
            "between turns none is added"
        );
    }

    #[test]
    fn tool_calls_without_their_tools_go_as_text_and_a_new_session_gets_a_system_prompt() {
        let (messages, tools) = request(&asked(called(), None, None), "why?");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        let text = messages[1].content.as_deref().unwrap();
        assert!(text.contains("[tool bash] {\"command\":\"ls\"}"), "{text}");
        assert!(tools.is_empty());
        let (messages, _) = request(&asked(Vec::new(), None, None), "hello?");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content.as_deref(), Some(SYSTEM));
    }

    #[test]
    fn the_turn_under_way_is_told_from_its_stream() {
        let mut stream = Transcript::default();
        stream.take(&glut::Event::ToolCall {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            arguments: "{\"path\":\"src/parse.rs\"}".into(),
        });
        stream.take(&glut::Event::AssistantText {
            turn: 1,
            text: "The bug is in the lexer.".into(),
        });
        stream.note("a note", Tone::Quiet);
        stream.push(Kind::Owner("also the tests".into()));
        let turn = under_way("Fix the parser", &[], &stream.blocks);
        assert_eq!(turn.prompt, "Fix the parser");
        assert_eq!(
            turn.steps,
            "[tool read_file] src/parse.rs (running)\n[you] The bug is in the lexer.\n[owner, while you worked] also \
             the tests\n"
        );
        let (messages, _) = request(&asked(Vec::new(), None, Some(turn)), "how far?");
        let text = messages[1].content.as_deref().unwrap();
        assert!(
            text.contains("right now:\n----\nFix the parser\n----"),
            "{text}"
        );
        assert!(text.contains("so far:\n----\n[tool read_file]"), "{text}");
        let quiet = under_way("Fix the parser", &[], &[]);
        let (messages, _) = request(&asked(Vec::new(), None, Some(quiet)), "how far?");
        assert!(
            messages[1]
                .content
                .as_deref()
                .unwrap()
                .contains("done nothing of it yet")
        );
    }

    #[tokio::test]
    async fn the_card_streams_and_settles() {
        let fake = |_| Call(tokio::spawn(async {}));
        let mut side = Side::new(
            Some("reviewer".into()),
            "what did you find?".into(),
            None,
            fake,
        );
        assert!(side.waiting());
        side.take(Came::Thinking("hm".into()));
        side.take(Came::Text("Two ".into()));
        side.take(Came::Text("bugs.".into()));
        assert!(!side.waiting());
        let usage = Usage {
            input_tokens: 10,
            cached_input_tokens: 8,
            output_tokens: 2,
        };
        let done = Came::Done(Ok(Answered {
            usage,
            text: Some("Two bugs.".into()),
            wanted_tools: false,
        }));
        assert_eq!(side.take(done.clone()), Some(usage));
        assert_eq!(side.state, State::Answered);
        assert_eq!(side.answer(), "Two bugs.", "streamed once, not twice");
        assert_eq!(side.take(done), None, "a late end counts nothing");
        let mut quiet = Side::new(None, "q".into(), None, fake);
        quiet.take(Came::Done(Ok(Answered {
            usage,
            text: Some("whole".into()),
            wanted_tools: true,
        })));
        assert_eq!(quiet.state, State::WantedTools);
        assert_eq!(quiet.answer(), "whole", "a model that did not stream");
        let mut failed = Side::new(None, "q".into(), None, fake);
        failed.take(Came::Done(Err("refused".into())));
        assert_eq!(failed.state, State::Failed);
    }

    #[tokio::test]
    async fn the_card_rolls_and_follows_its_end_again() {
        let fake = |_| Call(tokio::spawn(async {}));
        let mut side = Side::new(None, "q".into(), None, fake);
        side.rows = 30;
        side.room = 10;
        side.scroll = 20;
        side.roll(-5);
        assert_eq!((side.scroll, side.follow), (15, false));
        side.roll(-50);
        assert_eq!(side.scroll, 0);
        side.roll(100);
        assert_eq!((side.scroll, side.follow), (20, true));
    }
}
