//! The loop: ask the model, run the tools it calls, hand back their results, until it answers without a call.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::codemode;
use crate::config::RunConfig;
use crate::event::{EVENT_RESULT_CHARS, Event, EventSink, FinishReason};
use crate::history::{self, ResultStore};
use crate::hooks::{self, HookEvent, HookOutcome};
use crate::model::{ChatModel, Message, Role, ToolCall, ToolSpec, Usage};
use crate::permissions::{Access, Answer, Gate, Mode};
use crate::planning;
use crate::prompt::{subagent_prompt, system_prompt};
use crate::snapshot::{self, Snapshots};
use crate::tools::shell::{Session, ShellOptions};
use crate::tools::{StopFlag, Toolbox};

/// The most tool calls of one model reply that are run. Each call can write a file; a reply asking for hundreds is a
/// model gone wrong, not a plan. The rest are answered with an error, so every call still gets its reply (the API
/// wants one per call id) and the model learns to ask for fewer at once.
pub const MAX_CALLS_PER_TURN: usize = 16;

/// A way for the caller to call a run off from elsewhere (a Stop button, a closing pane). Cheap to clone; every clone
/// cancels the same run.
#[derive(Clone, Default)]
pub struct Cancel {
    flag: Arc<AtomicBool>,
    notify: Arc<Notify>,
}

impl Cancel {
    pub fn new() -> Self {
        Self::default()
    }

    /// Calls the run off: a model call or a tool in progress is dropped, and the run ends as `cancelled`.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
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
/// For a program that runs the harness as its main work (`axh`, the CLI); call it from inside the tokio runtime.
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
    run_inner(config, model, tools, Vec::new(), prompt, sink, cancel, 0)
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
    run_inner(config, model, tools, earlier, prompt, sink, cancel, 0).await
}

/// The loop at nesting `depth` (HS8, D3): depth 0 is the run itself — it offers the `task` tool, runs
/// `SessionStart` and speaks the caller's system prompt; depth ≥ 1 is a subagent, which gets the prompt of
/// `prompt::subagent_prompt` (D5), no `task` tool and no `SessionStart` (D4). Everything else — hooks, gate,
/// tools, events — is identical on every level.
#[allow(clippy::too_many_arguments)] // The loop's whole input; a struct for it would only rename the list.
async fn run_inner<M: ChatModel>(
    config: &RunConfig,
    model: &M,
    tools: &Toolbox,
    earlier: Vec<Message>,
    prompt: &str,
    sink: &mut dyn EventSink,
    cancel: &Cancel,
    depth: u32,
) -> (RunOutcome, Vec<Message>) {
    let continuing = !earlier.is_empty();
    let deadline = Instant::now() + config.timeout;
    // HS8, D1/D3: `task` exists at depth 0 only, and only for a run that offers subagents at all (D7) — a
    // subagent never sees the tool, so nesting deeper than one cannot start; and its name is in the run's
    // tool list from the first turn on.
    let offer_task = depth == 0 && config.subagents;
    let offer = Offer::of(config, tools, depth);
    sink.emit(Event::Started {
        model: model.name().to_owned(),
        cwd: config.cwd.display().to_string(),
        tools: offer.specs.iter().map(|spec| spec.name.clone()).collect(),
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
                "axh-{}-{}",
                std::process::id(),
                chrono::Local::now().timestamp_millis()
            ),
            deadline,
            stop: &stop,
            cancel,
            gate: tools.gate().cloned(),
        },
        messages: opening_messages(config, model.name(), earlier, prompt, depth),
        usage: Usage::default(),
        answer: None,
        turns: 0,
        subagents_started: 0,
        results: ResultStore::new(store_tag()),
        settled: 0,
        offer,
        kept: codemode::Kept::default(),
        todos: Vec::new(),
        snapshots: SnapshotState::Unopened,
        diagnostics: None,
    };
    if depth == 0 && !continuing {
        run.session_start(sink).await;
    }
    let reason = run.drive(sink).await;
    stop.store(true, Ordering::SeqCst);
    let Loop {
        usage,
        answer,
        turns,
        messages,
        ..
    } = run;
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
    };
    (outcome, messages)
}

/// The conversation a run starts with: its system prompt — the caller's instructions at depth 0, the
/// subagent's own prompt below (D5) — then the `earlier` conversation without its old system prompt, and the
/// user's message.
fn opening_messages(
    config: &RunConfig,
    model: &str,
    earlier: Vec<Message>,
    prompt: &str,
    depth: u32,
) -> Vec<Message> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let system = if depth == 0 {
        system_prompt(&config.cwd, &today, model, config.instructions.as_deref())
    } else {
        subagent_prompt(&config.cwd, &today)
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
    messages.extend(earlier);
    messages.push(Message::user(prompt));
    messages
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
    usage: Usage,
    /// The last reply's text only: a remark beside earlier tool calls is no answer to the task.
    answer: Option<String>,
    turns: u32,
    /// HS8: the subagents this run started, counted against `config.max_subagents`.
    subagents_started: u32,
    /// Glut 2, B: the whole texts of the results that shrank.
    results: ResultStore,
    /// Up to which message the conversation has been looked at for results to shrink.
    settled: usize,
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
}

/// Where a run's snapshots stand.
enum SnapshotState {
    Unopened,
    Open(Snapshots),
    /// They could not be taken (no git, no store): the run goes on without, and says so once.
    Off,
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
    fn of(config: &RunConfig, tools: &Toolbox, depth: u32) -> Self {
        let (direct, behind) = if config.mcp_behind_code {
            tools.split_specs()
        } else {
            (tools.specs(), Vec::new())
        };
        let codemode = config.codemode && !(direct.is_empty() && behind.is_empty());
        let catalog = Arc::new(codemode::Catalog {
            specs: direct.iter().chain(&behind).cloned().collect(),
        });
        let mut specs = if codemode {
            direct.clone()
        } else {
            tools.specs()
        };
        if codemode {
            specs.push(codemode::spec(&direct, &behind));
        }
        if depth == 0 && config.subagents {
            specs.push(crate::task::spec());
        }
        // Offered from the first turn on, so the tool list — and with it the prompt's cached head — stays put.
        if config.shrink_results {
            specs.push(history::spec());
        }
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

/// How often a script may be found waiting with no call open before it counts as stuck: it waits on a promise nothing
/// can settle (there are no timers). A few polls, not one, so a call answered a moment ago gets to land first.
const STUCK_POLLS: u32 = 8;

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
            self.turns += 1;
            let specs = self.offer.specs.clone();
            let calls = match self.ask(&specs, sink).await {
                Ok(calls) => calls,
                Err(ending) => return ending,
            };
            let ended = if calls.is_empty() {
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

    /// One model call: its usage and text reported, its message kept, its tool calls handed back.
    async fn ask(
        &mut self,
        specs: &[ToolSpec],
        sink: &mut dyn EventSink,
    ) -> Result<Vec<ToolCall>, FinishReason> {
        let turn = self.turns;
        if self.config.shrink_results {
            self.settled = history::shrink_over_budget(
                &mut self.messages,
                self.settled,
                &self.results,
                history::SHRINK_BUDGET_CHARS,
            );
        }
        let reply = {
            // The answer's text goes out as it streams in (CP5); the whole text follows below either way.
            let mut on_text = |text: &str| {
                sink.emit(Event::TextDelta {
                    turn,
                    text: text.to_owned(),
                });
            };
            let completion = self.model.complete_streaming(
                &self.messages,
                specs,
                self.config.max_output_tokens,
                &mut on_text,
            );
            tokio::select! {
                reply = completion => reply,
                () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
                () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
            }
        };
        let reply = reply.map_err(|err| {
            sink.emit(Event::Error {
                message: err.to_string(),
            });
            FinishReason::Failed
        })?;
        self.usage += reply.usage;
        sink.emit(Event::Usage {
            turn,
            input_tokens: reply.usage.input_tokens,
            output_tokens: reply.usage.output_tokens,
        });
        self.answer = reply.message.content.clone();
        if let Some(text) = &self.answer {
            sink.emit(Event::AssistantText {
                turn,
                text: text.clone(),
            });
        }
        let calls = reply.message.tool_calls.clone();
        self.messages.push(reply.message);
        Ok(calls)
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
        Ok(Ending::Refused)
    }

    /// The reply's tool calls, one after the other in the order asked: a later call may depend on an earlier
    /// one (write, then read). Past [`MAX_CALLS_PER_TURN`] each is answered with an error instead, so every call
    /// still gets its reply (the API wants one per call id).
    async fn run_calls(
        &mut self,
        calls: Vec<ToolCall>,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        for (index, call) in calls.into_iter().enumerate() {
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
            self.run_call(call, sink).await?;
        }
        Ok(())
    }

    /// One tool call: the PreToolUse hooks, the call itself, its permission events and result, the PostToolUse
    /// hooks, and the message the model gets back. `Err` when the run ended on the way.
    async fn run_call(
        &mut self,
        call: ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<(), FinishReason> {
        let output = self.perform(&call, sink).await?;
        self.messages.push(Message::tool(call.id, output));
        Ok(())
    }

    /// One call as [`Self::run_call`] runs it, without adding its answer to the conversation — what a script's call
    /// (Glut 2, A) shares with a direct one: the hooks, the gate's events, the result event. Returns the text the
    /// caller hands on.
    async fn perform(
        &mut self,
        call: &ToolCall,
        sink: &mut dyn EventSink,
    ) -> Result<String, FinishReason> {
        let turn = self.turns;
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
            let text = format!("error: {}", refusal(before, "a hook refused this call"));
            sink.emit(Event::tool_result(
                turn,
                &call.id,
                &call.function.name,
                false,
                &text,
            ));
            return Ok(text);
        }
        self.snapshot_before(call, sink).await;
        let (ok, mut output) = self.execute(call, sink).await?;
        if ok && let Some(note) = self.diagnostics_after(call).await {
            output.push_str("\n\n");
            output.push_str(&note);
        }
        if let Some(gate) = self.tools.gate() {
            for judged in gate.take_log() {
                sink.emit(Event::permission(turn, &call.id, &judged));
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
        self.messages.push(Message::tool(call.id, output));
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
        // Only the run's own store: nothing for the gate to judge, like `bash_output`.
        if self.config.shrink_results && name == history::NAME {
            return Ok(history::answer(&self.results, &call.function.arguments));
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
        tokio::select! {
            result = self.tools.call_until(name, &call.function.arguments, self.stop) => Ok(result),
            () = tokio::time::sleep_until(self.deadline) => Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => Err(FinishReason::Cancelled),
        }
    }

    /// Records the work folder before the first call of this turn that may change it — an edit or a command (Glut 2,
    /// D) — when the run keeps snapshots. A failure never stops the call: the run goes on without, said once.
    async fn snapshot_before(&mut self, call: &ToolCall, sink: &mut dyn EventSink) {
        let Some(root) = self.config.snapshots.clone() else {
            return;
        };
        let changes = matches!(
            self.tools
                .access_of(&call.function.name, &call.function.arguments),
            Some(Access::Edit { .. } | Access::Bash { .. })
        );
        if !changes || matches!(self.snapshots, SnapshotState::Off) {
            return;
        }
        let (turn, run, cwd) = (
            self.turns,
            self.hooks.session_id.clone(),
            self.config.cwd.clone(),
        );
        let state = std::mem::replace(&mut self.snapshots, SnapshotState::Off);
        // Git is blocking work; the snapshots move to a blocking thread and back.
        let taken = tokio::task::spawn_blocking(move || {
            let mut snaps = match state {
                SnapshotState::Open(snaps) => snaps,
                _ => {
                    snapshot::sweep(&root);
                    Snapshots::open(&root, &cwd, &run)?
                }
            };
            let commit = snaps.take(turn)?;
            Ok::<_, String>((snaps, commit))
        })
        .await
        .unwrap_or_else(|err| Err(format!("the snapshot task failed: {err}")));
        match taken {
            Ok((snaps, commit)) => {
                if let Some(commit) = commit {
                    sink.emit(Event::Snapshot {
                        turn,
                        run: self.hooks.session_id.clone(),
                        commit,
                    });
                }
                self.snapshots = SnapshotState::Open(snaps);
            }
            Err(reason) => sink.emit(Event::Error {
                message: format!("no snapshots for this run (undo is not possible): {reason}"),
            }),
        }
    }

    /// The language server's errors for the file an edit just changed (Glut 2, D), when the run asks for them and a
    /// server for its language is installed. The servers' pipes are blocking work, done on the blocking pool.
    async fn diagnostics_after(&mut self, call: &ToolCall) -> Option<String> {
        if !self.config.diagnostics {
            return None;
        }
        let Some(Access::Edit { path }) = self
            .tools
            .access_of(&call.function.name, &call.function.arguments)
        else {
            return None;
        };
        let servers = Arc::clone(self.diagnostics.get_or_insert_with(|| {
            Arc::new(std::sync::Mutex::new(crate::diagnostics::Diagnostics::new(
                &self.config.cwd,
            )))
        }));
        tokio::task::spawn_blocking(move || {
            servers
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .after_edit(&path)
        })
        .await
        .ok()
        .flatten()
    }

    /// The `exit_plan` arm (Glut 2, D): the plan goes to the owner; yes switches the gate to the run's build mode and
    /// offers the tools that mode allows from the next turn on, no keeps Plan. Headless the ask is refused.
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
        match gate.ask_owner(planning::EXIT_PLAN, &question).await {
            Answer::Allow | Answer::AllowAlways => {
                let mode = self.config.build_mode;
                gate.set_mode(mode);
                sink.emit(Event::ModeChanged {
                    turn: self.turns,
                    mode: gate.mode_name().to_owned(),
                });
                self.offer = Offer::of(self.config, self.tools, self.depth);
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
        if name == codemode::NAME || name == crate::task::NAME || name == planning::EXIT_PLAN {
            let text = format!("not permitted: a script cannot start {name}");
            sink.emit(Event::tool_result(self.turns, &id, &name, false, &text));
            return Ok((false, text));
        }
        if name == history::NAME {
            let (ok, text) = history::answer(&self.results, &arguments);
            sink.emit(Event::tool_result(self.turns, &id, &name, ok, &text));
            return Ok((ok, text.trim_start_matches("error: ").to_owned()));
        }
        let call = ToolCall {
            id,
            kind: "function".into(),
            function: crate::model::FunctionCall { name, arguments },
        };
        let text = self.perform(&call, sink).await?;
        // A tool's failure is text that starts with `error: ` (the toolbox's convention); the script gets a rejection.
        Ok(match text.strip_prefix("error: ") {
            Some(reason) => (false, reason.to_owned()),
            None => (true, text),
        })
    }

    /// The `task` arm (HS8, D2): judge the call, run the subagent, hand its answer back as the tool's result — and
    /// its tokens, summed into the run's own usage so the spend stays honest (D2.4). A subagent that really ran
    /// counts against `config.max_subagents`; one refused or asked for wrongly does not.
    async fn run_task(&mut self, call: &ToolCall, sink: &mut dyn EventSink) -> (bool, String) {
        let config = self.config;
        if self.subagents_started >= config.max_subagents {
            let limit = config.max_subagents;
            return (
                false,
                format!(
                    "error: this run has started its {limit} subagents; do the rest of the work yourself"
                ),
            );
        }
        let prompt = match serde_json::from_str::<Value>(&call.function.arguments) {
            Ok(arguments @ Value::Object(_)) => match crate::task::prompt_of(&arguments) {
                Ok(prompt) => prompt,
                Err(reason) => return (false, format!("error: {reason}")),
            },
            Ok(_) => {
                return (
                    false,
                    "error: the arguments must be a JSON object".to_owned(),
                );
            }
            Err(err) => {
                return (
                    false,
                    format!("error: the arguments are not valid JSON: {err}"),
                );
            }
        };
        let (turn, id) = (self.turns, call.id.as_str());
        // The judgement and its event belong to the parent (D2.2): the nested run drains this very log in its
        // own loop, so what the parent judged is read here, before the subagent starts — never inside its
        // stream, never lost. B1: `task` is judged like any other call of a name the rules know.
        if let Some(gate) = self.tools.gate() {
            let judged = gate.judge(crate::task::NAME, &Access::Other).await;
            for entry in gate.take_log() {
                sink.emit(Event::permission(turn, id, &entry));
            }
            if let Err(refused) = judged {
                return (false, format!("error: {refused}"));
            }
        }
        let mut sub = config.clone();
        // D5: the caller's instructions are the parent's contract; the subagent gets its own prompt and
        // whatever the parent's model put into `prompt`.
        sub.instructions = None;
        sub.max_turns = config.subagent_max_turns;
        // The subagent never outlives the run that started it (D2.3).
        sub.timeout = config
            .subagent_timeout
            .min(self.deadline.saturating_duration_since(Instant::now()));
        let depth = self.depth + 1;
        let mut wrapped = SubSink { depth, inner: sink };
        let nested = run_inner(
            &sub,
            self.model,
            self.tools,
            Vec::new(),
            &prompt,
            &mut wrapped,
            self.cancel,
            depth,
        );
        let (outcome, _) = Box::pin(nested).await;
        // What the subagent judged but did not get to report (it was cancelled mid-call) still belongs to its
        // stream: drained here, wrapped, before the parent's `ToolResult(task)` brackets it (D2.4).
        if let Some(gate) = self.tools.gate() {
            for entry in gate.take_log() {
                wrapped.emit(Event::permission(turn, id, &entry));
            }
        }
        self.usage += outcome.usage;
        self.subagents_started += 1;
        match outcome.reason {
            FinishReason::Answered => match outcome.answer {
                // Cut like every tool result — the model gets the whole, already capped, answer.
                Some(answer) => (true, crate::tools::cap(answer)),
                None => (true, "the subagent finished without text".to_owned()),
            },
            reason => (
                false,
                format!("error: the subagent stopped without answering: {reason:?}"),
            ),
        }
    }
}
/// The nested run's view of the parent's sink (HS8, D8): every event it emits arrives wrapped in
/// [`Event::Sub`] at the depth it happened at — nothing dropped. Accounting never counts events; the
/// parent sums `RunOutcome`s (D2.4), so a viewer may count each run's own `Finished` and no other's.
struct SubSink<'a> {
    depth: u32,
    inner: &'a mut dyn EventSink,
}

impl EventSink for SubSink<'_> {
    fn emit(&mut self, event: Event) {
        self.inner.emit(Event::Sub {
            depth: self.depth,
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use serde_json::{Value, json};

    use super::*;
    use crate::error::HarnessError;
    use crate::model::{FunctionCall, Reply, Role, ToolCall, ToolSpec};
    use crate::tools::{Tool, ToolFuture};

    /// A model that plays prepared replies in order and records each conversation it was sent.
    struct Script {
        replies: Mutex<Vec<Result<Reply, HarnessError>>>,
        seen: Mutex<Vec<Vec<Message>>>,
        /// The tool names each request offered, in call order (HS8): the parent's turns carry `task`, a
        /// subagent's never do (B2/D3).
        offered: Mutex<Vec<Vec<String>>>,
        delay: Duration,
    }

    impl Script {
        fn new(replies: Vec<Result<Reply, HarnessError>>) -> Self {
            Script {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
                offered: Mutex::new(Vec::new()),
                delay: Duration::ZERO,
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
    }

    fn text(answer: &str) -> Reply {
        Reply {
            message: Message {
                role: Role::Assistant,
                content: Some(answer.into()),
                tool_calls: vec![],
                tool_call_id: None,
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

    fn toolbox() -> Toolbox {
        let mut tools = Toolbox::new();
        tools.extend([Box::new(Echo) as Box<dyn Tool>]);
        tools
    }

    #[tokio::test]
    async fn a_long_result_shrinks_after_the_model_answered_on_it_and_comes_back_whole() {
        let long = "x".repeat(crate::history::SHRINK_BUDGET_CHARS + 100);
        let args = json!({ "text": long }).to_string();
        let model = Script::new(vec![
            Ok(calls(&[("c1", "echo", args.as_str())])),
            Ok(calls(&[("c2", "echo", r#"{"text":"short"}"#)])),
            // By now c1's result is shortened; the model asks for it whole. The id's tag is the run's own.
            Ok(calls(&[(
                "c3",
                crate::history::NAME,
                r#"{"id": "PLACEHOLDER"}"#,
            )])),
            Ok(text("Done.")),
        ]);
        let mut events = Vec::new();
        // The id is only known once the run shrank c1; the script reads it from the third request below.
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
        // Second request: c1 is the newest result, still whole (as long as the toolbox lets any result be).
        let second = seen[1][3].content.as_deref().unwrap();
        assert!(
            second.len() > crate::tools::MAX_TOOL_OUTPUT_CHARS,
            "{}",
            second.len()
        );
        // Third request: the model answered after c1 (with c2's call), so c1 shrank; c2 is short and stays.
        let shrunk = seen[2][3].content.as_deref().unwrap();
        assert!(
            shrunk.len() < 1_500 && shrunk.contains(crate::history::NAME),
            "{shrunk}"
        );
        assert_eq!(seen[2][5].content.as_deref(), Some("short"));
        // The made-up id is answered as another run's, never with some other text.
        assert!(
            seen[3][7]
                .content
                .as_deref()
                .unwrap()
                .contains("no longer available")
        );
        // The tool is offered from the first request on.
        let started = events.iter().find_map(|e| match e {
            Event::Started { tools, .. } => Some(tools.clone()),
            _ => None,
        });
        assert!(started.unwrap().iter().any(|t| t == crate::history::NAME));
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
            "axh-run-hooks-{tag}-{}-{}",
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
        let mut config = RunConfig::new("/no/such/folder/axh-hooks");
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
}
