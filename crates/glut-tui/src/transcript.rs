//! The stream (T2, T6): the session as blocks — the owner's messages, Glut's answers as they stream, one line per tool
//! call that unfolds, notes. Each block is wrapped to the screen's width once and kept until it changes, so a long
//! session costs only the blocks that are on the screen.
//!
//! A subagent (`task`, T18) is a tile in its parent's stream — its task, its live line, its calls and tokens — and
//! keeps a stream of its own that the owner opens as a view; a subagent's subagents are tiles in that stream.

use glut::Event;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::theme;

/// The tool that starts a subagent.
const TASK: &str = "task";

/// How many lines of a tool's result an unfolded tool line shows.
const RESULT_LINES: usize = 40;
/// How many lines of a step's change (`/steps`) the stream shows.
const DIFF_LINES: usize = 600;

/// How a tool call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    Running,
    Done,
    Failed,
    Refused,
}

/// One block of the stream.
#[derive(Debug, Clone)]
pub enum Kind {
    Owner(String),
    /// Glut's text; `streaming` while it still grows.
    Answer {
        text: String,
        streaming: bool,
    },
    Tool {
        id: String,
        name: String,
        target: String,
        state: ToolState,
        /// The gate's word when it was not a plain allow: "asked", a refusal's reason.
        gate: Option<String>,
        output: String,
        unfolded: bool,
        /// How deep in subagents the call is (HS8): 0 for the session's own.
        depth: u32,
    },
    Note {
        text: String,
        tone: Tone,
    },
    /// A subagent (T18): a tile here, its own stream inside.
    Agent(Box<Agent>),
    /// An image (T46): the owner's under their message, `read_file`'s under its call — drawn as `/images` says,
    /// its line under it.
    Picture(glut::images::Image),
    /// The model's thinking (T8), shown as the owner chose: hidden, a folded line, or open.
    Thinking {
        text: String,
        streaming: bool,
        /// Opened (or, in the open mode, folded) by hand.
        flipped: bool,
    },
}

impl Kind {
    /// What the reading mode's `c` copies (TUI9a): the text as written — a tool's whole output, its call where it
    /// has none.
    pub fn copied(&self) -> String {
        match self {
            Kind::Owner(text)
            | Kind::Answer { text, .. }
            | Kind::Note { text, .. }
            | Kind::Thinking { text, .. } => text.clone(),
            Kind::Tool {
                name,
                target,
                output,
                ..
            } => {
                if output.is_empty() {
                    format!("{name} {target}")
                } else {
                    output.clone()
                }
            }
            Kind::Agent(agent) => agent.task.clone(),
            Kind::Picture(image) => image.label(),
        }
    }
}

/// A subagent the parent started with `task`: what the tile shows and the stream its view shows.
#[derive(Debug, Clone)]
pub struct Agent {
    /// The parent's `task` call.
    pub id: String,
    /// What it does, as the parent's model labelled it.
    pub task: String,
    pub state: ToolState,
    /// The gate's word on the `task` call when it was not a plain allow.
    pub gate: Option<String>,
    /// What it answered, or why it stopped.
    pub answer: String,
    /// Its own stream, its subagents as tiles in it.
    pub stream: Transcript,
    /// Its own tool calls, its subagents' not counted.
    pub calls: u32,
    /// Tokens of every model call in it, its subagents' included.
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The last thing it did, for the tile.
    pub live: String,
    /// Characters of thinking or text streamed since its last reply ended: a sign of life while a model thinks for
    /// minutes before its first call (the owner took one for stuck).
    streamed: usize,
    /// The model a named agent runs on (T37a), as its stream's start says; `None` for a plain subagent.
    pub model: Option<String>,
    /// Whether its `task` call named an agent.
    named: bool,
    /// When it started and ended, as this window saw it (T69): a stream read back from a kept session saw neither.
    started: Option<std::time::Instant>,
    ended: Option<std::time::Instant>,
}

impl Agent {
    /// How long it ran, or runs; `None` for less than a second — a stream read back from a kept session ran none.
    pub fn ran(&self) -> Option<std::time::Duration> {
        let started = self.started?;
        let ran = self
            .ended
            .unwrap_or_else(std::time::Instant::now)
            .saturating_duration_since(started);
        (ran.as_secs() >= 1).then_some(ran)
    }

    /// `state` with the size streamed so far, counted as the thinking's header counts it.
    fn streaming(&self, state: &str) -> String {
        tr!(
            thought_size,
            state = state,
            size = tokens_short(self.streamed.div_ceil(4))
        )
        .to_string()
    }

    /// It ended: its time stops.
    fn end(&mut self) {
        self.ended.get_or_insert_with(std::time::Instant::now);
    }

    /// Takes in one event the subagent reported (unwrapped once: its own events at depth 0, its subagents' still
    /// wrapped).
    fn take(&mut self, event: &Event) {
        if let Event::Usage {
            input_tokens,
            output_tokens,
            ..
        } = innermost(event)
        {
            self.input_tokens += input_tokens;
            self.output_tokens += output_tokens;
        }
        if matches!(event, Event::Usage { .. } | Event::ToolCall { .. }) {
            self.streamed = 0;
        }
        match event {
            Event::Started { model, .. } if self.named => self.model = Some(model.clone()),
            Event::ToolCall {
                name, arguments, ..
            } => {
                self.calls += 1;
                self.live = format!("{name} {}", target_of(arguments));
            }
            Event::TextDelta { text, .. } => {
                self.streamed += text.chars().count();
                self.live = self.streaming(tr!(agent_writing));
            }
            Event::ReasoningDelta { text, .. } => {
                self.streamed += text.chars().count();
                self.live = self.streaming(tr!(thinking_live));
            }
            Event::AssistantText { text, .. } => {
                if let Some(line) = text.lines().find(|line| !line.trim().is_empty()) {
                    self.live = line.trim().to_owned();
                }
            }
            Event::Sub { event, .. } => {
                if let Event::ToolCall {
                    name, arguments, ..
                } = innermost(event)
                {
                    self.live = format!("↳ {name} {}", target_of(arguments));
                }
            }
            _ => {}
        }
        self.stream.take(event);
    }
}

/// The event inside every wrapping of nested runs.
fn innermost(event: &Event) -> &Event {
    match event {
        Event::Sub { event, .. } => innermost(event),
        other => other,
    }
}

/// How much of the model's thinking the stream shows (T8; `/thinking`, Ctrl+T).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Thinking {
    /// None of it.
    Off,
    /// One line per block; a click opens one.
    #[default]
    Folded,
    /// Every block open; a click folds one.
    Open,
}

/// A note's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Quiet,
    Fault,
    Fine,
    /// Worth a look, no fault: in Glut's orange (T42, a guide grown too large).
    Heed,
}

/// A block and its wrapped lines for the width they were made for.
#[derive(Debug, Clone)]
pub struct Block {
    pub kind: Kind,
    /// The lines made last, for the width, way of showing thinking and animation step they were made for.
    wrapped: Option<(u16, Thinking, usize, Vec<Line<'static>>)>,
    /// Those lines as text, made once for them: what the search looks through (T47).
    texts: Option<Vec<String>>,
}

impl Block {
    fn new(kind: Kind) -> Self {
        Block {
            kind,
            wrapped: None,
            texts: None,
        }
    }

    /// The block changed: its lines are made again when next drawn.
    fn touch(&mut self) {
        self.wrapped = None;
        self.texts = None;
    }

    /// Its lines' texts given up: the search that read them closed.
    pub fn forget_texts(&mut self) {
        self.texts = None;
    }

    /// The block's lines as text at `width`, made once for each making of its lines (T47: the search reads them at
    /// every frame it is open).
    pub fn texts(&mut self, width: u16, thinking: Thinking, frame: usize) -> &[String] {
        self.lines(width, thinking, frame);
        if self.texts.is_none() {
            let made = self
                .wrapped
                .as_ref()
                .map(|(_, _, _, lines)| lines.iter().map(crate::select::text_of).collect())
                .unwrap_or_default();
            self.texts = Some(made);
        }
        self.texts.as_deref().unwrap_or(&[])
    }

    /// The block's lines at `width`, made once per change, width and way of showing thinking — and, for a thought
    /// still streaming, once per animation step `frame`: Glimm flickers in front of it.
    pub fn lines(&mut self, width: u16, thinking: Thinking, frame: usize) -> &[Line<'static>] {
        let step = match self.kind {
            Kind::Thinking {
                streaming: true, ..
            } => frame,
            _ => 0,
        };
        if self
            .wrapped
            .as_ref()
            .is_none_or(|(at, shown, made, _)| *at != width || *shown != thinking || *made != step)
        {
            let lines = render(&self.kind, width, thinking, step);
            self.wrapped = Some((width, thinking, step, lines));
            self.texts = None;
            LAYOUTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        self.wrapped
            .as_ref()
            .map(|(_, _, _, lines)| lines.as_slice())
            .unwrap_or(&[])
    }
}

/// How many times a block's lines were made, in every stream: a search's matches stand while it has not changed — the
/// block count and the line count alone miss a line that grows as the answer streams (T47).
static LAYOUTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many times a block's lines were made so far.
pub fn layouts() -> u64 {
    LAYOUTS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The whole stream.
#[derive(Debug, Default, Clone)]
pub struct Transcript {
    pub blocks: Vec<Block>,
    /// How the model's thinking is shown.
    pub thinking: Thinking,
}

impl Transcript {
    /// The images a tool's answer showed (T46), under its call — once, when the turn's conversation is back. Returns
    /// where they went in and how many: blocks after them moved on by that many.
    pub fn show_pictures(
        &mut self,
        call: &str,
        images: &[glut::images::Image],
    ) -> Option<(usize, usize)> {
        // Most results show none, and the call is near the end: a resumed session's thousands of results would
        // each walk the whole stream otherwise (T47).
        if images.is_empty() {
            return None;
        }
        let at = self
            .blocks
            .iter()
            .rposition(|block| matches!(&block.kind, Kind::Tool { id, .. } if id == call))?;
        if matches!(
            self.blocks.get(at + 1).map(|block| &block.kind),
            Some(Kind::Picture(_))
        ) {
            return None;
        }
        for (offset, image) in images.iter().enumerate() {
            self.blocks
                .insert(at + 1 + offset, Block::new(Kind::Picture(image.clone())));
        }
        Some((at + 1, images.len()))
    }

    pub fn push(&mut self, kind: Kind) {
        self.blocks.push(Block::new(kind));
    }

    pub fn note(&mut self, text: impl Into<String>, tone: Tone) {
        self.push(Kind::Note {
            text: text.into(),
            tone,
        });
    }

    /// Takes in one step of a run.
    pub fn take(&mut self, event: &Event) {
        self.take_at(event, 0);
    }

    fn take_at(&mut self, event: &Event, depth: u32) {
        match event {
            Event::ReasoningDelta { text, .. } if depth == 0 => self.think(text),
            Event::TextDelta { text, .. } if depth == 0 => {
                self.close_thinking();
                self.stream(text)
            }
            Event::AssistantText { text, .. } if depth == 0 => self.answered(text),
            // The owner's message sent while Glut worked, where the run took it in.
            Event::Steered { text, .. } if depth == 0 => {
                self.close_answer();
                self.close_thinking();
                self.push(Kind::Owner(text.clone()));
            }
            Event::ToolCall {
                id,
                name,
                arguments,
                ..
            } => {
                // A streamed answer (or thinking) that stopped for a tool call is complete: the call comes after it.
                self.close_answer();
                self.close_thinking();
                if name == TASK && depth == 0 {
                    // A named agent heads its tile by its name (T37a).
                    let agent = serde_json::from_str::<serde_json::Value>(arguments)
                        .ok()
                        .and_then(|value| value.get("agent")?.as_str().map(str::to_owned))
                        .filter(|agent| !agent.trim().is_empty());
                    let task = match &agent {
                        Some(agent) => format!("{agent}: {}", target_of(arguments)),
                        None => target_of(arguments),
                    };
                    self.push(Kind::Agent(Box::new(Agent {
                        id: id.clone(),
                        task,
                        state: ToolState::Running,
                        gate: None,
                        answer: String::new(),
                        stream: Transcript {
                            thinking: self.thinking,
                            ..Transcript::default()
                        },
                        calls: 0,
                        input_tokens: 0,
                        output_tokens: 0,
                        live: tr!(agent_starting).into(),
                        streamed: 0,
                        model: None,
                        named: agent.is_some(),
                        started: Some(std::time::Instant::now()),
                        ended: None,
                    })));
                    return;
                }
                self.push(Kind::Tool {
                    id: id.clone(),
                    name: name.clone(),
                    target: target_of(arguments),
                    state: ToolState::Running,
                    gate: None,
                    output: String::new(),
                    unfolded: false,
                    depth,
                });
            }
            Event::Permission {
                id,
                allowed,
                reason,
                ..
            } => {
                if let Some(block) = self.tool(id) {
                    if let Some((state, gate)) = judged(&mut block.kind) {
                        if !allowed {
                            *state = ToolState::Refused;
                            *gate = Some(reason.clone());
                        } else if reason.contains("owner") || reason.contains("asked") {
                            *gate = Some(reason.clone());
                        }
                    }
                    block.touch();
                }
            }
            // T58: said beside the call, as the gate's word is — the placeholders stand in its output.
            Event::Masked { id, hidden, .. } => {
                if let Some(block) = self.tool(id)
                    && let Kind::Tool { gate, .. } = &mut block.kind
                {
                    let said = tr!(secrets_hidden_note, n = hidden.len()).to_string();
                    *gate = Some(match gate.take() {
                        Some(was) => format!("{was} · {said}"),
                        None => said,
                    });
                    block.touch();
                }
            }
            Event::ToolResult { id, ok, output, .. } => {
                if let Some(block) = self.tool(id) {
                    let kept = match &mut block.kind {
                        Kind::Tool {
                            state,
                            output: kept,
                            ..
                        } => Some((state, kept)),
                        Kind::Agent(agent) => {
                            // What still ran inside it ended with it.
                            agent.stream.settle();
                            agent.live.clear();
                            agent.end();
                            Some((&mut agent.state, &mut agent.answer))
                        }
                        _ => None,
                    };
                    if let Some((state, kept)) = kept {
                        if *state != ToolState::Refused {
                            *state = if *ok {
                                ToolState::Done
                            } else {
                                ToolState::Failed
                            };
                        }
                        kept.clone_from(output);
                    }
                    block.touch();
                }
            }
            Event::Compacted {
                before_tokens,
                after_tokens,
                ..
            } => self.note(
                tr!(
                    compacted_tokens,
                    before = before_tokens / 1000,
                    after = after_tokens / 1000
                ),
                Tone::Quiet,
            ),
            // Glut spoke to the model in its own words (T73): a quiet line, so the owner knows why it went on.
            Event::Nudged { kind, .. } if depth == 0 => self.note(
                match kind {
                    glut::NudgeKind::Reminder => tr!(nudged_reminder),
                    glut::NudgeKind::NoList => tr!(nudged_no_list),
                    glut::NudgeKind::OpenItems => tr!(nudged_open),
                    glut::NudgeKind::Announced => tr!(nudged_announced),
                    glut::NudgeKind::Repeated => tr!(nudged_repeated),
                    glut::NudgeKind::Reading => tr!(nudged_reading),
                },
                Tone::Quiet,
            ),
            Event::Error { message } if depth == 0 => self.note(message.clone(), Tone::Fault),
            Event::ModeChanged { mode, .. } => self.note(tr!(mode_now, mode = mode), Tone::Fine),
            // A subagent's step goes to its tile; one with no tile to go to (none runs) stands indented here.
            Event::Sub {
                depth: inner,
                task,
                event,
            } if depth > 0 || !self.hand_to_agent(task.as_deref(), event) => {
                self.take_at(event, *inner)
            }
            _ => {}
        }
    }

    /// A subagent's event to its tile: the one of its `task` call (T40: several run at once), or — from a log
    /// written before events named it — the last tile still running, as they then ran one at a time.
    fn hand_to_agent(&mut self, task: Option<&str>, event: &Event) -> bool {
        let Some(block) = self
            .blocks
            .iter_mut()
            .rev()
            .find(|block| match (&block.kind, task) {
                (Kind::Agent(agent), Some(task)) => agent.id == task,
                (Kind::Agent(agent), None) => agent.state == ToolState::Running,
                _ => false,
            })
        else {
            return false;
        };
        if let Kind::Agent(agent) = &mut block.kind {
            agent.take(event);
        }
        block.touch();
        true
    }

    /// The subagent at `path` — the block's index in this stream, then in its stream, and so on.
    pub fn agent(&mut self, path: &[usize]) -> Option<&mut Agent> {
        let (first, rest) = path.split_first()?;
        let Kind::Agent(agent) = &mut self.blocks.get_mut(*first)?.kind else {
            return None;
        };
        if rest.is_empty() {
            Some(agent)
        } else {
            agent.stream.agent(rest)
        }
    }

    /// Every subagent of the stream, each under its parent, with its path (one index per level, so its depth is the
    /// path's length less one).
    pub fn agents(&self) -> Vec<(Vec<usize>, &Agent)> {
        let mut found = Vec::new();
        self.gather(&mut Vec::new(), &mut found);
        found
    }

    fn gather<'a>(&'a self, path: &mut Vec<usize>, found: &mut Vec<(Vec<usize>, &'a Agent)>) {
        for (at, block) in self.blocks.iter().enumerate() {
            if let Kind::Agent(agent) = &block.kind {
                path.push(at);
                found.push((path.clone(), agent));
                agent.stream.gather(path, found);
                path.pop();
            }
        }
    }

    /// How many subagents the session started since the owner's last message: the turn's, as far as it has come.
    pub fn agents_this_turn(&self) -> usize {
        self.blocks
            .iter()
            .rev()
            .take_while(|block| !matches!(block.kind, Kind::Owner(_)))
            .filter(|block| matches!(block.kind, Kind::Agent(_)))
            .count()
    }

    /// The indexes of the blocks that are subagents, in order.
    pub fn agent_blocks(&self) -> Vec<usize> {
        self.blocks
            .iter()
            .enumerate()
            .filter(|(_, block)| matches!(block.kind, Kind::Agent(_)))
            .map(|(at, _)| at)
            .collect()
    }

    /// A piece of the model's thinking as it streams.
    fn think(&mut self, piece: &str) {
        // Thinking after streamed text: the text so far is complete, the thought comes after it.
        self.close_answer();
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Thinking {
                text,
                streaming: true,
                ..
            } = &mut block.kind
        {
            text.push_str(piece);
            block.touch();
            return;
        }
        self.push(Kind::Thinking {
            text: piece.to_owned(),
            streaming: true,
            flipped: false,
        });
    }

    fn close_thinking(&mut self) {
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Thinking { streaming, .. } = &mut block.kind
            && *streaming
        {
            *streaming = false;
            block.touch();
        }
    }

    /// Shows the thinking as `thinking` says from now on; a block opened or folded by hand keeps it.
    pub fn show_thinking(&mut self, thinking: Thinking) {
        self.thinking = thinking;
        for block in &mut self.blocks {
            if let Kind::Thinking { flipped, .. } = &mut block.kind {
                *flipped = false;
            }
        }
    }

    /// A piece of Glut's answer as it streams.
    fn stream(&mut self, piece: &str) {
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Answer {
                text,
                streaming: true,
            } = &mut block.kind
        {
            text.push_str(piece);
            block.touch();
            return;
        }
        self.push(Kind::Answer {
            text: piece.to_owned(),
            streaming: true,
        });
    }

    /// The answer's whole text: it replaces what streamed, or stands alone when nothing did.
    fn answered(&mut self, whole: &str) {
        self.close_thinking();
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Answer { text, streaming } = &mut block.kind
            && *streaming
        {
            whole.clone_into(text);
            *streaming = false;
            block.touch();
            return;
        }
        if !whole.trim().is_empty() {
            self.push(Kind::Answer {
                text: whole.to_owned(),
                streaming: false,
            });
        }
    }

    fn close_answer(&mut self) {
        if let Some(block) = self.blocks.last_mut()
            && let Kind::Answer { streaming, .. } = &mut block.kind
            && *streaming
        {
            *streaming = false;
            block.touch();
        }
    }

    /// The turn ended: whatever still runs or streams is settled.
    pub fn settle(&mut self) {
        self.close_answer();
        self.close_thinking();
        for block in &mut self.blocks {
            match &mut block.kind {
                Kind::Tool { state, .. } if *state == ToolState::Running => {
                    *state = ToolState::Failed;
                    block.touch();
                }
                Kind::Agent(agent) if agent.state == ToolState::Running => {
                    agent.state = ToolState::Failed;
                    agent.stream.settle();
                    agent.live.clear();
                    agent.end();
                    block.touch();
                }
                _ => {}
            }
        }
    }

    fn tool(&mut self, wanted: &str) -> Option<&mut Block> {
        self.blocks
            .iter_mut()
            .rev()
            .find(|block| match &block.kind {
                Kind::Tool { id, .. } => id == wanted,
                Kind::Agent(agent) => agent.id == wanted,
                _ => false,
            })
    }

    /// Every block laid out anew at its next frame: its colours changed (`/theme`).
    pub fn touch_all(&mut self) {
        for block in &mut self.blocks {
            if let Kind::Agent(agent) = &mut block.kind {
                agent.stream.touch_all();
            }
            block.touch();
        }
    }

    /// The pictures of `files` laid out anew: made beside the screen, they are ready now (T47).
    pub fn touch_pictures(&mut self, files: &[std::path::PathBuf]) {
        if files.is_empty() {
            return;
        }
        for block in &mut self.blocks {
            match &mut block.kind {
                Kind::Agent(agent) => agent.stream.touch_pictures(files),
                Kind::Picture(image) if files.contains(&image.path) => block.touch(),
                _ => {}
            }
        }
    }

    /// Unfolds every tool line, or — when all are unfolded already — folds them all.
    pub fn unfold_all(&mut self) {
        let open = self.blocks.iter().any(|block| {
            matches!(
                block.kind,
                Kind::Tool {
                    unfolded: false,
                    ..
                }
            )
        });
        for block in &mut self.blocks {
            if let Kind::Tool { unfolded, .. } = &mut block.kind {
                *unfolded = open;
                block.touch();
            }
        }
    }

    /// Folds or unfolds block `at`, when it is a tool line.
    pub fn toggle(&mut self, at: usize) {
        if let Some(block) = self.blocks.get_mut(at) {
            match &mut block.kind {
                Kind::Tool { unfolded, .. } => *unfolded = !*unfolded,
                Kind::Thinking { flipped, .. } => *flipped = !*flipped,
                _ => return,
            }
            block.touch();
        }
    }
}

/// The state and the gate's word of a tool line or a tile.
fn judged(kind: &mut Kind) -> Option<(&mut ToolState, &mut Option<String>)> {
    match kind {
        Kind::Tool { state, gate, .. } => Some((state, gate)),
        Kind::Agent(agent) => Some((&mut agent.state, &mut agent.gate)),
        _ => None,
    }
}

/// What a tool call works on, from its arguments: the command, the path, the pattern — else the arguments cut short.
fn target_of(arguments: &str) -> String {
    const KEYS: [&str; 8] = [
        "command",
        "path",
        "file_path",
        "pattern",
        "url",
        "query",
        "description",
        "code",
    ];
    let found = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| {
            KEYS.iter().find_map(|key| {
                value
                    .get(*key)
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
        });
    let text = found.unwrap_or_else(|| arguments.to_owned());
    let line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    cut(&line, 160)
}

/// `text` cut to `max` cells, with an ellipsis when it was longer.
pub(crate) fn cut(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_owned();
    }
    let mut short = String::new();
    let mut used = 0;
    for c in text.chars() {
        let cells = cell_width(c);
        if used + cells + 1 > max {
            break;
        }
        short.push(c);
        used += cells;
    }
    short.push('…');
    short
}

/// A block as lines of the screen's width.
fn render(kind: &Kind, width: u16, thinking: Thinking, frame: usize) -> Vec<Line<'static>> {
    let width = usize::from(width.max(8));
    let mut lines = Vec::new();
    match kind {
        Kind::Owner(text) => {
            lines.push(Line::default());
            let style = Style::default().fg(theme::c().owner);
            for (at, row) in wrap(text, width - 2).into_iter().enumerate() {
                let mark = if at == 0 { "› " } else { "  " };
                lines.push(Line::from(vec![
                    Span::styled(mark, theme::quiet()),
                    Span::styled(row, style),
                ]));
            }
        }
        Kind::Answer { text, streaming } => {
            lines.push(Line::default());
            // A dot in the theme's glow marks where the model speaks, its lines kept under the dot's column; the
            // tools' dots below say how each call went (the owner, 2026-10-26).
            for (at, mut row) in crate::markdown::render(text, width - 2)
                .into_iter()
                .enumerate()
            {
                let mark = if at == 0 {
                    Span::styled("● ", theme::glow())
                } else {
                    Span::raw("  ")
                };
                row.spans.insert(0, mark);
                lines.push(row);
            }
            if *streaming {
                lines.push(Line::from(Span::styled("  ▍", theme::glow())));
            }
        }
        Kind::Tool {
            name,
            target,
            state,
            gate,
            output,
            unfolded,
            depth,
            ..
        } => {
            let indent = "  ".repeat(*depth as usize);
            let (mark, mark_style) = match state {
                ToolState::Running => ("●", theme::glow()),
                ToolState::Done => ("●", Style::default().fg(theme::c().fine)),
                ToolState::Failed => ("●", Style::default().fg(theme::c().fault)),
                ToolState::Refused => ("⊘", Style::default().fg(theme::c().fault)),
            };
            // The head is one line: the name and the gate's word cut first to what fits, the target gets the rest.
            // A step's change (`/steps`) is read whole and in colour; a tool's result is a glance.
            let diff = name == "diff";
            let name = cut(&clean(name), 32);
            let gate = gate
                .as_deref()
                .map(|gate| format!("  ({})", cut(&clean(gate), 48)));
            let used = indent.len()
                + 2
                + name.width()
                + 1
                + gate.as_deref().map_or(0, UnicodeWidthStr::width);
            let room = width.saturating_sub(used).max(8);
            let mut head = vec![
                Span::raw(indent.clone()),
                Span::styled(format!("{mark} "), mark_style),
                Span::styled(name, theme::strong()),
                Span::raw(" "),
                Span::styled(cut(&clean(target), room), theme::quiet()),
            ];
            if let Some(gate) = gate {
                head.push(Span::styled(gate, theme::quiet()));
            }
            lines.push(Line::from(head));
            if *unfolded && !output.is_empty() {
                let shown: Vec<&str> = output.lines().collect();
                let most = if diff { DIFF_LINES } else { RESULT_LINES };
                for row in shown.iter().take(most) {
                    let style = match row.chars().next() {
                        Some('+') if diff && !row.starts_with("+++") => {
                            Style::default().fg(theme::c().fine)
                        }
                        Some('-') if diff && !row.starts_with("---") => {
                            Style::default().fg(theme::c().fault)
                        }
                        Some('@') if diff => theme::glow(),
                        _ => theme::quiet(),
                    };
                    for part in wrap(row, width.saturating_sub(indent.len() + 4)) {
                        lines.push(Line::from(vec![
                            Span::raw(format!("{indent}  │ ")),
                            Span::styled(part, style),
                        ]));
                    }
                }
                if shown.len() > most {
                    lines.push(Line::from(Span::styled(
                        format!("{indent}  │ … {} more lines", shown.len() - most),
                        theme::quiet().add_modifier(Modifier::ITALIC),
                    )));
                }
            }
        }
        Kind::Thinking {
            text,
            streaming,
            flipped,
        } => {
            let open = match thinking {
                Thinking::Off => return lines,
                Thinking::Folded => *flipped,
                Thinking::Open => !*flipped,
            };
            // About four characters to a token: the size of the thought, not a count to bill by.
            let size = text.chars().count().div_ceil(4);
            let state = if *streaming {
                tr!(thinking_live)
            } else {
                tr!(thought)
            };
            let style = if *streaming {
                theme::glow()
            } else {
                theme::quiet()
            };
            lines.push(Line::default());
            let header = vec![
                Span::styled(if open { "▾ " } else { "▸ " }, style),
                Span::styled(
                    tr!(thought_size, state = state, size = tokens_short(size)),
                    style,
                ),
            ];
            if *streaming {
                // While the model thinks, a small flame flickers in front of it, slowly — two rows, so it reads as a
                // flame and not as a dot; the thought goes back to one row once it is done.
                let mut glimm = crate::glimm::lines(
                    crate::glimm::Size::Mini,
                    crate::glimm::Mood::Working,
                    frame,
                );
                // The reader mode has no flame: the thought's one row.
                if glimm.is_empty() {
                    lines.push(Line::from(header));
                } else {
                    let lower = glimm.pop().unwrap_or_default();
                    let mut upper = glimm.pop().unwrap_or_default();
                    upper.spans.push(Span::raw(" "));
                    upper.spans.extend(header);
                    lines.push(upper);
                    lines.push(lower);
                }
            } else {
                lines.push(Line::from(header));
            }
            if open {
                let italic = theme::quiet().add_modifier(Modifier::ITALIC);
                for row in wrap(text.trim(), width.saturating_sub(2)) {
                    lines.push(Line::from(vec![Span::raw("  "), Span::styled(row, italic)]));
                }
            }
        }
        Kind::Agent(agent) => tile(agent, width, &mut lines),
        Kind::Picture(image) => {
            // Half the stream wide at most, ten rows high, its proportions kept; inline drawing (T46b) stands on
            // the same cells.
            if crate::pictures::drawing() != crate::pictures::Drawing::Off {
                let room = u16::try_from(width.saturating_sub(2) / 2)
                    .unwrap_or(u16::MAX)
                    .max(8);
                let (columns, rows) = crate::pictures::fitted(
                    image.width,
                    image.height,
                    room,
                    crate::pictures::MOST_ROWS,
                );
                if let Some(rows) = crate::pictures::lines(image, columns, rows) {
                    for mut row in rows {
                        row.spans.insert(0, Span::raw("  "));
                        lines.push(row);
                    }
                }
            }
            lines.push(Line::from(Span::styled(
                format!("  {}", image.label()),
                theme::quiet(),
            )));
        }
        Kind::Note { text, tone } => {
            let style = match tone {
                Tone::Quiet => theme::quiet(),
                Tone::Fault => Style::default().fg(theme::c().fault),
                Tone::Fine => Style::default().fg(theme::c().fine),
                Tone::Heed => theme::glow(),
            };
            // A note may name keys: the reader mode says them in words (TUI9b).
            for row in wrap(&crate::motion::spoken_keys(text), width - 2) {
                lines.push(Line::from(vec![
                    Span::styled("· ", theme::quiet()),
                    Span::styled(row, style),
                ]));
            }
        }
    }
    lines
}

/// A subagent as a tile of three rows: its task and state on the frame, what it does (or answered) inside, its calls
/// and tokens below. The frame glows while it works, as everything that lives does.
fn tile(agent: &Agent, width: usize, lines: &mut Vec<Line<'static>>) {
    let (state, style) = match agent.state {
        ToolState::Running => (tr!(agent_running), theme::glow()),
        ToolState::Done => (tr!(agent_done), theme::quiet()),
        ToolState::Failed => (tr!(agent_stopped), Style::default().fg(theme::c().fault)),
        ToolState::Refused => (tr!(agent_refused), Style::default().fg(theme::c().fault)),
    };
    // Inside the side rows' "│ " and " │".
    let inner = width.saturating_sub(4).max(4);
    let state = match &agent.gate {
        Some(gate) if agent.state == ToolState::Refused => format!("{state}: {}", clean(gate)),
        _ => state.to_owned(),
    };
    // Too narrow for a frame: one line.
    if width < 24 {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            cut(&format!("⬡ {} · {state}", clean(&agent.task)), width),
            style,
        )));
        return;
    }
    let state = cut(&state, inner / 2);
    // The top row: "╭─ ⬡ ", the task, " ─… ", the state, " ─╮" — ten cells of frame and at least one of line.
    let task = cut(
        &clean(&agent.task),
        width.saturating_sub(state.width() + 11).max(1),
    );
    let fill = width.saturating_sub(task.width() + state.width() + 10);
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled("╭─ ", style),
        Span::styled("⬡ ", style),
        Span::styled(task, theme::strong()),
        Span::styled(format!(" {} ", "─".repeat(fill)), style),
        Span::styled(state, style),
        Span::styled(" ─╮", style),
    ]));
    let live = if agent.live.is_empty() {
        agent
            .answer
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .to_owned()
    } else {
        agent.live.clone()
    };
    let live = cut(&clean(&live).replace('\n', " "), inner);
    let pad = inner.saturating_sub(live.width());
    lines.push(Line::from(vec![
        Span::styled("│ ", style),
        Span::styled(live, theme::quiet()),
        Span::raw(" ".repeat(pad)),
        Span::styled(" │", style),
    ]));
    let calls = crate::texts::t().agent_calls.of(agent.calls as u64);
    let model = agent
        .model
        .as_deref()
        .map(|model| format!(" · {}", clean(model)))
        .unwrap_or_default();
    let stats = cut(
        &format!(
            "{}{model}",
            tr!(
                agent_stats,
                calls = calls,
                input = tokens_short(agent.input_tokens as usize),
                output = tokens_short(agent.output_tokens as usize)
            )
        ),
        inner,
    );
    let hint = if inner >= stats.width() + 16 {
        tr!(tile_opens)
    } else {
        ""
    };
    // The bottom row: "╰─ ", the counts, " ─… ", the hint, " ─╯".
    let fill = width.saturating_sub(stats.width() + hint.width() + 8);
    lines.push(Line::from(vec![
        Span::styled("╰─ ", style),
        Span::styled(stats, theme::quiet()),
        Span::styled(format!(" {} ", "─".repeat(fill)), style),
        Span::styled(hint, theme::quiet()),
        Span::styled(" ─╯", style),
    ]));
}

/// A count as people read it: 840, 1.2k.
pub(crate) fn tokens_short(count: usize) -> String {
    if count < 1000 {
        count.to_string()
    } else {
        format!("{}k", glut::lang::decimal(count as f64 / 1000.0, 1))
    }
}

/// `text` broken into rows of at most `width` cells: at spaces where it can (the spaces at a break dropped), inside a
/// word where it must. What a model or a tool sent is shown, never obeyed: control characters become spaces, and
/// characters that turn or hide text (bidi overrides, zero-width marks) are left out — an asked command must read as
/// it would run.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut rows = Vec::new();
    for line in clean(text).split('\n') {
        let mut row = String::new();
        let mut used = 0;
        // After a break the spaces that caused it are dropped; a line's own indentation is kept.
        let mut broke = false;
        for part in parts(line) {
            let part_width: usize = part.chars().map(cell_width).sum();
            if part.starts_with(' ') {
                if broke && used == 0 {
                    continue;
                }
                if used + part_width > width {
                    rows.push(std::mem::take(&mut row).trim_end().to_owned());
                    used = 0;
                    broke = true;
                    continue;
                }
                row.push_str(part);
                used += part_width;
                continue;
            }
            if used + part_width > width && used > 0 {
                rows.push(std::mem::take(&mut row).trim_end().to_owned());
                used = 0;
                broke = true;
            }
            for c in part.chars() {
                let cells = cell_width(c);
                if used + cells > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                    broke = true;
                }
                row.push(c);
                used += cells;
            }
        }
        rows.push(row);
    }
    rows
}

/// `text` as it may reach the terminal: control characters (but the line break) as spaces, characters that reorder or
/// hide text left out.
pub fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !hides(*c))
        .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
        .collect()
}

/// Bidi controls and zero-width characters: they make what is shown differ from what is there.
pub(crate) fn hides(c: char) -> bool {
    matches!(
        c,
        '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}' | '\u{feff}'
    )
}

/// A character's cells.
fn cell_width(c: char) -> usize {
    c.width().unwrap_or(0)
}

/// `line` as runs of spaces and runs of everything else, in order — a run of Chinese or Japanese parted where a row
/// may break inside it (`linebreak::between`).
fn parts(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut in_space = None;
    let mut last = None;
    for (at, c) in line.char_indices() {
        let space = c == ' ';
        let breaks = !space
            && in_space == Some(false)
            && last.is_some_and(|before| crate::linebreak::between(before, c));
        if in_space.is_some_and(|was| was != space) || breaks {
            parts.push(&line[start..at]);
            start = at;
        }
        in_space = Some(space);
        last = Some(c);
    }
    if start < line.len() {
        parts.push(&line[start..]);
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_thinking_subagent_shows_how_much_it_thought_until_its_reply_ends() {
        let mut stream = Transcript::default();
        stream.take(&Event::ToolCall {
            turn: 1,
            id: "t1".into(),
            name: "task".into(),
            arguments: r#"{"description":"write a poem","prompt":"p"}"#.into(),
        });
        let sub = |event: Event| Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(event),
        };
        let live = |stream: &Transcript| match &stream.blocks[0].kind {
            Kind::Agent(agent) => agent.live.clone(),
            other => panic!("{other:?}"),
        };
        for _ in 0..3 {
            stream.take(&sub(Event::ReasoningDelta {
                turn: 1,
                text: "x".repeat(2000),
            }));
        }
        assert!(live(&stream).contains("1.5k"), "{}", live(&stream));
        stream.take(&sub(Event::Usage {
            turn: 1,
            input_tokens: 10,
            cached_input_tokens: 0,
            output_tokens: 1500,
        }));
        stream.take(&sub(Event::TextDelta {
            turn: 2,
            text: "abcd".into(),
        }));
        assert!(
            live(&stream).ends_with("~1 tokens"),
            "counted anew: {}",
            live(&stream)
        );
    }

    #[test]
    fn an_answer_starts_at_a_dot_and_a_tool_says_how_it_went_by_its_dots_colour() {
        let mut stream = Transcript::default();
        stream.push(Kind::Answer {
            text: "first line\n\nsecond".into(),
            streaming: false,
        });
        let rows = render(&stream.blocks[0].kind, 40, Thinking::Folded, 0);
        let text = |row: &Line| {
            row.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };
        assert_eq!(text(&rows[1]), "● first line");
        assert!(
            rows[2..]
                .iter()
                .all(|row| text(row).is_empty() || text(row).starts_with("  "))
        );
        let tool = |state| Kind::Tool {
            id: "c".into(),
            name: "bash".into(),
            target: "ls".into(),
            state,
            gate: None,
            output: String::new(),
            unfolded: false,
            depth: 0,
        };
        let dot = |state| {
            render(&tool(state), 40, Thinking::Folded, 0)
                .into_iter()
                .flat_map(|row| row.spans)
                .find(|span| span.content.trim() == "●")
                .map(|span| span.style.fg)
        };
        assert_eq!(dot(ToolState::Done), Some(Some(theme::c().fine)));
        assert_eq!(dot(ToolState::Failed), Some(Some(theme::c().fault)));
    }

    #[test]
    fn a_subagent_is_a_tile_with_its_steps_in_its_own_stream() {
        let mut stream = Transcript::default();
        stream.take(&Event::ToolCall {
            turn: 1,
            id: "t1".into(),
            name: "task".into(),
            arguments: r#"{"description":"count the tests","prompt":"p"}"#.into(),
        });
        let sub = |event: Event| Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(event),
        };
        stream.take(&sub(Event::ToolCall {
            turn: 1,
            id: "s1".into(),
            name: "grep".into(),
            arguments: r#"{"pattern":"fn test"}"#.into(),
        }));
        stream.take(&sub(Event::Sub {
            depth: 2,
            task: None,
            event: Box::new(Event::Usage {
                turn: 1,
                input_tokens: 2000,
                cached_input_tokens: 0,
                output_tokens: 100,
            }),
        }));
        assert_eq!(stream.blocks.len(), 1);
        let Kind::Agent(agent) = &stream.blocks[0].kind else {
            panic!("a tile")
        };
        assert_eq!(
            (agent.calls, agent.input_tokens),
            (1, 2000),
            "a nested one's tokens count too"
        );
        assert_eq!(agent.live, "grep fn test");
        assert_eq!(agent.stream.blocks.len(), 1);
        let tile: Vec<String> = stream.blocks[0]
            .lines(50, Thinking::Folded, 0)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(tile.len(), 4);
        assert!(tile[1].starts_with("╭─ ⬡ count the tests") && tile[1].ends_with("running ─╮"));
        assert!(tile[2].contains("grep fn test"));
        assert!(tile[3].contains("1 call · 2.0k in · 100 out"));
        assert!(tile.iter().all(|row| row.width() <= 50), "{tile:#?}");
        // The turn ends with it still running: it stopped, and so did what ran inside it.
        stream.settle();
        let Kind::Agent(agent) = &stream.blocks[0].kind else {
            panic!("a tile")
        };
        assert_eq!(agent.state, ToolState::Failed);
        assert!(matches!(
            agent.stream.blocks[0].kind,
            Kind::Tool {
                state: ToolState::Failed,
                ..
            }
        ));
    }

    #[test]
    fn subagents_at_once_each_get_their_own_events_by_their_task_call() {
        let mut stream = Transcript::default();
        for (id, label) in [("t1", "first"), ("t2", "second")] {
            stream.take(&Event::ToolCall {
                turn: 1,
                id: id.into(),
                name: "task".into(),
                arguments: format!(r#"{{"description":"{label}","prompt":"p"}}"#),
            });
        }
        let sub = |task: &str, tool: &str| Event::Sub {
            depth: 1,
            task: Some(task.into()),
            event: Box::new(Event::ToolCall {
                turn: 1,
                id: "s1".into(),
                name: tool.into(),
                arguments: "{}".into(),
            }),
        };
        // Interleaved, and the first one's step after the second started: each to its own tile.
        stream.take(&sub("t2", "grep"));
        stream.take(&sub("t1", "read_file"));
        stream.take(&sub("t1", "glob"));
        let live: Vec<(String, u32)> = stream
            .agents()
            .iter()
            .map(|(_, agent)| (agent.live.clone(), agent.calls))
            .collect();
        assert_eq!(
            live,
            [("glob {}".to_owned(), 2), ("grep {}".to_owned(), 1)],
            "{live:?}"
        );
    }

    #[test]
    fn a_subagent_event_with_no_tile_running_stands_indented_as_before() {
        let mut stream = Transcript::default();
        stream.take(&Event::Sub {
            depth: 1,
            task: None,
            event: Box::new(Event::ToolCall {
                turn: 1,
                id: "s1".into(),
                name: "grep".into(),
                arguments: "{}".into(),
            }),
        });
        assert!(matches!(stream.blocks[0].kind, Kind::Tool { depth: 1, .. }));
    }

    #[test]
    fn wrapping_breaks_at_spaces_counts_cells_and_never_passes_the_width() {
        assert_eq!(wrap("the quick brown fox", 10), ["the quick", "brown fox"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap("日本語テキスト", 6), ["日本語", "テキス", "ト"]);
        assert_eq!(
            wrap("Glut 会读取文件。然后它运行测试。", 12),
            ["Glut 会读取", "文件。然后它", "运行测试。"],
            "a Chinese run breaks between its characters, never before 。"
        );
        assert_eq!(wrap("one\n\ntwo", 10), ["one", "", "two"]);
        assert_eq!(
            wrap("a\u{1b}[2Jb", 10),
            ["a [2Jb"],
            "an escape is shown, not sent"
        );
        for row in wrap("lorem ipsum dolor sit amet, consectetur 日本 adipiscing", 7) {
            assert!(row.width() <= 7, "{row:?}");
        }
    }

    #[test]
    fn a_turn_streams_into_one_answer_and_tools_unfold() {
        let mut stream = Transcript::default();
        stream.take(&Event::TextDelta {
            turn: 1,
            text: "Let me ".into(),
        });
        stream.take(&Event::TextDelta {
            turn: 1,
            text: "look.".into(),
        });
        stream.take(&Event::ToolCall {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            arguments: r#"{"path":"src/main.rs"}"#.into(),
        });
        stream.take(&Event::ToolResult {
            turn: 1,
            id: "c1".into(),
            name: "read_file".into(),
            ok: true,
            output: "fn main() {}".into(),
            truncated: false,
        });
        stream.take(&Event::AssistantText {
            turn: 2,
            text: "Done.".into(),
        });
        assert_eq!(stream.blocks.len(), 3);
        assert!(
            matches!(&stream.blocks[0].kind, Kind::Answer { text, streaming: false } if text == "Let me look.")
        );
        assert!(matches!(
            &stream.blocks[1].kind,
            Kind::Tool { target, state: ToolState::Done, .. } if target == "src/main.rs"
        ));
        assert_eq!(
            stream.blocks[1].lines(80, Thinking::Folded, 0).len(),
            1,
            "folded: one line"
        );
        stream.toggle(1);
        assert_eq!(
            stream.blocks[1].lines(80, Thinking::Folded, 0).len(),
            2,
            "unfolded: the result below"
        );
        assert!(matches!(&stream.blocks[2].kind, Kind::Answer { text, .. } if text == "Done."));
    }

    #[test]
    fn thinking_streams_into_one_block_shown_as_the_owner_chose_and_opened_by_hand() {
        let mut stream = Transcript::default();
        for piece in ["Let me ", "think."] {
            stream.take(&Event::ReasoningDelta {
                turn: 1,
                text: piece.into(),
            });
        }
        stream.take(&Event::TextDelta {
            turn: 1,
            text: "Answer.".into(),
        });
        assert_eq!(stream.blocks.len(), 2);
        assert!(matches!(
            &stream.blocks[0].kind,
            Kind::Thinking { text, streaming: false, .. } if text == "Let me think."
        ));
        let rows = |stream: &mut Transcript| {
            let thinking = stream.thinking;
            stream.blocks[0].lines(80, thinking, 0).len()
        };
        assert_eq!(
            rows(&mut stream),
            2,
            "folded: a blank line and the one line"
        );
        stream.toggle(0);
        assert_eq!(rows(&mut stream), 3, "opened by hand: the text below");
        stream.show_thinking(Thinking::Off);
        assert_eq!(rows(&mut stream), 0, "off: nothing");
        stream.show_thinking(Thinking::Open);
        assert_eq!(rows(&mut stream), 3);
        stream.toggle(0);
        assert_eq!(rows(&mut stream), 2, "in the open mode a click folds one");
    }

    #[test]
    fn a_refused_call_stays_refused_and_a_cut_turn_settles() {
        let mut stream = Transcript::default();
        for (id, name) in [("c1", "run_command"), ("c2", "edit_file")] {
            stream.take(&Event::ToolCall {
                turn: 1,
                id: id.into(),
                name: name.into(),
                arguments: "{}".into(),
            });
        }
        stream.take(&Event::Permission {
            turn: 1,
            id: "c1".into(),
            tool: "run_command".into(),
            target: "rm -rf /".into(),
            allowed: false,
            reason: "rule Bash(rm:*)".into(),
        });
        stream.take(&Event::ToolResult {
            turn: 1,
            id: "c1".into(),
            name: "run_command".into(),
            ok: false,
            output: "refused".into(),
            truncated: false,
        });
        stream.settle();
        assert!(matches!(
            &stream.blocks[0].kind,
            Kind::Tool {
                state: ToolState::Refused,
                gate: Some(_),
                ..
            }
        ));
        assert!(matches!(
            &stream.blocks[1].kind,
            Kind::Tool {
                state: ToolState::Failed,
                ..
            }
        ));
    }
}
