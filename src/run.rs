//! The loop: ask the model, run the tools it calls, hand back their results, until it answers without a call.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::{Value, json};
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::config::RunConfig;
use crate::event::{EVENT_RESULT_CHARS, Event, EventSink, FinishReason};
use crate::hooks::{self, HookEvent, HookOutcome};
use crate::model::{ChatModel, Message, Usage};
use crate::permissions::Gate;
use crate::prompt::system_prompt;
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
    let started = Instant::now();
    let deadline = started + config.timeout;
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let mut messages = vec![
        Message::system(system_prompt(
            &config.cwd,
            &today,
            config.instructions.as_deref(),
        )),
        Message::user(prompt),
    ];
    let specs = tools.specs();
    sink.emit(Event::Started {
        model: model.name().to_owned(),
        cwd: config.cwd.display().to_string(),
        tools: tools.names(),
    });

    let mut usage = Usage::default();
    let mut answer = None;
    // Set when the run stops, so a tool still at work (a search over a large tree) stops too and none starts after.
    let stop = StopFlag::default();
    // HS7: the session the hooks run in, built on the first event that needs one (D8), and the run's identity
    // for their payloads (D4) — there are no sessions yet, so one id for the whole run.
    let mut hook_session = HookSession::NotBuilt;
    let session_id = format!(
        "axh-{}-{}",
        std::process::id(),
        chrono::Local::now().timestamp_millis()
    );
    // HS7, D3: SessionStart once, after `Started` and before the first model call. Its context becomes a
    // system message right behind the harness's own; it can never block.
    if !config.hooks.is_empty() {
        let outcome = hooks_event(
            &mut hook_session,
            config,
            &session_id,
            HookEvent::SessionStart,
            "startup",
            json!({"source": "startup"}),
            None,
            deadline,
            &stop,
            cancel,
            tools.gate().cloned(),
            sink,
        )
        .await
        .unwrap_or_default();
        if let Some(context) = outcome.context {
            messages.insert(1, Message::system(context));
        }
    }
    // Set once a Stop hook blocked the answer: its payload then says so, and the hook lets it end (D3).
    let mut stop_hook_active = false;
    let mut turns = 0;
    let reason = loop {
        if cancel.is_cancelled() {
            break FinishReason::Cancelled;
        }
        if turns >= config.max_turns {
            break FinishReason::MaxTurns;
        }
        turns += 1;
        let reply = tokio::select! {
            reply = model.complete(&messages, &specs, config.max_output_tokens) => reply,
            () = tokio::time::sleep_until(deadline) => break FinishReason::Timeout,
            () = cancel.cancelled() => break FinishReason::Cancelled,
        };
        let reply = match reply {
            Ok(reply) => reply,
            Err(err) => {
                sink.emit(Event::Error {
                    message: err.to_string(),
                });
                break FinishReason::Failed;
            }
        };
        usage += reply.usage;
        sink.emit(Event::Usage {
            turn: turns,
            input_tokens: reply.usage.input_tokens,
            output_tokens: reply.usage.output_tokens,
        });
        // The last reply's text only: a remark beside earlier tool calls is no answer to the task.
        answer = reply.message.content.clone();
        if let Some(text) = &answer {
            sink.emit(Event::AssistantText {
                turn: turns,
                text: text.clone(),
            });
        }
        let calls = reply.message.tool_calls.clone();
        messages.push(reply.message);
        if calls.is_empty() {
            // HS7, D3: a Stop hook may refuse the answer; its reason goes back as a user message and the loop
            // takes another turn (`stop_hook_active` tells the hook it already did this once).
            if !config.hooks.is_empty() {
                match hooks_event(
                    &mut hook_session,
                    config,
                    &session_id,
                    HookEvent::Stop,
                    "",
                    json!({"stop_hook_active": stop_hook_active}),
                    Some(turns),
                    deadline,
                    &stop,
                    cancel,
                    tools.gate().cloned(),
                    sink,
                )
                .await
                {
                    Ok(outcome) if outcome.blocked => {
                        let block_reason = outcome
                            .context
                            .unwrap_or_else(|| "a hook refused to end the run".to_owned());
                        messages.push(Message::user(format!(
                            "A Stop hook blocked ending the run: {block_reason}\n\
                             Continue, and address what it asked for."
                        )));
                        stop_hook_active = true;
                        continue;
                    }
                    Ok(_) => {}
                    Err(ending) => break ending,
                }
            }
            break FinishReason::Answered;
        }
        // One after the other, in the order asked: a later call may depend on an earlier one (write, then read).
        let mut stopped = None;
        for (index, call) in calls.into_iter().enumerate() {
            sink.emit(Event::tool_call(
                turns,
                &call.id,
                &call.function.name,
                &call.function.arguments,
            ));
            if index >= MAX_CALLS_PER_TURN {
                let output = format!(
                    "error: at most {MAX_CALLS_PER_TURN} tool calls are run per turn; ask again"
                );
                sink.emit(Event::tool_result(
                    turns,
                    &call.id,
                    &call.function.name,
                    false,
                    &output,
                ));
                messages.push(Message::tool(call.id, output));
                continue;
            }
            // HS7, D3/B2: the owner's hooks may block a call before anything runs — they never let one
            // through, the gate stays the only decider.
            if !config.hooks.is_empty() {
                match hooks_event(
                    &mut hook_session,
                    config,
                    &session_id,
                    HookEvent::PreToolUse,
                    &call.function.name,
                    json!({
                        "tool_name": &call.function.name,
                        "tool_input": hooks::tool_input(&call.function.arguments),
                    }),
                    Some(turns),
                    deadline,
                    &stop,
                    cancel,
                    tools.gate().cloned(),
                    sink,
                )
                .await
                {
                    Ok(outcome) if outcome.blocked => {
                        let text = format!(
                            "error: {}",
                            outcome
                                .context
                                .as_deref()
                                .unwrap_or("a hook refused this call")
                        );
                        sink.emit(Event::tool_result(
                            turns,
                            &call.id,
                            &call.function.name,
                            false,
                            &text,
                        ));
                        messages.push(Message::tool(call.id, text));
                        continue;
                    }
                    Ok(_) => {}
                    Err(ending) => {
                        stopped = Some(ending);
                        break;
                    }
                }
            }
            let (ok, output) = tokio::select! {
                result = tools.call_until(&call.function.name, &call.function.arguments, &stop) => result,
                () = tokio::time::sleep_until(deadline) => { stopped = Some(FinishReason::Timeout); break }
                () = cancel.cancelled() => { stopped = Some(FinishReason::Cancelled); break }
            };
            if let Some(gate) = tools.gate() {
                for judged in gate.take_log() {
                    sink.emit(Event::permission(turns, &call.id, &judged));
                }
            }
            // In the stream the tool's own result is shown first (D3); the model's message may still differ.
            sink.emit(Event::tool_result(
                turns,
                &call.id,
                &call.function.name,
                ok,
                &output,
            ));
            // HS7, D3: PostToolUse after the tool ran, whatever it answered; a block replaces what the model
            // gets with the reason — the `ToolResult` above keeps what the tool really answered, and the
            // `Hook` event beside it says that a hook blocked.
            if !config.hooks.is_empty() {
                let response: String = output.chars().take(EVENT_RESULT_CHARS).collect();
                match hooks_event(
                    &mut hook_session,
                    config,
                    &session_id,
                    HookEvent::PostToolUse,
                    &call.function.name,
                    json!({
                        "tool_name": &call.function.name,
                        "tool_input": hooks::tool_input(&call.function.arguments),
                        "tool_response": response,
                    }),
                    Some(turns),
                    deadline,
                    &stop,
                    cancel,
                    tools.gate().cloned(),
                    sink,
                )
                .await
                {
                    Ok(outcome) if outcome.blocked => {
                        let text = format!(
                            "error: {}",
                            outcome
                                .context
                                .as_deref()
                                .unwrap_or("a hook refused this result")
                        );
                        messages.push(Message::tool(call.id, text));
                        continue;
                    }
                    Ok(_) => {}
                    Err(ending) => {
                        stopped = Some(ending);
                        break;
                    }
                }
            }
            messages.push(Message::tool(call.id, output));
        }
        if let Some(reason) = stopped {
            break reason;
        }
    };
    stop.store(true, std::sync::atomic::Ordering::SeqCst);
    if reason != FinishReason::Answered {
        answer = None;
    }
    sink.emit(Event::Finished {
        reason,
        turns,
        usage,
    });
    RunOutcome {
        reason,
        answer,
        turns,
        usage,
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

/// One hook event of the run (HS7, D3): the session built on demand (D8); the run's deadline and its cancel
/// honoured — dropping the wait mid-hook kills the hook's process group; every hook reported as an event
/// inside. `Err(reason)` when the run itself ran out of time or was cancelled while a hook worked; a hook's
/// own failure is a note and `Ok` (B3).
#[allow(clippy::too_many_arguments)]
async fn hooks_event(
    runner: &mut HookSession,
    config: &RunConfig,
    session_id: &str,
    event: HookEvent,
    target: &str,
    extra: Value,
    turn: Option<u32>,
    deadline: Instant,
    stop: &StopFlag,
    cancel: &Cancel,
    gate: Option<Arc<Gate>>,
    sink: &mut dyn EventSink,
) -> Result<HookOutcome, FinishReason> {
    let stdin = hooks::payload(session_id, &config.cwd, event, extra);
    let Some(session) = runner.get(config, sink).await else {
        return Ok(HookOutcome::default());
    };
    let limit = deadline.saturating_duration_since(Instant::now());
    let outcome = tokio::select! {
        outcome = hooks::run_event(turn, event, target, &stdin, &config.hooks, session, gate, stop, limit, sink) => outcome,
        () = tokio::time::sleep_until(deadline) => return Err(FinishReason::Timeout),
        () = cancel.cancelled() => return Err(FinishReason::Cancelled),
    };
    Ok(outcome)
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
        delay: Duration,
    }

    impl Script {
        fn new(replies: Vec<Result<Reply, HarnessError>>) -> Self {
            Script {
                replies: Mutex::new(replies),
                seen: Mutex::new(Vec::new()),
                delay: Duration::ZERO,
            }
        }
    }

    impl ChatModel for Script {
        async fn complete(
            &self,
            messages: &[Message],
            _tools: &[ToolSpec],
            _max: Option<u32>,
        ) -> Result<Reply, HarnessError> {
            self.seen.lock().unwrap().push(messages.to_vec());
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
                output_tokens: 1,
            },
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
}
