//! The loop: ask the model, run the tools it calls, hand back their results, until it answers without a call.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::codemode;
use crate::compact::{self, COMPACTION_FAILED, KEEP_RECENT_TOKENS};
use crate::config::{Budget, MIN_CONTEXT_WINDOW, RunConfig};
use crate::error::HarnessError;
use crate::event::{EVENT_RESULT_CHARS, Event, EventSink, FinishReason, NudgeKind};
use crate::hooks::{self, HookEvent, HookOutcome};
use crate::journal::{self, Journal, Line, Recorded};
use crate::model::{ChatModel, Message, Role, ToolCall, ToolSpec, Usage};
use crate::permissions::{Access, Answer, Gate, Mode};
use crate::planning;
use crate::prompt::{subagent_prompt, system_prompt};
use crate::refusals::{Ledger, Refusal};
use crate::results::{self, ResultStore};
use crate::snapshot::{self, Snapshots};
use crate::tools::shell::{Session, ShellOptions};
use crate::tools::{StopFlag, Toolbox, glut_files};

/// The most tool calls of one model reply that are run. Each call can write a file; a reply asking for hundreds is a
/// model gone wrong, not a plan. The rest are answered with an error, so every call still gets its reply (the API
/// wants one per call id) and the model learns to ask for fewer at once.
pub const MAX_CALLS_PER_TURN: usize = 16;
/// Tool calls without `todo_write` while its list has items open before Glut reminds the model of it (T73).
const REMIND_EVERY: u32 = 8;

/// A way for the caller to call a run off from elsewhere (a Stop button, a closing pane). Cheap to clone; every clone
/// cancels the same run.
#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
    /// The subagents running under this run, by their `task` call (TUI T40): each can be called off alone.
    subagents: Arc<std::sync::Mutex<HashMap<String, Cancel>>>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Calls the run off: a model call or a tool in progress is dropped, and the run ends as `cancelled`.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
        for subagent in self.subagents_now() {
            subagent.cancel();
        }
    }

    /// Calls off the subagent of the `task` call `id` alone (TUI T40): it ends as `cancelled`, its call answers that,
    /// the run and the others go on. `false` when no such subagent runs.
    pub fn cancel_subagent(&self, id: &str) -> bool {
        let found = self
            .subagents
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(id)
            .cloned();
        found.inspect(Cancel::cancel).is_some()
    }

    /// A cancel of its own for the subagent of `id`, called off with this one; dropped from it by [`Self::forget`].
    fn subagent(&self, id: &str) -> Cancel {
        let own = Cancel::new();
        self.subagents
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(id.to_owned(), own.clone());
        // A cancel just before it was listed is not missed.
        if self.is_cancelled() {
            own.cancel();
        }
        own
    }

    fn forget(&self, id: &str) {
        self.subagents
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .remove(id);
    }

    fn subagents_now(&self) -> Vec<Cancel> {
        self.subagents
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .values()
            .cloned()
            .collect()
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }

    async fn cancelled(&self) {
        // Registered before the flag is read, so a cancel between the two is not missed.
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// Cancels the run on Ctrl-C, on SIGTERM (a supervisor ending the process) and on SIGHUP (its terminal closed), so the
/// run still ends with its last event and its toolbox is dropped — which ends every background job of the shell.
/// For a program that runs the harness as its main work (`glut`, the CLI); call it from inside the tokio runtime.
pub fn cancel_on_signals(cancel: &Cancel) {
    use tokio::signal::unix::{SignalKind, signal};
    let cancel = cancel.clone();
    tokio::spawn(async move {
        let (Ok(mut term), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
        ) else {
            // Without the handlers the process ends the default way; Ctrl-C alone is still watched.
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel.cancel();
            }
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = hup.recv() => {}
        }
        cancel.cancel();
    });
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutcome {
    pub reason: FinishReason,
    /// The model's last text, the answer when it `Answered`.
    pub answer: Option<String>,
    pub turns: u32,
    pub usage: Usage,
    /// How many calls the run had refused (#112), so a parent counts its subagent's toward its own.
    pub refusals: u32,
    /// The call whose cached head the next turn reuses (TUI T36e): the run's first call — or the first after its last
    /// compaction. Later calls carry this run's thinking, which the next turn leaves out, so their cache entries are
    /// no head the next turn begins with. `None` for a run that called no model.
    pub warm: Option<Warm>,
}

/// A run's model call, to be sent again to keep its cache: how many messages of the conversation it held (the run's
/// returned conversation begins with them) and the tools on offer then.
#[derive(Debug, Clone, PartialEq)]
pub struct Warm {
    pub length: usize,
    pub tools: Vec<ToolSpec>,
}

impl RunOutcome {
    /// A subagent whose outcome never came back: failed, nothing spent that the parent could count.
    fn lost() -> Self {
        RunOutcome {
            reason: FinishReason::Failed,
            answer: None,
            turns: 0,
            usage: Usage::default(),
            refusals: 0,
            warm: None,
        }
    }
}

/// Runs `prompt` with `model` and `tools` under `config`, reporting each step to `sink`.
///
/// Never fails as a function: a failing model call ends the run as [`FinishReason::Failed`] after an
/// [`Event::Error`], and a failing tool goes back to the model as text.
pub async fn run<M: ChatModel>(
    config: &RunConfig,
    model: &M,
    tools: &Toolbox,
    prompt: &str,
    sink: &mut dyn EventSink,
    cancel: &Cancel,
) -> RunOutcome {
    let opening = Opening::Fresh {
        earlier: Vec::new(),
        prompt,
    };
    run_inner(config, model, tools, opening, sink, cancel, 0, None)
        .await
        .0
}

/// A run that goes on with an earlier conversation (`docs/plans/harness-choice.md`, CP4): `earlier` is what a
/// previous call of this function returned — empty for a new conversation — and `prompt` the next user message.
/// The system prompt is written anew (today's date, the caller's instructions as they are now), the rest is kept
/// as it was; `SessionStart` fires for a new conversation only. Returns the outcome and the whole conversation,
/// this turn included, for the caller to keep for the next one.
pub async fn run_continuing<M: ChatModel>(
    config: &RunConfig,
    model: &M,
    tools: &Toolbox,
    earlier: Vec<Message>,
    prompt: &str,
    sink: &mut dyn EventSink,
    cancel: &Cancel,
) -> (RunOutcome, Vec<Message>) {
    run_inner(
        config,
        model,
        tools,
        Opening::Fresh { earlier, prompt },
        sink,
        cancel,
        0,
        None,
    )
    .await
}

/// Resumes the run `run` from its journal in `config.journal`'s folder (durable runs, `docs/plans/durable.md`): the
/// conversation as it was saved, the steps and tokens used so far counted on, the time limit afresh (D6). A tool call
/// that was under way when the run stopped runs again only when it only reads (D4/D7); any other gets an
/// "interrupted" result and the model decides. A call that had not started yet runs as it would have. The caller
/// builds `config`, `model` and `tools` as for the run at its start — from the journal's `meta`, which
/// [`journal::open`] hands back for that — and the journal goes on being written; a finished run deletes it.
///
/// # Errors
///
/// No journal folder in `config`, no such unfinished run, the run still going, or a journal that cannot be read.
pub async fn resume<M: ChatModel>(
    config: &RunConfig,
    model: &M,
    tools: &Toolbox,
    run: &str,
    sink: &mut dyn EventSink,
    cancel: &Cancel,
) -> Result<(RunOutcome, Vec<Message>), String> {
    let dir = config
        .journal
        .as_ref()
        .map(|spec| spec.dir.clone())
        .ok_or("this run has no journal folder")?;
    // Opening waits a moment for the lock (`journal::open`): on a blocking thread, not the run's.
    let (held, recorded) = {
        let run = run.to_owned();
        tokio::task::spawn_blocking(move || journal::open(&dir, &run))
            .await
            .map_err(|err| format!("the journal could not be opened: {err}"))??
    };
    // Lost before even its task was saved: there is nothing to go on from.
    if !recorded.messages.iter().any(|m| m.role == Role::User) {
        return Err(format!(
            "the run {run} was lost before its task was saved: start it anew"
        ));
    }
    let opening = Opening::Resumed {
        recorded: Box::new(recorded),
        journal: held,
    };
    Ok(run_inner(config, model, tools, opening, sink, cancel, 0, None).await)
}

/// How a run begins: anew with a prompt (after an earlier conversation, maybe), or from its journal.
enum Opening<'p> {
    Fresh {
        earlier: Vec<Message>,
        prompt: &'p str,
    },
    Resumed {
        recorded: Box<Recorded>,
        journal: Journal,
    },
}

/// The tools the loop answers itself that only read or note (D4): an interrupted call of them may simply run again.
const LOOP_REPLAY_SAFE: [&str; 2] = [results::NAME, planning::TODO];

/// The loop at nesting `depth` (HS8, D3): depth 0 is the run itself — it offers the `task` tool, runs
/// `SessionStart` and speaks the caller's system prompt; depth ≥ 1 is a subagent, which gets the prompt of
/// `prompt::subagent_prompt` (D5), no `task` tool and no `SessionStart` (D4). Everything else — hooks, gate,
/// tools, events — is identical on every level.
#[allow(clippy::too_many_arguments)] // The loop's whole input; a struct for it would only rename the list.
async fn run_inner<M: ChatModel>(
    config: &RunConfig,
    model: &M,
    tools: &Toolbox,
    opening: Opening<'_>,
    sink: &mut dyn EventSink,
    cancel: &Cancel,
    depth: u32,
    sibling: Option<Sibling<'_>>,
) -> (RunOutcome, Vec<Message>) {
    let continuing = !matches!(&opening, Opening::Fresh { earlier, .. } if earlier.is_empty());
    let deadline = Instant::now() + config.timeout;
    // HS8, D1/D3: `task` exists at depth 0 only, and only for a run that offers subagents at all (D7) — a
    // subagent never sees the tool, so nesting deeper than one cannot start; and its name is in the run's
    // tool list from the first turn on.
    let offer_task = depth == 0 && config.subagents;
    // T37b: `glut_file` from the start when the turn begins with a maker skill or the conversation already loaded one
    // — the make-* flow asks, shows and writes over several turns, each of them a run of its own.
    let history = match &opening {
        Opening::Fresh { earlier, .. } => earlier.as_slice(),
        Opening::Resumed { recorded, .. } => recorded.messages.as_slice(),
    };
    let glut = config.glut_files || maker_loaded(history);
    let offer = Offer::of(config, tools, depth, glut);
    sink.emit(Event::Started {
        model: model.name().to_owned(),
        cwd: config.cwd.display().to_string(),
        tools: offer.specs.iter().map(|spec| spec.name.clone()).collect(),
        prices: config.budget.prices,
    });
    // Set when the run stops, so a tool still at work (a search over a large tree) stops too and none starts after.
    let stop = StopFlag::default();
    let mut run = Loop {
        config,
        model,
        tools,
        cancel,
        stop: &stop,
        deadline,
        depth,
        offer_task,
        hooks: RunHooks {
            session: HookSession::NotBuilt,
            config,
            // The run's identity for the hooks' payloads (D4) — there are no sessions yet, so one id per run.
            session_id: format!(
                "glut-{}-{}",
                std::process::id(),
                chrono::Local::now().timestamp_millis()
            ),
            deadline,
            stop: &stop,
            cancel,
            gate: tools.gate().cloned(),
        },
        messages: Vec::new(),
        last_call: None,
        usage: Usage::default(),
        answer: None,
        turns: 0,
        journal: None,
        journaled: 0,
        subagents_started: 0,
        results: match (&config.results, depth) {
            (Some(spec), 0) => ResultStore::persistent(spec.clone()),
            _ => ResultStore::new(store_tag()),
        },
        offer,
        kept: codemode::Kept::default(),
        // A turn before's list goes on (T73) — at depth 0 only: a subagent has a task of its own.
        todos: if depth == 0 && planning::open(&config.todo_list) {
            config.todo_list.clone()
        } else {
            Vec::new()
        },
        snapshots: SnapshotState::Unopened,
        diagnostics: None,
        chain: None,
        shown: std::collections::HashMap::new(),
        refusals: Ledger::new(config.refusals),
        refused_now: Vec::new(),
        subagent_refusals: 0,
        window: config
            .compaction
            .context_window
            .filter(|tokens| *tokens >= MIN_CONTEXT_WINDOW)
            .map_or(Window::Unasked, Window::Known),
        last_prompt: None,
        compacted_at: None,
        compacted_size: None,
        sibling,
        read_at: HashMap::new(),
        paid_usd: 0.0,
        since_todo: 0,
        reminded_at: 0,
        updating_todos: false,
        nudged: false,
    };
    let mut resumed_calls = Vec::new();
    match opening {
        Opening::Fresh { earlier, prompt } => {
            run.messages = opening_messages(
                config,
                model.name(),
                earlier,
                prompt,
                depth,
                &tools.folders().prompt(),
            );
            if depth == 0
                && let Some(spec) = &config.journal
            {
                match Journal::create(spec, &config.cwd, model.name()) {
                    Ok(journal) => run.journal = Some(journal),
                    Err(reason) => sink.emit(Event::Error {
                        message: format!(
                            "no journal for this run (it cannot be resumed): {reason}"
                        ),
                    }),
                }
            }
        }
        Opening::Resumed { recorded, journal } => {
            let recorded = *recorded;
            // The system prompt is written anew (today's date, the instructions as they are now); the rest is kept.
            let mut messages = vec![
                opening_messages(
                    config,
                    model.name(),
                    Vec::new(),
                    "",
                    depth,
                    &tools.folders().prompt(),
                )
                .remove(0),
            ];
            // Only the opening system prompt is replaced; a SessionStart context behind it stays (HS7).
            let mut saved = recorded.messages.into_iter().peekable();
            if saved.peek().is_some_and(|m| m.role == Role::System) {
                saved.next();
            }
            messages.extend(saved);
            run.journaled = messages.len();
            run.messages = messages;
            run.turns = recorded.turns;
            run.usage = recorded.usage;
            run.journal = Some(journal);
            resumed_calls = recorded.open_calls;
        }
    }
    // T58: numbers go on above the conversation's own placeholders, and the instructions (a project's `AGENTS.md`)
    // are masked like a file read — the same values the same placeholders, so the cache holds.
    if let Some(masks) = &config.masks {
        masks.continue_after(
            run.messages
                .iter()
                .filter_map(|message| message.content.as_deref()),
        );
        if let Some(system) = run
            .messages
            .first_mut()
            .filter(|first| first.role == Role::System)
            && let Some(text) = &system.content
        {
            let masked = masks.mask(text, "the instructions");
            if !masked.hidden.is_empty() {
                system.content = Some(masked.text);
            }
        }
    }
    // SessionStart first: its context goes in behind the system prompt, and the journal must hold it there.
    if depth == 0 && !continuing {
        run.session_start(sink).await;
    }
    run.flush_journal(sink);
    let reason = match run.settle_resumed(resumed_calls, sink).await {
        Err(ending) => ending,
        // A run that had already answered when it stopped needs no further step.
        Ok(()) if run.answered_before(sink) => FinishReason::Answered,
        Ok(()) => run.drive(sink).await,
    };
    stop.store(true, Ordering::SeqCst);
    let refusals = run.refusals.total();
    let Loop {
        usage,
        answer,
        turns,
        messages,
        journal,
        last_call,
        ..
    } = run;
    // A run that ended — answered, failed, cancelled, out of steps — is no unfinished one (D8). Only a run the process
    // lost keeps its journal: then this line is never reached.
    if let Some(journal) = journal {
        journal.finish();
    }
    // Only an answer is one: the text of a run that stopped is a remark on the way, not its result.
    let answer = answer.filter(|_| reason == FinishReason::Answered);
    sink.emit(Event::Finished {
        reason,
        turns,
        usage,
    });
    let outcome = RunOutcome {
        reason,
        answer,
        turns,
        usage,
        refusals,
        warm: last_call,
    };
    (outcome, messages)
}

/// Where a result was seen, for `/secrets` (T58): the tool and its file, address or pattern — of a command only its
/// program, since the rest may hold what is hidden, and this text goes back to the model in a refusal.
fn seen_at(call: &ToolCall) -> String {
    let arguments: serde_json::Value =
        serde_json::from_str(&call.function.arguments).unwrap_or_default();
    let field = |key: &str| arguments.get(key).and_then(serde_json::Value::as_str);
    let target = field("path")
        .or_else(|| field("url"))
        .or_else(|| field("pattern"))
        .map(|target| target.chars().take(60).collect::<String>())
        .or_else(|| {
            field("command")
                .and_then(|command| command.split_whitespace().next())
                .map(str::to_owned)
        });
    match target {
        Some(target) => format!("{} {target}", call.function.name),
        None => call.function.name.clone(),
    }
}

/// The most a file may hold for Glut to read it before a write puts a hidden value back (T58).
const MOST_HELD_BYTES: u64 = 8 * 1024 * 1024;

/// What the file a write names holds now (T58) — the file itself, its links followed, inside the folder only, a
/// plain file not too large; anything else holds nothing, so no hidden value goes into it.
fn held_text(cwd: &Path, beside: &crate::folders::Folders, path: &str) -> String {
    // T60c: a file in a folder beside the working one holds what it holds there, and only there.
    let (root, full) = match beside.place(path) {
        Some((folder, _)) => (folder.real, beside.full_path(path).unwrap_or_default()),
        None => (cwd.to_path_buf(), lexical(cwd, path)),
    };
    let (Ok(root), Ok(real)) = (root.canonicalize(), full.canonicalize()) else {
        return String::new();
    };
    let plain =
        std::fs::metadata(&real).is_ok_and(|meta| meta.is_file() && meta.len() <= MOST_HELD_BYTES);
    if !real.starts_with(&root) || !plain {
        return String::new();
    }
    std::fs::read_to_string(&real).unwrap_or_default()
}

/// The conversation a run starts with: its system prompt — the caller's instructions at depth 0, the
/// subagent's own prompt below (D5) — then the `earlier` conversation without its old system prompt, and the
/// user's message.
///
/// `beside`: what the model is told of the folders beside the working one (T60c), after everything else so a folder
/// added later keeps the cached start of the prompt.
fn opening_messages(
    config: &RunConfig,
    model: &str,
    earlier: Vec<Message>,
    prompt: &str,
    depth: u32,
    beside: &str,
) -> Vec<Message> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let system = if depth == 0 {
        system_prompt(
            &config.cwd,
            &today,
            model,
            config.instructions.as_deref(),
            config.todos,
        )
    } else {
        // A named agent's own text follows the subagent's frame (T37a); a plain subagent has none (D5).
        let frame = subagent_prompt(&config.cwd, &today);
        match config
            .instructions
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            Some(agent) => format!("{frame}\n\n{agent}"),
            None => frame,
        }
    };
    let system = if beside.is_empty() {
        system
    } else {
        format!("{system}\n\n{beside}")
    };
    let mut messages = vec![Message::system(system)];
    let mut earlier = earlier.into_iter().peekable();
    // The opening system message is written anew; a `SessionStart` context behind it (HS7) is kept.
    if earlier
        .peek()
        .is_some_and(|first| first.role == Role::System)
    {
        earlier.next();
    }
    // E4: the earlier turns' reasoning is never sent again, so the conversation does not carry it on either.
    messages.extend(earlier.map(|mut message| {
        message.forget_reasoning();
        message
    }));
    // The list a turn before left, shown where the model reads the owner's words (T73) — only where the conversation
    // no longer holds it (a new session's first turn, a compaction): a list shown at every turn would pile up.
    let held = messages.iter().any(|message| {
        message
            .tool_calls
            .iter()
            .any(|call| call.function.name == planning::TODO)
    });
    let carried = (depth == 0 && config.todos && !held)
        .then(|| planning::carried(&config.todo_list))
        .flatten();
    let mut asked = Message::user(match carried {
        Some(list) => format!("{prompt}\n\n{list}"),
        None => prompt.to_owned(),
    });
    // The owner's images go with the owner's prompt — a subagent's prompt is its parent's words alone.
    if depth == 0 {
        asked.images.clone_from(&config.prompt_images);
    }
    messages.push(asked);
    messages
}

/// The most bytes of images one request carries (T45), as base64 grows them: the newest first, the older ones said
/// by their line. Anthropic takes at most 32 MB a request, and every image goes with every step.
const MOST_SENT_IMAGE_BYTES: u64 = 16 * 1024 * 1024;
/// The most images one request carries, the newest.
const MOST_SENT_IMAGES: usize = 8;

/// The conversation as it is sent (T45): to a model that sees no images, none — each said by its line —; to any
/// other, the newest images within [`MOST_SENT_IMAGES`] and [`MOST_SENT_IMAGE_BYTES`], the older by their line. A
/// call beside the loop (a TUI's side question) sends what the loop would.
pub fn sendable(messages: &[Message], sees: Option<bool>) -> std::borrow::Cow<'_, [Message]> {
    if !messages.iter().any(|message| !message.images.is_empty()) {
        return std::borrow::Cow::Borrowed(messages);
    }
    let mut room = if sees == Some(false) {
        0
    } else {
        MOST_SENT_IMAGES
    };
    let mut bytes = 0u64;
    let mut sent = messages.to_vec();
    for message in sent.iter_mut().rev() {
        let mut kept = Vec::new();
        let mut said = Vec::new();
        for image in message.images.drain(..).rev() {
            let base64 = image.bytes.div_ceil(3) * 4;
            if room > 0 && bytes + base64 <= MOST_SENT_IMAGE_BYTES {
                room -= 1;
                bytes += base64;
                kept.push(image);
            } else {
                said.push(image.label());
            }
        }
        kept.reverse();
        message.images = kept;
        if !said.is_empty() {
            said.reverse();
            let text = message.content.get_or_insert_with(String::new);
            text.push_str(&format!("\n{} (not sent again)", said.join(" ")));
        }
    }
    std::borrow::Cow::Owned(sent)
}

/// How the model's answer turned out once the Stop hooks had their say (HS7, D3).
enum Ending {
    /// The answer stands: the run is done.
    Done,
    /// A hook refused it; its reason went back to the model, which takes another turn.
    Refused,
}

/// One run at one nesting level while its loop goes on: what it was given, and what it keeps from turn to
/// turn — the conversation, the tokens spent, the last text, the subagents started.
struct Loop<'a, M: ChatModel> {
    config: &'a RunConfig,
    model: &'a M,
    tools: &'a Toolbox,
    cancel: &'a Cancel,
    stop: &'a StopFlag,
    deadline: Instant,
    depth: u32,
    offer_task: bool,
    hooks: RunHooks<'a>,
    messages: Vec<Message>,
    /// The first model call since the run began or last compacted: how many messages it held and the tools it
    /// offered (T36e, [`Warm`]).
    last_call: Option<Warm>,
    usage: Usage,
    /// The last reply's text only: a remark beside earlier tool calls is no answer to the task.
    answer: Option<String>,
    turns: u32,
    /// HS8: the subagents this run started, counted against `config.max_subagents`.
    subagents_started: u32,
    /// Glut lean, N2: the whole texts of results too long for the conversation.
    results: ResultStore,
    /// What the model is offered, and what a script can find; built anew when the mode changes (`exit_plan`).
    offer: Offer,
    /// Glut 2, A: what the run's scripts `store()`.
    kept: codemode::Kept,
    /// Glut 2, D: the run's task list as `todo_write` last sent it.
    todos: Vec<planning::TodoItem>,
    /// Glut 2, D: the work folder's snapshots of this run, opened at its first change.
    snapshots: SnapshotState,
    /// Glut 2, D: the language servers this run started for its edits, at the first one.
    diagnostics: Option<Arc<std::sync::Mutex<crate::diagnostics::Diagnostics>>>,
    /// Durable runs: the run's journal, when it keeps one (depth 0 only).
    journal: Option<Journal>,
    /// How many of `messages` the journal holds.
    journaled: usize,
    /// Glut lean, N7: the last chain's answer as the tool gave it and where its parts lie — taken by that call's
    /// [`Self::run_call`], and only while its answer is still that text (a hook may have replaced it).
    chain: Option<ChainAnswer>,
    /// The images a call's answer shows (T45), by the call's id, until its answer joins the conversation.
    shown: std::collections::HashMap<String, Vec<Vec<u8>>>,
    /// #112: what the run had refused, and when that ends it.
    refusals: Ledger,
    /// The current call's refusals judged outside [`Self::perform`] (the `task` judgement), taken by it.
    refused_now: Vec<Refusal>,
    /// The refusals of the subagent the current call started, taken by [`Self::perform`] (R5).
    subagent_refusals: u32,
    /// #111: what the run knows of its model's context window.
    window: Window,
    /// The provider's prompt tokens of the last model call, and how many messages that prompt had (K1).
    last_prompt: Option<(u64, usize)>,
    /// The conversation's length after the last compaction (or a failed one): not compacted again before it grew.
    compacted_at: Option<usize>,
    /// The prompt's estimated size right after the last compaction.
    compacted_size: Option<u64>,
    /// A subagent run beside others (TUI T40): the batch it shares a budget, the paths' locks and their writes with.
    sibling: Option<Sibling<'a>>,
    /// The batch's version of each path at this run's last read or write of it, for a write after another's.
    read_at: HashMap<PathBuf, u64>,
    /// What its subagents on models of their own cost beyond their tokens at this run's prices (T37a).
    paid_usd: f64,
    /// Tool calls answered since the last `todo_write` (T73): the reminder's count.
    since_todo: u32,
    /// The step Glut last reminded the model of its list in, and whether the step under way sends `todo_write`
    /// itself: one reminder a step at most, none to a step that updates the list anyway (MiMo, a reminder thrice in
    /// one batch of sixteen calls, sent the same batch again and again).
    reminded_at: u32,
    updating_todos: bool,
    /// Whether the run already asked its model to go on (T73): once per run.
    nudged: bool,
}

/// What a run knows of its model's context window (#111, K2).
#[derive(Debug, Clone, Copy)]
enum Window {
    /// Not known yet: the provider is asked once the conversation is long enough to matter.
    Unasked,
    Known(u32),
    /// The provider's list said nothing: only its "too long" compacts (K4).
    Unknown,
}

/// From how many estimated tokens on the run asks the provider for the window: below it no model's is near.
const ASK_WINDOW_FROM: u64 = 16_384;
/// The share of the window kept free (K1, the measure Opencode and Pi chose), and at least [`MIN_RESERVE_TOKENS`].
const RESERVE_PERCENT: u64 = 15;
const MIN_RESERVE_TOKENS: u64 = 16_384;
/// The answer's room when the run sets no `max_output_tokens`.
const DEFAULT_OUTPUT_RESERVE: u64 = 8_192;

/// The prompt size from which a run with a `window`-token model compacts (K1). Each reserve takes at most a quarter
/// of the window, so a small one (a local model's 8k) still compacts from half of it — not from nothing at every step.
/// Public for a meter that shows where compaction will come (TUI T16).
pub fn compact_from(window: u32, max_output_tokens: Option<u32>) -> u64 {
    let window = u64::from(window);
    let quarter = window / 4;
    let reserve = (window * RESERVE_PERCENT / 100)
        .max(MIN_RESERVE_TOKENS)
        .min(quarter);
    let output = max_output_tokens
        .map_or(DEFAULT_OUTPUT_RESERVE, u64::from)
        .min(quarter);
    window - reserve - output
}

/// How much of the newest conversation a compaction keeps (K3): at most half of the room before the limit, so the
/// summary and the next steps fit beside it.
fn keep_for(window: Option<u32>, max_output_tokens: Option<u32>) -> u64 {
    window.map_or(KEEP_RECENT_TOKENS, |window| {
        KEEP_RECENT_TOKENS.min(compact_from(window, max_output_tokens) / 2)
    })
}

/// A `bash` chain's answer and its parts (N7).
struct ChainAnswer {
    call: String,
    text: String,
    parts: Vec<crate::tools::Part>,
}

/// Where a run's snapshots stand.
enum SnapshotState {
    Unopened,
    Open(Box<Taken>),
    /// They could not be taken (no git, no store): the run goes on without, and says so once.
    Off,
}

/// The snapshots a run takes: the working folder's, and those of the folders beside it (T60c), each in a store of its
/// own under the run's id — the working folder's record names them, so an undo puts them back together.
struct Taken {
    working: Snapshots,
    /// The folders beside it recorded at every step, as they stand in the session now.
    beside: Vec<Snapshots>,
    /// Every folder beside it that has a step of this run recorded — one taken away since included: an undo puts it
    /// back as well.
    recorded: Vec<PathBuf>,
    /// Folders whose store could not be opened or whose snapshot failed: said once, not tried at every step.
    given_up: Vec<PathBuf>,
    /// The snapshots of folders taken away while the run went on: one added again goes on with its own record, never
    /// with a new one that would write over its first steps.
    parked: Vec<Snapshots>,
}

impl Taken {
    fn new(working: Snapshots) -> Self {
        Taken {
            working,
            beside: Vec::new(),
            recorded: Vec::new(),
            given_up: Vec::new(),
            parked: Vec::new(),
        }
    }

    /// Records every folder before step `turn`: first those of `folders` beside the working one — one added while the
    /// session runs joins here, one taken away is recorded no more —, then the working folder, whose record names
    /// them. The working folder's new commit, and what could not be recorded beside it, said once.
    ///
    /// # Errors
    ///
    /// The working folder's snapshot failing: the run then goes without any.
    fn take(
        &mut self,
        root: &Path,
        run: &str,
        folders: &[PathBuf],
        turn: u32,
    ) -> Result<(Option<String>, Vec<String>), String> {
        let mut notes = Vec::new();
        let (kept, away): (Vec<Snapshots>, Vec<Snapshots>) = std::mem::take(&mut self.beside)
            .into_iter()
            .partition(|snaps| folders.iter().any(|folder| snaps.work_tree() == folder));
        self.beside = kept;
        self.parked.extend(away);
        for folder in folders {
            let known = self.beside.iter().any(|snaps| snaps.work_tree() == folder);
            if known || self.given_up.contains(folder) {
                continue;
            }
            if let Some(at) = self
                .parked
                .iter()
                .position(|snaps| snaps.work_tree() == folder)
            {
                self.beside.push(self.parked.remove(at));
                continue;
            }
            match Snapshots::open(root, folder, run) {
                Ok(snaps) => self.beside.push(snaps),
                Err(reason) => {
                    notes.push(format!(
                        "no snapshots of {} (undo cannot put it back): {reason}",
                        folder.display()
                    ));
                    self.given_up.push(folder.clone());
                }
            }
        }
        let mut failed = Vec::new();
        for snaps in &mut self.beside {
            match snaps.take(turn) {
                Ok(_) => {
                    let folder = snaps.work_tree().to_path_buf();
                    if !self.recorded.contains(&folder) {
                        self.recorded.push(folder);
                    }
                }
                Err(reason) => {
                    notes.push(format!(
                        "no more snapshots of {} (undo cannot put it back from here on): {reason}",
                        snaps.work_tree().display()
                    ));
                    failed.push(snaps.work_tree().to_path_buf());
                }
            }
        }
        self.beside
            .retain(|snaps| !failed.iter().any(|folder| snaps.work_tree() == folder));
        self.given_up.extend(failed);
        self.working.set_beside(self.recorded.clone());
        let commit = self.working.take(turn)?;
        Ok((commit, notes))
    }
}

/// The tools a run offers the model, in the order they go out, and what a script finds in its catalog.
struct Offer {
    specs: Vec<ToolSpec>,
    codemode: bool,
    catalog: Arc<codemode::Catalog>,
}

impl Offer {
    /// The toolbox's offered tools — in code mode the MCP tools behind `codemode` (G3) unless the run keeps them
    /// direct (G7) — then the loop's own: `codemode`, `task` (depth 0, HS8), `read_full_result` (B), `todo_write`
    /// and, in Plan mode, `exit_plan` (D).
    fn of(config: &RunConfig, tools: &Toolbox, depth: u32, glut: bool) -> Self {
        let (mut direct, mut behind) = if config.mcp_behind_code {
            tools.split_specs()
        } else {
            (tools.specs(), Vec::new())
        };
        // T37b: Glut's own files only while a maker skill runs — from the start when the turn began with one — and
        // never in a subagent.
        let offers_glut = glut && depth == 0;
        if !offers_glut {
            direct.retain(|spec| spec.name != glut_files::NAME);
            behind.retain(|spec| spec.name != glut_files::NAME);
        }
        // T36c: the panes' tools only where the screen is split, and never to a subagent.
        let offers_panes = config.pane_tools && depth == 0;
        let pane_tool = |name: &str| crate::config::PANE_TOOLS.contains(&name);
        if !offers_panes {
            direct.retain(|spec| !pane_tool(&spec.name));
            behind.retain(|spec| !pane_tool(&spec.name));
        }
        // T60a: the loop's end only to a loop's own model.
        let offers_loop = config.loop_tool && depth == 0;
        if !offers_loop {
            direct.retain(|spec| spec.name != crate::config::LOOP_TOOL);
            behind.retain(|spec| spec.name != crate::config::LOOP_TOOL);
        }
        let codemode = config.codemode && !(direct.is_empty() && behind.is_empty());
        let catalog = Arc::new(codemode::Catalog {
            specs: direct.iter().chain(&behind).cloned().collect(),
        });
        let mut specs = if codemode {
            direct.clone()
        } else {
            let mut all = tools.specs();
            all.retain(|spec| offers_glut || spec.name != glut_files::NAME);
            all.retain(|spec| offers_panes || !pane_tool(&spec.name));
            all.retain(|spec| offers_loop || spec.name != crate::config::LOOP_TOOL);
            all
        };
        if codemode {
            specs.push(codemode::spec(&direct, &behind));
        }
        // T36c: not to a role that names no `task` — every call would be refused.
        let task_offered = tools
            .gate()
            .is_none_or(|gate| gate.offers(crate::task::NAME, &Access::Other));
        if depth == 0 && config.subagents && task_offered {
            specs.push(crate::task::spec(&config.agents));
        }
        // Offered from the first turn on, so the tool list — and with it the prompt's cached head — stays put.
        specs.push(results::spec());
        if config.todos {
            specs.push(planning::todo_spec());
        }
        if depth == 0 && tools.gate().is_some_and(|gate| gate.mode() == Mode::Plan) {
            specs.push(planning::exit_plan_spec());
        }
        Offer {
            specs,
            codemode,
            catalog,
        }
    }
}

/// Whether a `skill` call's `arguments` load one of the maker skills (T37b).
fn names_maker(arguments: &str) -> bool {
    serde_json::from_str::<Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("name")?
                .as_str()
                .map(|name| name.trim().to_owned())
        })
        .is_some_and(|name| glut_files::MAKERS.contains(&name.as_str()))
}

/// Whether `messages` hold a `skill` call that loaded a maker skill (T37b): the conversation is making something.
fn maker_loaded(messages: &[Message]) -> bool {
    messages
        .iter()
        .flat_map(|message| message.tool_calls.iter())
        .any(|call| {
            call.function.name == crate::skills::tool::NAME && names_maker(&call.function.arguments)
        })
}

/// How often a script may be found waiting with no call open before it counts as stuck: it waits on a promise nothing
/// can settle (there are no timers). A few polls, not one, so a call answered a moment ago gets to land first.
const STUCK_POLLS: u32 = 8;

/// How long building a named agent's own model may take (T37a): its provider's model list is fetched for it.
const MODEL_BUILD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Commands that only read, by their first word — what needs no snapshot before it runs (`date` in every step of a
/// skill must not record the folder each time). Anything else, a redirection, a nested shell or what the lexer cannot
/// read counts as a change.
const READ_ONLY_COMMANDS: [&str; 19] = [
    "ls", "cat", "head", "tail", "grep", "egrep", "fgrep", "rg", "wc", "date", "echo", "printf",
    "pwd", "which", "file", "stat", "du", "df", "basename",
];
/// Git's subcommands that only read.
const READ_ONLY_GIT: [&str; 6] = ["status", "log", "diff", "show", "rev-parse", "ls-files"];

/// Whether `command` only reads: every part a read-only command, no output redirection, nothing opaque or nested.
fn reads_only(command: &str) -> bool {
    let analysis = crate::permissions::command::analyze(command);
    !analysis.is_opaque()
        && analysis.writes.is_empty()
        && analysis.nested.is_empty()
        && analysis
            .parts
            .iter()
            .all(|part| match part.words.first().map(String::as_str) {
                Some("git") => part
                    .words
                    .get(1)
                    .is_some_and(|sub| READ_ONLY_GIT.contains(&sub.as_str())),
                Some(word) => READ_ONLY_COMMANDS.contains(&word),
                None => true,
            })
}

/// A tag no other run of this process — an earlier turn of the same chat included — gives its results.
fn store_tag() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:x}{n}",
        chrono::Local::now().timestamp_millis() % 0x10_0000
    )
}

impl<M: ChatModel> Loop<'_, M> {
    /// HS7, D3: SessionStart once, after `Started` and before the first model call — at depth 0 only (HS8, D4:
    /// it belongs to the run, not to each subagent). Its context becomes a system message right behind the
    /// harness's own; it can never block, and a run that ends while it works ends at its first turn.
    async fn session_start(&mut self, sink: &mut dyn EventSink) {
        let payload = || json!({"source": "startup"});
        let outcome = self
            .hooks
            .fire(HookEvent::SessionStart, "startup", payload, None, sink)
            .await
            .unwrap_or_default();
        if let Some(context) = outcome.context {
            self.messages.insert(1, Message::system(context));
        }
    }

    /// The turns, until the model answers without a tool call (and no Stop hook objects) or a limit, a cancel or
    /// a failing model call ends the run. Returns why it ended.
    async fn drive(&mut self, sink: &mut dyn EventSink) -> FinishReason {
        // Set once a Stop hook refused the answer: its payload then says so, and the hook lets it end (D3).
        let mut stop_hook_active = false;
        loop {
            if self.cancel.is_cancelled() {
                return FinishReason::Cancelled;
            }
            if self.turns >= self.config.max_turns {
                return FinishReason::MaxTurns;
            }
            // Before the next model call, never between a reply and its results: every call of the last reply is
            // answered.
            // Beside others, what all of them spent, their money at each one's own prices (T37a).
            let budget = Budget {
                paid_usd: self.config.budget.paid_usd + self.paid_usd,
                ..self.config.budget
            };
            let over = match &self.sibling {
                Some(sibling) => budget.spent_costing(&sibling.group.spent(), sibling.group.paid()),
                None => budget.spent(&self.usage),
            };
            if let Some(why) = over {
                sink.emit(Event::Error { message: why });
                return FinishReason::Budget;
            }
            self.turns += 1;
            self.take_steering(sink);
            if let Err(ending) = self.compact_if_full(sink).await {
                return ending;
            }
            let specs = self.offer.specs.clone();
            let calls = match self.ask(&specs, sink).await {
                Ok(calls) => calls,
                Err(ending) => return ending,
            };
            let ended = if calls.is_empty() && self.steering_waits() {
                // The owner wrote while the answer came: the run goes on with what they wrote, and the Stop hooks
                // wait for the answer that ends it.
                Ok(())
            } else if calls.is_empty()
                && let Some((kind, text)) = self.nudge()
            {
                // Asked once to go on (T73), before the Stop hooks: they judge the end that stands, not this one.
                self.nudged = true;
                sink.emit(Event::Nudged {
                    turn: self.turns,
                    kind,
                });
                self.messages.push(Message::user(text));
                self.flush_journal(sink);
                Ok(())
            } else if calls.is_empty() {
                match self.stop_hooks(stop_hook_active, sink).await {
                    Ok(Ending::Done) => return FinishReason::Answered,
                    Ok(Ending::Refused) => {
                        stop_hook_active = true;
                        Ok(())
                    }
                    Err(ending) => Err(ending),
                }
            } else {
                self.run_calls(calls, sink).await
            };
            if let Err(ending) = ended {
                return ending;
            }
        }
    }

    /// The owner's messages sent while the run worked, into the conversation before the next model call — the run's
    /// own inbox only (depth 0): a subagent shares the run's config but not its owner's words.
    fn take_steering(&mut self, sink: &mut dyn EventSink) {
        let Some(inbox) = self.config.inbox.as_ref().filter(|_| self.depth == 0) else {
            return;
        };
        for text in inbox.take() {
            sink.emit(Event::Steered {
                turn: self.turns,
                text: text.clone(),
            });
            self.messages.push(Message::user(text));
        }
    }

    /// Whether a message of the owner's waits for the run.
    fn steering_waits(&self) -> bool {
        self.depth == 0
            && self
                .config
                .inbox
                .as_ref()
                .is_some_and(|inbox| !inbox.is_empty())
    }

    /// One model call: its usage and text reported, its message kept, its tool calls handed back.
    async fn ask(
        &mut self,
        specs: &[ToolSpec],
        sink: &mut dyn EventSink,
    ) -> Result<Vec<ToolCall>, FinishReason> {
        let turn = self.turns;
        let mut retried = false;
        let reply = loop {
            let reply = self.complete(specs, sink).await?;
            match reply {
                // K4: the provider says the prompt does not fit — compacted once, asked again; a second time fails.
                Err(err)
                    if !retried && self.compacts() && crate::model::is_context_overflow(&err) =>
                {
                    retried = true;
                    let before = self.estimated_prompt();
                    let window = match self.window {
                        Window::Known(window) => Some(window),
                        Window::Unasked | Window::Unknown => None,
                    };
                    let keep = keep_for(window, self.config.max_output_tokens);
                    if self.compact_now(keep, before, sink).await? {
                        continue;
                    }
                    break Err(err);
                }
                other => break other,
            }
        };
        let reply = reply.map_err(|err| {
            sink.emit(Event::Error {
                message: err.to_string(),
            });
            FinishReason::Failed
        })?;
        self.usage += reply.usage;
        if let Some(sibling) = &self.sibling {
            sibling.group.spend(reply.usage, self.config.budget.prices);
        }
        sink.emit(Event::Usage {
            turn,
            input_tokens: reply.usage.input_tokens,
            cached_input_tokens: reply.usage.cached_input_tokens,
            output_tokens: reply.usage.output_tokens,
        });
        // K1: the provider's own count of this prompt; what is added after it is estimated.
        self.last_prompt = (reply.usage.input_tokens > 0)
            .then_some((reply.usage.input_tokens, self.messages.len()));
        self.answer = reply.message.content.clone();
        if let Some(text) = &self.answer {
            sink.emit(Event::AssistantText {
                turn,
                text: text.clone(),
            });
        }
        let calls = reply.message.tool_calls.clone();
        self.messages.push(reply.message);
        self.flush_journal(sink);
        let progress = Line::Progress {
            turns: self.turns,
            usage: self.usage,
        };
        self.journal_line(&progress, sink);
        Ok(calls)
    }

    /// One model call on the conversation as it is, its text streamed out as it comes (CP5); `Err` when the run ended
    /// while it waited, the API's own error inside.
    async fn complete(
        &mut self,
        specs: &[ToolSpec],
        sink: &mut dyn EventSink,
    ) -> Result<Result<crate::model::Reply, HarnessError>, FinishReason> {
        let turn = self.turns;
        let mut on_text = |piece: crate::model::Piece<'_>| {
            sink.emit(match piece {
                crate::model::Piece::Text(text) => Event::TextDelta {
                    turn,
                    text: text.to_owned(),
                },
                crate::model::Piece::Reasoning(text) => Event::ReasoningDelta {
                    turn,
                    text: text.to_owned(),
                },
            });
        };
        if self.last_call.is_none() {
            self.last_call = Some(Warm {
                length: self.messages.len(),
                tools: specs.to_vec(),
            });
        }
        let sent = sendable(&self.messages, self.config.sees_images);
        let completion = self.model.complete_streaming(
            &sent,
            specs,
            self.config.max_output_tokens,
            &mut on_text,
        );
        tokio::select! {
            reply = completion => Ok(reply),
            () = tokio::time::sleep_until(self.deadline) => Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => Err(FinishReason::Cancelled),
        }
    }

    /// Whether this run compacts itself (#111): when the settings leave it on, and never in a subagent — its
    /// conversation is short and its own.
    fn compacts(&self) -> bool {
        self.config.compaction.auto && self.depth == 0
    }

    /// The prompt's size now (K1): the provider's count of the last one and an estimate of what came after it —
    /// before the first call, of the whole conversation and the tools on offer.
    fn estimated_prompt(&self) -> u64 {
        match self.last_prompt {
            Some((tokens, len)) if len <= self.messages.len() => {
                tokens + compact::estimate_tokens(&self.messages[len..])
            }
            _ => {
                let specs: usize = self
                    .offer
                    .specs
                    .iter()
                    .map(|spec| {
                        spec.name.len() + spec.description.len() + spec.parameters.to_string().len()
                    })
                    .sum();
                compact::estimate_tokens(&self.messages) + (specs / 4) as u64
            }
        }
    }

    /// K1/K2: compacts before the model call when the prompt nears the window — the window asked for once, when the
    /// conversation is long enough for it to matter.
    async fn compact_if_full(&mut self, sink: &mut dyn EventSink) -> Result<(), FinishReason> {
        if !self.compacts() || self.compacted_at == Some(self.messages.len()) {
            return Ok(());
        }
        let estimate = self.estimated_prompt();
        let window = match self.window {
            Window::Known(window) => window,
            Window::Unknown => return Ok(()),
            Window::Unasked if estimate < ASK_WINDOW_FROM => return Ok(()),
            Window::Unasked => {
                let asked = tokio::select! {
                    asked = self.model.context_window() => asked,
                    () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
                    () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
                };
                let asked = asked.filter(|tokens| *tokens >= MIN_CONTEXT_WINDOW);
                self.window = asked.map_or(Window::Unknown, Window::Known);
                match asked {
                    Some(window) => window,
                    None => return Ok(()),
                }
            }
        };
        let from = compact_from(window, self.config.max_output_tokens);
        // Not again before the conversation grew by a quarter of the room since the last one: a tail that stays large
        // (the prompt and tools of a small window) must not be summarised anew at every step.
        let grown = self
            .compacted_size
            .is_none_or(|after| estimate >= after + from / 4);
        if estimate < from || !grown {
            return Ok(());
        }
        let keep = keep_for(Some(window), self.config.max_output_tokens);
        self.compact_now(keep, estimate, sink).await.map(|_| ())
    }

    /// Compacts the older part of the conversation (K3, K5): `true` when it did. A failure leaves the conversation as it
    /// was, says so, and is not tried again before the conversation grew.
    async fn compact_now(
        &mut self,
        keep: u64,
        before: u64,
        sink: &mut dyn EventSink,
    ) -> Result<bool, FinishReason> {
        self.flush_journal(sink);
        let compacted = tokio::select! {
            compacted = compact::compact_older(self.model, &self.messages, keep) => compacted,
            () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
        };
        let compacted = match compacted {
            Ok(compacted) => compacted,
            Err(err) => {
                self.compacted_at = Some(self.messages.len());
                sink.emit(Event::Error {
                    message: format!("{COMPACTION_FAILED}: {err}"),
                });
                return Ok(false);
            }
        };
        self.usage += compacted.usage;
        if let Some(sibling) = &self.sibling {
            sibling
                .group
                .spend(compacted.usage, self.config.budget.prices);
        }
        sink.emit(Event::Usage {
            turn: self.turns,
            input_tokens: compacted.usage.input_tokens,
            cached_input_tokens: compacted.usage.cached_input_tokens,
            output_tokens: compacted.usage.output_tokens,
        });
        // Relocated, never lost (Glut lean, N2): what the summary replaces stays in the run's store, and the summary
        // says where — the model reads it back as it reads a relocated result.
        // The model's reasoning stays out: it is no part of the conversation a later step needs (E4), and the store
        // writes to disk in a chat.
        let older: String = compacted
            .older
            .iter()
            .map(|message| Message {
                reasoning: None,
                reasoning_content: None,
                reasoning_details: Vec::new(),
                ..message.clone()
            })
            .filter_map(|message| serde_json::to_string(&message).ok())
            .map(|line| line + "\n")
            .collect();
        let id = self.results.put(older);
        let mut conversation = compacted.conversation;
        if let Some(notes) = conversation.iter_mut().find(|m| m.role == Role::User) {
            let text = notes.content.get_or_insert_default();
            text.push_str(&format!(
                "\n\n[Glut: the conversation this summary replaces is kept whole as result {id}, one message per \
                 line; {} reads it.]",
                results::NAME
            ));
        }
        self.messages = conversation;
        self.last_prompt = None;
        // The conversation is new: the next call's head is the one the next turn reuses.
        self.last_call = None;
        self.compacted_at = Some(self.messages.len());
        self.compacted_size = Some(self.estimated_prompt());
        sink.emit(Event::Compacted {
            turn: self.turns,
            before_tokens: before,
            after_tokens: self.estimated_prompt(),
            replaced: compacted.replaced,
        });
        let line = Line::Compacted {
            messages: self.messages.clone(),
        };
        self.journal_line(&line, sink);
        self.journaled = self.messages.len();
        Ok(true)
    }

    /// HS7, D3: a Stop hook may refuse the answer; its reason goes back as a user message and the loop takes
    /// another turn (`stop_hook_active` tells the hook it already did this once).
    async fn stop_hooks(
        &mut self,
        stop_hook_active: bool,
        sink: &mut dyn EventSink,
    ) -> Result<Ending, FinishReason> {
        let payload = || json!({"stop_hook_active": stop_hook_active});
        let outcome = self
            .hooks
            .fire(HookEvent::Stop, "", payload, Some(self.turns), sink)
            .await?;
        if !outcome.blocked {
            return Ok(Ending::Done);
        }
        let reason = refusal(outcome, "a hook refused to end the run");
        self.messages.push(Message::user(format!(
            "A Stop hook blocked ending the run: {reason}\nContinue, and address what it asked for."
        )));
        // Saved now: a run lost before the next reply must not look as if it had answered.
        self.flush_journal(sink);
        Ok(Ending::Refused)
    }

    /// The reply's tool calls, one after the other in the order asked: a later call may depend on an earlier
    /// one (write, then read). `task` calls side by side run at once (TUI T40, [`Self::run_task_batch`]). Past
    /// [`MAX_CALLS_PER_TURN`] each is answered with an error instead, so every call still gets its reply (the API
    /// wants one per call id).
    async fn run_calls(
        &mut self,
        calls: Vec<ToolCall>,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        self.updating_todos = calls
            .iter()
            .any(|call| call.function.name == planning::TODO);
        let mut calls = calls.into_iter().enumerate().peekable();
        while let Some((index, call)) = calls.next() {
            sink.emit(Event::tool_call(
                self.turns,
                &call.id,
                &call.function.name,
                &call.function.arguments,
            ));
            if index >= MAX_CALLS_PER_TURN {
                let output = format!(
                    "error: at most {MAX_CALLS_PER_TURN} tool calls are run per turn; ask again"
                );
                self.answer_call(call, false, output, sink);
                continue;
            }
            let ran = if self.offer_task && call.function.name == crate::task::NAME {
                let mut batch = vec![call];
                while let Some((_, next)) = calls.next_if(|(index, next)| {
                    *index < MAX_CALLS_PER_TURN && next.function.name == crate::task::NAME
                }) {
                    sink.emit(Event::tool_call(
                        self.turns,
                        &next.id,
                        &next.function.name,
                        &next.function.arguments,
                    ));
                    batch.push(next);
                }
                self.run_task_batch(batch, sink).await
            } else {
                self.journal_line(
                    &Line::Call {
                        id: call.id.clone(),
                    },
                    sink,
                );
                self.run_call(call, sink).await
            };
            if let Err(ending) = ran {
                // #112, R3: the reply's other calls still get an answer, so the conversation stays one a provider
                // takes — a caller may go on with it (`run_continuing`).
                if ending == FinishReason::Refused {
                    self.answer_not_run(calls.by_ref().map(|(_, call)| call), sink);
                }
                return Err(ending);
            }
            self.flush_journal(sink);
        }
        self.flush_journal(sink);
        Ok(())
    }

    /// The calls a refusal-ended run leaves: each answered that it was not run.
    fn answer_not_run(&mut self, calls: impl Iterator<Item = ToolCall>, sink: &mut dyn EventSink) {
        let message = self.refusals.ended().unwrap_or_default().to_owned();
        for call in calls {
            let output = format!("error: not run, the run is ending — {message}");
            self.answer_call(call, false, output, sink);
        }
        self.flush_journal(sink);
    }

    /// `task` calls of one reply side by side (TUI T40): each judged and prepared in turn — its PreToolUse hooks, the
    /// gate's word on it —, then all of them run at once, at most `config.parallel_subagents` at a time, and last each
    /// finished in the order asked — its PostToolUse hooks, its answer into the conversation —, so the conversation
    /// reads as if they had run one after another.
    async fn run_task_batch(
        &mut self,
        calls: Vec<ToolCall>,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        /// How far a call of the batch came before the subagents ran.
        enum Step {
            /// Answered without running: the run is ending, or a hook refused it.
            Answered(String),
            /// Not started (over the limit, wrong arguments, refused by the gate): its result and the refusals.
            Stopped(bool, String, Vec<Refusal>),
            /// Its subagent runs: its place among them.
            Runs(usize),
        }
        let mut steps = Vec::with_capacity(calls.len());
        let mut ready = Vec::new();
        for call in &calls {
            self.journal_line(
                &Line::Call {
                    id: call.id.clone(),
                },
                sink,
            );
            if let Some(text) = self.perform_before(call, sink).await? {
                steps.push(Step::Answered(text));
                continue;
            }
            match self.prepare_task(call, sink).await {
                Ok(task) => {
                    steps.push(Step::Runs(ready.len()));
                    ready.push(task);
                }
                Err((ok, text, refused)) => steps.push(Step::Stopped(ok, text, refused)),
            }
        }
        let mut outcomes: Vec<Option<(Vec<Refusal>, RunOutcome)>> = self
            .run_subagents(&ready, sink)
            .await
            .into_iter()
            .zip(ready)
            .map(|(outcome, task)| Some((task.refused, outcome)))
            .collect();
        let mut calls = calls.into_iter().zip(steps);
        while let Some((call, step)) = calls.next() {
            let answered = match step {
                Step::Answered(text) => Ok(text),
                Step::Stopped(ok, text, refused) => {
                    self.perform_after(&call, ok, text, refused, 0, sink).await
                }
                Step::Runs(at) => {
                    let (refused, outcome) = outcomes[at]
                        .take()
                        .unwrap_or_else(|| (Vec::new(), RunOutcome::lost()));
                    let from_subagent = outcome.refusals;
                    let (ok, text) = self.task_answer(outcome);
                    self.perform_after(&call, ok, text, refused, from_subagent, sink)
                        .await
                }
            };
            let ended = match answered {
                Ok(text) => self.answer_with(call, text, sink),
                Err(ending) => Err(ending),
            };
            if let Err(ending) = ended {
                if ending == FinishReason::Refused {
                    self.answer_not_run(calls.by_ref().map(|(call, _)| call), sink);
                }
                return Err(ending);
            }
            self.flush_journal(sink);
        }
        Ok(())
    }

    /// Writes the messages added since the last write to the journal.
    /// One write and one sync for all of them: a chat turn opens with its whole earlier conversation.
    fn flush_journal(&mut self, sink: &mut dyn EventSink) {
        if self.journal.is_none() || self.journaled >= self.messages.len() {
            return;
        }
        let lines: Vec<Line> = self.messages[self.journaled..]
            .iter()
            .map(|message| Line::Message {
                message: message.clone(),
            })
            .collect();
        self.journaled = self.messages.len();
        self.journal_lines(&lines, sink);
    }

    /// One line to the journal; a journal that cannot be written is given up — the run goes on, resumable no more.
    fn journal_line(&mut self, line: &Line, sink: &mut dyn EventSink) {
        self.journal_lines(std::slice::from_ref(line), sink);
    }

    fn journal_lines(&mut self, lines: &[Line], sink: &mut dyn EventSink) {
        let Some(journal) = self.journal.as_mut() else {
            return;
        };
        if let Err(reason) = journal.write_all(lines) {
            // A journal that stopped would offer a resume from a step long passed: it goes now, not at the sweep.
            if let Some(journal) = self.journal.take() {
                journal.finish();
            }
            sink.emit(Event::Error {
                message: format!(
                    "the journal stopped (this run can no longer be resumed): {reason}"
                ),
            });
        }
    }

    /// The calls a resumed run found open (D4/D7): one that had not started runs as it would have; one that was under
    /// way runs again when it only reads, else the model is told it was interrupted.
    async fn settle_resumed(
        &mut self,
        calls: Vec<(ToolCall, bool)>,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        for (call, started) in calls {
            let replay = !started
                || LOOP_REPLAY_SAFE.contains(&call.function.name.as_str())
                || self
                    .tools
                    .replay_safe(&call.function.name, &call.function.arguments);
            if replay {
                self.journal_line(
                    &Line::Call {
                        id: call.id.clone(),
                    },
                    sink,
                );
                self.run_call(call, sink).await?;
            } else {
                let output = format!(
                    "error: interrupted — the run stopped while `{}` was running, so whether it took effect is \
                     unknown. Check the state (the files, git status, the service) before you go on; run it again \
                     only if it did not.",
                    call.function.name
                );
                self.answer_call(call, false, output, sink);
            }
            self.flush_journal(sink);
        }
        Ok(())
    }

    /// The conversation ends in an answer: the last message is the model's, without tool calls.
    fn answered_before(&mut self, sink: &mut dyn EventSink) -> bool {
        let Some(last) = self.messages.last() else {
            return false;
        };
        let answered =
            last.role == Role::Assistant && last.tool_calls.is_empty() && last.content.is_some();
        if answered {
            self.answer = last.content.clone();
            // The answer is said again: a reader of this resumed run's events would otherwise see none.
            if let Some(text) = &self.answer {
                sink.emit(Event::AssistantText {
                    turn: self.turns,
                    text: text.clone(),
                });
            }
        }
        answered
    }

    /// One tool call: the PreToolUse hooks, the call itself, its permission events and result, the PostToolUse
    /// hooks, and the message the model gets back. `Err` when the run ended on the way.
    async fn run_call(
        &mut self,
        call: ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        let output = self.perform(&call, sink).await?;
        self.answer_with(call, output, sink)
    }

    /// A tool's answer with the images it shows (T45): kept and numbered on through the conversation — or, where the
    /// model sees none or the run keeps none, said in the answer instead.
    fn with_images(&self, mut message: Message, images: Vec<Vec<u8>>) -> Message {
        if images.is_empty() {
            return message;
        }
        // The model asked for it, not the owner: shown only to a model its list says sees images.
        let said = match (&self.config.images_home, self.config.sees_images) {
            (_, Some(false)) => Some("this model sees no images".to_owned()),
            (_, None) => {
                Some("whether this model sees images is not known: it is not shown".to_owned())
            }
            (None, _) => Some("images are not shown in this run".to_owned()),
            (Some(home), Some(true)) => {
                // The prompt's images are in the conversation already: the numbers go on from every one there.
                let mut number = self
                    .messages
                    .iter()
                    .flat_map(|message| &message.images)
                    .map(|image| image.number)
                    .max()
                    .unwrap_or(0);
                let mut faults = Vec::new();
                for bytes in &images {
                    number += 1;
                    match crate::images::keep(home, bytes, number) {
                        Ok(image) => message.images.push(image),
                        Err(reason) => faults.push(reason),
                    }
                }
                (!faults.is_empty()).then(|| faults.join("; "))
            }
        };
        if let Some(said) = said {
            let text = message.content.get_or_insert_with(String::new);
            text.push_str(&format!(" — {said}"));
        }
        message
    }

    /// What [`Self::run_call`] does with a call's text: Glut lean's savings, the bound, the message the model gets —
    /// and the run's end when refusals reached their limit with it.
    fn answer_with(
        &mut self,
        call: ToolCall,
        output: String,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        let parts = self
            .chain
            .take()
            .filter(|chain| chain.call == call.id && chain.text == output)
            .map(|chain| chain.parts);
        let output = match unchanged_read(&self.messages, &call, &output) {
            Some(pointer) => {
                self.saved(&call, "unchanged read", &output, &pointer, sink);
                pointer
            }
            None => output,
        };
        let (output, savings) = compacted(&call, output, &self.results, parts);
        for saving in savings {
            self.saved_chars(&call, saving.what, saving.before, saving.after, sink);
        }
        if let Some(what) = piped(&call) {
            let chars = output.chars().count();
            self.saved_chars(
                &call,
                &format!("{}{what}", crate::lean::IN_A_PIPE),
                chars,
                chars,
                sink,
            );
        }
        // Relocated, never cut (Glut lean, N2): the conversation gets head and tail, the store keeps the whole. A part
        // read back from the store is already sized to fit — bounding it again would hide its middle.
        let output = if call.function.name == results::NAME {
            output
        } else {
            let whole = output.chars().count();
            let bounded =
                results::bounded(output, &self.results, crate::tools::MAX_TOOL_OUTPUT_CHARS);
            let shown = bounded.chars().count();
            if shown < whole {
                self.saved_chars(&call, "relocated", whole, shown, sink);
            }
            bounded
        };
        // A call that failed — or whose answer a hook replaced (hooks veto) — shows no image.
        let images = self
            .shown
            .remove(&call.id)
            .filter(|_| !output.starts_with("error: "))
            .unwrap_or_default();
        // Glut's reminder of the task list joins the conversation, not the result the owner sees (T73).
        let output = self.reminded(&call.function.name, output, sink);
        let message = self.with_images(Message::tool(call.id, output), images);
        self.messages.push(message);
        // #112, R3: the call that reached a limit still has its answer above — the conversation stays whole for the
        // journal — and the run ends here instead of at the step limit.
        if let Some(message) = self.refusals.ended() {
            sink.emit(Event::Error {
                message: message.to_owned(),
            });
            return Err(FinishReason::Refused);
        }
        Ok(())
    }

    /// Reports and counts one saving of Glut lean (N9): an event, and a line in the run's count when it keeps one.
    fn saved(
        &self,
        call: &ToolCall,
        what: &str,
        before: &str,
        after: &str,
        sink: &mut dyn EventSink,
    ) {
        self.saved_chars(
            call,
            what,
            before.chars().count(),
            after.chars().count(),
            sink,
        );
    }

    /// [`Self::saved`] with the characters already counted.
    fn saved_chars(
        &self,
        call: &ToolCall,
        what: &str,
        before: usize,
        after: usize,
        sink: &mut dyn EventSink,
    ) {
        sink.emit(Event::Lean {
            turn: self.turns,
            id: call.id.clone(),
            what: what.to_owned(),
            chars_before: before,
            chars_after: after,
        });
        if let Some(path) = &self.config.savings {
            let saving = crate::lean::Saving {
                at: chrono::Utc::now(),
                what: what.to_owned(),
                before,
                after,
            };
            // Best-effort: a count that cannot be written never fails the call it counts.
            let _ = crate::lean::record(path, &saving);
        }
    }

    /// One call as [`Self::run_call`] runs it, without adding its answer to the conversation — what a script's call
    /// (Glut 2, A) shares with a direct one: the hooks, the gate's events, the result event. Returns the text the
    /// caller hands on.
    async fn perform(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<String, FinishReason> {
        if let Some(text) = self.perform_before(call, sink).await? {
            return Ok(text);
        }
        let (ok, output) = self.execute(call, sink).await?;
        let refused = std::mem::take(&mut self.refused_now);
        let from_subagent = std::mem::take(&mut self.subagent_refusals);
        self.perform_after(call, ok, output, refused, from_subagent, sink)
            .await
    }

    /// [`Self::perform`] before the call runs: `Some` with the call's text when it does not run at all — the run is
    /// ending, or a PreToolUse hook refused it —, else its snapshot taken.
    async fn perform_before(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<Option<String>, FinishReason> {
        let turn = self.turns;
        // #112: once refusals ended the run, a script's further calls are not run — the script finishes, the loop ends.
        if let Some(message) = self.refusals.ended() {
            let text = format!("error: not run, the run is ending — {message}");
            sink.emit(Event::tool_result(
                turn,
                &call.id,
                &call.function.name,
                false,
                &text,
            ));
            return Ok(Some(text));
        }
        // HS7, D3/B2: the owner's hooks may block a call before anything runs — they never let one through,
        // the gate stays the only decider.
        let payload = || {
            json!({
                "tool_name": &call.function.name,
                "tool_input": hooks::tool_input(&call.function.arguments),
            })
        };
        let before = self
            .hooks
            .fire(
                HookEvent::PreToolUse,
                &call.function.name,
                payload,
                Some(turn),
                sink,
            )
            .await?;
        if before.blocked {
            let reason = refusal(before, "a hook refused this call");
            let blocked = Refusal {
                tool: call.function.name.clone(),
                target: call.function.arguments.clone(),
                reason: reason.clone(),
            };
            let mut text = format!("error: {reason}");
            if let Some(note) = self.refusals.record(&[blocked], 0) {
                text.push_str("\n\n");
                text.push_str(&note);
            }
            sink.emit(Event::tool_result(
                turn,
                &call.id,
                &call.function.name,
                false,
                &text,
            ));
            return Ok(Some(text));
        }
        self.snapshot_before(call, sink).await;
        Ok(None)
    }

    /// Offers `glut_file` once the `skill` call `arguments` loaded a maker skill (T37b) — the tool list grows by one
    /// for the rest of the run.
    fn offer_glut_files(&mut self, arguments: &str) {
        if !names_maker(arguments)
            || self
                .offer
                .specs
                .iter()
                .any(|spec| spec.name == glut_files::NAME)
        {
            return;
        }
        if let Some(spec) = self
            .tools
            .specs()
            .into_iter()
            .find(|spec| spec.name == glut_files::NAME)
        {
            self.offer.specs.push(spec);
        }
    }

    /// [`Self::perform`] after the call ran (or was stopped before its work): the diagnostics of an edit, the gate's
    /// events and the refusals — `refused` judged outside the gate's log, `from_subagent` its subagent's —, the result
    /// event, the PostToolUse hooks. Returns the text the model gets.
    async fn perform_after(
        &mut self,
        call: &ToolCall,
        ok: bool,
        mut output: String,
        mut refused: Vec<Refusal>,
        from_subagent: u32,
        sink: &mut dyn EventSink,
    ) -> Result<String, FinishReason> {
        let turn = self.turns;
        // T37b: a maker skill loaded mid-run brings `glut_file` from the next model call on.
        if ok && self.depth == 0 && call.function.name == crate::skills::tool::NAME {
            self.offer_glut_files(&call.function.arguments);
        }
        if ok && let Some(note) = self.diagnostics_after(call, sink).await {
            output.push_str("\n\n");
            output.push_str(&note);
        }
        if let Some(gate) = self.tools.gate() {
            for judged in gate.take_log() {
                sink.emit(Event::permission(turn, &call.id, &judged));
                if !judged.allowed {
                    refused.push(Refusal {
                        tool: judged.tool,
                        target: judged.target,
                        reason: judged.reason,
                    });
                }
            }
        }
        if let Some(note) = self.refusals.record(&refused, from_subagent) {
            output.push_str("\n\n");
            output.push_str(&note);
        }
        // T58: before anything sees it — the stream, the hooks, the store, the model.
        if let Some(masks) = &self.config.masks {
            let masked = masks.mask(&output, &seen_at(call));
            if !masked.hidden.is_empty() {
                output = masked.text;
                sink.emit(Event::Masked {
                    turn,
                    id: call.id.clone(),
                    hidden: masked.hidden,
                });
            }
        }
        // In the stream the tool's own result is shown first (D3); the model's message may still differ.
        sink.emit(Event::tool_result(
            turn,
            &call.id,
            &call.function.name,
            ok,
            &output,
        ));
        // HS7, D3: PostToolUse after the tool ran, whatever it answered; a block replaces what the model gets
        // with the reason — the `ToolResult` above keeps what the tool really answered, and the `Hook` event
        // beside it says that a hook blocked.
        let payload = || {
            json!({
                "tool_name": &call.function.name,
                "tool_input": hooks::tool_input(&call.function.arguments),
                "tool_response": output.chars().take(EVENT_RESULT_CHARS).collect::<String>(),
            })
        };
        let after = self
            .hooks
            .fire(
                HookEvent::PostToolUse,
                &call.function.name,
                payload,
                Some(turn),
                sink,
            )
            .await?;
        Ok(if after.blocked {
            format!("error: {}", refusal(after, "a hook refused this result"))
        } else {
            output
        })
    }

    /// A call answered without running it: its result shown, and the model told.
    fn answer_call(&mut self, call: ToolCall, ok: bool, output: String, sink: &mut dyn EventSink) {
        sink.emit(Event::tool_result(
            self.turns,
            &call.id,
            &call.function.name,
            ok,
            &output,
        ));
        let output = self.reminded(&call.function.name, output, sink);
        self.messages.push(Message::tool(call.id, output));
    }

    /// `output` with Glut's reminder of the task list after it (T73), when the list has items open and
    /// [`REMIND_EVERY`] calls passed without `todo_write` — at the end, so the cached conversation stays as it was.
    fn reminded(&mut self, name: &str, output: String, sink: &mut dyn EventSink) -> String {
        // Only Glut speaks as `[glut]` (the prompt tells the model to follow such lines): a file, a page or a server
        // writing one is quoted, never obeyed.
        let output = planning::unforged(output);
        if name == planning::TODO {
            self.since_todo = 0;
            return output;
        }
        self.since_todo += 1;
        if !self.config.todos
            || self.since_todo < REMIND_EVERY
            || self.updating_todos
            || self.reminded_at == self.turns
            || !planning::open(&self.todos)
        {
            return output;
        }
        self.since_todo = 0;
        self.reminded_at = self.turns;
        let note = planning::reminder(&self.todos, REMIND_EVERY);
        sink.emit(Event::Nudged {
            turn: self.turns,
            kind: NudgeKind::Reminder,
        });
        format!("{output}\n\n{note}")
    }

    /// Glut's one question when the model ends its run (T73): its list has items open, or its last words announce
    /// a next step — `None` when it is done as it says, asked once already, in plan mode, asking the owner, or off.
    fn nudge(&self) -> Option<(NudgeKind, String)> {
        if !self.config.nudge || self.nudged || self.depth > 0 {
            return None;
        }
        // No step left to take it in — the limit, the budget or the time: the answer stands as it is.
        if self.turns + 1 >= self.config.max_turns
            || self.config.budget.spent(&self.usage).is_some()
            || Instant::now() >= self.deadline
        {
            return None;
        }
        if self
            .tools
            .gate()
            .is_some_and(|gate| gate.mode() == Mode::Plan)
        {
            return None;
        }
        let said = self.answer.as_deref().unwrap_or_default().trim_end();
        // A question to the owner ends the turn as it should — "Shall I? (y/n)" too.
        let last = said.lines().last().unwrap_or_default();
        if last.contains('?') || last.contains('？') {
            return None;
        }
        if self.config.todos && planning::open(&self.todos) {
            return Some((
                NudgeKind::OpenItems,
                planning::open_items_nudge(&self.todos),
            ));
        }
        planning::announces(said)
            .then(|| (NudgeKind::Announced, planning::ANNOUNCED_NUDGE.to_owned()))
    }

    /// What the call answers: a subagent for `task` (HS8, D2 — only this step differs from an ordinary call),
    /// the toolbox for everything else, held to the run's deadline and cancel. `Err` when the run ended while it
    /// worked.
    async fn execute(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<(bool, String), FinishReason> {
        let name = &call.function.name;
        if self.offer_task && name == crate::task::NAME {
            return Ok(self.run_task(call, sink).await);
        }
        // T36c: nor are the panes' tools.
        if crate::config::PANE_TOOLS.contains(&name.as_str())
            && !self.offer.specs.iter().any(|spec| spec.name == *name)
        {
            return Ok((
                false,
                format!("error: {name} is offered only while the screen is split into panes"),
            ));
        }
        if name == crate::config::LOOP_TOOL
            && !self.offer.specs.iter().any(|spec| spec.name == *name)
        {
            return Ok((
                false,
                format!("error: {name} is offered only to a loop's own model"),
            ));
        }
        // T37b: what is not offered is not run, whatever the model calls.
        if name == glut_files::NAME && !self.offer.specs.iter().any(|spec| spec.name == *name) {
            return Ok((
                false,
                format!(
                    "error: {name} is offered only while one of {} runs",
                    glut_files::MAKERS.join(", ")
                ),
            ));
        }
        // Only the run's own store: nothing for the gate to judge, like `bash_output`.
        if name == results::NAME {
            return Ok(results::answer(&self.results, &call.function.arguments));
        }
        if self.config.todos && name == planning::TODO {
            return Ok(match planning::parse_todos(&call.function.arguments) {
                Ok(items) => {
                    sink.emit(Event::Todo {
                        turn: self.turns,
                        items: items.clone(),
                    });
                    let answer = planning::todo_answer(&items);
                    self.todos = items;
                    (true, answer)
                }
                Err(reason) => (false, format!("error: {reason}")),
            });
        }
        if name == planning::EXIT_PLAN
            && let Some(gate) = self.tools.gate().filter(|gate| gate.mode() == Mode::Plan)
        {
            return Ok(self.exit_plan(Arc::clone(gate), call, sink).await);
        }
        if self.offer.codemode && name == codemode::NAME {
            return Box::pin(self.run_codemode(call, sink)).await;
        }
        self.chain = None;
        // T40: beside other subagents a write holds its path, and one after another's write since this run's read
        // of that file is said so.
        let shared = self.sibling.as_ref().and_then(|sibling| {
            match self.tools.access_of(name, &call.function.arguments)? {
                Access::Edit { path } => Some((sibling, true, self.full_path(&path))),
                Access::Read { path } => Some((sibling, false, self.full_path(&path))),
                _ => None,
            }
        });
        let mut stale = None;
        let _held = match &shared {
            Some((sibling, true, path)) => {
                let held = tokio::select! {
                    held = sibling.group.hold(path) => held,
                    () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
                    () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
                };
                let seen = self.read_at.get(path).copied();
                if sibling
                    .group
                    .changed_by_other(path, &sibling.id, seen.unwrap_or(0))
                {
                    stale = Some((path.clone(), seen.is_some()));
                }
                Some(held)
            }
            _ => None,
        };
        let arguments = match self.unmasked(call).await {
            Ok(arguments) => arguments,
            Err(refused) => return Ok((false, refused)),
        };
        let called = tokio::select! {
            called = self.tools.call_reporting(name, &arguments, self.stop) => called,
            () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
        };
        if let Some((sibling, writes, path)) = shared {
            let version = if writes && called.ok {
                sibling.group.wrote(&path, &sibling.id)
            } else {
                sibling.group.version(&path)
            };
            self.read_at.insert(path, version);
        }
        let mut called = called;
        if let Some((path, read)) = stale {
            let when = if read {
                "after you last read it"
            } else {
                "and you had not read it"
            };
            called.text.push_str(&format!(
                "\n\nNote: another subagent changed {} {when}; read it again before you change it further.",
                path.display()
            ));
        }
        if !called.images.is_empty() {
            self.shown
                .insert(call.id.clone(), std::mem::take(&mut called.images));
        }
        if let Some(parts) = called.parts {
            self.chain = Some(ChainAnswer {
                call: call.id.clone(),
                text: called.text.clone(),
                parts,
            });
        }
        Ok((called.ok, called.text))
    }

    /// Where a path the rules see (`src/a.rs`, `~/Vault/a.md`) is on disk: in the working folder or one beside it.
    fn full_path(&self, path: &str) -> PathBuf {
        self.tools
            .folders()
            .full_path(path)
            .unwrap_or_else(|| lexical(&self.config.cwd, path))
    }

    /// The arguments a call runs with (T58): a write's placeholders with their values put back where the file
    /// already holds them, anything else as the model wrote it. `Err` with the model's words when a write would put a
    /// hidden value somewhere new.
    async fn unmasked(&self, call: &ToolCall) -> Result<String, String> {
        let arguments = &call.function.arguments;
        let Some(masks) = self
            .config
            .masks
            .clone()
            .filter(|_| arguments.contains(crate::masks::OPEN))
        else {
            return Ok(arguments.clone());
        };
        let Some(Access::Edit { path }) = self.tools.access_of(&call.function.name, arguments)
        else {
            return Ok(arguments.clone());
        };
        let (cwd, arguments) = (self.config.cwd.clone(), arguments.clone());
        let beside = self.tools.folders().clone();
        // The file is read only when a value is to go back, and off the runtime: it is the model's to name.
        let job = tokio::task::spawn_blocking(move || {
            let held = std::cell::OnceCell::new();
            masks.unmask(&arguments, |value| {
                held.get_or_init(|| held_text(&cwd, &beside, &path))
                    .contains(value)
            })
        });
        let deadline = self.deadline;
        tokio::select! {
            joined = job => joined.unwrap_or_else(|err| Err(format!("error: the write could not be prepared: {err}"))),
            () = tokio::time::sleep_until(deadline) => Err("error: the run's time ran out".into()),
            () = self.cancel.cancelled() => Err("error: the run was stopped".into()),
        }
    }

    /// Records the work folder before the first call of this turn that may change it — an edit or a command (Glut 2,
    /// D) — when the run keeps snapshots. A failure never stops the call: the run goes on without, said once.
    async fn snapshot_before(&mut self, call: &ToolCall, sink: &mut dyn EventSink) {
        let Some(root) = self.config.snapshots.clone() else {
            return;
        };
        // A subagent's steps are covered by the snapshot its parent took before the `task` call: one record per run.
        let changes = self.depth == 0
            && (call.function.name == crate::task::NAME
                || match self
                    .tools
                    .access_of(&call.function.name, &call.function.arguments)
                {
                    Some(Access::Edit { .. }) => true,
                    Some(Access::Bash { command, .. }) => !reads_only(&command),
                    _ => false,
                });
        if !changes || matches!(self.snapshots, SnapshotState::Off) {
            return;
        }
        let (turn, run, cwd) = (
            self.turns,
            self.hooks.session_id.clone(),
            self.config.cwd.clone(),
        );
        let state = std::mem::replace(&mut self.snapshots, SnapshotState::Off);
        let beside: Vec<PathBuf> = self
            .tools
            .folders()
            .list()
            .into_iter()
            .map(|folder| folder.real)
            .collect();
        // Git is blocking work; the snapshots move to a blocking thread and back.
        let job = tokio::task::spawn_blocking(move || {
            let mut taken = match state {
                SnapshotState::Open(taken) => *taken,
                _ => {
                    snapshot::sweep(&root);
                    Taken::new(Snapshots::open(&root, &cwd, &run)?)
                }
            };
            let (commit, notes) = taken.take(&root, &run, &beside, turn)?;
            Ok::<_, String>((taken, commit, notes))
        });
        // The run's end does not wait for a snapshot: git finishes on its own thread, the run goes without.
        let taken = tokio::select! {
            joined = job => joined.unwrap_or_else(|err| Err(format!("the snapshot task failed: {err}"))),
            () = tokio::time::sleep_until(self.deadline) => return,
            () = self.cancel.cancelled() => return,
        };
        match taken {
            Ok((taken, commit, notes)) => {
                for message in notes {
                    sink.emit(Event::Error { message });
                }
                if let Some(commit) = commit {
                    sink.emit(Event::Snapshot {
                        turn,
                        run: self.hooks.session_id.clone(),
                        commit,
                    });
                }
                self.snapshots = SnapshotState::Open(Box::new(taken));
            }
            Err(reason) => sink.emit(Event::Error {
                message: format!("no snapshots for this run (undo is not possible): {reason}"),
            }),
        }
    }

    /// The language server's errors for the file an edit just changed (Glut 2, D, G12). A server reads files the agent
    /// wrote and may run programs because of them, so it is treated as a command of the run: it starts only at the top
    /// level, only where the run has a sandbox (it runs in it, without network), and only when the gate allows its
    /// command line as it would a shell command — judged once, at its start. The servers' pipes are blocking work,
    /// done on the blocking pool; the run's end does not wait for them.
    async fn diagnostics_after(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Option<String> {
        if !self.config.diagnostics || self.depth > 0 {
            return None;
        }
        let Some(Access::Edit { path }) = self
            .tools
            .access_of(&call.function.name, &call.function.arguments)
        else {
            return None;
        };
        // The language servers know the working folder's project only; a folder beside it (T60c) is no part of it.
        if path.starts_with(['~', '/']) {
            return None;
        }
        if self.diagnostics.is_none() {
            let profile = self.sandbox_profile()?;
            self.diagnostics = Some(Arc::new(std::sync::Mutex::new(
                crate::diagnostics::Diagnostics::new(&self.config.cwd, profile),
            )));
        }
        let servers = Arc::clone(self.diagnostics.as_ref()?);
        let launch = servers
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .launch(&path);
        if let (Some(launch), Some(gate)) = (launch, self.tools.gate()) {
            let access = Access::Bash {
                command: launch.line.clone(),
                folder: String::new(),
                unsandboxed: false,
                sandboxed: false,
            };
            let judged = gate.judge("bash", &access).await;
            for entry in gate.take_log() {
                sink.emit(Event::permission(self.turns, &call.id, &entry));
            }
            if judged.is_err() {
                servers
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .refuse(launch.server);
                return None;
            }
        }
        let job = tokio::task::spawn_blocking(move || {
            servers
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .after_edit(&path)
        });
        tokio::select! {
            joined = job => joined.ok().flatten(),
            () = tokio::time::sleep_until(self.deadline) => None,
            () = self.cancel.cancelled() => None,
        }
    }

    /// The run's sandbox as a Seatbelt profile without network — `None` where there is no sandbox (not on, not on this
    /// system, no home): then nothing else is started for the run either.
    fn sandbox_profile(&self) -> Option<String> {
        let settings = self
            .config
            .sandbox
            .as_ref()
            .filter(|settings| settings.enabled)?;
        if !crate::sandbox::available() {
            return None;
        }
        let home = std::env::var_os("HOME")
            .map(std::path::PathBuf::from)
            .filter(|home| home.is_absolute())?;
        let root = self.config.cwd.canonicalize().ok()?;
        let policy =
            crate::sandbox::Policy::new(&root, &home, &settings.allow_write, &settings.deny_read);
        Some(policy.profile())
    }

    /// The `exit_plan` arm (Glut 2, D): the plan goes to the owner; yes switches the gate to the run's build mode — or
    /// to the mode the owner picked among the ask's scopes ([`planning::leave_modes`], TUI T19) — and offers the tools
    /// that mode allows from the next turn on, no keeps Plan. Headless the ask is refused.
    async fn exit_plan(
        &mut self,
        gate: Arc<Gate>,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> (bool, String) {
        let (_, question) = match planning::plan_of(&call.function.arguments) {
            Ok(plan) => plan,
            Err(reason) => return (false, format!("error: {reason}")),
        };
        // The owner approves a mode as much as a plan: the question names the one the run switches to.
        let question = format!("{question} (then mode {})", self.config.build_mode.name());
        let modes = planning::leave_modes(self.config.build_mode);
        let scopes = modes.iter().map(|mode| mode.name().to_owned()).collect();
        let answer = gate
            .ask_owner_within(planning::EXIT_PLAN, &question, scopes)
            .await;
        let chosen = match answer {
            Answer::AllowWithin(at) => modes.get(usize::from(at)).copied(),
            Answer::Allow | Answer::AllowAlways => modes.first().copied(),
            Answer::Deny => None,
        };
        match answer {
            Answer::Allow | Answer::AllowAlways | Answer::AllowWithin(_) => {
                let mode = chosen.unwrap_or(self.config.build_mode);
                gate.set_mode(mode);
                sink.emit(Event::ModeChanged {
                    turn: self.turns,
                    mode: gate.mode_name().to_owned(),
                });
                let glut = self.offer.specs.iter().any(|spec| spec.name == glut_files::NAME);
                self.offer = Offer::of(self.config, self.tools, self.depth, glut);
                (
                    true,
                    format!("The owner approved the plan: carry it out now (mode {}).", gate.mode_name()),
                )
            }
            Answer::Deny if !gate.can_ask() => (
                false,
                "Nobody can be asked from here: present the plan in your answer and ask the owner to switch to build \
                 mode to carry it out. Until then you stay in plan mode."
                    .to_owned(),
            ),
            Answer::Deny => (
                false,
                "The owner did not approve the plan: stay in plan mode — refine the plan, or answer with your \
                 questions."
                    .to_owned(),
            ),
        }
    }

    /// The `codemode` arm (Glut 2, A): runs the script and answers each of its tool calls on the path a direct call
    /// takes ([`Self::perform`]), one after the other, until it ends. A script found waiting with no call open is
    /// ended — nothing could ever settle what it waits for. `Err` when the run ended meanwhile.
    async fn run_codemode(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<(bool, String), FinishReason> {
        let code = match codemode::code_of(&call.function.arguments) {
            Ok(code) => code,
            Err(reason) => return Ok((false, format!("error: {reason}"))),
        };
        // Bounded: a script that fires many calls at once waits for room instead of queueing them all in memory.
        let (requests, mut asked) = tokio::sync::mpsc::channel(8);
        // A place among the scripts of this process, before the script starts — waiting for one is no stuck script.
        let _slot = tokio::select! {
            slot = codemode::slot() => match slot {
                Some(slot) => slot,
                None => return Ok((false, "error: code mode is shut down".into())),
            },
            () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
        };
        let cancel = self.cancel.clone();
        let interrupt = codemode::Interrupt {
            stop: Arc::clone(self.stop),
            cancelled: Box::new(move || cancel.is_cancelled()),
            deadline: self.deadline.into_std(),
        };
        let script = codemode::run(
            code,
            Arc::clone(&self.offer.catalog),
            Arc::clone(&self.kept),
            self.results.clone(),
            interrupt,
            requests,
        );
        let mut script = std::pin::pin!(script);
        let (mut calls, mut idle) = (0u32, 0u32);
        loop {
            let polled =
                std::future::poll_fn(|cx| std::task::Poll::Ready(script.as_mut().poll(cx))).await;
            if let std::task::Poll::Ready(ended) = polled {
                return Ok((ended.ok, ended.text));
            }
            if self.cancel.is_cancelled() {
                return Err(FinishReason::Cancelled);
            }
            if Instant::now() >= self.deadline {
                return Err(FinishReason::Timeout);
            }
            let Ok(request) = asked.try_recv() else {
                idle += 1;
                if idle > STUCK_POLLS {
                    return Ok((
                        false,
                        "Script error: the script waits for a promise nothing will settle (no tool call is open, and there are no timers)"
                            .into(),
                    ));
                }
                tokio::task::yield_now().await;
                continue;
            };
            idle = 0;
            calls += 1;
            let id = format!("{}.{calls}", call.id);
            let answer = self
                .script_call(id, request.name, request.arguments, sink)
                .await?;
            // The script may have ended meanwhile (an error elsewhere): nobody left to hear the answer.
            let _ = request.reply.send(answer);
        }
    }

    /// One call from a script: refused for what a script may not start (another script, a subagent), answered from
    /// the run's own store for `read_full_result`, otherwise run like a direct call. Its events carry `<call>.<n>`.
    async fn script_call(
        &mut self,
        id: String,
        name: String,
        arguments: String,
        sink: &mut dyn EventSink,
    ) -> Result<(bool, String), FinishReason> {
        sink.emit(Event::tool_call(self.turns, &id, &name, &arguments));
        // Another script, a subagent, or the owner's plan question are the model's own steps, not a script's.
        // T36c: nor a message to another pane — each one is the model's own word, judged and seen as such.
        if name == codemode::NAME
            || name == crate::task::NAME
            || name == planning::EXIT_PLAN
            || crate::config::PANE_TOOLS.contains(&name.as_str())
            || name == crate::config::LOOP_TOOL
        {
            let text = format!("not permitted: a script cannot start {name}");
            sink.emit(Event::tool_result(self.turns, &id, &name, false, &text));
            return Ok((false, text));
        }
        if name == results::NAME {
            let (ok, text) = results::answer(&self.results, &arguments);
            sink.emit(Event::tool_result(self.turns, &id, &name, ok, &text));
            return Ok((ok, text.trim_start_matches("error: ").to_owned()));
        }
        let call = ToolCall {
            id,
            kind: "function".into(),
            function: crate::model::FunctionCall { name, arguments },
        };
        let mut text = self.perform(&call, sink).await?;
        // A script gets text: an image a call of it read is not shown to the model (T45).
        if self.shown.remove(&call.id).is_some() {
            text.push_str(" — not shown from a script: read it with read_file to see it");
        }
        // A tool's failure is text that starts with `error: ` (the toolbox's convention); the script gets a rejection.
        Ok(match text.strip_prefix("error: ") {
            Some(reason) => (false, reason.to_owned()),
            None => (true, text),
        })
    }

    /// The `task` arm (HS8, D2) of a single call — a call of a resumed run that had not started: judge it, run the
    /// subagent, hand its answer back as the tool's result. A reply's own `task` calls go through
    /// [`Self::run_task_batch`].
    async fn run_task(&mut self, call: &ToolCall, sink: &mut dyn EventSink) -> (bool, String) {
        let task = match self.prepare_task(call, sink).await {
            Ok(task) => task,
            Err((ok, text, refused)) => {
                self.refused_now.extend(refused);
                return (ok, text);
            }
        };
        let outcome = self
            .run_subagents(std::slice::from_ref(&task), sink)
            .await
            .pop()
            .unwrap_or_else(RunOutcome::lost);
        self.refused_now.extend(task.refused);
        self.subagent_refusals += outcome.refusals;
        self.task_answer(outcome)
    }

    /// A `task` call up to its subagent's start: the limit, the arguments, the gate's word on it (D2.2 — the parent's
    /// judgement and event, read before the subagent runs, never inside its stream). A subagent that will run counts
    /// against `config.max_subagents` from here (T40: several start before any ends); one refused or asked for
    /// wrongly does not. `Err` with the call's result and the refusals when it does not run.
    async fn prepare_task(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<Subagent, (bool, String, Vec<Refusal>)> {
        let config = self.config;
        if self.subagents_started >= config.max_subagents {
            let limit = config.max_subagents;
            return Err((
                false,
                format!(
                    "error: this run has started its {limit} subagents; do the rest of the work yourself"
                ),
                Vec::new(),
            ));
        }
        let parsed = serde_json::from_str::<Value>(&call.function.arguments);
        let (prompt, label, agent) = match &parsed {
            Ok(arguments @ Value::Object(_)) => {
                match crate::task::prompt_of(arguments).and_then(|prompt| {
                    let agent = crate::task::agent_of(arguments, &config.agents)?.cloned();
                    Ok((prompt, agent))
                }) {
                    Ok((prompt, agent)) => (
                        prompt,
                        arguments
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("subagent")
                            .to_owned(),
                        agent,
                    ),
                    Err(reason) => return Err((false, format!("error: {reason}"), Vec::new())),
                }
            }
            Ok(_) => {
                return Err((
                    false,
                    "error: the arguments must be a JSON object".to_owned(),
                    Vec::new(),
                ));
            }
            Err(err) => {
                return Err((
                    false,
                    format!("error: the arguments are not valid JSON: {err}"),
                    Vec::new(),
                ));
            }
        };
        let mut refused = Vec::new();
        // B1: `task` is judged like any other call of a name the rules know.
        if let Some(gate) = self.tools.gate() {
            // A named agent is named in the ask, with the provider and model its file picks (T37a).
            let access = match &agent {
                Some(agent) => Access::Agent {
                    what: crate::permissions::printable(&agent.ask_line()),
                },
                None => Access::Other,
            };
            let judged = gate.judge(crate::task::NAME, &access).await;
            for entry in gate.take_log() {
                sink.emit(Event::permission(self.turns, &call.id, &entry));
                if !entry.allowed {
                    refused.push(Refusal {
                        tool: entry.tool,
                        target: entry.target,
                        reason: entry.reason,
                    });
                }
            }
            if let Err(why) = judged {
                return Err((false, format!("error: {why}"), refused));
            }
        }
        // T37a: an agent with a model of its own gets it built now; one that cannot be built is the call's answer and
        // starts nothing — never a run on another model than the file names.
        let picked = match &agent {
            Some(agent) if agent.own_model() => {
                let Some(models) = &config.models else {
                    return Err((
                        false,
                        format!(
                            "error: agent {}: this host cannot run a model of an agent's own",
                            agent.name
                        ),
                        refused,
                    ));
                };
                // Bounded like the run itself: a provider that does not answer neither holds the run past its time
                // nor past the owner's stop.
                let building = models.0.model(
                    agent.provider.as_deref(),
                    agent.model.as_deref(),
                    agent.effort,
                );
                let left = self
                    .deadline
                    .saturating_duration_since(Instant::now())
                    .min(MODEL_BUILD_TIMEOUT);
                let built = tokio::select! {
                    built = tokio::time::timeout(left, building) => built
                        .unwrap_or_else(|_| Err("its model did not answer in time".to_owned())),
                    () = self.cancel.cancelled() => Err("stopped".to_owned()),
                };
                match built {
                    Ok(picked) => Some(picked),
                    Err(why) => {
                        return Err((
                            false,
                            format!("error: agent {}: {why}", agent.name),
                            refused,
                        ));
                    }
                }
            }
            _ => None,
        };
        let mut sub = config.clone();
        // D5: the caller's instructions are the parent's contract; the subagent gets its own prompt and
        // whatever the parent's model put into `prompt` — a named agent its file's text (T37a).
        sub.instructions = agent.as_ref().map(|agent| agent.prompt.clone());
        sub.max_turns = config.subagent_max_turns;
        // Its usage counts from nothing: it may spend only what the run has left (glut-cards C6) — with the subagents
        // beside it, which count against the same rest (T40, `Group`). On a model of its own, at that model's prices.
        sub.budget = Budget {
            paid_usd: config.budget.paid_usd + self.paid_usd,
            ..config.budget
        }
        .less(&self.usage);
        if let Some(picked) = &picked {
            // Unpriced, it counts at the run's prices: a cap in money never goes blind for a model it does not know.
            sub.budget.prices = picked.prices.or(config.budget.prices);
            // Its own model's window and output limit, not the parent's: a smaller model compacts in time.
            sub.compaction.context_window = picked.window;
            sub.max_output_tokens = None;
        }
        self.subagents_started += 1;
        Ok(Subagent {
            id: call.id.clone(),
            label,
            prompt,
            config: sub,
            refused,
            agent,
            model: picked.map(|picked| picked.model),
        })
    }

    /// Runs `tasks` at once, at most `config.parallel_subagents` at a time and the rest in the order asked, and
    /// returns their outcomes in that order. Their events arrive in `sink` as they come, each wrapped in the
    /// [`Event::Sub`] of its `task` call; what they spend is the run's (D2.4).
    async fn run_subagents(
        &mut self,
        tasks: &[Subagent],
        sink: &mut dyn EventSink,
    ) -> Vec<RunOutcome> {
        let group = Group::default();
        let permits = tokio::sync::Semaphore::new(self.config.parallel_subagents.max(1) as usize);
        let (events, mut arriving) = tokio::sync::mpsc::unbounded_channel();
        let place = Place {
            model: self.model,
            tools: self.tools,
            cancel: self.cancel,
            deadline: self.deadline,
            depth: self.depth + 1,
            turn: self.turns,
            timeout: self.config.subagent_timeout,
        };
        let running: Vec<_> = tasks
            .iter()
            .map(|task| Box::pin(place.subagent(task, &group, &permits, events.clone())))
            .collect();
        drop(events);
        let all = join_all(running);
        tokio::pin!(all);
        let outcomes = loop {
            tokio::select! {
                Some(event) = arriving.recv() => sink.emit(event),
                outcomes = &mut all => break outcomes,
            }
        };
        while let Ok(event) = arriving.try_recv() {
            sink.emit(event);
        }
        for (task, outcome) in tasks.iter().zip(&outcomes) {
            self.usage += outcome.usage;
            // A subagent on a model of its own costs at its prices: the difference to this run's goes beside the
            // tokens (T37a).
            if task.model.is_some()
                && let (Some(own), Some(mine)) =
                    (task.config.budget.prices, self.config.budget.prices)
            {
                self.paid_usd += own.cost_usd(&outcome.usage) - mine.cost_usd(&outcome.usage);
            }
        }
        outcomes
    }

    /// A subagent's outcome as its `task` call's result.
    fn task_answer(&self, outcome: RunOutcome) -> (bool, String) {
        match outcome.reason {
            // Called off alone (T40, `Cancel::cancel_subagent`): the run goes on, the model is told whose doing it was.
            FinishReason::Cancelled if !self.cancel.is_cancelled() => (
                false,
                "error: the owner stopped this subagent before it answered".to_owned(),
            ),
            FinishReason::Answered => match outcome.answer {
                // Bounded like every tool result when it enters the conversation (`answer_with`).
                Some(answer) => (true, answer),
                None => (true, "the subagent finished without text".to_owned()),
            },
            reason => (
                false,
                format!("error: the subagent stopped without answering: {reason:?}"),
            ),
        }
    }
}

/// A `task` call ready to run (HS8, T40): its id and label, the subagent's prompt and config, and what the gate
/// refused while judging the call.
struct Subagent {
    id: String,
    label: String,
    prompt: String,
    config: RunConfig,
    refused: Vec<Refusal>,
    /// The named agent it runs as (T37a), if any.
    agent: Option<crate::agents::Agent>,
    /// The model of its own an agent named; `None`: the parent's.
    model: Option<Arc<crate::model::Model>>,
}

/// What the subagents of one parent share from it: its model, tools and cancel, the time it has, where they nest.
struct Place<'a, M: ChatModel> {
    model: &'a M,
    tools: &'a Toolbox,
    cancel: &'a Cancel,
    deadline: Instant,
    depth: u32,
    turn: u32,
    timeout: std::time::Duration,
}

impl<M: ChatModel> Place<'_, M> {
    /// One subagent, once a place among those running at once is free: its own run, as the subagent of its `task`
    /// call (its judgements and asks its own), its events sent to the parent.
    async fn subagent(
        &self,
        task: &Subagent,
        group: &Group,
        permits: &tokio::sync::Semaphore,
        events: tokio::sync::mpsc::UnboundedSender<Event>,
    ) -> RunOutcome {
        // Never closed: the semaphore lives as long as the batch.
        let _place = permits.acquire().await;
        let mut config = task.config.clone();
        // From its own start, which may come late: the subagent never outlives the run that started it (D2.3).
        config.timeout = self
            .timeout
            .min(self.deadline.saturating_duration_since(Instant::now()));
        let mut sink = SubSink {
            depth: self.depth,
            task: Some(task.id.clone()),
            inner: &mut ChannelSink(events),
        };
        let sibling = Sibling {
            group,
            id: task.id.clone(),
        };
        let cancel = self.cancel.subagent(&task.id);
        let run = async {
            let opening = Opening::Fresh {
                earlier: Vec::new(),
                prompt: &task.prompt,
            };
            let (outcome, _) = match &task.model {
                Some(model) => {
                    Box::pin(run_inner(
                        &config,
                        model.as_ref(),
                        self.tools,
                        opening,
                        &mut sink,
                        &cancel,
                        self.depth,
                        Some(sibling),
                    ))
                    .await
                }
                None => {
                    Box::pin(run_inner(
                        &config,
                        self.model,
                        self.tools,
                        opening,
                        &mut sink,
                        &cancel,
                        self.depth,
                        Some(sibling),
                    ))
                    .await
                }
            };
            // What the subagent judged but did not get to report (it was cancelled mid-call) still belongs to its
            // stream (D2.4).
            if let Some(gate) = self.tools.gate() {
                for entry in gate.take_log() {
                    sink.emit(Event::permission(self.turn, &task.id, &entry));
                }
            }
            outcome
        };
        // A named agent's `tools` narrow its own calls only (T37a).
        let tools = task
            .agent
            .as_ref()
            .and_then(|agent| Some((format!("the agent {}", agent.name), agent.tools.clone()?)));
        let outcome =
            crate::permissions::as_agent(task.id.clone(), task.label.clone(), tools, run).await;
        self.cancel.forget(&task.id);
        outcome
    }
}

/// Polls `futures` together until each is done; their outputs in their order.
async fn join_all<F: std::future::Future + Unpin>(mut futures: Vec<F>) -> Vec<F::Output> {
    let mut outputs: Vec<Option<F::Output>> = futures.iter().map(|_| None).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (future, output) in futures.iter_mut().zip(outputs.iter_mut()) {
            if output.is_none() {
                match std::pin::Pin::new(future).poll(cx) {
                    std::task::Poll::Ready(done) => *output = Some(done),
                    std::task::Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            std::task::Poll::Pending
        } else {
            std::task::Poll::Ready(())
        }
    })
    .await;
    outputs.into_iter().flatten().collect()
}

/// What the subagents of one batch share (TUI T40): what they spent together, which the budget is checked against
/// before each of their model calls; a lock per path, held by a write; and which of them last wrote a file.
#[derive(Default)]
struct Group {
    usage: std::sync::Mutex<Usage>,
    /// Their money, each one's tokens at its own prices (T37a).
    paid: std::sync::Mutex<f64>,
    unpriced: std::sync::atomic::AtomicBool,
    paths: std::sync::Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>,
    written: std::sync::Mutex<HashMap<PathBuf, (u64, String)>>,
    writes: std::sync::atomic::AtomicU64,
}

impl Group {
    /// Adds what one of them spent, and its cost at its own `prices` when it has them (T37a); returns what all of
    /// them have spent.
    fn spend(&self, usage: Usage, prices: Option<crate::config::Prices>) -> Usage {
        match prices {
            Some(prices) => {
                *self
                    .paid
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner()) += prices.cost_usd(&usage);
            }
            None => self
                .unpriced
                .store(true, std::sync::atomic::Ordering::Relaxed),
        }
        let mut total = self
            .usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        *total += usage;
        *total
    }

    /// What all of them cost, each at its own prices; `None` once one of them spent without prices.
    fn paid(&self) -> Option<f64> {
        if self.unpriced.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        Some(
            *self
                .paid
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        )
    }

    fn spent(&self) -> Usage {
        *self
            .usage
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// The path's lock, held for one write.
    async fn hold(&self, path: &Path) -> tokio::sync::OwnedMutexGuard<()> {
        let lock = Arc::clone(
            self.paths
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .entry(path.to_owned())
                .or_default(),
        );
        lock.lock_owned().await
    }

    /// The path's last write among them (0: none).
    fn version(&self, path: &Path) -> u64 {
        self.written
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(path)
            .map_or(0, |(version, _)| *version)
    }

    /// Whether another than `me` wrote the path after the write `seen`.
    fn changed_by_other(&self, path: &Path, me: &str, seen: u64) -> bool {
        matches!(
            self.written
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .get(path),
            Some((version, by)) if *version > seen && by != me
        )
    }

    /// `me` wrote the path: its new version.
    fn wrote(&self, path: &Path, me: &str) -> u64 {
        let version = self.writes.fetch_add(1, Ordering::SeqCst) + 1;
        self.written
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .insert(path.to_owned(), (version, me.to_owned()));
        version
    }
}

/// A subagent's place in its batch: the batch it shares, and its own `task` call's id.
struct Sibling<'a> {
    group: &'a Group,
    id: String,
}

/// A path as the tools reach it, below `cwd`, without `.` and `..` — the same file named two ways is one lock.
fn lexical(cwd: &Path, path: &str) -> PathBuf {
    let mut whole = PathBuf::new();
    for part in cwd.join(path).components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                whole.pop();
            }
            other => whole.push(other),
        }
    }
    whole
}

/// The parent's end of a subagent's events while several run at once: each sent on, in the order it came.
struct ChannelSink(tokio::sync::mpsc::UnboundedSender<Event>);

impl EventSink for ChannelSink {
    fn emit(&mut self, event: Event) {
        // The parent drains until every subagent ended: a send can only fail after that, when nobody reads.
        let _ = self.0.send(event);
    }
}

/// The nested run's view of the parent's sink (HS8, D8): every event it emits arrives wrapped in
/// [`Event::Sub`] at the depth it happened at, with the `task` call it answers — nothing dropped. Accounting never
/// counts events; the parent sums `RunOutcome`s (D2.4), so a viewer may count each run's own `Finished` and no other's.
struct SubSink<'a> {
    depth: u32,
    task: Option<String>,
    inner: &'a mut dyn EventSink,
}

impl EventSink for SubSink<'_> {
    fn emit(&mut self, event: Event) {
        self.inner.emit(Event::Sub {
            depth: self.depth,
            task: self.task.clone(),
            event: Box::new(event),
        });
    }
}

/// The hooks' session of a run (HS7, D8), built on the first event that needs one: a session that cannot be
/// built leaves the run without hooks, with an error note — B3 applied to the runner itself.
enum HookSession {
    NotBuilt,
    Ready(Box<Session>),
    GaveUp,
}

impl HookSession {
    /// The session to run a hook in, or `None` when it could not be built (the run then goes on without
    /// hooks). One session per run: the profile snapshot is taken once, not per hook.
    async fn get(&mut self, config: &RunConfig, sink: &mut dyn EventSink) -> Option<&Session> {
        if matches!(self, HookSession::GaveUp) {
            return None;
        }
        if matches!(self, HookSession::NotBuilt) {
            // The hooks get what a command gets (D5): the owner's shell, the profile snapshot, the cleaned
            // environment, and the sandbox the run's settings word on.
            let options = ShellOptions {
                sandbox: config.sandbox.clone(),
                ..ShellOptions::from_environment()
            };
            *self = match Session::new(&config.cwd, options) {
                Ok(session) => HookSession::Ready(Box::new(session)),
                Err(err) => {
                    sink.emit(Event::Error {
                        message: format!("hooks left out: {err}"),
                    });
                    HookSession::GaveUp
                }
            };
        }
        match self {
            HookSession::Ready(session) => Some(&**session),
            _ => None,
        }
    }
}

/// The run's hooks (HS7) with what every one of their events needs: the session they run in, built on the first
/// event that needs one (D8), the run's id for their payloads (D4), its deadline, stop flag and cancel, and the
/// gate their commands are judged by.
struct RunHooks<'a> {
    session: HookSession,
    config: &'a RunConfig,
    session_id: String,
    deadline: Instant,
    stop: &'a StopFlag,
    cancel: &'a Cancel,
    gate: Option<Arc<Gate>>,
}

impl RunHooks<'_> {
    /// One hook event of the run (HS7, D3), and nothing at all for a run without hooks. The run's deadline and
    /// its cancel are honoured — dropping the wait mid-hook kills the hook's process group; every hook is
    /// reported as an event inside. `Err(reason)` when the run itself ran out of time or was cancelled while a
    /// hook worked; a hook's own failure is a note and `Ok` (B3).
    ///
    /// `extra` builds the event's own payload fields; it is only called when there are hooks, so a run without
    /// any pays nothing for them.
    async fn fire(
        &mut self,
        event: HookEvent,
        target: &str,
        extra: impl FnOnce() -> Value,
        turn: Option<u32>,
        sink: &mut dyn EventSink,
    ) -> Result<HookOutcome, FinishReason> {
        let config = self.config;
        if config.hooks.is_empty() {
            return Ok(HookOutcome::default());
        }
        let stdin = hooks::payload(&self.session_id, &config.cwd, event, extra());
        let Some(session) = self.session.get(config, sink).await else {
            return Ok(HookOutcome::default());
        };
        let limit = self.deadline.saturating_duration_since(Instant::now());
        let gate = self.gate.clone();
        let hooks = &config.hooks;
        let running = hooks::run_event(
            turn, event, target, &stdin, hooks, session, gate, self.stop, limit, sink,
        );
        let outcome = tokio::select! {
            outcome = running => outcome,
            () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
        };
        Ok(outcome)
    }
}

/// What a blocking hook said, or `fallback` when it gave no reason.
fn refusal(outcome: HookOutcome, fallback: &str) -> String {
    outcome.context.unwrap_or_else(|| fallback.to_owned())
}

/// The arguments that make two `read_file` calls the same read: the path (without a leading `./`), the range and
/// whether it asked for the outline.
fn read_key(arguments: &str) -> Option<(String, Option<u64>, Option<u64>, bool)> {
    let args = serde_json::from_str::<Value>(arguments).ok()?;
    let path = args.get("path")?.as_str()?;
    let path = path.trim_start_matches("./").to_owned();
    let offset = args
        .get("offset")
        .and_then(Value::as_u64)
        .filter(|o| *o > 1);
    let limit = args.get("limit").and_then(Value::as_u64);
    let outline = args.get("outline").and_then(Value::as_bool) == Some(true);
    Some((path, offset, limit, outline))
}

/// A short answer for a `read_file` whose result the conversation already holds word for word (Glut lean, N4): the
/// same file, the same range, unchanged since. `None` when it does not — the file changed, it was never read, or the
/// earlier result is gone from the conversation (a `/compact`) — and the result is sent as it is.
fn unchanged_read(messages: &[Message], call: &ToolCall, output: &str) -> Option<String> {
    if call.function.name != "read_file" || output.starts_with("error:") {
        return None;
    }
    let key = read_key(&call.function.arguments)?;
    let earlier = messages.iter().rev().find_map(|message| {
        let id = message.tool_call_id.as_deref()?;
        (message.role == Role::Tool && message.content.as_deref() == Some(output)).then_some(id)
    })?;
    let same_read = messages.iter().any(|message| {
        message.tool_calls.iter().any(|c| {
            c.id == earlier
                && c.function.name == "read_file"
                && read_key(&c.function.arguments).as_ref() == Some(&key)
        })
    });
    let pointer = format!(
        "[unchanged: the same text as your call {earlier} read above — {} has not changed since]",
        key.0
    );
    // A tiny file is shorter than the pointer to it: then it is simply sent again.
    (same_read && pointer.len() < output.len()).then_some(pointer)
}

/// One compacted output for the count (N9): its kind and characters before and after.
struct Spared {
    /// The kind of output ("cargo").
    what: &'static str,
    /// Its characters before …
    before: usize,
    /// … and compacted.
    after: usize,
}

/// The command of a foreground `bash` call, `None` for any other call.
fn foreground_command(call: &ToolCall) -> Option<String> {
    if call.function.name != "bash" {
        return None;
    }
    let args = serde_json::from_str::<Value>(&call.function.arguments).ok()?;
    if args.get("run_in_background").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    args.get("command")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The kind of command a foreground `bash` call pipes on, when Glut knows it (N8).
fn piped(call: &ToolCall) -> Option<&'static str> {
    crate::tools::output::piped(&foreground_command(call)?)
}

/// A foreground `bash` call's output in its compact form when Glut knows the command (Glut lean, N3) — a chain's part
/// by part where the tool said where they lie (N7): the raw output goes into `store`, and a note on top says what was
/// left out and how to read the raw output back. Also what each compaction spared, for the count.
fn compacted(
    call: &ToolCall,
    output: String,
    store: &ResultStore,
    parts: Option<Vec<crate::tools::Part>>,
) -> (String, Vec<Spared>) {
    let Some(command) = foreground_command(call) else {
        return (output, Vec::new());
    };
    let pieces: Vec<(&str, &str)> = match &parts {
        Some(parts) if covers(parts, &output) => parts
            .iter()
            .map(|part| (part.command.as_str(), &output[part.range.clone()]))
            .collect(),
        _ => vec![(command.as_str(), output.as_str())],
    };
    let mut text = String::with_capacity(output.len());
    let mut spared = Vec::new();
    let mut left_out = Vec::new();
    for (part, piece) in &pieces {
        match crate::tools::output::compact(part, piece) {
            Some(compact) => {
                text.push_str(&compact.text);
                // The next part's output starts on a line of its own, as it did.
                if piece.ends_with('\n') && !compact.text.ends_with('\n') {
                    text.push('\n');
                }
                spared.push(Spared {
                    what: compact.what,
                    before: piece.chars().count(),
                    after: compact.text.chars().count(),
                });
                left_out.push(if pieces.len() > 1 {
                    format!("for `{part}`: {}", compact.left_out)
                } else {
                    compact.left_out.to_owned()
                });
            }
            None => text.push_str(piece),
        }
    }
    if spared.is_empty() {
        return (output, spared);
    }
    let lines_before = output.lines().count();
    let lines_after = text.lines().count();
    let id = store.put(output);
    let shown = format!(
        "[Glut compacted this output: {lines_before} → {lines_after} lines, left out: {}. \
         The raw output: `{}` with id \"{id}\".]\n{text}",
        left_out.join("; "),
        results::NAME,
    );
    (shown, spared)
}

/// Whether `parts` lie side by side over the whole of `output`, each on character boundaries.
fn covers(parts: &[crate::tools::Part], output: &str) -> bool {
    let mut at = 0;
    for part in parts {
        if part.range.start != at
            || part.range.end < part.range.start
            || !output.is_char_boundary(part.range.end)
        {
            return false;
        }
        at = part.range.end;
    }
    at == output.len()
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;
    use crate::model::{FunctionCall, Reply, Role, ToolCall, ToolSpec};
    use crate::tools::{Tool, ToolFuture};

    /// E4: a turn that goes on from earlier ones starts without their reasoning — it is never sent again, so it is
    /// not carried into the chat's file or the journal either.
    #[test]
    fn a_request_carries_the_newest_images_and_none_to_a_model_that_sees_none() {
        let image = |number: u32, bytes: u64| crate::images::Image {
            number,
            path: std::path::PathBuf::from(format!("/h/.glut/images/{number}.png")),
            media: "image/png".into(),
            width: 1,
            height: 1,
            bytes,
        };
        let mut messages: Vec<Message> = (1..=10)
            .map(|number| {
                let mut message = Message::user(format!("look {number}"));
                message.images = vec![image(number, 100)];
                message
            })
            .collect();
        let sent = sendable(&messages, Some(true));
        let carried: Vec<u32> = sent
            .iter()
            .flat_map(|m| &m.images)
            .map(|i| i.number)
            .collect();
        assert_eq!(carried, (3..=10).collect::<Vec<_>>(), "the newest eight");
        assert!(
            sent[0]
                .content
                .as_deref()
                .unwrap()
                .contains("[image #1 · 1×1 · 1 kB] (not sent again)")
        );
        let none = sendable(&messages, Some(false));
        assert!(none.iter().all(|message| message.images.is_empty()));
        // Within the bytes: one image as large as the budget leaves no room for an older one.
        messages[9].images = vec![image(10, MOST_SENT_IMAGE_BYTES * 3 / 4)];
        let sent = sendable(&messages, None);
        let carried: Vec<u32> = sent
            .iter()
            .flat_map(|m| &m.images)
            .map(|i| i.number)
            .collect();
        assert_eq!(carried, [10]);
        assert!(matches!(
            sendable(&[Message::user("hi")], None),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn a_new_turn_starts_without_the_earlier_turns_reasoning() {
        let thought = Message {
            reasoning: Some("old thinking".into()),
            reasoning_details: vec![json!({"type": "reasoning.text", "text": "old"})],
            ..Message::user("answer")
        };
        let thought = Message {
            role: Role::Assistant,
            ..thought
        };
        let config = RunConfig::new(std::env::temp_dir());
        let messages = opening_messages(
            &config,
            "m",
            vec![Message::user("first"), thought],
            "second",
            0,
            "",
        );
        assert_eq!(messages.len(), 4);
        assert!(messages.iter().all(|message| !message.has_reasoning()));
        assert_eq!(messages[2].content.as_deref(), Some("answer"));
    }

    /// A model that plays prepared replies in order and records each conversation it was sent.
    struct Script {
        replies: Mutex<Vec<Result<Reply, HarnessError>>>,
        seen: Mutex<Vec<Vec<Message>>>,
        /// The tool names each request offered, in call order (HS8): the parent's turns carry `task`, a
        /// subagent's never do (B2/D3).
        offered: Mutex<Vec<Vec<String>>>,
        delay: Duration,
        /// What `context_window` answers, and how often it was asked (#111, K2).
        window: Option<u32>,
        window_asked: std::sync::atomic::AtomicU32,
    }

    impl Script {
        fn new(replies: Vec<Result<Reply, HarnessError>>) -> Self {
            Script {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
                offered: Mutex::new(Vec::new()),
                delay: Duration::ZERO,
                window: None,
                window_asked: std::sync::atomic::AtomicU32::new(0),
            }
        }
    }

    impl ChatModel for Script {
        async fn complete(
            &self,
            messages: &[Message],
            tools: &[ToolSpec],
            _max: Option<u32>,
        ) -> Result<Reply, HarnessError> {
            self.seen.lock().unwrap().push(messages.to_vec());
            self.offered
                .lock()
                .unwrap()
                .push(tools.iter().map(|spec| spec.name.clone()).collect());
            tokio::time::sleep(self.delay).await;
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                return Ok(calls(&[("again", "echo", r#"{"text":"more"}"#)]));
            }
            replies.remove(0)
        }

        fn name(&self) -> &str {
            "script"
        }

        async fn context_window(&self) -> Option<u32> {
            self.window_asked
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.window
        }
    }

    fn text(answer: &str) -> Reply {
        Reply {
            message: Message {
                role: Role::Assistant,
                content: Some(answer.into()),
                tool_calls: vec![],
                tool_call_id: None,
                reasoning: None,
                reasoning_content: None,
                reasoning_details: Vec::new(),
                images: Vec::new(),
            },
            usage: Usage {
                input_tokens: 10,
                cached_input_tokens: 0,
                output_tokens: 2,
            },
        }
    }

    fn calls(list: &[(&str, &str, &str)]) -> Reply {
        Reply {
            message: Message {
                role: Role::Assistant,
                content: None,
                tool_calls: list
                    .iter()
                    .map(|(id, name, args)| ToolCall {
                        id: (*id).into(),
                        kind: "function".into(),
                        function: FunctionCall {
                            name: (*name).into(),
                            arguments: (*args).into(),
                        },
                    })
                    .collect(),
                tool_call_id: None,
                reasoning: None,
                reasoning_content: None,
                reasoning_details: Vec::new(),
                images: Vec::new(),
            },
            usage: Usage {
                input_tokens: 7,
                cached_input_tokens: 0,
                output_tokens: 1,
            },
        }
    }

    /// A reply whose single call is `task` (HS8), with the arguments as JSON.
    fn task_call(id: &str, arguments: Value) -> Reply {
        let arguments = arguments.to_string();
        calls(&[(id, "task", arguments.as_str())])
    }

    /// A tool that calls the run off (HS8's cancel test: fired from *inside* the subagent).
    struct CancelRun(Cancel);

    impl Tool for CancelRun {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "cancel_run".into(),
                description: "c".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Other
        }

        fn call(&self, _arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            let cancel = self.0.clone();
            Box::pin(async move {
                cancel.cancel();
                Ok("the run was called off".to_owned())
            })
        }
    }

    struct Echo;

    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "echo".into(),
                description: "e".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Other
        }

        fn call(&self, arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            Box::pin(async move { Ok(arguments["text"].as_str().unwrap_or_default().to_owned()) })
        }
    }

    /// A stand-in for the screen's `tell` (TUI T36c).
    struct PaneTool;

    impl Tool for PaneTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: crate::config::PANE_TOOLS[0].into(),
                description: "t".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Other
        }

        fn call(&self, _arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            Box::pin(async move { Ok("told".to_owned()) })
        }
    }

    #[test]
    fn the_panes_tools_are_offered_only_where_the_screen_is_split_and_never_to_a_subagent() {
        let mut tools = Toolbox::new();
        tools.extend([
            Box::new(PaneTool) as Box<dyn Tool>,
            Box::new(Echo) as Box<dyn Tool>,
        ]);
        let names = |config: &RunConfig, depth: u32| -> Vec<String> {
            Offer::of(config, &tools, depth, false)
                .specs
                .iter()
                .map(|spec| spec.name.clone())
                .collect()
        };
        let mut config = RunConfig::new("/tmp");
        config.codemode = false;
        assert!(!names(&config, 0).contains(&"tell".to_owned()));
        config.pane_tools = true;
        assert!(names(&config, 0).contains(&"tell".to_owned()));
        assert!(
            !names(&config, 1).contains(&"tell".to_owned()),
            "never to a subagent"
        );
        assert!(names(&config, 1).contains(&"echo".to_owned()));
    }

    /// A stand-in for the screen's `end_loop` (TUI T60a).
    struct LoopTool;

    impl Tool for LoopTool {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: crate::config::LOOP_TOOL.into(),
                description: "l".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Own
        }

        fn call(&self, _arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            Box::pin(async move { Ok("ends".to_owned()) })
        }
    }

    #[test]
    fn the_loops_end_is_offered_only_to_a_loop_and_never_to_a_subagent() {
        let mut tools = Toolbox::new();
        tools.extend([
            Box::new(LoopTool) as Box<dyn Tool>,
            Box::new(PaneTool) as Box<dyn Tool>,
        ]);
        for codemode in [false, true] {
            let names = |config: &RunConfig, depth: u32| -> Vec<String> {
                let offer = Offer::of(config, &tools, depth, false);
                offer
                    .specs
                    .iter()
                    .chain(&offer.catalog.specs)
                    .map(|spec| spec.name.clone())
                    .collect()
            };
            let mut config = RunConfig::new("/tmp");
            config.codemode = codemode;
            config.pane_tools = true;
            assert!(!names(&config, 0).contains(&"end_loop".to_owned()));
            config.loop_tool = true;
            config.pane_tools = false;
            let offered = names(&config, 0);
            assert!(offered.contains(&"end_loop".to_owned()));
            assert!(
                !offered.contains(&"tell".to_owned()),
                "the panes' tool keeps its own switch"
            );
            assert!(
                !names(&config, 1).contains(&"end_loop".to_owned()),
                "never to a subagent"
            );
        }
    }

    #[test]
    fn task_is_not_offered_to_a_role_that_names_it_not() {
        let mut tools = Toolbox::new();
        let gate = Arc::new(Gate::new(
            crate::permissions::RuleSet::default(),
            Mode::Default,
            Arc::new(crate::permissions::Nobody),
        ));
        tools.set_gate(Arc::clone(&gate));
        let mut config = RunConfig::new("/tmp");
        config.codemode = false;
        let offered = |tools: &Toolbox| -> bool {
            Offer::of(&config, tools, 0, false)
                .specs
                .iter()
                .any(|spec| spec.name == crate::task::NAME)
        };
        assert!(offered(&tools));
        gate.set_role(Some((
            "the role reviewer".into(),
            vec![crate::permissions::Rule::parse("Read").unwrap()],
        )));
        assert!(!offered(&tools));
    }

    fn toolbox() -> Toolbox {
        let mut tools = Toolbox::new();
        tools.extend([Box::new(Echo) as Box<dyn Tool>]);
        tools
    }

    #[test]
    fn an_unchanged_re_read_is_answered_with_a_pointer_and_a_changed_one_whole() {
        let read = |id: &str, args: &str| ToolCall {
            id: id.into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "read_file".into(),
                arguments: args.into(),
            },
        };
        let first = read("r1", r#"{"path":"src/a.rs"}"#);
        let text: String = (1..=40)
            .map(|n| format!("{n:>6}\tfn a{n}() {{}}\n"))
            .collect();
        let text = text.as_str();
        let mut messages = vec![Message::user("go")];
        messages.push(Message {
            role: Role::Assistant,
            content: None,
            tool_calls: vec![first],
            tool_call_id: None,
            reasoning: None,
            reasoning_content: None,
            reasoning_details: Vec::new(),
            images: Vec::new(),
        });
        messages.push(Message::tool("r1", text));
        let again = read("r2", r#"{"path":"./src/a.rs"}"#);
        let note = unchanged_read(&messages, &again, text).unwrap();
        assert!(
            note.contains("call r1") && note.contains("src/a.rs"),
            "{note}"
        );
        // Changed since, another range, another file, an error: sent as they are.
        assert_eq!(
            unchanged_read(&messages, &again, &text.replace("a1()", "b1()")),
            None
        );
        assert_eq!(
            unchanged_read(
                &messages,
                &read("r3", r#"{"path":"src/a.rs","offset":5}"#),
                text
            ),
            None
        );
        assert_eq!(
            unchanged_read(&messages, &read("r4", r#"{"path":"src/b.rs"}"#), text),
            None
        );
        assert_eq!(
            unchanged_read(&messages, &again, "error: no such file"),
            None
        );
        // A file shorter than the pointer to it is sent again.
        let tiny = "     1\tx\n";
        let mut with_tiny = messages.clone();
        with_tiny.push(Message::tool("r1", tiny));
        assert_eq!(unchanged_read(&with_tiny, &again, tiny), None);
        // After a `/compact` the earlier result is gone: the file is sent again.
        assert_eq!(unchanged_read(&messages[..1], &again, text), None);
    }

    #[tokio::test]
    async fn a_relocated_result_is_read_back_whole_through_the_tool() {
        let long: String = (0..70_000)
            .map(|n| char::from(b'a' + (n % 26) as u8))
            .collect();
        let args = json!({ "text": long }).to_string();
        // A store with a known tag, so the script can name the id the note will name.
        let dir = std::env::temp_dir().join(format!("glut-readback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let config = RunConfig {
            results: Some(results::StoreSpec {
                dir: dir.clone(),
                tag: "fixed".into(),
            }),
            ..RunConfig::new("/w")
        };
        let reads: Vec<String> = [0, 30_000, 60_000]
            .iter()
            .map(|offset| json!({"id": "fixed-1", "offset": offset}).to_string())
            .collect();
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", args.as_str())])),
            Ok(calls(&[("r1", results::NAME, reads[0].as_str())])),
            Ok(calls(&[("r2", results::NAME, reads[1].as_str())])),
            Ok(calls(&[("r3", results::NAME, reads[2].as_str())])),
            Ok(text("Done.")),
        ]);
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let seen = model.seen.lock().unwrap();
        let last = seen.last().unwrap();
        let mut back = String::new();
        for message in last.iter().filter(|m| {
            m.tool_call_id
                .as_deref()
                .is_some_and(|id| id.starts_with('r'))
        }) {
            let part = message.content.as_deref().unwrap();
            back.push_str(part.split("\n\n[characters").next().unwrap());
        }
        assert_eq!(back, long, "every character comes back, in order");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_known_commands_output_is_compacted_with_a_note_and_its_raw_text_kept() {
        let bash = |args: Value| ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "bash".into(),
                arguments: args.to_string(),
            },
        };
        let mut raw = "   Compiling axiomata-core v0.1.0 (/w/crates/axiomata-core)\n".repeat(40);
        raw.push_str("warning: unused variable: `x`\n    Finished `dev` profile in 3.1s\n");
        let store = ResultStore::new("t");
        let (shown, spared) = compacted(
            &bash(json!({"command": "cargo build"})),
            raw.clone(),
            &store,
            None,
        );
        assert!(
            shown.starts_with("[Glut compacted this output: 42 → 2 lines"),
            "{shown}"
        );
        assert!(
            shown.ends_with("warning: unused variable: `x`\n    Finished `dev` profile in 3.1s\n")
        );
        assert_eq!(spared.len(), 1);
        assert_eq!(
            (spared[0].what, spared[0].before),
            ("cargo", raw.chars().count())
        );
        assert!(store.read("t-1", 0).unwrap().starts_with(&raw[..200]));
        // A job in the background, a piped command and any other tool stay as they are.
        let same = |call: ToolCall| compacted(&call, raw.clone(), &store, None).0;
        let job = json!({"command": "cargo build", "run_in_background": true});
        assert_eq!(same(bash(job)), raw);
        assert_eq!(same(bash(json!({"command": "cargo build | tail"}))), raw);
        let mut echo = bash(json!({"command": "cargo build"}));
        echo.function.name = "echo".into();
        assert_eq!(same(echo), raw);
    }

    #[test]
    fn a_chains_parts_are_compacted_each_by_its_own_command() {
        let command = "git status && cargo build && echo done";
        let call = ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "bash".into(),
                arguments: json!({ "command": command }).to_string(),
            },
        };
        let status = "On branch main\nChanges not staged for commit:\n  (use \"git add <file>...\" to update what will be committed)\n  (use \"git restore <file>...\" to discard changes in working directory)\n\tmodified:   a.rs\n\tmodified:   b.rs\n\tmodified:   c.rs\n\tmodified:   d.rs\n\tmodified:   e.rs\n\tmodified:   f.rs\n\tmodified:   g.rs\n\tmodified:   h.rs\n\nno changes added to commit (use \"git add\" and/or \"git commit -a\")\n";
        let mut build = "   Compiling axiomata-core v0.1.0 (/w/crates/axiomata-core)\n".repeat(40);
        build.push_str("    Finished `dev` profile in 3.1s\n");
        let raw = format!("{status}{build}done\n");
        let part = |command: &str, start: usize, end: usize| crate::tools::Part {
            command: command.to_owned(),
            range: start..end,
        };
        let (a, b) = (status.len(), status.len() + build.len());
        let parts = vec![
            part("git status", 0, a),
            part("cargo build", a, b),
            part("echo done", b, raw.len()),
        ];
        let store = ResultStore::new("t");
        let (shown, spared) = compacted(&call, raw.clone(), &store, Some(parts.clone()));
        let kinds: Vec<_> = spared.iter().map(|s| s.what).collect();
        assert_eq!(kinds, ["git status", "cargo"], "{shown}");
        assert!(shown.contains("for `git status`: "), "{shown}");
        assert!(shown.contains("; for `cargo build`: "), "{shown}");
        assert!(shown.contains("## main\n M a.rs\n"), "{shown}");
        // Each part's output starts on a line of its own; the part Glut does not know stays word for word.
        assert!(
            shown.contains(" M h.rs\n    Finished `dev` profile in 3.1s\ndone\n"),
            "{shown}"
        );
        assert!(!shown.contains("Compiling"), "{shown}");
        assert_eq!(store.read("t-1", 0).unwrap(), raw);
        // Parts that do not lie over the whole answer (a hook changed it) leave the chain whole.
        let mut gap = parts.clone();
        gap[1].range.start += 1;
        assert_eq!(compacted(&call, raw.clone(), &store, Some(gap)).0, raw);
        let mut short = parts;
        short.pop();
        assert_eq!(compacted(&call, raw.clone(), &store, Some(short)).0, raw);
    }

    #[tokio::test]
    async fn an_over_long_result_enters_the_conversation_bounded_once_and_stays_as_it_was() {
        let long: String = "x".repeat(crate::tools::MAX_TOOL_OUTPUT_CHARS) + &"y".repeat(5_000);
        let args = json!({ "text": long }).to_string();
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", args.as_str())])),
            Ok(calls(&[("c2", "echo", r#"{"text":"short"}"#)])),
            Ok(text("Done.")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let seen = model.seen.lock().unwrap();
        // Head and tail with the note between them — the end of the output is there, not cut away.
        let bounded = seen[1][3].content.as_deref().unwrap();
        assert!(bounded.chars().count() < crate::tools::MAX_TOOL_OUTPUT_CHARS + 300);
        assert!(bounded.starts_with("xxx") && bounded.ends_with("yyy"));
        assert!(bounded.contains(crate::results::NAME), "{bounded}");
        // The next request sends it byte for byte the same: the cached prefix holds.
        assert_eq!(seen[2][3].content.as_deref(), Some(bounded));
        assert_eq!(seen[2][5].content.as_deref(), Some("short"));
        // The tool is offered from the first request on.
        let started = events.iter().find_map(|e| match e {
            Event::Started { tools, .. } => Some(tools.clone()),
            _ => None,
        });
        assert!(started.unwrap().iter().any(|t| t == crate::results::NAME));
    }

    #[tokio::test]
    async fn tool_results_go_back_to_the_model_until_it_answers() {
        let model = Script::new(vec![
            Ok(calls(&[
                ("c1", "echo", r#"{"text":"pong"}"#),
                ("c2", "missing", "{}"),
            ])),
            Ok(text("All done.")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "ping",
            &mut events,
            &Cancel::new(),
        )
        .await;

        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(outcome.answer.as_deref(), Some("All done."));
        assert_eq!(outcome.turns, 2);
        assert_eq!(
            outcome.usage,
            Usage {
                input_tokens: 17,
                cached_input_tokens: 0,
                output_tokens: 3
            }
        );

        let second = &model.seen.lock().unwrap()[1];
        assert_eq!(second[0].role, Role::System);
        assert_eq!(second[1].content.as_deref(), Some("ping"));
        assert_eq!(second[3], Message::tool("c1", "pong"));
        assert_eq!(second[4].tool_call_id.as_deref(), Some("c2"));
        assert!(
            second[4]
                .content
                .as_deref()
                .unwrap()
                .contains("no tool named")
        );

        assert!(matches!(events.first(), Some(Event::Started { .. })));
        assert!(
            events.iter().any(
                |e| matches!(e, Event::ToolResult { ok: false, name, .. } if name == "missing")
            )
        );
        assert!(matches!(
            events.last(),
            Some(Event::Finished {
                reason: FinishReason::Answered,
                turns: 2,
                ..
            })
        ));
    }

    /// CP4 of harness-choice: a conversation goes on — the second turn sees the first one, tool calls included,
    /// under a system prompt written anew, and comes back whole for the turn after.
    #[tokio::test]
    async fn a_conversation_goes_on_with_what_came_before() {
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", r#"{"text":"pong"}"#)])),
            Ok(text("Remembered.")),
            Ok(text("It was ping.")),
        ]);
        let config = RunConfig::new("/w");
        let (first, kept) = run_continuing(
            &config,
            &model,
            &toolbox(),
            Vec::new(),
            "ping",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(first.answer.as_deref(), Some("Remembered."));
        // system, user, the call, its result, the answer
        assert_eq!(kept.len(), 5);
        // T36e: the first call — system and prompt — is the head the next turn reuses (later calls carry this run's
        // thinking, which the next turn leaves out); a keep-warm sends it again.
        let warm = first.warm.as_ref().unwrap();
        assert_eq!(warm.length, 2);
        assert_eq!(
            &kept[..warm.length],
            model.seen.lock().unwrap()[0].as_slice()
        );
        assert!(warm.tools.iter().any(|spec| spec.name == "echo"));
        let mut old = kept.clone();
        old[0] = Message::system("an old system prompt");

        let (second, kept) = run_continuing(
            &config,
            &model,
            &toolbox(),
            old,
            "what did I say?",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(second.answer.as_deref(), Some("It was ping."));
        assert_eq!(second.turns, 1);
        let sent = &model.seen.lock().unwrap()[2];
        assert_eq!(sent.len(), 6);
        assert_eq!(sent[0].role, Role::System);
        assert_ne!(sent[0].content.as_deref(), Some("an old system prompt"));
        assert_eq!(sent[1].content.as_deref(), Some("ping"));
        assert_eq!(sent[3], Message::tool("c1", "pong"));
        assert_eq!(sent[5].content.as_deref(), Some("what did I say?"));
        assert_eq!(kept.len(), 7);
        assert_eq!(kept[6].content.as_deref(), Some("It was ping."));
    }

    #[tokio::test]
    async fn the_answer_is_the_last_replys_text_and_extra_calls_are_refused() {
        let many: Vec<(String, String)> = (0..MAX_CALLS_PER_TURN + 2)
            .map(|i| (format!("c{i}"), "{\"text\":\"x\"}".to_owned()))
            .collect();
        let list: Vec<(&str, &str, &str)> = many
            .iter()
            .map(|(id, args)| (id.as_str(), "echo", args.as_str()))
            .collect();
        let mut first = calls(&list);
        first.message.content = Some("Let me look.".into());
        let mut last = text("");
        last.message.content = None;
        let model = Script::new(vec![Ok(first), Ok(last)]);
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(outcome.answer, None, "the earlier remark is no answer");
        let second = &model.seen.lock().unwrap()[1];
        let replies: Vec<&Message> = second.iter().filter(|m| m.role == Role::Tool).collect();
        assert_eq!(
            replies.len(),
            MAX_CALLS_PER_TURN + 2,
            "every call id is answered"
        );
        assert!(
            replies[MAX_CALLS_PER_TURN]
                .content
                .as_deref()
                .unwrap()
                .contains("at most")
        );
        assert_eq!(replies[0].content.as_deref(), Some("x"));
    }

    fn journal_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glut-run-journal-{tag}-{}-{}",
            std::process::id(),
            chrono::Local::now()
                .timestamp_nanos_opt()
                .unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn journaled(dir: &std::path::Path, run: &str) -> RunConfig {
        let mut config = RunConfig::new("/w");
        config.journal = Some(crate::journal::JournalSpec {
            dir: dir.to_owned(),
            run: run.to_owned(),
            meta: json!({"kind": "test"}),
        });
        config
    }

    /// Durable runs (D5/D6/D8): a run the process loses mid-step keeps its journal; resumed, it goes on from the last
    /// saved step with its steps counted on, and the finished run leaves no journal.
    #[tokio::test]
    async fn a_lost_run_goes_on_from_its_journal() {
        let dir = journal_dir("lost");
        let config = journaled(&dir, "run-lost");
        let mut slow = Script::new(vec![Ok(calls(&[("c1", "echo", r#"{"text":"one"}"#)]))]);
        slow.delay = Duration::from_millis(60);
        // The process "dies" during the second model call: the future is dropped, nothing ends the run.
        let lost = tokio::time::timeout(
            Duration::from_millis(100),
            run(
                &config,
                &slow,
                &toolbox(),
                "go",
                &mut Vec::new(),
                &Cancel::new(),
            ),
        )
        .await;
        assert!(lost.is_err());
        assert_eq!(crate::journal::list(&dir).len(), 1);

        let model = Script::new(vec![Ok(text("Done."))]);
        let (outcome, messages) = resume(
            &config,
            &model,
            &toolbox(),
            "run-lost",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(outcome.answer.as_deref(), Some("Done."));
        assert_eq!(outcome.turns, 2, "the step before the loss counts on");
        assert!(
            outcome.usage.input_tokens > 10,
            "the tokens before the loss count on too"
        );
        // The model saw the whole saved conversation: the prompt, its call and the call's result.
        let first: Vec<Option<String>> = model.seen.lock().unwrap()[0]
            .iter()
            .map(|m| m.content.clone())
            .collect();
        assert!(
            first.contains(&Some("go".into())) && first.contains(&Some("one".into())),
            "{first:?}"
        );
        assert_eq!(messages.last().unwrap().content.as_deref(), Some("Done."));
        assert!(
            crate::journal::list(&dir).is_empty(),
            "a finished run leaves no journal"
        );
        assert!(
            resume(
                &config,
                &model,
                &toolbox(),
                "run-lost",
                &mut Vec::new(),
                &Cancel::new()
            )
            .await
            .is_err()
        );
    }

    /// D4: a call that was under way is not run again unless it only reads — the model is told; one that had not
    /// started runs as it would have. A run that had already answered ends without another model call.
    #[tokio::test]
    async fn an_interrupted_call_is_reported_and_an_unstarted_one_runs() {
        use crate::journal::{Journal, Line};
        let dir = journal_dir("calls");
        let config = journaled(&dir, "run-calls");
        let spec = config.journal.clone().unwrap();
        let mut journal = Journal::create(&spec, Path::new("/w"), "script").unwrap();
        let reply = calls(&[
            ("c1", "echo", r#"{"text":"sent"}"#),
            ("c2", "echo", r#"{"text":"later"}"#),
        ]);
        for message in [Message::user("go"), reply.message] {
            journal.write(&Line::Message { message }).unwrap();
        }
        journal.write(&Line::Call { id: "c1".into() }).unwrap();
        drop(journal);

        let model = Script::new(vec![Ok(text("Checked."))]);
        let (outcome, messages) = resume(
            &config,
            &model,
            &toolbox(),
            "run-calls",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await
        .unwrap();
        assert_eq!(outcome.reason, FinishReason::Answered);
        let results: Vec<(&str, &str)> = messages
            .iter()
            .filter(|m| m.role == Role::Tool)
            .map(|m| {
                (
                    m.tool_call_id.as_deref().unwrap(),
                    m.content.as_deref().unwrap(),
                )
            })
            .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].0, "c1");
        assert!(
            results[0].1.starts_with("error: interrupted"),
            "{}",
            results[0].1
        );
        assert_eq!(results[1], ("c2", "later"));

        // Answered before the loss: resumed, it ends at once.
        let config = journaled(&dir, "run-done");
        let spec = config.journal.clone().unwrap();
        let mut journal = Journal::create(&spec, Path::new("/w"), "script").unwrap();
        for message in [Message::user("go"), text("All done.").message] {
            journal.write(&Line::Message { message }).unwrap();
        }
        drop(journal);
        let idle = Script::new(vec![]);
        let (outcome, _) = resume(
            &config,
            &idle,
            &toolbox(),
            "run-done",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await
        .unwrap();
        assert_eq!(
            (outcome.reason, outcome.answer.as_deref()),
            (FinishReason::Answered, Some("All done."))
        );
        assert!(idle.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_model_that_never_stops_calling_tools_ends_at_the_turn_limit() {
        let model = Script::new(vec![]);
        let mut config = RunConfig::new("/w");
        config.max_turns = 3;
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(
            (outcome.reason, outcome.turns, outcome.answer),
            (FinishReason::MaxTurns, 3, None)
        );
    }

    #[tokio::test]
    async fn a_spent_budget_ends_the_run_before_the_next_call_with_every_call_answered() {
        let model = Script::new(vec![
            Ok(echo_long("c1", 600)),
            Ok(echo_long("c2", 600)),
            Ok(text("never asked")),
        ]);
        let mut config = RunConfig::new("/w");
        config.budget = crate::config::Budget {
            max_tokens: Some(1_000),
            ..crate::config::Budget::default()
        };
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!((outcome.reason, outcome.turns), (FinishReason::Budget, 2));
        let results = events
            .iter()
            .filter(|event| matches!(event, Event::ToolResult { .. }))
            .count();
        assert_eq!(
            results, 2,
            "the last reply's call was answered before the run ended"
        );
        assert!(
            error_events(&events)
                .iter()
                .any(|message| message.contains("budget of 1000 tokens")),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn a_failing_model_call_ends_the_run_with_an_error_event() {
        let model = Script::new(vec![Err(HarnessError::Http {
            status: 401,
            body: "bad key".into(),
        })]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Failed);
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Error { message } if message.contains("401")))
        );
    }

    #[tokio::test]
    async fn the_time_limit_and_a_cancel_stop_a_slow_model_call() {
        let mut slow = Script::new(vec![Ok(text("late"))]);
        slow.delay = Duration::from_secs(30);
        let mut config = RunConfig::new("/w");
        config.timeout = Duration::from_millis(50);
        let outcome = run(
            &config,
            &slow,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Timeout);

        let cancel = Cancel::new();
        let later = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            later.cancel();
        });
        let outcome = run(
            &RunConfig::new("/w"),
            &slow,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &cancel,
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Cancelled);
    }

    #[tokio::test]
    async fn the_real_client_and_the_loop_work_together_over_http() {
        use crate::model::fake::{Answer, FakeApi};
        let api = FakeApi::spawn(vec![
            Answer::ok(json!({"choices": [{"message": {"role": "assistant", "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "echo", "arguments": "{\"text\":\"hi\"}"}}
            ]}}]})),
            Answer::ok(json!({"choices": [{"message": {"role": "assistant", "content": "said hi"}}]})),
        ])
        .await;
        let model = crate::model::OpenAiCompat::new(crate::config::ModelEndpoint {
            base_url: api.base_url.clone(),
            api_key: None,
            model: "m".into(),
            effort: None,
            versioned: false,
        })
        .unwrap();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.answer.as_deref(), Some("said hi"));
        let bodies = api.bodies();
        assert_eq!(
            bodies[1]["messages"][3],
            json!({"role": "tool", "content": "hi", "tool_call_id": "c1"})
        );
        assert_eq!(
            bodies[1]["messages"][2]["tool_calls"][0]["function"]["name"],
            "echo"
        );
    }

    /// CP5: a streamed answer's text goes out piece by piece while it arrives, then once whole.
    #[tokio::test]
    async fn a_streamed_answer_reaches_the_sink_in_pieces_and_then_whole() {
        use crate::model::fake::{Answer, FakeApi};
        let api = FakeApi::spawn(vec![Answer::stream(&[
            json!({"choices": [{"delta": {"content": "said "}}]}),
            json!({"choices": [{"delta": {"content": "hi"}}]}),
        ])])
        .await;
        let model = crate::model::OpenAiCompat::new(crate::config::ModelEndpoint {
            base_url: api.base_url.clone(),
            api_key: None,
            model: "m".into(),
            effort: None,
            versioned: false,
        })
        .unwrap();
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.answer.as_deref(), Some("said hi"));
        let pieces: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                Event::TextDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(pieces, ["said ", "hi"]);
        let whole = events
            .iter()
            .position(|event| matches!(event, Event::AssistantText { .. }))
            .unwrap();
        let last_piece = events
            .iter()
            .rposition(|event| matches!(event, Event::TextDelta { .. }))
            .unwrap();
        assert!(last_piece < whole);
    }

    /// An MCP-like tool: a run in code mode keeps it behind `codemode` (Glut 2, G3).
    struct Look;

    impl Tool for Look {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "mcp__srv__look".into(),
                description: "Looks.".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Mcp {
                server: "srv".into(),
                tool: "look".into(),
            }
        }

        fn call(&self, _arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            Box::pin(async { Ok("looked".to_owned()) })
        }
    }

    /// Runs one `codemode` call with `code` over echo and the MCP-like tool; returns the script's answer and the events.
    async fn script_run(code: &str, tools: Toolbox) -> (String, Vec<Event>) {
        let args = json!({ "code": code }).to_string();
        let model = Script::new(vec![
            Ok(calls(&[("c1", crate::codemode::NAME, args.as_str())])),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let seen = model.seen.lock().unwrap();
        let answer = seen[1].last().unwrap().content.clone().unwrap_or_default();
        (answer, events)
    }

    fn with_look() -> Toolbox {
        let mut tools = toolbox();
        tools.extend([Box::new(Look) as Box<dyn Tool>]);
        tools
    }

    #[tokio::test]
    async fn a_script_calls_tools_as_the_run_does_and_mcp_stays_behind_code() {
        let (answer, events) = script_run(
            "const [a, b] = await Promise.all([tools.echo({text: 'x'}), tools.mcp__srv__look({})]); \
             return a + '|' + b",
            with_look(),
        )
        .await;
        assert_eq!(answer, "x|looked");
        let started = events.iter().find_map(|e| match e {
            Event::Started { tools, .. } => Some(tools.clone()),
            _ => None,
        });
        let started = started.unwrap();
        assert!(
            started.iter().any(|t| t == "echo")
                && started.iter().any(|t| t == crate::codemode::NAME)
        );
        assert!(
            !started.iter().any(|t| t == "mcp__srv__look"),
            "{started:?}"
        );
        // Each call from the script is a call of the run, reported under the script's id.
        let inner: Vec<String> = events
            .iter()
            .filter_map(|e| match e {
                Event::ToolCall { id, .. } if id.starts_with("c1.") => Some(id.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(inner, ["c1.1", "c1.2"]);
    }

    #[tokio::test]
    async fn a_script_waiting_for_nothing_ends_and_cannot_start_a_subagent_or_a_script() {
        let (stuck, _) = script_run("await new Promise(() => {}); return 1", with_look()).await;
        assert!(stuck.contains("nothing will settle"), "{stuck}");
        let (task, _) = script_run(
            "try { await tools.task({description: 'd', prompt: 'p'}) } catch (e) { return e.message }",
            with_look(),
        )
        .await;
        assert!(task.contains("cannot start task"), "{task}");
        let (nested, _) = script_run(
            "try { await tools.codemode({code: 'return 1'}) } catch (e) { return e.message }",
            with_look(),
        )
        .await;
        assert!(nested.contains("cannot start codemode"), "{nested}");
    }

    #[tokio::test]
    async fn a_script_waits_for_a_place_instead_of_counting_as_stuck() {
        // Both places taken, as by two scripts of other runs: this one waits, and runs once one is free.
        let first = crate::codemode::slot().await.unwrap();
        let second = crate::codemode::slot().await.unwrap();
        let waiting = tokio::spawn(script_run("return 'ran'", with_look()));
        tokio::time::sleep(Duration::from_millis(100)).await;
        drop((first, second));
        let (answer, _) = waiting.await.unwrap();
        assert_eq!(answer, "ran");
    }

    #[tokio::test]
    async fn the_gate_judges_every_call_from_a_script() {
        use crate::permissions::{Gate, Mode, Nobody, RuleSet};
        let mut tools = with_look();
        tools.set_gate(Arc::new(Gate::new(
            RuleSet::default(),
            Mode::Default,
            Arc::new(Nobody),
        )));
        let (answer, events) = script_run(
            "try { await tools.echo({text: 'x'}); return 'ran' } catch (e) { return 'refused: ' + e.message }",
            tools,
        )
        .await;
        assert!(answer.starts_with("refused: not permitted"), "{answer}");
        assert!(events.iter().any(
            |e| matches!(e, Event::Permission { allowed: false, tool, .. } if tool == "echo")
        ));
    }

    #[tokio::test]
    async fn without_code_mode_mcp_tools_are_offered_directly() {
        let model = Script::new(vec![Ok(text("done"))]);
        let mut events = Vec::new();
        let config = RunConfig {
            codemode: false,
            ..RunConfig::new("/w")
        };
        run(
            &config,
            &model,
            &with_look(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let started = events.iter().find_map(|e| match e {
            Event::Started { tools, .. } => Some(tools.clone()),
            _ => None,
        });
        let started = started.unwrap();
        assert!(started.iter().any(|t| t == "mcp__srv__look"));
        assert!(!started.iter().any(|t| t == crate::codemode::NAME));
    }

    #[test]
    fn read_only_commands_need_no_snapshot() {
        for command in [
            "date",
            "ls -la News",
            "grep -r x . | head -5",
            "git status",
            "git log -3",
        ] {
            assert!(reads_only(command), "{command}");
        }
        for command in [
            "rm a",
            "echo x > f",
            "date; touch f",
            "sh -c 'ls'",
            "git commit -m x",
            "mkdir -p News",
        ] {
            assert!(!reads_only(command), "{command}");
        }
    }

    #[tokio::test]
    async fn todo_write_reports_the_list_and_answers_with_what_is_open() {
        let model = Script::new(vec![
            Ok(calls(&[(
                "c1",
                crate::planning::TODO,
                r#"{"items":[{"text":"read","status":"done"},{"text":"fix","status":"in_progress"}]}"#,
            )])),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(tool_answer(&model), "list saved: 2 items, 1 done\n▸ fix");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Todo { items, .. } if items.len() == 2))
        );
    }

    fn nudges(events: &[Event]) -> Vec<NudgeKind> {
        events
            .iter()
            .filter_map(|event| match event {
                Event::Nudged { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect()
    }

    const OPEN_LIST: &str =
        r#"{"items":[{"text":"read","status":"done"},{"text":"fix","status":"in_progress"}]}"#;

    #[tokio::test]
    async fn a_run_ending_with_its_list_open_is_asked_once_to_go_on() {
        let model = Script::new(vec![
            Ok(calls(&[("c1", crate::planning::TODO, OPEN_LIST)])),
            Ok(text("done")),
            Ok(text("stopping: the fix needs the owner")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(nudges(&events), [NudgeKind::OpenItems], "once, not again");
        let asked = model.seen.lock().unwrap()[2]
            .last()
            .and_then(|message| message.content.clone())
            .unwrap_or_default();
        assert!(
            asked.starts_with("[glut]") && asked.contains("▸ fix"),
            "{asked}"
        );

        // Off, a question to the owner, or plan mode: the run ends where the model ends it.
        let quiet = RunConfig {
            nudge: false,
            ..RunConfig::new("/w")
        };
        let model = Script::new(vec![
            Ok(calls(&[("c1", crate::planning::TODO, OPEN_LIST)])),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        run(
            &quiet,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert!(nudges(&events).is_empty());
        let model = Script::new(vec![
            Ok(calls(&[("c1", crate::planning::TODO, OPEN_LIST)])),
            Ok(text("Shall I fix it the other way?")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert!(nudges(&events).is_empty() && outcome.reason == FinishReason::Answered);
    }

    #[tokio::test]
    async fn a_run_ending_with_an_announcement_is_asked_to_take_the_step() {
        let model = Script::new(vec![Ok(text("Now the tests:")), Ok(text("the tests pass"))]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(nudges(&events), [NudgeKind::Announced]);
        assert_eq!(outcome.answer.as_deref(), Some("the tests pass"));
        assert!(crate::planning::announces("Weiter mit den CLI-Tests:"));
        assert!(crate::planning::announces(
            "Done.\nNext, I'll add the README"
        ));
        assert!(!crate::planning::announces(
            "Done: all three commands pass."
        ));
    }

    #[tokio::test]
    async fn a_list_left_open_is_carried_and_reminded_of_after_eight_calls() {
        // Sixteen calls in one reply: one reminder, at the eighth, never a second in the same step.
        let echoes: Vec<(&str, &str, &str)> =
            (0..16).map(|_| ("e", "echo", r#"{"text":"x"}"#)).collect();
        let mut replies = vec![Ok(calls(&echoes))];
        replies.push(Ok(calls(&[(
            "t",
            crate::planning::TODO,
            r#"{"items":[{"text":"fix","status":"done"}]}"#,
        )])));
        replies.push(Ok(text("all done")));
        let model = Script::new(replies);
        let config = RunConfig {
            todo_list: vec![crate::planning::TodoItem {
                text: "fix".into(),
                status: crate::planning::TodoStatus::InProgress,
            }],
            ..RunConfig::new("/w")
        };
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go on",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let seen = model.seen.lock().unwrap();
        let prompt = seen[0].last().unwrap().content.clone().unwrap_or_default();
        assert!(
            prompt.starts_with("go on\n\n[glut] Your task list from before (0 of 1 done)"),
            "{prompt}"
        );
        let answers: Vec<String> = seen[1]
            .iter()
            .filter(|m| m.role == Role::Tool)
            .filter_map(|m| m.content.clone())
            .collect();
        assert_eq!(
            answers
                .iter()
                .filter(|a| a.contains("[glut] Your task list has not changed"))
                .count(),
            1
        );
        assert!(answers[7].contains("[glut]"), "at the eighth call");
        assert_eq!(
            nudges(&events),
            [NudgeKind::Reminder],
            "all done: no question at the end"
        );
    }

    /// An edit-like tool: Plan mode does not offer it.
    struct Write;

    impl Tool for Write {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "write".into(),
                description: "w".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, _arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Edit { path: "f".into() }
        }

        fn call(&self, _arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            Box::pin(async { Ok("written".to_owned()) })
        }
    }

    /// Says yes to every ask.
    struct Yes;

    impl crate::permissions::Asker for Yes {
        fn ask<'a>(
            &'a self,
            _request: &'a crate::permissions::AskRequest,
        ) -> crate::permissions::AnswerFuture<'a> {
            Box::pin(async { crate::permissions::Answer::Allow })
        }
    }

    async fn plan_run(asker: Arc<dyn crate::permissions::Asker>) -> (Script, Vec<Event>) {
        use crate::permissions::{Gate, Mode, RuleSet};
        let mut tools = toolbox();
        tools.extend([Box::new(Write) as Box<dyn Tool>]);
        tools.set_gate(Arc::new(Gate::new(RuleSet::default(), Mode::Plan, asker)));
        let model = Script::new(vec![
            Ok(calls(&[(
                "c1",
                crate::planning::EXIT_PLAN,
                r#"{"plan":"change f"}"#,
            )])),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        let config = RunConfig {
            build_mode: crate::permissions::Mode::AcceptEdits,
            ..RunConfig::new("/w")
        };
        run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        (model, events)
    }

    #[tokio::test]
    async fn an_approved_plan_leaves_plan_mode_and_offers_the_edit_tools_from_the_next_turn() {
        let (model, events) = plan_run(Arc::new(Yes)).await;
        assert!(tool_answer(&model).contains("approved"));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ModeChanged { mode, .. } if mode == "accept_edits"))
        );
        let tools_sent = model.offered.lock().unwrap();
        assert!(
            !tools_sent[0].iter().any(|t| t == "write")
                && tools_sent[0].iter().any(|t| t == "exit_plan")
        );
        assert!(
            tools_sent[1].iter().any(|t| t == "write")
                && !tools_sent[1].iter().any(|t| t == "exit_plan")
        );
    }

    /// Picks the second mode the `exit_plan` ask offers, and keeps what it was offered.
    struct SecondMode(std::sync::Mutex<Vec<String>>);

    impl crate::permissions::Asker for SecondMode {
        fn ask<'a>(
            &'a self,
            request: &'a crate::permissions::AskRequest,
        ) -> crate::permissions::AnswerFuture<'a> {
            self.0.lock().unwrap().clone_from(&request.scopes);
            Box::pin(async { crate::permissions::Answer::AllowWithin(1) })
        }
    }

    #[tokio::test]
    async fn the_owner_may_approve_a_plan_into_another_mode() {
        let asker = Arc::new(SecondMode(std::sync::Mutex::new(Vec::new())));
        let (_, events) = plan_run(Arc::clone(&asker) as Arc<dyn crate::permissions::Asker>).await;
        assert_eq!(
            *asker.0.lock().unwrap(),
            ["accept_edits", "auto", "default"]
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::ModeChanged { mode, .. } if mode == "auto"))
        );
    }

    #[tokio::test]
    async fn without_an_answer_the_run_stays_in_plan_mode() {
        let (model, events) = plan_run(Arc::new(crate::permissions::Nobody)).await;
        assert!(tool_answer(&model).contains("ask the owner to switch to build"));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::ModeChanged { .. }))
        );
    }

    /// A tool that changes the file its `path` names — asked for in the default mode, so refused headless.
    struct Put;

    impl Tool for Put {
        fn spec(&self) -> ToolSpec {
            ToolSpec {
                name: "put".into(),
                description: "p".into(),
                parameters: json!({"type": "object"}),
            }
        }

        fn access(&self, arguments: &Value) -> crate::permissions::Access {
            crate::permissions::Access::Edit {
                path: arguments["path"].as_str().unwrap_or_default().to_owned(),
            }
        }

        fn call(&self, _arguments: Value, _context: crate::tools::CallContext) -> ToolFuture<'_> {
            Box::pin(async { Ok("written".to_owned()) })
        }
    }

    /// The toolbox with `put` behind a headless gate in the default mode, `allow` rules on top.
    fn refusing_tools(allow: &[&str]) -> Toolbox {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let mut tools = Toolbox::new();
        tools.extend([Box::new(Put) as Box<dyn Tool>]);
        let rules = RuleSet {
            allow: allow
                .iter()
                .map(|rule| Rule::parse(rule).unwrap())
                .collect(),
            ..RuleSet::default()
        };
        tools.set_gate(Arc::new(Gate::new(rules, Mode::Default, Arc::new(Nobody))));
        tools
    }

    fn put(id: &str, path: &str) -> Reply {
        let arguments = json!({ "path": path }).to_string();
        calls(&[(id, "put", arguments.as_str())])
    }

    /// The tool answers the model was sent, in order.
    fn tool_answers(model: &Script) -> Vec<String> {
        let seen = model.seen.lock().unwrap();
        let last = seen.last().cloned().unwrap_or_default();
        last.iter()
            .filter(|m| m.role == Role::Tool)
            .filter_map(|m| m.content.clone())
            .collect()
    }

    fn error_events(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Error { message } => Some(message.clone()),
                _ => None,
            })
            .collect()
    }

    /// `echo` with a long text: one step of about 4,000 tokens (the call's arguments and its result).
    fn echo_long(id: &str, prompt_tokens: u64) -> Reply {
        let arguments = json!({ "text": "e".repeat(8_000) }).to_string();
        let mut reply = calls(&[(id, "echo", arguments.as_str())]);
        reply.usage.input_tokens = prompt_tokens;
        reply
    }

    fn compactions(events: &[Event]) -> Vec<(u64, u64, usize)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Compacted {
                    before_tokens,
                    after_tokens,
                    replaced,
                    ..
                } => Some((*before_tokens, *after_tokens, *replaced)),
                _ => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn a_run_near_its_window_compacts_the_older_steps_before_the_next_call() {
        // Window 30k, 1k output: compacts from 21,500 tokens and keeps the newest ~10k — two steps.
        let model = Script::new(vec![
            Ok(echo_long("c1", 10)),
            Ok(echo_long("c2", 10)),
            Ok(echo_long("c3", 10)),
            Ok(echo_long("c4", 20_000)),
            Ok(text("Echoed three long texts.")),
            Ok(text("done")),
        ]);
        let mut config = RunConfig::new("/w");
        config.compaction.context_window = Some(30_000);
        config.max_output_tokens = Some(1_000);
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(outcome.answer.as_deref(), Some("done"));
        let done = compactions(&events);
        assert_eq!(done.len(), 1, "{done:?}");
        let (before, after, replaced) = done[0];
        assert!(before > 21_500 && after < before, "{done:?}");
        assert_eq!(replaced, 4, "two older steps of two messages");
        let seen = model.seen.lock().unwrap();
        let last = seen.last().unwrap();
        let roles: Vec<Role> = last.iter().map(|m| m.role).collect();
        assert_eq!(
            roles,
            [
                Role::System,
                Role::User,
                Role::Assistant,
                Role::User,
                Role::Assistant,
                Role::Tool,
                Role::Assistant,
                Role::Tool
            ]
        );
        let notes = last[1].content.as_deref().unwrap();
        assert!(notes.contains("Echoed three long texts."), "{notes}");
        assert!(
            notes.contains("kept whole as result") && notes.contains("read_full_result"),
            "{notes}"
        );
        assert_eq!(last[3].content.as_deref(), Some("go"));
        let metered = events
            .iter()
            .filter(|e| matches!(e, Event::Usage { .. }))
            .count();
        assert_eq!(metered, 6, "five model calls and the summary");
    }

    #[tokio::test]
    async fn a_too_long_prompt_compacts_once_and_asks_again() {
        let overflow = || HarnessError::Http {
            status: 400,
            body: "This model's maximum context length is 8192 tokens".into(),
        };
        let earlier = vec![Message::user("before"), text("an earlier answer").message];
        let model = Script::new(vec![
            Err(overflow()),
            Ok(text("They talked before.")),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        let (outcome, conversation) = run_continuing(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            earlier.clone(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(compactions(&events).len(), 1);
        assert!(
            conversation
                .iter()
                .all(|m| m.content.as_deref() != Some("before")),
            "summarised away"
        );

        // Too long again after compacting: the run fails as before, without a second compaction.
        let model = Script::new(vec![Err(overflow()), Ok(text("summary")), Err(overflow())]);
        let mut events = Vec::new();
        let (outcome, _) = run_continuing(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            earlier.clone(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Failed);
        assert_eq!(compactions(&events).len(), 1);

        // Switched off: no compaction, the provider's error stands.
        let mut config = RunConfig::new("/w");
        config.compaction.auto = false;
        let model = Script::new(vec![Err(overflow())]);
        let mut events = Vec::new();
        let (outcome, _) = run_continuing(
            &config,
            &model,
            &toolbox(),
            earlier,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Failed);
        assert!(compactions(&events).is_empty());
    }

    #[tokio::test]
    async fn nothing_older_to_compact_says_so_once_and_the_run_goes_on() {
        let model = Script::new(vec![Ok(echo_long("c1", 29_000)), Ok(text("done"))]);
        let mut config = RunConfig::new("/w");
        config.compaction.context_window = Some(30_000);
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let failed: Vec<String> = error_events(&events)
            .into_iter()
            .filter(|m| m.starts_with(COMPACTION_FAILED))
            .collect();
        assert_eq!(failed.len(), 1, "{failed:?}");
    }

    #[test]
    fn a_small_window_still_compacts_from_half_of_it() {
        assert_eq!(compact_from(8_192, None), 4_096);
        assert_eq!(keep_for(Some(8_192), None), 2_048);
        // A large one keeps the full reserve and output room.
        assert_eq!(compact_from(131_072, None), 131_072 - 19_660 - 8_192);
        assert_eq!(keep_for(Some(131_072), None), KEEP_RECENT_TOKENS);
        assert_eq!(keep_for(None, None), KEEP_RECENT_TOKENS);
    }

    #[tokio::test]
    async fn the_window_is_asked_for_once_when_it_matters_and_a_tiny_one_is_ignored() {
        // A short run never asks.
        let model = Script::new(vec![Ok(text("done"))]);
        let config = RunConfig::new("/w");
        run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(
            model.window_asked.load(std::sync::atomic::Ordering::SeqCst),
            0
        );

        // Past 16k it asks once; a window below the floor counts as none — the run goes on without compacting.
        let mut model = Script::new(vec![
            Ok(echo_long("c1", 20_000)),
            Ok(echo_long("c2", 21_000)),
            Ok(text("done")),
        ]);
        model.window = Some(1_000);
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(
            model.window_asked.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert!(compactions(&events).is_empty());
    }

    #[tokio::test]
    async fn a_run_ended_by_refusals_still_answers_every_call_of_its_last_reply() {
        let two = || {
            let (a, b) = (
                json!({"path": "a"}).to_string(),
                json!({"path": "b"}).to_string(),
            );
            calls(&[("x", "put", a.as_str()), ("y", "put", b.as_str())])
        };
        let model = Script::new(vec![Ok(put("c1", "a")), Ok(put("c2", "a")), Ok(two())]);
        let mut events = Vec::new();
        let (outcome, conversation) = run_continuing(
            &RunConfig::new("/w"),
            &model,
            &refusing_tools(&[]),
            Vec::new(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Refused);
        let last = conversation.last().unwrap();
        assert_eq!(
            last.tool_call_id.as_deref(),
            Some("y"),
            "the second call has its answer"
        );
        assert!(
            last.content
                .as_deref()
                .unwrap()
                .contains("not run, the run is ending")
        );
    }

    #[tokio::test]
    async fn a_message_sent_while_the_run_works_joins_before_its_next_call() {
        let mut model = Script::new(vec![Ok(text("first")), Ok(text("second"))]);
        model.delay = Duration::from_millis(80);
        let inbox = crate::config::Inbox::default();
        let config = RunConfig {
            inbox: Some(inbox.clone()),
            ..RunConfig::new("/w")
        };
        let tools = Toolbox::new();
        let sender = inbox.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            sender.send("also do X");
        });
        let mut events = Vec::new();
        let outcome = run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen.len(), 2, "an answer with a message waiting goes on");
        let last = seen[1].last().unwrap();
        assert_eq!(
            (last.role, last.content.as_deref()),
            (Role::User, Some("also do X"))
        );
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Steered { text, .. } if text == "also do X"))
        );
        assert!(inbox.is_empty());
    }

    #[tokio::test]
    async fn the_same_refused_call_is_warned_about_once_and_ends_the_run_the_third_time() {
        let model = Script::new(vec![
            Ok(put("c1", "a.txt")),
            Ok(put("c2", "a.txt")),
            Ok(put("c3", "a.txt")),
            Ok(text("never asked")),
        ]);
        let mut events = Vec::new();
        let config = RunConfig::new("/w");
        let tools = refusing_tools(&[]);
        let outcome = run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        assert_eq!(outcome.reason, FinishReason::Refused);
        assert_eq!(outcome.turns, 3, "not at the step limit");
        assert_eq!(outcome.refusals, 3);
        let answers = tool_answers(&model);
        assert!(!answers[0].contains("[Glut:"), "{}", answers[0]);
        assert!(answers[1].contains("refused 2 times now"), "{}", answers[1]);
        let errors = error_events(&events);
        assert!(
            errors
                .iter()
                .any(|m| m.starts_with("stopped: `put` a.txt was refused 3 times")),
            "{errors:?}"
        );
        assert!(matches!(
            events.last(),
            Some(Event::Finished {
                reason: FinishReason::Refused,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn refusals_of_different_calls_end_the_run_at_the_total() {
        let model = Script::new(vec![
            Ok(put("c1", "a")),
            Ok(put("c2", "b")),
            Ok(put("c3", "c")),
            Ok(put("c4", "d")),
            Ok(text("never asked")),
        ]);
        let mut config = RunConfig::new("/w");
        config.refusals = crate::config::RefusalLimits {
            repeat: 3,
            total: 4,
        };
        let mut events = Vec::new();
        let tools = refusing_tools(&[]);
        let outcome = run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        assert_eq!(outcome.reason, FinishReason::Refused);
        assert_eq!(outcome.turns, 4);
        let answers = tool_answers(&model);
        assert!(
            answers[1].contains("2 calls were refused in this run; at 4 it ends"),
            "{}",
            answers[1]
        );
        assert!(
            error_events(&events)
                .iter()
                .any(|m| m.starts_with("stopped: 4 calls were refused in this run")),
        );
    }

    #[tokio::test]
    async fn a_subagents_refusals_count_toward_the_runs_total() {
        // The parent starts a subagent that is refused twice and answers; the parent's next refusal is the third.
        let model = Script::new(vec![
            Ok(task_call(
                "t1",
                json!({"description": "d", "prompt": "try"}),
            )),
            Ok(put("s1", "x")),
            Ok(put("s2", "y")),
            Ok(text("could not write")),
            Ok(put("c2", "z")),
            Ok(text("never asked")),
        ]);
        let mut config = RunConfig::new("/w");
        config.refusals = crate::config::RefusalLimits {
            repeat: 3,
            total: 3,
        };
        let mut events = Vec::new();
        let tools = refusing_tools(&["task"]);
        let outcome = run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        assert_eq!(
            outcome.reason,
            FinishReason::Refused,
            "{:?}",
            error_events(&events)
        );
        assert_eq!(outcome.refusals, 3);
    }

    #[tokio::test]
    async fn a_run_with_an_allowed_call_counts_nothing() {
        let model = Script::new(vec![Ok(put("c1", "a")), Ok(text("done"))]);
        let mut events = Vec::new();
        let tools = refusing_tools(&["Edit"]);
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(outcome.refusals, 0);
    }

    #[tokio::test]
    async fn a_gated_call_is_judged_reported_and_only_run_when_allowed() {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let reply = || {
            Script::new(vec![
                Ok(calls(&[("c1", "echo", r#"{"text":"ran"}"#)])),
                Ok(text("done")),
            ])
        };

        // Headless, default mode: the call is asked, nobody answers, it is refused and never runs.
        let mut tools = toolbox();
        tools.set_gate(Arc::new(Gate::new(
            RuleSet::default(),
            Mode::Default,
            Arc::new(Nobody),
        )));
        let model = reply();
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let answer = tool_answer(&model);
        assert!(answer.contains("not permitted"), "{answer}");
        assert!(events.iter().any(
            |e| matches!(e, Event::Permission { allowed: false, tool, .. } if tool == "echo")
        ));

        // An allow rule by the tool's name lets it run.
        let mut tools = toolbox();
        let rules = RuleSet {
            allow: vec![Rule::parse("echo").unwrap()],
            ..RuleSet::default()
        };
        tools.set_gate(Arc::new(Gate::new(rules, Mode::Default, Arc::new(Nobody))));
        let model = reply();
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(tool_answer(&model), "ran");
        let reasons: Vec<&str> = events
            .iter()
            .filter_map(|e| match e {
                Event::Permission {
                    allowed: true,
                    reason,
                    ..
                } => Some(reason.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(reasons, ["rule echo"]);
    }

    /// The tool answer the model was sent on its second turn; the lock is let go before anything awaits again.
    fn tool_answer(model: &Script) -> String {
        let seen = model.seen.lock().unwrap();
        seen[1]
            .iter()
            .find(|m| m.role == Role::Tool)
            .and_then(|m| m.content.clone())
            .unwrap_or_default()
    }

    /// A run config with Claude-shaped hooks over an existing working folder (the hooks' session is built
    /// from it, D8).
    fn hooks_config(tag: &str, events: Value) -> RunConfig {
        let cwd = std::env::temp_dir().join(format!(
            "glut-run-hooks-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&cwd).unwrap();
        let section: crate::hooks::HooksSection = serde_json::from_value(events).unwrap();
        let mut config = RunConfig::new(cwd);
        config.hooks = crate::hooks::Hooks::compile(&section).unwrap();
        config
    }

    fn one(event: &str, command: &str) -> Value {
        json!({(event): [{"hooks": [{"type": "command", "command": command}]}]})
    }

    #[tokio::test]
    async fn a_session_start_hook_becomes_context_right_behind_the_harnesss_own() {
        let model = Script::new(vec![Ok(text("done"))]);
        let config = hooks_config("start", one("SessionStart", r#"echo "house rules""#));
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let first = &model.seen.lock().unwrap()[0];
        assert_eq!(first[0].role, Role::System, "the harness's own prompt");
        assert_eq!(
            first[1],
            Message::system("house rules"),
            "the hook's context, before the prompt"
        );
        assert_eq!(first[2].content.as_deref(), Some("go"));
        assert!(
            events.iter().any(|e| matches!(
                e,
                Event::Hook { turn: None, hook_event, blocked: false, .. } if hook_event == "SessionStart"
            ))
        );
    }

    #[tokio::test]
    async fn a_pre_tool_use_deny_blocks_before_the_gate_and_allow_changes_nothing() {
        // The block: the tool never runs, the model gets the reason as its tool result.
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", r#"{"text":"ran"}"#)])),
            Ok(text("done")),
        ]);
        let config = hooks_config(
            "deny",
            one(
                "PreToolUse",
                r#"echo '{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"not today"}}'"#,
            ),
        );
        let mut events = Vec::new();
        run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(tool_answer(&model), "error: not today");
        assert!(events.iter().any(
            |e| matches!(e, Event::Hook { blocked: true, hook_event, .. } if hook_event == "PreToolUse")
        ));

        // B2, the load-bearing test: `allow` never carries a call past a deny rule — the gate still refuses.
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", r#"{"text":"ran"}"#)])),
            Ok(text("done")),
        ]);
        let config = hooks_config(
            "allow",
            one(
                "PreToolUse",
                r#"echo '{"hookSpecificOutput":{"permissionDecision":"allow"}}'"#,
            ),
        );
        let mut tools = toolbox();
        let rules = crate::permissions::RuleSet {
            deny: vec![crate::permissions::Rule::parse("echo").unwrap()],
            ..crate::permissions::RuleSet::default()
        };
        tools.set_gate(Arc::new(crate::permissions::Gate::new(
            rules,
            crate::permissions::Mode::Default,
            Arc::new(crate::permissions::Nobody),
        )));
        run(
            &config,
            &model,
            &tools,
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert!(
            tool_answer(&model).contains("not permitted"),
            "the gate stays the only decider"
        );
    }

    #[tokio::test]
    async fn a_post_tool_use_block_replaces_what_the_model_gets() {
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", r#"{"text":"ran"}"#)])),
            Ok(text("done")),
        ]);
        let config = hooks_config(
            "post",
            one("PostToolUse", r#"echo "the result is wrong" >&2; exit 2"#),
        );
        let mut events = Vec::new();
        run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(tool_answer(&model), "error: the result is wrong");
        // The event keeps what the tool really answered, beside the hook's own.
        assert!(
            events.iter().any(
                |e| matches!(e, Event::ToolResult { ok: true, output, .. } if output == "ran")
            )
        );
        assert!(events.iter().any(
            |e| matches!(e, Event::Hook { blocked: true, hook_event, .. } if hook_event == "PostToolUse")
        ));
    }

    #[tokio::test]
    async fn a_stop_hook_feeds_its_reason_back_and_the_second_turn_ends() {
        // Blocks the first answer, lets the second go: the second payload carries `stop_hook_active: true`.
        let model = Script::new(vec![Ok(text("done")), Ok(text("final"))]);
        let config = hooks_config(
            "stop",
            one(
                "Stop",
                r#"grep -q '"stop_hook_active":true' || { echo 'explain more' >&2; exit 2; }"#,
            ),
        );
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(
            (outcome.reason, outcome.answer.as_deref(), outcome.turns),
            (FinishReason::Answered, Some("final"), 2)
        );
        let second = &model.seen.lock().unwrap()[1];
        assert!(
            second.iter().any(|m| m.role == Role::User
                && m.content
                    .as_deref()
                    .is_some_and(|text| text.contains("Stop hook blocked"))),
            "the reason went back as a user message"
        );
    }

    #[tokio::test]
    async fn a_broken_hook_session_is_an_error_note_and_the_run_goes_on() {
        // The hooks' session needs a working folder; one that cannot be resolved leaves the run without
        // hooks (D8), with a note — B3 applied to the runner itself.
        let model = Script::new(vec![Ok(text("done"))]);
        let mut config = RunConfig::new("/no/such/folder/glut-hooks");
        let section: crate::hooks::HooksSection =
            serde_json::from_value(one("SessionStart", "true")).unwrap();
        config.hooks = crate::hooks::Hooks::compile(&section).unwrap();
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert!(
            events.iter().any(
                |e| matches!(e, Event::Error { message } if message.contains("hooks left out"))
            )
        );
    }

    // ── HS8: subagents ────────────────────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn a_task_without_a_description_or_a_prompt_is_a_tool_error_and_no_subagent() {
        let model = Script::new(vec![
            Ok(task_call("c1", json!({"description": "x"}))),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let answer = tool_answer(&model);
        assert!(answer.contains("description and prompt"), "{answer}");
        assert_eq!(
            model.seen.lock().unwrap().len(),
            2,
            "the parent's two turns and nothing else: no subagent started"
        );

        // Arguments that are not even an object are the tool's own error, too.
        let model = Script::new(vec![
            Ok(task_call("c1", json!(["not", "an", "object"]))),
            Ok(text("done")),
        ]);
        run(
            &RunConfig::new("/w"),
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        let answer = tool_answer(&model);
        assert!(answer.contains("JSON object"), "{answer}");
    }

    #[tokio::test]
    async fn a_task_past_the_runs_subagent_budget_is_a_tool_error_and_starts_nothing() {
        let task = || {
            Ok(task_call(
                "c1",
                json!({"description": "d", "prompt": "Do it."}),
            ))
        };
        let model = Script::new(vec![
            task(),
            Ok(text("sub answer")),
            task(),
            Ok(text("done")),
        ]);
        let mut config = RunConfig::new("/w");
        config.max_subagents = 1;
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(
            model.seen.lock().unwrap().len(),
            4,
            "parent, the one subagent, parent, parent — no second subagent"
        );
        // The parent's last request carries the refused second call's result.
        let seen = model.seen.lock().unwrap();
        let answer = seen[3]
            .iter()
            .rev()
            .find(|m| m.role == Role::Tool)
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        assert!(answer.contains("started its 1 subagents"), "{answer}");
    }

    #[tokio::test]
    async fn only_a_subagent_that_ran_uses_up_the_budget_and_a_budget_of_zero_starts_none() {
        let good = || {
            Ok(task_call(
                "c2",
                json!({"description": "d", "prompt": "Do it."}),
            ))
        };
        // A `task` with bad arguments starts nothing, so the one subagent the budget allows still runs.
        let model = Script::new(vec![
            Ok(task_call("c1", json!({"description": "d"}))),
            good(),
            Ok(text("sub answer")),
            Ok(text("done")),
        ]);
        let mut config = RunConfig::new("/w");
        config.max_subagents = 1;
        run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(
            model.seen.lock().unwrap().len(),
            4,
            "parent, parent, the subagent, parent"
        );

        let model = Script::new(vec![good(), Ok(text("done"))]);
        config.max_subagents = 0;
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(model.seen.lock().unwrap().len(), 2, "no subagent at all");
    }

    /// A model that answers by the conversation's first user message (T40: subagents ask in no fixed order), each
    /// reply after `delay`, and keeps how many requests were in flight at most.
    struct ByPrompt {
        replies: Mutex<HashMap<String, Vec<Reply>>>,
        delay: Duration,
        flying: std::sync::atomic::AtomicUsize,
        most: std::sync::atomic::AtomicUsize,
        last_parent: Mutex<Vec<Message>>,
    }

    impl ByPrompt {
        fn new(replies: Vec<(&str, Vec<Reply>)>, delay: Duration) -> Self {
            ByPrompt {
                replies: Mutex::new(
                    replies
                        .into_iter()
                        .map(|(prompt, replies)| (prompt.to_owned(), replies))
                        .collect(),
                ),
                delay,
                flying: std::sync::atomic::AtomicUsize::new(0),
                most: std::sync::atomic::AtomicUsize::new(0),
                last_parent: Mutex::new(Vec::new()),
            }
        }
    }

    impl ChatModel for ByPrompt {
        async fn complete(
            &self,
            messages: &[Message],
            _tools: &[ToolSpec],
            _max: Option<u32>,
        ) -> Result<Reply, HarnessError> {
            use std::sync::atomic::Ordering::SeqCst;
            let prompt = messages
                .iter()
                .find(|m| m.role == Role::User)
                .and_then(|m| m.content.clone())
                .unwrap_or_default();
            if prompt == "go" {
                *self.last_parent.lock().unwrap() = messages.to_vec();
            }
            let now = self.flying.fetch_add(1, SeqCst) + 1;
            self.most.fetch_max(now, SeqCst);
            tokio::time::sleep(self.delay).await;
            self.flying.fetch_sub(1, SeqCst);
            let mut replies = self.replies.lock().unwrap();
            let left = replies.get_mut(&prompt).filter(|left| !left.is_empty());
            Ok(left.map_or_else(|| text("nothing more"), |left| left.remove(0)))
        }

        fn name(&self) -> &str {
            "by-prompt"
        }
    }

    /// One reply with a `task` call per prompt, ids `t0`, `t1`, ….
    fn tasks(prompts: &[&str]) -> Reply {
        let arguments: Vec<String> = prompts
            .iter()
            .map(|prompt| {
                json!({"description": format!("do {prompt}"), "prompt": prompt}).to_string()
            })
            .collect();
        let ids: Vec<String> = (0..prompts.len()).map(|at| format!("t{at}")).collect();
        let list: Vec<(&str, &str, &str)> = ids
            .iter()
            .zip(&arguments)
            .map(|(id, arguments)| (id.as_str(), "task", arguments.as_str()))
            .collect();
        calls(&list)
    }

    #[tokio::test]
    async fn a_replys_task_calls_run_at_once_as_many_as_allowed_and_answer_in_their_order() {
        for (parallel, most) in [(3, 3), (2, 2), (1, 1)] {
            let model = ByPrompt::new(
                vec![
                    ("go", vec![tasks(&["A", "B", "C"]), text("All done.")]),
                    ("A", vec![text("answer A")]),
                    ("B", vec![text("answer B")]),
                    ("C", vec![text("answer C")]),
                ],
                Duration::from_millis(40),
            );
            let mut config = RunConfig::new("/w");
            config.parallel_subagents = parallel;
            let mut events = Vec::new();
            let outcome = run(
                &config,
                &model,
                &toolbox(),
                "go",
                &mut events,
                &Cancel::new(),
            )
            .await;
            assert_eq!(outcome.reason, FinishReason::Answered);
            assert_eq!(
                model.most.load(std::sync::atomic::Ordering::SeqCst),
                most,
                "at most {parallel} at once"
            );
            let answers: Vec<(String, String)> = model
                .last_parent
                .lock()
                .unwrap()
                .iter()
                .filter(|m| m.role == Role::Tool)
                .map(|m| (m.tool_call_id.clone().unwrap(), m.content.clone().unwrap()))
                .collect();
            assert_eq!(
                answers,
                [("t0", "answer A"), ("t1", "answer B"), ("t2", "answer C")]
                    .map(|(id, text)| (id.to_owned(), text.to_owned())),
                "in the order asked"
            );
            // Each subagent's events name its `task` call.
            for id in ["t0", "t1", "t2"] {
                assert!(events.iter().any(|event| matches!(event,
                    Event::Sub { task: Some(task), event, .. }
                    if task == id && matches!(&**event, Event::Finished { .. }))));
            }
            // Spent: the parent's two calls and the three subagents'.
            assert_eq!(outcome.usage.input_tokens, 7 + 10 + 3 * 10);
        }
    }

    #[tokio::test]
    async fn one_subagent_called_off_alone_leaves_the_run_and_the_others_going() {
        let model = ByPrompt::new(
            vec![
                ("go", vec![tasks(&["A", "B"]), text("All done.")]),
                ("A", vec![text("answer A")]),
                ("B", vec![text("answer B")]),
            ],
            Duration::from_millis(150),
        );
        let config = RunConfig::new("/w");
        let cancel = Cancel::new();
        let mut events = Vec::new();
        let tools = toolbox();
        let (outcome, stopped) = tokio::join!(
            run(&config, &model, &tools, "go", &mut events, &cancel),
            async {
                tokio::time::sleep(Duration::from_millis(200)).await;
                cancel.cancel_subagent("t0")
            }
        );
        assert!(stopped, "t0 was running");
        assert_eq!(outcome.reason, FinishReason::Answered);
        let answers: Vec<String> = model
            .last_parent
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.role == Role::Tool)
            .map(|m| m.content.clone().unwrap())
            .collect();
        assert!(
            answers[0].contains("the owner stopped this subagent"),
            "{answers:?}"
        );
        assert_eq!(answers[1], "answer B");
        assert!(
            !cancel.cancel_subagent("t1"),
            "an ended one is no longer listed"
        );
    }

    #[tokio::test]
    async fn subagents_at_once_share_what_is_left_of_the_budget() {
        let work = || vec![calls(&[("e", "echo", r#"{"text":"x"}"#)]), text("done")];
        let model = ByPrompt::new(
            vec![
                ("go", vec![tasks(&["A", "B"]), text("All done.")]),
                ("A", work()),
                ("B", work()),
            ],
            Duration::from_millis(20),
        );
        let mut config = RunConfig::new("/w");
        // The parent's first call spends 8, which leaves 12; a subagent's first call spends 8 — alone either goes on
        // and answers, together they are past what is left once both have called (which checks first is timing).
        config.budget.max_tokens = Some(8 + 12);
        let mut events = Vec::new();
        run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let stopped = events
            .iter()
            .filter(|event| {
                matches!(event, Event::ToolResult { name, output, .. }
                if name == "task" && output.contains("Budget"))
            })
            .count();
        assert!(
            stopped >= 1,
            "the shared budget stopped one of them at least"
        );
    }

    #[test]
    fn a_batch_knows_who_wrote_a_file_after_whose_read() {
        let group = Group::default();
        let file = lexical(Path::new("/w"), "src/../src/./a.rs");
        assert_eq!(file, Path::new("/w/src/a.rs"));
        let seen = group.version(&file);
        assert_eq!(seen, 0);
        assert!(!group.changed_by_other(&file, "t1", seen));
        let written = group.wrote(&file, "t0");
        assert!(
            group.changed_by_other(&file, "t1", seen),
            "t0 wrote after t1's read"
        );
        assert!(
            !group.changed_by_other(&file, "t0", seen),
            "its own write is no news to it"
        );
        assert!(
            !group.changed_by_other(&file, "t1", written),
            "read again since"
        );
        assert_eq!(
            group
                .spend(
                    Usage {
                        input_tokens: 5,
                        ..Usage::default()
                    },
                    None
                )
                .input_tokens,
            5
        );
        assert_eq!(
            group
                .spend(
                    Usage {
                        input_tokens: 2,
                        ..Usage::default()
                    },
                    None
                )
                .input_tokens,
            7
        );
    }

    #[tokio::test]
    async fn a_write_holds_its_path_until_it_is_done() {
        let group = Group::default();
        let file = PathBuf::from("/w/a.rs");
        let held = group.hold(&file).await;
        let waiting = tokio::time::timeout(Duration::from_millis(30), group.hold(&file)).await;
        assert!(waiting.is_err(), "a second write of the path waits");
        assert!(
            tokio::time::timeout(Duration::from_millis(30), group.hold(Path::new("/w/b.rs")))
                .await
                .is_ok(),
            "another path does not"
        );
        drop(held);
        assert!(
            tokio::time::timeout(Duration::from_millis(30), group.hold(&file))
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn a_task_runs_a_subagent_and_brings_its_answer_and_tokens_back() {
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "find it", "prompt": "Look up the thing."}),
            )),
            Ok(text("found it")),
            Ok(text("All done.")),
        ]);
        let mut config = RunConfig::new("/w");
        config.instructions = Some("parent's instructions".into());
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;

        assert_eq!(outcome.reason, FinishReason::Answered);
        assert_eq!(outcome.answer.as_deref(), Some("All done."));
        assert_eq!(
            outcome.turns, 2,
            "a subagent's turns are its own, not the parent's"
        );
        // 7/1 (the task call) + 10/2 (the subagent) + 10/2 (the parent's answer): the subagent's
        // tokens summed into the run's usage, counted once (D2.4).
        assert_eq!(
            outcome.usage,
            Usage {
                input_tokens: 27,
                cached_input_tokens: 0,
                output_tokens: 5
            }
        );

        let seen = model.seen.lock().unwrap();
        // The parent's tool result is the subagent's answer.
        assert!(
            seen[2]
                .iter()
                .any(|m| m.role == Role::Tool && m.content.as_deref() == Some("found it"))
        );
        // The subagent's own conversation: its own prompt (D5), the parent's instructions nowhere.
        assert_eq!(seen[1].len(), 2, "[system, user] and nothing else");
        assert_eq!(seen[1][0].role, Role::System);
        let sub_system = seen[1][0].content.as_deref().unwrap();
        assert!(sub_system.contains("subagent"));
        assert!(
            !sub_system.contains("parent's instructions"),
            "the parent's instructions are not the subagent's (D5)"
        );
        assert_eq!(seen[1][1].content.as_deref(), Some("Look up the thing."));
        // The parent keeps them; `task` is offered to the parent only (D3/B2).
        assert!(
            seen[0][0]
                .content
                .as_deref()
                .unwrap()
                .contains("parent's instructions")
        );
        let offered = model.offered.lock().unwrap();
        assert!(offered[0].contains(&"task".to_owned()));
        assert!(
            !offered[1].contains(&"task".to_owned()),
            "a subagent is never offered `task` itself (B2)"
        );
        assert!(offered[2].contains(&"task".to_owned()));
        drop(offered);
        drop(seen);

        // The subagent's stream arrives wrapped at depth 1: its answer and its own `Finished` …
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Sub { depth: 1, event, .. }
                if matches!(&**event, Event::AssistantText { .. })))
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::Sub { depth: 1, event, .. }
                if matches!(&**event, Event::Finished { .. })))
        );
        // … and exactly one unwrapped `Finished` — the run's own, with the summed tokens.
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Finished { .. }))
                .count(),
            1
        );
        assert!(matches!(
            events.last(),
            Some(Event::Finished {
                reason: FinishReason::Answered,
                turns: 2,
                usage: Usage {
                    input_tokens: 27,
                    cached_input_tokens: 0,
                    output_tokens: 5
                },
            })
        ));
        // The stream opens with `task` in the run's own tool list (D3).
        assert!(matches!(
            events.first(),
            Some(Event::Started { tools, .. }) if tools.contains(&"task".to_owned())
        ));
    }

    #[tokio::test]
    async fn a_named_agent_runs_with_its_own_prompt_and_an_unknown_or_unbuildable_one_starts_nothing()
     {
        let agent = |raw: &str| {
            crate::agents::Agent::parse(
                raw.split('\n').nth(1).unwrap().trim_start_matches("name: "),
                raw,
            )
            .unwrap()
        };
        let mut config = RunConfig::new("/w");
        config.agents = Arc::new(crate::agents::Catalog::of(vec![
            agent(
                "---\nname: finder\ndescription: Finds things.\n---\nYou find things, nothing else.",
            ),
            agent("---\nname: big\ndescription: Thinks hard.\nmodel: other/model\n---\nThink."),
        ]));
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "find", "prompt": "Find it.", "agent": "finder"}),
            )),
            Ok(text("found")),
            Ok(task_call(
                "c2",
                json!({"description": "x", "prompt": "Go.", "agent": "nobody"}),
            )),
            Ok(task_call(
                "c3",
                json!({"description": "x", "prompt": "Go.", "agent": "big"}),
            )),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.answer.as_deref(), Some("done"));
        let seen = model.seen.lock().unwrap();
        let sub_system = seen[1][0].content.as_deref().unwrap();
        assert!(
            sub_system.contains("You find things, nothing else."),
            "{sub_system}"
        );
        let answer = |at: usize| {
            seen[at]
                .iter()
                .rev()
                .find(|m| m.role == Role::Tool)
                .and_then(|m| m.content.clone())
                .unwrap_or_default()
        };
        assert!(
            answer(3).contains("no agent \"nobody\": the agents are big, finder"),
            "{}",
            answer(3)
        );
        assert!(
            answer(4).contains("agent big: this host cannot run a model"),
            "{}",
            answer(4)
        );
        assert_eq!(
            events.iter().filter(|e| matches!(e, Event::Sub { event, .. } if matches!(&**event, Event::Started { .. }))).count(),
            1,
            "only the agent that could run started"
        );
    }

    #[tokio::test]
    async fn glut_file_is_offered_only_while_a_maker_skill_runs_and_not_run_otherwise() {
        let scratch = std::env::temp_dir().join(format!("glut-run-files-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        let mut tools = toolbox();
        tools.extend([
            Box::new(glut_files::GlutFiles::new(&scratch, &scratch, true))
                as Box<dyn crate::tools::Tool>,
        ]);
        let listing = json!({"action": "list", "scope": "owner"}).to_string();
        let model = || {
            Script::new(vec![
                Ok(calls(&[("c1", glut_files::NAME, listing.as_str())])),
                Ok(text("done")),
            ])
        };

        let plain = model();
        run(
            &RunConfig::new("/w"),
            &plain,
            &tools,
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert!(!plain.offered.lock().unwrap()[0].contains(&glut_files::NAME.to_owned()));
        assert!(
            tool_answer(&plain).contains("offered only while one of make-skill"),
            "{}",
            tool_answer(&plain)
        );

        let mut config = RunConfig::new("/w");
        config.glut_files = true;
        let making = model();
        run(
            &config,
            &making,
            &tools,
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert!(making.offered.lock().unwrap()[0].contains(&glut_files::NAME.to_owned()));
        assert!(
            tool_answer(&making).contains("nothing there yet"),
            "{}",
            tool_answer(&making)
        );
        // A later turn of a conversation that loaded a maker keeps it: the flow asks, shows and writes over turns.
        let earlier = vec![
            Message::user("make me an agent"),
            Message {
                role: Role::Assistant,
                tool_calls: vec![ToolCall {
                    id: "s1".into(),
                    kind: "function".into(),
                    function: FunctionCall {
                        name: crate::skills::tool::NAME.into(),
                        arguments: json!({"name": " make-agent"}).to_string(),
                    },
                }],
                ..Message::user("")
            },
            Message::tool("s1", "the skill's text"),
        ];
        let later = model();
        run_continuing(
            &RunConfig::new("/w"),
            &later,
            &tools,
            earlier,
            "yes, write it",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert!(later.offered.lock().unwrap()[0].contains(&glut_files::NAME.to_owned()));
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[tokio::test]
    async fn task_is_judged_like_a_change_deny_refuses_it_allow_runs_it() {
        use crate::permissions::{Mode, Nobody, Rule, RuleSet};
        let refused_script = || {
            Script::new(vec![
                Ok(task_call(
                    "c1",
                    json!({"description": "x", "prompt": "Do it."}),
                )),
                Ok(text("done")),
            ])
        };

        // B1: no rule, mode default, nobody there — an ask nobody answers is a refusal, and the
        // subagent never starts. (An interactive run answers it; `allow task` or
        // `--permission-mode auto` switches it on without asking.)
        let mut tools = toolbox();
        tools.set_gate(Arc::new(Gate::new(
            RuleSet::default(),
            Mode::Default,
            Arc::new(Nobody),
        )));
        let model = refused_script();
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let answer = tool_answer(&model);
        assert!(answer.contains("no one to ask"), "{answer}");
        assert!(events.iter().any(
            |e| matches!(e, Event::Permission { allowed: false, tool, .. } if tool == "task")
        ));
        assert!(
            !events.iter().any(|e| matches!(e, Event::Sub { .. })),
            "a refused task never starts a subagent"
        );

        // `deny task` refuses it: the rules speak first.
        let mut tools = toolbox();
        let rules = RuleSet {
            deny: vec![Rule::parse("task").unwrap()],
            ..RuleSet::default()
        };
        tools.set_gate(Arc::new(Gate::new(rules, Mode::Default, Arc::new(Nobody))));
        let model = refused_script();
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert!(tool_answer(&model).contains("not permitted"));
        assert!(!events.iter().any(|e| matches!(e, Event::Sub { .. })));

        // `allow task` runs it — judged, reported, then the subagent.
        let mut tools = toolbox();
        let rules = RuleSet {
            allow: vec![Rule::parse("task").unwrap()],
            ..RuleSet::default()
        };
        tools.set_gate(Arc::new(Gate::new(rules, Mode::Default, Arc::new(Nobody))));
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Do it."}),
            )),
            Ok(text("the subagent's answer")),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.answer.as_deref(), Some("done"));
        assert!(events.iter().any(|e| matches!(e, Event::Sub { .. })));
        assert!(
            events.iter().any(
                |e| matches!(e, Event::Permission { allowed: true, tool, .. } if tool == "task")
            )
        );
        let seen = model.seen.lock().unwrap();
        assert!(
            seen[2]
                .iter()
                .any(|m| m.role == Role::Tool
                    && m.content.as_deref() == Some("the subagent's answer"))
        );
    }

    #[tokio::test]
    async fn a_narrowed_run_needs_task_declared_and_still_judges_it_afterwards() {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let script = || {
            Script::new(vec![
                Ok(task_call(
                    "c1",
                    json!({"description": "x", "prompt": "Do it."}),
                )),
                Ok(text("done")),
            ])
        };
        let narrowed = |allowed: Vec<Rule>, allow: Vec<Rule>| {
            let mut gate = Gate::new(
                RuleSet {
                    allow,
                    ..RuleSet::default()
                },
                Mode::Default,
                Arc::new(Nobody),
            );
            gate.narrow("the command /demo", allowed);
            gate
        };

        // Not declared: refused before any rule or mode, naming the command (HS6b-B3).
        let mut tools = toolbox();
        tools.set_gate(Arc::new(narrowed(
            vec![Rule::parse("Read").unwrap()],
            Vec::new(),
        )));
        let model = script();
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let answer = tool_answer(&model);
        assert!(answer.contains("demo"), "{answer}");
        assert!(answer.contains("allows only"), "{answer}");
        assert!(!events.iter().any(|e| matches!(e, Event::Sub { .. })));

        // Declared: the narrowing lets it through, the gate still judges it (B1) — a declaration
        // alone never allows anything (C3).
        let mut tools = toolbox();
        tools.set_gate(Arc::new(narrowed(
            vec![Rule::parse("task").unwrap()],
            Vec::new(),
        )));
        let model = script();
        let mut events = Vec::new();
        run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        let answer = tool_answer(&model);
        assert!(answer.contains("no one to ask"), "{answer}");
        assert!(!events.iter().any(|e| matches!(e, Event::Sub { .. })));

        // Declared and allowed by the run's own rules: the subagent runs.
        let mut tools = toolbox();
        tools.set_gate(Arc::new(narrowed(
            vec![Rule::parse("task").unwrap()],
            vec![Rule::parse("task").unwrap()],
        )));
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Do it."}),
            )),
            Ok(text("the subagent's answer")),
            Ok(text("done")),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.answer.as_deref(), Some("done"));
        assert!(events.iter().any(|e| matches!(e, Event::Sub { .. })));
    }

    #[tokio::test]
    async fn the_owners_hooks_hold_inside_a_subagent_and_session_start_stays_run_once() {
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Use the echo tool."}),
            )),
            Ok(calls(&[("s1", "echo", r#"{"text":"hi"}"#)])),
            Ok(text("sub done")),
            Ok(text("all done")),
        ]);
        let config = hooks_config(
            "sub",
            json!({
                "SessionStart": [{"hooks": [{"type": "command", "command": "echo \"house rules\""}]}],
                "PreToolUse": [{"matcher": "echo", "hooks": [{"type": "command",
                    "command": "echo '{\"hookSpecificOutput\":{\"permissionDecision\":\"deny\",\"permissionDecisionReason\":\"not today\"}}'"}]}],
            }),
        );
        let mut events = Vec::new();
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert_eq!(
            (outcome.reason, outcome.answer.as_deref()),
            (FinishReason::Answered, Some("all done"))
        );

        // D4: the veto held inside the subagent — its echo never ran, the block arrived as its result.
        assert!(events.iter().any(|e| matches!(
            e,
            Event::Sub { depth: 1, event, .. }
                if matches!(&**event, Event::Hook { blocked: true, hook_event, .. }
                    if hook_event == "PreToolUse")
        )));
        {
            let seen = model.seen.lock().unwrap();
            assert!(seen[2].iter().any(|m| {
                m.role == Role::Tool
                    && m.content
                        .as_deref()
                        .is_some_and(|text| text.contains("error: not today"))
            }));
            // The subagent never saw the SessionStart context: it belongs to the run (D4).
            assert!(
                !seen[1][0]
                    .content
                    .as_deref()
                    .unwrap()
                    .contains("house rules")
            );
        }

        // SessionStart runs once, and never wrapped — not even a copy for the subagent.
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e, Event::Hook { hook_event, .. }
                    if hook_event == "SessionStart"))
                .count(),
            1
        );
        assert!(!events.iter().any(|e| matches!(e, Event::Sub { event, .. }
            if matches!(&**event, Event::Hook { hook_event, .. }
                if hook_event == "SessionStart"))));
    }

    #[tokio::test]
    async fn a_subagents_own_turn_limit_ends_it_and_its_time_limit_stops_it_early() {
        // Its own turn limit (D6): one turn may call a tool, the next one is already over it — and
        // the parent carries on untouched.
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Work."}),
            )),
            Ok(calls(&[("s1", "echo", r#"{"text":"go"}"#)])),
            Ok(text("all done")),
        ]);
        let mut config = RunConfig::new("/w");
        config.subagent_max_turns = 1;
        let outcome = run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(
            (outcome.reason, outcome.answer.as_deref()),
            (FinishReason::Answered, Some("all done")),
            "the subagent's limit is not the parent's"
        );
        {
            let seen = model.seen.lock().unwrap();
            let answer = seen[2]
                .iter()
                .find(|m| m.role == Role::Tool)
                .and_then(|m| m.content.clone())
                .unwrap_or_default();
            assert!(answer.contains("MaxTurns"), "{answer}");
        }

        // Its own time limit (D6): a slow model cannot hold the subagent past it, and the parent's
        // queued replies stay the parent's — the subagent never finished its first call.
        let mut slow = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Work."}),
            )),
            Ok(text("the parent carried on")),
        ]);
        slow.delay = Duration::from_millis(300);
        let mut config = RunConfig::new("/w");
        config.subagent_timeout = Duration::from_millis(50);
        let outcome = run(
            &config,
            &slow,
            &toolbox(),
            "go",
            &mut Vec::new(),
            &Cancel::new(),
        )
        .await;
        assert_eq!(outcome.answer.as_deref(), Some("the parent carried on"));
        let seen = slow.seen.lock().unwrap();
        assert_eq!(
            seen.len(),
            3,
            "the parent's two turns and the subagent's one unfinished"
        );
        let answer = seen[2]
            .iter()
            .find(|m| m.role == Role::Tool)
            .and_then(|m| m.content.clone())
            .unwrap_or_default();
        assert!(answer.contains("Timeout"), "{answer}");
    }

    #[tokio::test]
    async fn a_cancel_from_inside_the_subagent_ends_the_whole_run() {
        let cancel = Cancel::new();
        let mut tools = toolbox();
        tools.extend([Box::new(CancelRun(cancel.clone())) as Box<dyn Tool>]);
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Work."}),
            )),
            Ok(calls(&[("s1", "cancel_run", "{}")])),
        ]);
        let mut events = Vec::new();
        let outcome = run(
            &RunConfig::new("/w"),
            &model,
            &tools,
            "go",
            &mut events,
            &cancel,
        )
        .await;
        assert_eq!(outcome.reason, FinishReason::Cancelled);
        assert_eq!(outcome.answer, None);
        // Both replies were spent and both are the run's tokens: 7/1 (the task) + 7/1 (the
        // subagent's last turn).
        assert_eq!(
            outcome.usage,
            Usage {
                input_tokens: 14,
                cached_input_tokens: 0,
                output_tokens: 2
            }
        );
        // The bracket still closed: the parent's stream shows why the subagent stopped.
        assert!(events.iter().any(
            |e| matches!(e, Event::ToolResult { ok: false, output, .. } if output.contains("Cancelled"))
        ));
    }

    #[tokio::test]
    async fn a_run_without_subagents_never_offers_task_and_a_task_call_has_no_tool() {
        let model = Script::new(vec![
            Ok(task_call(
                "c1",
                json!({"description": "x", "prompt": "Work."}),
            )),
            Ok(text("done")),
        ]);
        let mut config = RunConfig::new("/w");
        config.subagents = false;
        let mut events = Vec::new();
        run(
            &config,
            &model,
            &toolbox(),
            "go",
            &mut events,
            &Cancel::new(),
        )
        .await;
        assert!(!model.offered.lock().unwrap()[0].contains(&"task".to_owned()));
        assert!(matches!(
            events.first(),
            Some(Event::Started { tools, .. }) if !tools.contains(&"task".to_owned())
        ));
        // Not intercepted, so the ordinary "no tool named" of the toolbox answers it.
        assert!(tool_answer(&model).contains("no tool named"));
    }

    #[tokio::test]
    async fn a_secret_read_is_masked_and_goes_back_only_into_its_own_file() {
        let dir = Scratch::new("glut-masks-write");
        std::fs::write(
            dir.path().join(".env"),
            "API_KEY=abc123def456ghi\nDEBUG=0\n",
        )
        .unwrap();
        let placeholder = "‹secret 1: API_KEY, 15 chars›";
        let rewrite =
            json!({"path": ".env", "content": format!("API_KEY={placeholder}\nDEBUG=1\n")})
                .to_string();
        let smuggle = json!({"path": "out.txt", "content": placeholder}).to_string();
        // A link named like the placeholder leads to `.env`: the path still never gets the value.
        std::os::unix::fs::symlink(".env", dir.path().join(placeholder)).unwrap();
        let named = json!({"path": placeholder, "content": "x"}).to_string();
        let model = Script::new(vec![
            Ok(calls(&[("r", "read_file", r#"{"path":".env"}"#)])),
            Ok(calls(&[("w", "write_file", rewrite.as_str())])),
            Ok(calls(&[("s", "write_file", smuggle.as_str())])),
            Ok(calls(&[("n", "write_file", named.as_str())])),
            Ok(text("done")),
        ]);
        let mut tools = Toolbox::new();
        tools.extend(
            crate::tools::files::file_tools(dir.path(), Default::default(), Default::default())
                .unwrap(),
        );
        let config = RunConfig::new(dir.path());
        let mut events = Vec::new();
        let outcome = run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        assert_eq!(outcome.reason, FinishReason::Answered);
        let answers = tool_answers(&model);
        assert!(
            answers[0].contains(placeholder) && !answers[0].contains("abc123"),
            "{}",
            answers[0]
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".env")).unwrap(),
            "API_KEY=abc123def456ghi\nDEBUG=1\n"
        );
        assert!(answers[2].contains("not in this file"), "{}", answers[2]);
        assert!(!dir.path().join("out.txt").exists());
        assert!(answers[3].contains("`path`"), "{}", answers[3]);
        assert!(!dir.path().join("abc123def456ghi").exists());
        assert!(
            events
                .iter()
                .any(|event| matches!(event, Event::Masked { id, .. } if id == "r"))
        );
        // Nothing of the value reached an event either.
        let printed = serde_json::to_string(&events).unwrap();
        assert!(!printed.contains("abc123def456ghi"), "{printed}");
    }

    #[tokio::test]
    async fn without_masks_a_result_goes_as_it_is() {
        let dir = Scratch::new("glut-masks-off");
        std::fs::write(dir.path().join(".env"), "API_KEY=abc123def456ghi\n").unwrap();
        let model = Script::new(vec![
            Ok(calls(&[("r", "read_file", r#"{"path":".env"}"#)])),
            Ok(text("done")),
        ]);
        let mut tools = Toolbox::new();
        tools.extend(
            crate::tools::files::file_tools(dir.path(), Default::default(), Default::default())
                .unwrap(),
        );
        let config = RunConfig {
            masks: None,
            ..RunConfig::new(dir.path())
        };
        let mut events = Vec::new();
        run(&config, &model, &tools, "go", &mut events, &Cancel::new()).await;
        assert!(tool_answers(&model)[0].contains("abc123def456ghi"));
    }

    /// A folder of its own for one test, gone with it.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Scratch(dir.canonicalize().unwrap())
        }

        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// T60c: the snapshots of a run take a folder beside the working one at the step after it was added, stop with it
    /// taken away — still named for the undo —, and say a folder that cannot be recorded once.
    #[test]
    fn the_snapshots_follow_the_folders_beside_and_name_each_for_the_undo() {
        let made = |name: &str| {
            let dir = std::env::temp_dir().join(format!(
                "glut-taken-{name}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir.canonicalize().unwrap()
        };
        let (root, work, vault) = (made("root"), made("work"), made("vault"));
        std::fs::write(vault.join("v.txt"), "v").unwrap();
        let mut taken = Taken::new(Snapshots::open(&root, &work, "run1").unwrap());
        let (_, notes) = taken.take(&root, "run1", &[], 1).unwrap();
        assert!(notes.is_empty());
        let gone = work.with_file_name("glut-taken-never-there");
        let (_, notes) = taken
            .take(&root, "run1", &[vault.clone(), gone.clone()], 2)
            .unwrap();
        assert_eq!(notes.len(), 1, "{notes:?}");
        let (_, notes) = taken
            .take(&root, "run1", &[vault.clone(), gone], 3)
            .unwrap();
        assert!(notes.is_empty(), "said once: {notes:?}");
        let (_, _) = taken.take(&root, "run1", &[], 4).unwrap();
        assert!(taken.beside.is_empty(), "taken away: recorded no more");
        // Added again: its record goes on, its first step kept.
        let (_, _) = taken
            .take(&root, "run1", std::slice::from_ref(&vault), 5)
            .unwrap();
        assert_eq!(
            snapshot::beside_of(&root, &work, "run1"),
            vec![vault.clone()]
        );
        let steps: Vec<u32> = snapshot::runs(&root, &vault)[0]
            .steps
            .iter()
            .map(|step| step.turn)
            .collect();
        assert_eq!(steps, vec![2, 3, 5]);
        for dir in [root, work, vault] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
