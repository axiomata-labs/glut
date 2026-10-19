//! The loop: ask the model, run the tools it calls, hand back their results, until it answers without a call.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::config::RunConfig;
use crate::event::{EVENT_RESULT_CHARS, Event, EventSink, FinishReason};
use crate::hooks::{self, HookEvent, HookOutcome};
use crate::model::{ChatModel, Message, Role, ToolCall, ToolSpec, Usage};
use crate::permissions::{Access, Gate};
use crate::prompt::{subagent_prompt, system_prompt};
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
    let mut specs = tools.specs();
    let mut offered = tools.names();
    if offer_task {
        specs.push(crate::task::spec());
        offered.push(crate::task::NAME.to_owned());
    }
    sink.emit(Event::Started {
        model: model.name().to_owned(),
        cwd: config.cwd.display().to_string(),
        tools: offered,
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
    };
    if depth == 0 && !continuing {
        run.session_start(sink).await;
    }
    let reason = run.drive(&specs, sink).await;
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
    async fn drive(&mut self, specs: &[ToolSpec], sink: &mut dyn EventSink) -> FinishReason {
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
            let calls = match self.ask(specs, sink).await {
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
        let completion = self
            .model
            .complete(&self.messages, specs, self.config.max_output_tokens);
        let reply = tokio::select! {
            reply = completion => reply,
            () = tokio::time::sleep_until(self.deadline) => return Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => return Err(FinishReason::Cancelled),
        };
        let reply = reply.map_err(|err| {
            sink.emit(Event::Error {
                message: err.to_string(),
            });
            FinishReason::Failed
        })?;
        let turn = self.turns;
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
            self.answer_call(call, false, text, sink);
            return Ok(());
        }
        let (ok, output) = self.execute(&call, sink).await?;
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
        let output = if after.blocked {
            format!("error: {}", refusal(after, "a hook refused this result"))
        } else {
            output
        };
        self.messages.push(Message::tool(call.id, output));
        Ok(())
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
        tokio::select! {
            result = self.tools.call_until(name, &call.function.arguments, self.stop) => Ok(result),
            () = tokio::time::sleep_until(self.deadline) => Err(FinishReason::Timeout),
            () = self.cancel.cancelled() => Err(FinishReason::Cancelled),
        }
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
