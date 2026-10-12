//! The loop: ask the model, run the tools it calls, hand back their results, until it answers without a call.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Notify;
use tokio::time::Instant;

use crate::config::RunConfig;
use crate::event::{Event, EventSink, FinishReason};
use crate::model::{ChatModel, Message, Usage};
use crate::prompt::system_prompt;
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
            let (ok, output) = tokio::select! {
                result = tools.call_until(&call.function.name, &call.function.arguments, &stop) => result,
                () = tokio::time::sleep_until(deadline) => { stopped = Some(FinishReason::Timeout); break }
                () = cancel.cancelled() => { stopped = Some(FinishReason::Cancelled); break }
            };
            sink.emit(Event::tool_result(
                turns,
                &call.id,
                &call.function.name,
                ok,
                &output,
            ));
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

        fn call(&self, arguments: Value, _stop: crate::tools::StopFlag) -> ToolFuture<'_> {
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
}
