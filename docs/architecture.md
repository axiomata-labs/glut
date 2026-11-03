# Architecture

This page describes how Glut is put together: the crates, what happens during one turn, how the terminal UI is
layered on the library, and where Glut keeps its data. It is written for contributors. If you want to embed the
library in another program, read [`library.md`](library.md). The security model is covered in
[`security.md`](security.md). Before you change anything, read the traps in [`AGENTS.md`](../AGENTS.md).

## The workspace

- **`crates/glut`** (library `glut`): the harness. It holds the loop, the model clients, the gate, the toolbox, the
  sandbox, MCP, skills, hooks, journals and snapshots. It also holds the headless commands in `glut::cli`: `run`,
  `resume`, `undo`, `gain`, `providers`, `models`, `mcp`, `keychain` and `sandbox`.
- **`crates/glut-tui`** (binary `glut`, library `glut_tui`): the terminal UI. Every headless command is also a
  subcommand of it (`glut run …` reaches `glut::cli::dispatch`). It adds its own subcommands: `watch`, `export`,
  `search`, `schedule`, `clean` and `acp`.
- **`crates/glut-probe`** (binary `glut-probe`): a standalone tool that measures what a terminal offers a full-screen
  TUI (queries, colours, keys, mouse, paste, clipboard) and writes a Markdown report. It depends only on `crossterm`
  and `rustix`, not on `glut`. Its reports are in `docs/probe/`.

**External dependency.** The only external dependency is `axiomata-files`, from the
[Axiomata-Foundation](https://github.com/NamorX68/Axiomata-Foundation) repository. This is the guarded file service.
Every file tool opens paths through its `Root` (`Root::dir(cwd, LinkPolicy::Contained)` in `tools/files.rs`), never
through a raw path, and containment is checked there. The workspace manifest names the git repository. A local
`.cargo/config.toml`, which is not checked in, patches the dependency to the `../Axiomata-Foundation` checkout next
door.

Glut depends on nothing else of Axiomata's. Axiomata-OS builds Glut as a path dependency. Whatever Axiomata adds comes
in as plain values (fields of `RunConfig`) or as traits the caller implements (see [`library.md`](library.md)).

## One turn

`glut::run`, `run_continuing` and `resume` (`crates/glut/src/run.rs`) all build a `Loop` and drive it. A turn is a
series of steps, and every step reports `Event`s to the caller's `EventSink`:

```text
 prompt / earlier conversation
        │
        ▼
 ┌──────────────────────────────────────────────────────────────────────────────┐
 │ Loop (run.rs)                                                                │
 │  system prompt (prompt.rs) + instructions + conversation                     │
 │  compact_if_full ── compact.rs: older part → summary (whole steps only)      │
 │        │                                                                     │
 │        ▼                                                                     │
 │  ChatModel::complete_streaming ── Model (model.rs)                           │
 │     ├─ OpenAiCompat        chat completions (OpenRouter, Ollama, …)          │
 │     ├─ AnthropicMessages   anthropic.rs (signed thinking blocks)             │
 │     └─ ResponsesApi        responses.rs (ChatGPT plan via subscription/)     │
 │     gateways (OpenCode Zen/Go): providers::route picks one of these first    │
 │        │  Reply { message (text + tool calls), usage, served_by }            │
 │        ▼                                                                     │
 │  for each tool call (at most MAX_CALLS_PER_TURN = 16):                       │
 │    perform_before ── PreToolUse hooks (veto only), snapshot_before           │
 │    execute                                                                   │
 │      ├─ loop-own tools: task (subagent run), codemode (QuickJS script whose  │
 │      │  tools.x() calls come back through perform), lsp, read_full_result,   │
 │      │  todo_write, exit_plan                                                │
 │      └─ Toolbox::call_reporting (tools/mod.rs)                               │
 │           offered? → arguments parsed → Gate::judge_call (permissions/)      │
 │              rules → mode → kinds (reader / heavy) → Judge → Asker           │
 │           → Tool::call                                                       │
 │               file tools (tools/files.rs → axiomata-files Root)              │
 │               bash / bash_output / kill_shell (tools/shell.rs → sandbox/)    │
 │               web_fetch, web_search, editor, skill, memory, glut_file        │
 │               MCP tools (mcp/, behind the gate as Access::Mcp)               │
 │    perform_after                                                             │
 │      diagnostics (LSP errors after an edit) → gate log → Permission events   │
 │      → refusals ledger → masks (masks.rs) → ToolResult event                 │
 │      → PostToolUse hooks                                                     │
 │    answer_with ── compact-output filters (tools/output.rs),                  │
 │      results::bounded → head + tail into the conversation,                   │
 │      the whole text into the ResultStore                                     │
 │      planning::unforged → foreign "[glut]" quoted                            │
 │  journal line per step (journal.rs), Usage / Lean / Nudged / … events        │
 └──────────────────────────────────────────────────────────────────────────────┘
        │
        ▼
 Finished { reason, turns, usage }  →  RunOutcome (+ conversation for the next turn)
```

Below are the parts of the diagram in the order they act.

- **Model clients.** `glut::Model` is an enum over `OpenAiCompat`, `AnthropicMessages` and `ResponsesApi`, chosen
  by `providers::catalog::Protocol`. A `Protocol::Gateway` has no client of its own:
  `providers::route(endpoint, protocol, list)` first turns it into one of the other three, and `Model::new` refuses
  it. `Model::signed` takes a `subscription::Credential` for the ChatGPT plan, which is Responses only. Providers and
  their keys come from `~/.glut/providers.json`, and their secrets from the Keychain or `credentials.json`
  (`providers::secrets`).
- **Gate.** `permissions::Gate` judges every call from the `Access` that the tool's `access()` reports. The order is:
  deny, ask and allow rules (Claude Code's rule syntax, `RuleSet`), then what a command is (`permissions::kinds`: a
  listed reader runs in every mode, a heavy command is asked even in `auto`), then the `Mode` (`default`,
  `accept_edits`, `plan`, `dont_ask`, `auto`). An optional `Judge` may only declare a command a reader. The `Asker`
  answers whatever is still open. `allowed-tools` lists and roles can only narrow the gate. `build_gate` builds a gate
  from the settings files.
- **Toolbox.** `tools::Toolbox` holds the `Tool`s, the MCP `Servers`, and the state that the tools and the gate share:
  `Folders` (folders beside the working one), the Keychain switch, the arena `Confinement` and the `Editor`. A tool
  that keeps a current folder (the shell) reports it through `Tool::here()`. `set_gate` and `extend` pass it to the
  gate (`Gate::set_here`, held weakly), so that a command's glob is judged where it runs. `setup::toolbox` puts the
  standard set together in its one order.
- **Shell and sandbox.** `tools/shell.rs` runs every command through `sandbox::Policy::command`: Seatbelt on macOS,
  bubblewrap plus Glut's own seccomp filter on Linux (`sandbox/{bwrap,linux,seccomp,scan,watcher}.rs`). Network
  access goes through the run's proxy (`sandbox/proxy.rs`), which asks the gate per host (`Access::Network`). A chain
  (`a && b`) is split with random marks (`output::marked`/`unmark`) so that the output of each part can be filtered.
- **Code mode.** `codemode.rs` runs a model-written JavaScript in QuickJS, without regular expressions. Every
  `tools.<name>()` call goes back to the loop and takes the same path as a direct call.
- **Subagents.** A `task` call starts a nested run on the parent's toolbox without `task` (one level only). Its events
  arrive wrapped as `Event::Sub { depth, task, event }`. Named agents come from `agents::Catalog` (`AGENT.md` files),
  and their own models from `RunConfig::models` (`agents::ModelSource`).
- **Results.** `results::bounded` cuts a result to `MAX_TOOL_OUTPUT_CHARS` (30 000) once, as it enters the
  conversation. The whole result stays in the run's `ResultStore`, where `read_full_result` can read it. Tools never
  cut their own output.
- **Masks.** `masks::Masks` replaces secrets in every tool result with numbered placeholders before any event, hook,
  store or model sees them. `Loop::unmasked` puts a value back only into `write_file`/`edit_file` text fields, and only
  for a file that already holds it.
- **Snapshots.** `snapshot.rs` records the work folder (and any folders beside it) in a shadow git store under
  `~/.glut/snapshots` before every step that may change files. `glut undo` restores from it.
- **Journals.** `journal.rs` writes one JSON line per step to `~/.glut/runs/<run>.jsonl`, so `resume` can continue a
  run the process lost. A finished run deletes its journal.
- **Compaction.** `compact.rs` replaces the older part of a long conversation with a summary. It keeps the system
  prompt, the latest user message and whole steps. It can run when the conversation nears the context window
  (`Compaction`, `compact_from`) or after a provider reports overflow. A summary may also be prepared ahead of time
  (`RunOutcome::prepared`).
- **Refusals.** `refusals.rs` counts refused calls per tool and target, once per call. The run ends as
  `FinishReason::Refused` once `RefusalLimits` (`repeat`, `total`) are passed.
- **Hooks.** `hooks.rs` runs the owner's `SessionStart`, `PreToolUse`, `PostToolUse` and `Stop` hooks from
  `~/.glut/settings.json`. Hooks can only block.

## The terminal UI

The program `glut` (`crates/glut-tui`) is a screen over the library. Its layers, from the outside in:

- **Entry** (`main.rs`, `lib.rs`): argument parsing and subcommands. `sandbox::serve_helper()` runs first thing in
  `main`. `glut_tui::run` and `standalone` take the terminal.
- **Screen** (`screen.rs`): takes the terminal, runs the event loop until the owner quits, and gives the terminal
  back.
- **Host** (`host.rs`): `Launch` says how a session is built. The `Host` trait is how an embedding program, such as
  Axiomata's Studio, takes part: settings, budget, asks, events and the conversation. `Standalone` is the plain host.
- **Panes** (`panes.rs`): `Panes` holds several sessions side by side or as tabs, at most `panes::MOST` (seven).
  Keys go to the active pane. `worktree.rs` gives a further pane a worktree of its own.
- **App** (`app.rs`): `App` is one pane's session on screen. It handles keys, commands, the ask card
  (`on_ask_key`) and the run's reports.
- **Session** (`session.rs`): `session::build` builds once what `glut run` builds per run (gate, tools, settings)
  into `Parts`. `Engine` is the model, which `/model`, `/provider` and `/effort` replace between turns.
- **View** (`view.rs`, `transcript.rs`, `markdown.rs`, `highlight.rs`, `live.rs`, `tiles.rs`, `glimm/`): drawing.
  `transcript::wrap`/`clean` strip control, bidi and zero-width characters from model and tool text before it
  reaches the terminal.
- **Commands** (`commands.rs`, `said.rs`): slash commands and keys by topic. `said.rs` holds the logic the TUI shares
  with the editor protocol.
- **Kept sessions** (`store.rs`, `layout.rs`): `~/.glut/sessions/<id>.json` for `glut -c`, `glut -r` and
  `/resume`, plus the pane layout kept for the next start.
- **Windows** (`sessions.rs`, `tell.rs`): one owner-only Unix socket per window under `~/.glut/sessions.d/`. `/send`
  and the `tell` tool use these sockets.
- **Loops** (`looping.rs`): `/loop` repeats a task in a background agent, whose model is offered `end_loop`.
- **Plans** (`schedule.rs`, `scheduled.rs`, `runs.rs`): timed runs while Glut is closed (a LaunchAgent on macOS, a
  systemd user timer on Linux) and the Runs tab.
- **Arena** (`arena.rs`, `bench.rs`, `worktree.rs`): one task on two to four models, each in a full copy of the folder
  under a confinement. The winner's patch is checked, then applied. `bench.rs` runs fixed tasks from
  `~/.glut/arena/tasks`.
- **Editors** (`acp/`): `glut acp` speaks the Agent Client Protocol (v1) as JSON-RPC over stdin and stdout, for Zed,
  JetBrains, Neovim (CodeCompanion) and Emacs.
- **Editor plugin** (`ide.rs`): Glut's own editor protocol ([`editor-protocol.md`](editor-protocol.md),
  `glut.nvim`). The plugin listens on loopback with a token in `~/.glut/ide/<port>.json`, and Glut connects to it as
  an MCP server named `ide`.
- **Housekeeping** (`clean.rs`, `export.rs`, `finder.rs`, `follow.rs`): `glut clean`, `glut export`, `glut search`
  and `glut watch`.

## Where data lives

The owner's data lives in `~/.glut/` (`providers::DIR`). A project may have its own `.glut/` folder. It is read with
care: its allow rules, mode, skills, agents and servers count only in a folder the owner trusted, and its hooks never
run.

- `settings.json`: the owner's rules, mode, sandbox, hooks, refusal limits, compaction, prices and more
  (`permissions::settings`).
- `providers.json`, `credentials.json`: the providers (rewritten by Glut) and file-stored keys (mode 0600). Otherwise
  keys are kept in the Keychain.
- `models/`: the model lists kept per provider.
- `mcp.json`, `mcp-approved.json`: the owner's MCP servers, and the project servers the owner approved.
- `trusted.json`: the folders whose project `.glut/` counts.
- `sandbox.json`, `sandbox/`, `quarantine/`: the sandbox switches, the mode-000 placeholders (Linux), and the files the
  Linux watch moved away.
- `secrets.json`, `classifier.json`, `clean.json`: mask settings, the judge model, and the cleaning terms.
- `skills/`, `agents/`, `commands/`, `AGENTS.md`: the owner's skills, named agents, slash commands and global guide.
- `memory/<project>/`: a project's memory (in `~/.claude/projects/` instead when Claude Code already keeps one).
- `runs/`: the journals of unfinished runs.
- `snapshots/`: the shadow git stores for `glut undo`.
- `lean.jsonl`: the savings counted for `glut gain`.
- `sessions/`, `results/`, `images/`, `exports/`: the kept TUI sessions, their relocated long results, images and
  exports.
- `sessions.d/`: the window sockets.
- `schedules.json`, `schedules/`: the plans and their run logs.
- `arena/`: the bench tasks, the checks, and the arena copies with their stores.
- `ide/`: the editor plugins' port files.

`glut clean` (`clean::ITEMS`) reports on and prunes this folder by the owner's terms.

## Where to start reading

- **The loop:** `run.rs`. Start with `run`, `run_continuing` and `Loop::drive`, then `run_step`, `perform` and
  `execute`.
- **Settings and inputs:** `config.rs` (`RunConfig`, `ModelEndpoint`, `Budget`, `Compaction`) and `setup.rs`
  (`toolbox`, `prepare`).
- **Models:** `model.rs` (`ChatModel`, `Model`, `OpenAiCompat`), `anthropic.rs`, `responses.rs`, and `providers/`
  (`resolve`, `route`, `catalog`).
- **Permissions:** `permissions/mod.rs` (`Access`, `Gate::judge_call`, `Mode`, `Asker`), `permissions/kinds.rs` and
  `permissions/settings.rs`.
- **Tools:** `tools/mod.rs` (`Tool`, `Toolbox`), `tools/files.rs`, `tools/shell.rs`, `tools/output.rs`, `codemode.rs`,
  `lsp.rs` and `task.rs`.
- **Sandbox:** `sandbox/mod.rs` (`Policy`, `serve_helper`), then `bwrap.rs`, `linux.rs`, `seccomp.rs`, `scan.rs`,
  `watcher.rs` and `proxy.rs`.
- **MCP:** `mcp/mod.rs`, `mcp/client.rs`, `mcp/config.rs`, `mcp/approval.rs` and `mcp/elicit.rs`.
- **Events:** `event.rs`, and `cli/mod.rs` (`JsonLines`, `dispatch`) for how `glut run` writes them.
- **TUI:** `glut-tui/src/screen.rs` → `panes.rs` → `app.rs` → `session.rs`, and `host.rs` for embedding.

## Key invariants

Each line points to a trap in [`AGENTS.md`](../AGENTS.md). The reasoning behind them is in [`security.md`](security.md).

- Code mode has no `RegExp`, and a script's tool calls go through `run.rs` `perform`, never straight to the toolbox.
- Every sandboxed start goes through `sandbox::Policy::command`. Never spawn `sandbox-exec` or `bwrap` yourself.
- Language servers asked after an edit (`diagnostics.rs` `SAFE_PROGRAMS`) must never run the project's code.
- Refusals are counted once per call. `FinishReason::Refused` still answers every call of the last reply.
- Compaction keeps whole steps. What it replaces goes into the result store.
- No tool cuts its own result. `results::bounded` bounds it once, as it enters the conversation.
- Hooks only veto. `allow`/`ask` from a hook are no-ops, and project hooks never run.
- Offering `task` and judging it are separate layers. Subagents get the toolbox without `task`, and sinks must
  recurse into `Event::Sub`.
- A provider key is bound to its host (`<id>@<host>`) and never comes from `GLUT_API_KEY`.
- Subscription tokens renew only under `Lock::take`, after re-reading the store.
- Secrets are masked where a result enters the conversation (`perform_after`). A new path of tool text into the
  conversation must mask too.
- Only Glut speaks as `[glut]`: foreign text goes through `planning::unforged`.
- The second model (`Judge`) may only declare a command a reader.
- An editor (ACP or plugin) shows Glut's questions but never decides them. Glut never writes through the editor.
- The TUI never lets a key answer a question it did not show (`app.rs` `on_ask_key`).
- Anthropic thinking blocks travel unchanged in `Message::reasoning_details`.
- A gateway's model is routed (`providers::route`) before its client is made.
- Folders beside the working one are the owner's word alone. Path resolution goes through `Folders`.
- A loop repeats only the owner's words. A plan runs with its own rights alone, with nobody to ask.
- `lsp` renames go through `Gate::judge_all` as one change.
- An MCP server's question (elicitation) goes to the owner, never to the model.
- Window-to-window messages bring no rights, and sandboxed commands never reach the sockets.
- `clean::ITEMS` names are read by Axiomata. Add names, never rename them.
- An arena's winner is checked as the exact bytes git applies, and all git over a model's copy goes through
  `git_alone`.
- A command's kind (`permissions::kinds`) decides beside the rules. A new reader must refuse every option that writes
  or runs.
