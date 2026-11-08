# Using the `glut` library

The `glut` crate (`crates/glut`) is the harness without the terminal UI. You give it a model, a toolbox and a prompt;
it runs the tool-calling loop and reports every step as an `Event`. The program `glut` uses it, and so do other apps.
This page covers what another program needs in order to do the same. The crate structure is described in
[`architecture.md`](architecture.md), and the security model in [`security.md`](security.md).

This page describes the code as it stands. The crate is at version `0.1.0`, and its API is not stable yet.

## Adding the dependency

`glut` is on crates.io:

```toml
[dependencies]
glut = "0.1"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
serde_json = "1"
```

`glut` depends on `axiomata-files`, the guarded file service from the
[Axiomata-Foundation](https://github.com/axiomata-labs/Axiomata-Foundation) repository, also on crates.io. A tree
that has a checkout of the Foundation beside it can patch it to that checkout, so there is only one copy of its
types:

```toml
[patch.crates-io]
axiomata-files = { path = "../Axiomata-Foundation/crates/axiomata-files" }
```

Feature `dev-keys` makes macOS builds read `~/.glut/credentials.json` too. Without it, only the Keychain is used. Leave
the feature off for builds that go to others.

## Linux sandbox: call `serve_helper` first

On Linux, the sandbox starts the current program again as its own helper inside the bubblewrap namespace. A program
that runs shell commands through Glut's sandbox must therefore call `glut::sandbox::serve_helper()` first thing in
`main`, before any thread starts:

```rust,ignore
fn main() -> std::process::ExitCode {
    if let Some(code) = glut::sandbox::serve_helper() {
        return std::process::ExitCode::from(code as u8);
    }
    // ... build the tokio runtime and run
}
```

On other systems the call does nothing. `glut-tui`'s `main.rs` does exactly this.

## The public API

`crates/glut/src/lib.rs` re-exports these items at the crate root:

- From `run`: `run`, `run_continuing` and `resume` (the entry points, see below); `Cancel`, the cancellation
  handle; `RunOutcome`, how a run ended; `Warm` and `Ahead`, the cache and prepared-summary data; `compact_from`, the
  compaction threshold.
- From `config`: `RunConfig`, `ModelEndpoint`, `Effort`, `Budget`, `Prices`, `Compaction`, `RefusalLimits` and
  `Inbox`, which make up a run's settings. Also `PANE_TOOLS` and `LOOP_TOOL`, the names of the screen-only tools
  (`tell`, `end_loop`).
- From `model`: `ChatModel`, `Model`, `OpenAiCompat`, `Message`, `ToolCall`, `ToolSpec`, `Reply`, `Usage` and `Piece`,
  which cover models and the conversation.
- From `tools`: `Tool`, `ToolFuture` and `Toolbox`, the tools and the box that holds them.
- From `permissions`: `Gate`, `Access`, `Mode`, `Rule` and `RuleSet`, the gate every tool call passes.
- From `event`: `Event`, `EventSink`, `SharedSink`, `FinishReason`, `McpStatus`, `NudgeKind` and `SkillHow`, the event
  stream.
- From `diagnostics`: `LanguageServers` and `ServerState`, the language servers whose errors are added after an edit.
- From `error`: `HarnessError`, a failure of the run itself (not of a tool).

Most modules are public as well. The ones an embedder uses most are `setup`, `providers`, `permissions`, `tools`,
`mcp`, `skills`, `agents`, `journal`, `results`, `snapshot`, `sandbox` and `cli`. The modules `lsp`, `refusals` and
`task` are private, and so is `diagnostics` apart from the two re-exported types.

### Entry points

All three entry points take `&RunConfig`, a model `&M` with `M: ChatModel`, a `&Toolbox`, a `&mut dyn EventSink` and a
`&Cancel`:

- `run(config, model, tools, prompt, sink, cancel) -> RunOutcome` runs one prompt in a new conversation.
- `run_continuing(config, model, tools, earlier: Vec<Message>, prompt, sink, cancel) -> (RunOutcome, Vec<Message>)`
  continues a conversation. Pass the `Vec<Message>` it returned last time, or an empty one to start. The system prompt
  is written anew on every call.
- `resume(config, model, tools, run: &str, sink, cancel) -> Result<(RunOutcome, Vec<Message>), String>` continues a run
  the process lost, from its journal in `config.journal`'s folder.

`run` and `run_continuing` never fail as functions. A failing model call ends the run as `FinishReason::Failed`, after
an `Event::Error`. A failing tool's error goes back to the model as text.

`RunOutcome` holds `reason: FinishReason`, `answer: Option<String>` (the model's last text), `turns`, `usage`,
`refusals`, `warm` and `prepared`.

`Cancel` is cheap to clone. `cancel()` stops the run, and `cancel_subagent(id)` stops one subagent by the id of its
`task` call. A program whose main work is the run can call `run::cancel_on_signals(&cancel)` from inside the tokio
runtime, so the run also stops on Ctrl-C, SIGTERM and SIGHUP.

### `RunConfig`

`RunConfig::new(cwd)` starts from the defaults: 100 turns, a 20-minute timeout, subagents on, code mode on, masks on,
the `Default` build mode, and no journal, snapshots or result store on disk. All fields are public. These are the ones
a host most often sets:

- `instructions`: text appended to Glut's own system prompt (a role, a project guide).
- `max_turns`, `timeout`, `max_output_tokens`, `budget` (`Budget { max_tokens, max_usd, prices, paid_usd }`).
- `compaction` (`Compaction { auto, context_window }`) and `refusals` (`RefusalLimits { repeat, total }`).
- `hooks` and `sandbox`: the owner's hooks, and the sandbox settings the hooks run under. Both come from
  `permissions::build_gate`'s `BuiltGate`.
- `snapshots: Option<PathBuf>`, `journal: Option<journal::JournalSpec>`, `results: Option<results::StoreSpec>` and
  `savings: Option<PathBuf>`: where snapshots, the journal, relocated results and lean savings go. `None` disables
  each of them (`results: None` keeps relocated results in memory).
- `subagents`, `agents`, `models`: whether `task` is offered, the named agents (`agents::Catalog`), and what builds an
  agent's own model (`agents::Models`).
- `codemode`, `mcp_behind_code`, `todos`, `diagnostics`, `servers`, `classifier`, `masks`, `inbox`.
- `pane_tools`, `loop_tool`: offer the screen's own tools (`tell`, `end_loop`) that the host has added to the toolbox.

### Models

`ChatModel` is the trait the loop needs:

```rust,ignore
pub trait ChatModel: Send + Sync {
    fn complete(&self, messages: &[Message], tools: &[ToolSpec], max_output_tokens: Option<u32>)
        -> impl Future<Output = Result<Reply, HarnessError>> + Send;
    fn complete_streaming(/* … */, on_text: &mut (dyn FnMut(Piece<'_>) + Send))
        -> impl Future<Output = Result<Reply, HarnessError>> + Send;   // default: calls `complete`
    fn warm(&self, messages: &[Message], tools: &[ToolSpec])
        -> impl Future<Output = Result<Usage, HarnessError>> + Send;   // default: one token of answer
    fn name(&self) -> &str;
    fn context_window(&self) -> impl Future<Output = Option<u32>> + Send;   // default provided
}
```

`ChatModel` uses return-position `impl Future`, so it is not object-safe, and the entry points are generic over it. The
library ships `Model`, an enum that implements it over three clients:

- `Model::new(endpoint: ModelEndpoint, protocol: providers::catalog::Protocol)`. The protocol is `ChatCompletions`
  (OpenAI-compatible: OpenRouter, Ollama, …), `AnthropicMessages` or `Responses`.
- `Model::signed(endpoint, protocol, Some(credential))` for a signed-in subscription (`subscription::Credential`,
  Responses only).
- A `Protocol::Gateway` provider (OpenCode Zen/Go) must first go through
  `providers::route(endpoint, protocol, list) -> Result<(ModelEndpoint, Protocol), String>`. `Model::new` refuses the
  `Gateway` protocol.

`ModelEndpoint` has five public fields: `base_url`, `api_key: Option<String>`, `model`, `effort: Option<Effort>` and
`versioned`. With `versioned: false`, the chat client posts to `{base_url}/v1/chat/completions`. With `true`, it posts
to `{base_url}/chat/completions`, for a root that already includes its version. A key is sent only over HTTPS or to
the local machine.

To use the owner's configured providers rather than an endpoint of your own, use `providers::resolve(providers, store,
id, model, env)`. It reads `~/.glut/providers.json` and finds the key in a `providers::secrets::SecretStore`. A
provider's key is bound to its host, and `resolve` never reads `GLUT_API_KEY`.

### Tools

```rust,ignore
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;                                     // name, description, JSON schema
    fn access(&self, arguments: &Value) -> Access;                  // what a call would touch — required
    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_>;
    fn offered_as(&self) -> Access { /* access(&{}) */ }
    fn offered_now(&self) -> bool { true }
    fn replay_safe(&self, arguments: &Value) -> bool { /* Read or Network access */ }
    fn back_to_top(&self) {}
    fn here(&self) -> Option<std::path::PathBuf> { None }
}
```

- `ToolFuture<'a>` is `Pin<Box<dyn Future<Output = Result<String, String>> + Send + 'a>>`. An `Err` reaches the model
  as `error: <text>`.
- `access` is what the gate judges. Choose the `Access` variant honestly: `Read { path }`, `Edit { path }`,
  `Bash { … }`, `Network { host }`, `Mcp { server, tool }`, `Own`, `Memory`, `GlutFile`, `Agent`, `Pane`, or `Other`
  (known to the rules only by the tool's name).
- `CallContext` carries the run's `stop` flag, the `gate`, and a `report` for images and chain parts.
- `here()` is for a tool that keeps a current folder between calls, as the shell does. It returns the folder the next
  call starts in. When `Toolbox::extend` or `set_gate` finds such a tool, the toolbox tells the gate (crate-internal
  `Gate::set_here`, held weakly). The gate can then expand a command's glob where the command will run, to see
  whether it names a protected file. Most tools keep the default `None`.
- Return the whole text. Do not cut it: the loop bounds a result once, as it enters the conversation
  (`results::bounded`), and keeps the whole in the result store.

`Toolbox::new()` starts empty. `extend` adds tools, `set_gate` puts every call under a gate (without a gate, every
call runs), and `withdraw(name)` removes a tool. `folders()`, `keychain()`, `editor()` and `confinement()` are shared
state that the tools and the gate need to have in common. Take clones of them from the toolbox before building tools
that use them. `setup::toolbox(cwd, catalog, gate, servers, sandbox, extras, sink)` builds the standard set in its one
order: the file tools, `editor`, `web_fetch`, optionally `web_search`, `skill`, `memory` and `glut_file`, then the
gate, the MCP servers and the shell. All of Glut's callers use it so they cannot drift apart.

`task`, `codemode`, `lsp`, `read_full_result`, `todo_write` and `exit_plan` are not `Tool`s. The loop answers them
itself, depending on `RunConfig` and the gate.

### The gate

`Gate::new(rules: RuleSet, mode: Mode, asker: Arc<dyn Asker>)` builds a gate from plain values.
`permissions::build_gate(home, project, GateOptions { mode, allow, ask, deny, trusted }, asker)` builds one from
`~/.glut/settings.json`, from the project's `.glut/` when it is trusted, and from your options. It returns a
`BuiltGate`, which also holds the sandbox settings, hooks, refusal limits, compaction, prices and other settings. Its
`notes()` are the lines to show to the person starting the run.

`Rule::parse` reads Claude Code's rule syntax (`Bash(git status:*)`, `Read(src/**)`, `mcp__server__tool`). Deny beats
ask, and ask beats allow. Where no rule speaks, the `Mode` decides: `Default`, `AcceptEdits`, `Plan`, `DontAsk` or
`Auto`.

What is left to ask goes to an `Asker`:

```rust,ignore
pub trait Asker: Send + Sync {
    fn ask<'a>(&'a self, request: &'a AskRequest) -> AnswerFuture<'a>;     // Allow, AllowAlways, AllowWithin(i), Deny
    fn present(&self) -> bool { true }                                    // false: every ask is refused unasked
    fn fill<'a>(&'a self, asked: &'a mcp::elicit::Asked) -> mcp::elicit::ReplyFuture<'a>;  // default: decline
}
```

`permissions::Nobody` refuses every ask. Glut uses it for headless runs. `permissions::Terminal` asks on
stderr/stdin. Never let a model answer an `AskRequest`. An `MCP` form (`fill`) is the owner's to answer too.

`Gate::set_judge(Some(Arc<dyn Judge>))` adds a second model. It may only declare a sandboxed command a reader
(`permissions::Judge`, see `classifier.rs`).

### Events

```rust,ignore
pub trait EventSink: Send {
    fn emit(&mut self, event: Event);
}
```

`Vec<Event>` implements `EventSink`. `SharedSink::new(Box<dyn EventSink>)` lets a tool that reports from inside a call
(the `skill` tool) share the sink with the loop. `setup::toolbox` takes one.

A sink must handle `Event::Sub`, which wraps everything a subagent reports. Recurse into it, or the subagent's events
are lost.

## Example

This example builds a run on an OpenAI-compatible endpoint, with Glut's file tools and sandboxed shell, plus one
tool of the caller's own, under a gate that asks nobody. Every event is printed as a JSON line. The example passes
`cargo check` against the crate as it stands (2026-11-04), but it is not compiled as a doctest.

```rust,no_run
use std::sync::Arc;

use glut::permissions::Nobody;
use glut::permissions::settings::SandboxSettings;
use glut::providers::catalog::Protocol;
use glut::tools::files::{SkillRoots, file_tools};
use glut::tools::shell::{ShellOptions, shell_tools};
use glut::tools::CallContext;
use glut::{
    Access, Cancel, Event, EventSink, Gate, Mode, Model, ModelEndpoint, Rule, RuleSet, RunConfig, Tool, ToolFuture,
    ToolSpec, Toolbox,
};
use serde_json::{Value, json};

/// A tool of the caller's own: counts the words of a text.
struct WordCount;

impl Tool for WordCount {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "word_count".into(),
            description: "Counts the words of `text`.".into(),
            parameters: json!({
                "type": "object",
                "properties": {"text": {"type": "string"}},
                "required": ["text"]
            }),
        }
    }

    fn access(&self, _arguments: &Value) -> Access {
        // Touches nothing; the rules know it by its name.
        Access::Other
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        Box::pin(async move {
            let text = arguments["text"].as_str().ok_or("missing text")?;
            Ok(text.split_whitespace().count().to_string())
        })
    }
}

/// Writes each event as one JSON line, as `glut run` does.
struct JsonLines;

impl EventSink for JsonLines {
    fn emit(&mut self, event: Event) {
        if let Ok(line) = serde_json::to_string(&event) {
            println!("{line}");
        }
    }
}

fn main() -> Result<std::process::ExitCode, Box<dyn std::error::Error>> {
    // Linux: this process may be the sandbox's helper.
    if let Some(code) = glut::sandbox::serve_helper() {
        return Ok(std::process::ExitCode::from(code as u8));
    }
    tokio::runtime::Runtime::new()?.block_on(run_once())
}

async fn run_once() -> Result<std::process::ExitCode, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;

    let model = Model::new(
        ModelEndpoint {
            base_url: "https://openrouter.ai/api".into(),
            api_key: std::env::var("GLUT_API_KEY").ok(),
            model: "some/model-id".into(),
            effort: None,
            versioned: false,
        },
        Protocol::ChatCompletions,
    )?;

    // The shared state first: the file tools, the shell and the gate must hold the same handles.
    let mut tools = Toolbox::new();
    let beside = tools.folders().clone();
    let editor = tools.editor().clone();
    let keychain = tools.keychain().clone();
    let confinement = tools.confinement().clone();
    tools.extend(file_tools(&cwd, SkillRoots::default(), beside.clone(), editor)?);
    tools.extend([Box::new(WordCount) as Box<dyn Tool>]);

    // Nobody to ask: what the rules and the mode leave open is refused.
    let rules = RuleSet {
        allow: vec![Rule::parse("word_count")?],
        ..RuleSet::default()
    };
    tools.set_gate(Arc::new(Gate::new(rules, Mode::Default, Arc::new(Nobody))));

    tools.extend(shell_tools(
        &cwd,
        ShellOptions {
            sandbox: Some(SandboxSettings::default()),
            beside,
            keychain,
            confinement,
            ..ShellOptions::from_environment()
        },
    )?);

    let config = RunConfig::new(cwd.clone());
    let cancel = Cancel::new();
    glut::run::cancel_on_signals(&cancel);

    let outcome = glut::run(
        &config,
        &model,
        &tools,
        "How many words does README.md have?",
        &mut JsonLines,
        &cancel,
    )
    .await;
    eprintln!("ended: {:?} after {} turns", outcome.reason, outcome.turns);
    Ok(std::process::ExitCode::SUCCESS)
}
```

To continue the conversation, call `run_continuing` with the returned `Vec<Message>`. For the owner's full setup (the
settings files, skills, MCP servers, hooks, web search), follow the `run` path of `glut::cli` in
`crates/glut/src/cli/mod.rs`: `build_gate`, `skills::Catalog::load`, `mcp::config::load` with
`mcp::approval::permitted`, `setup::prepare` and `setup::toolbox`.

## The event stream

`glut run` writes each `Event` to stdout as one JSON line and flushes after each line (`cli::JsonLines`). Notes for
the person go to stderr. The process exits with `0` for `answered`, `1` for `failed` or `refused`, and `2` for
`max_turns`, `timeout`, `cancelled` or `budget`.

Events are serialised with `#[serde(tag = "event", rename_all = "snake_case")]`, so the variant name is the value of
the `event` field:

```json
{"event":"tool_call","turn":1,"id":"c1","name":"read_file","arguments":"{\"path\":\"README.md\"}"}
{"event":"finished","reason":"answered","turns":2,"usage":{"input_tokens":9,"cached_input_tokens":0,"output_tokens":3}}
```

These are the kinds of events, with their fields:

- `started` (`model`, `cwd`, `tools`, `prices?`): the run begins.
- `command` (`name`, `source`, `prompt_chars`): a slash command became the prompt. `prompt_chars` is `0` for a
  built-in.
- `text_delta` (`turn`, `text`): a streamed piece of the answer. The whole text follows as `assistant_text`.
- `reasoning_delta` (`turn`, `text`): a streamed piece of the model's thinking. No whole text follows.
- `assistant_text` (`turn`, `text`): the model's text, either the answer or a remark beside its calls.
- `tool_call` (`turn`, `id`, `name`, `arguments`): a call. The arguments are cut to 2 000 characters, except
  `exit_plan`'s.
- `tool_result` (`turn`, `id`, `name`, `ok`, `output`, `truncated`): a result, masked, with `output` cut to 2 000
  characters for the event.
- `usage` (`turn`, `input_tokens`, `cached_input_tokens`, `output_tokens`): the tokens of one model call.
- `served` (`turn`, `by`): the host behind a router (OpenRouter) changed.
- `permission` (`turn`, `id`, `tool`, `target`, `allowed`, `reason`): how the gate judged a call.
- `mcp_server` (`name`, `status`, `tools`, `reason`): an MCP server came up or failed. `status` is `connected` or
  `failed`.
- `skill` (`name`, `how`): a skill was used. `how` is `run` or `loaded`.
- `hook` (`turn?`, `hook_event`, `command`, `blocked`, `context_chars`, `note`): a hook ran.
- `sub` (`depth`, `task?`, `inner`): one event of a subagent. `inner` is a whole event.
- `error` (`message`): what went wrong. It comes before a `finished` that is not `answered`.
- `todo` (`turn`, `items`): the task list as `todo_write` sent it.
- `snapshot` (`turn`, `run`, `commit`): the folder as it was before a changing step. `glut undo --run … --step …`
  restores it.
- `mode_changed` (`turn`, `mode`): the run left plan mode.
- `lean` (`turn`, `id`, `what`, `chars_before`, `chars_after`): what compact output or relocation saved.
- `masked` (`turn`, `id`, `hidden`): secrets in a result were replaced by placeholders.
- `nudged` (`turn`, `kind`): Glut reminded the model, or asked it to go on. `kind` is one of `reminder`, `open_items`,
  `announced`, `repeated`, `reading`, `no_list` or `pressed`.
- `steered` (`turn`, `text`): a message sent while the run worked joined the conversation (`RunConfig::inbox`).
- `compacted` (`turn`, `before_tokens`, `after_tokens`, `replaced`, `prepared?`): the older part of the conversation
  became a summary.
- `preparing` (`turn`): a summary is being prepared ahead of the threshold.
- `finished` (`reason`, `turns`, `usage`): always the last event. `reason` is one of `answered`, `max_turns`,
  `timeout`, `cancelled`, `failed`, `refused` or `budget`.

Fields marked `?` are left out when empty. A TUI session's event log (`~/.glut/sessions/<id>.events.jsonl`, which
`glut watch` follows) carries these events too, one JSON line each, with the time in `at` and the owner's messages
between them (`crates/glut-tui/src/log.rs`).

## Hosts: passing values in

Glut takes nothing from its host's internals. A host such as Axiomata-OS passes plain values and implements traits:

- **Plain values:** every `RunConfig` field (the folder, instructions, limits, budget and prices, compaction window,
  journal and result-store folders), `ModelEndpoint`, `RuleSet`/`Mode`/`GateOptions`, and `setup::Extras` (`search`,
  `direct` programs, the `memory` folder, `glut_files`). Axiomata passes its catalog's context window and prices this
  way, below the owner's own settings.
- **`Tool`:** the host's own tools (Axiomata's app tools), added with `Toolbox::extend` and judged like any other.
- **`EventSink`:** where the steps go (a log, a database, a UI). It must recurse into `Event::Sub`.
- **`Asker`:** who answers the gate's questions. Use `Nobody` when nobody is there.
- **`Judge`** (`permissions`): an optional second model for reader-only commands.
- **`ChatModel`:** a model client of the host's own, if `Model` does not fit.
- **`agents::ModelSource`** (wrapped as `agents::Models` in `RunConfig::models`): builds a named agent's own model
  from a provider id, a model id and an effort.
- **`tools::search::SearchBudget`:** counts and limits `web_search` calls (`reserve`/`release`). It is passed with
  `tools::search::WebSearch::new(keys, budget)` in `setup::Extras::search`. `Unlimited` allows everything.
- **`editor::EditorFiles`:** an editor's open buffers, set through `Toolbox::editor().set(Some(…))`.
- **`providers::secrets::SecretStore`:** where provider keys and sign-ins are kept.
- **`tools::glut_files::ModelPicker`:** lets the owner pick a model while a maker skill runs.
- **`mcp::client::Transport`:** an MCP transport.

The terminal UI can be embedded too. `glut_tui::run(launch, host, open, linked)` takes the terminal for one session.
It is built as a `glut_tui::Launch` says, and the host takes part through the `glut_tui::Host` trait (`configure`,
`before_turn`, `ask`, `fill`, `event`, `state`, `kept`, …; every method has a default). See
`crates/glut-tui/src/host.rs`.

Public items in `glut` are used by Axiomata-OS. If you change one, run Axiomata's gate as well (see
[`AGENTS.md`](../AGENTS.md)).
