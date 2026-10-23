# AGENTS.md

Guidance for coding agents working in this repository — Claude Code reads it through `CLAUDE.md`. Short and
pointer-heavy: it is loaded on every turn.

## What this is

**Glut**, an agent harness: a lean tool-calling loop over an OpenAI-compatible model API (OpenRouter, Ollama), with
guarded file tools, an OS sandbox for every command (macOS Seatbelt) and a network gate, refusal limits, compaction of
its own, snapshots before every changing step, LSP errors after an edit, code mode (QuickJS), MCP, skills, hooks,
subagents and journals that let a lost run resume.

- `crates/glut` — the harness as a library, with its headless commands (`glut::cli`: `glut run` writes JSON lines on
  stdout, `glut resume|undo|gain|providers|models`).
- `crates/glut-tui` — the program `glut`: the terminal UI, and the headless commands as its subcommands (T43: `axh`
  is gone, and so is `~/.axh` — everything is `~/.glut` / `.glut`).
- Next: the terminal UI, `docs/plans/glut-tui.md` (the master plan, phases TUI0–TUI9).

Glut grew inside Axiomata-OS (`../Axiomata-OS`, its plans `docs/plans/harness*.md`, `glut-*.md`, `durable.md` are the
history of every decision so far) and became an app of its own on 2026-10-21. **Axiomata builds Glut from this folder**
(a path dependency in its root `Cargo.toml`): a change here takes effect there at the next build, so run Axiomata's gate
too when a public item changes.

## Rules

- **Glut depends on nothing of Axiomata's** but `axiomata-files` (`../Axiomata-Foundation`, the guarded file service):
  no `axiomata-core`, no Tauri. What Axiomata adds comes in as plain values or a trait the caller implements.
- **A new Glut setting starts here** — `~/.glut/settings.json` (the owner's only; a project's `.glut/` is read with
  care), a `glut run` flag or frontmatter; Axiomata's settings only pass a value on top.
- Runtime data: `~/.glut/` (settings, mcp.json, runs, snapshots, lean.jsonl).

## Commands

```sh
cargo build                      # the library and glut (the TUI and the headless commands, crates/glut-tui)
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                       # live tests are #[ignore]d (tests/live.rs: GLUT_LIVE_* variables)
cargo run --bin glut -- run --base-url https://openrouter.ai/api --model <id> "prompt"   # key in GLUT_API_KEY
cargo run --bin glut -- providers add openrouter --model <id>   # once: a provider, its key in the Keychain
cargo run --release --bin glut   # the TUI in the current folder, on the default provider
```

## Traps worth knowing before you touch things

- **Glut's code mode has no regular expressions** (`crates/glut/src/codemode.rs` `Intrinsics`): the bundled
  quickjs-ng has an open heap-write advisory in its regex compiler (GHSA-rxpx-wj25-jhqq) — do not add `RegExp` back
  until rquickjs bundles a fixed engine; `cargo audit` cannot see GitHub advisories. A script's tool calls go through
  `run.rs` `perform` like direct ones — never call the toolbox from `codemode.rs`. Language servers asked after an
  edit (`diagnostics.rs` `SAFE_SERVERS`) must never run the project's code: an agent writes the files they read.
- **Glut ends what refusals keep repeating, and compacts itself** (Glut robust): `refusals.rs` counts per tool and
  target *once per call* (the network proxy judges every connection) — keep that when feeding it from a new place;
  `FinishReason::Refused` must still answer every call of the last reply. A compaction keeps the system prompt, the
  latest user message and whole steps (a call never parted from its result); what it replaces goes into the result
  store, never away. Its limits and window are Glut's own (`~/.glut/settings.json` `refusals`, `compact`,
  `contextWindow`); Axiomata only passes its catalog's window below them.
- **Glut never cuts a tool result** (Glut lean): the toolbox hands back the whole text, the loop bounds it once as it
  enters the conversation (`results::bounded`: head + tail, the whole in the run's `ResultStore`) — never cap inside a
  tool. A compact-output filter (`tools/output.rs`) rewrites only the *output* of a simple command, keeps Glut's own
  `[…]` notes and the `error: ` prefix, and puts the raw output in the store; a new filter needs a real output as a
  test fixture (`tools/output/fixtures/`, anonymised) and must leave the model's own format flags alone. A chain
  (`a && b; c`) runs with a random mark printed before each part (`output::marked`/`unmark` in `shell.rs`) — the only
  place Glut touches a command: the gate, the hooks and the events see the command and the output as written, never
  the marks; keep it that way when changing either side.
- **Hooks veto, they never permit** (HS7, Axiomata-OS `docs/plans/harness-hs7.md`): the `hooks` of
  `~/.glut/settings.json` (Claude's shape) may only *block* a call or the answer — `allow`/`ask` parse and
  are no-ops (the gate stays the only decider), a crash/timeout/odd exit is a fail-open `Event::Hook` note,
  and only the owner's file counts: a project's `.glut/` hooks are parsed (`project_hooks_skipped`) and
  printed/folded into `stderr` as a note, never executed.
- **Subagents: offering `task` and judging `task` are two layers** (HS8, Axiomata-OS `docs/plans/harness-hs8.md`): the
  tool is *offered* when the run configures it (`RunConfig.subagents`, default on; in the core set only
  when the skill's `allowed_tools` names `task` — one reading of the frontmatter feeds both the gate and
  the tool list), but whether a call *runs* is the gate's word like any change (`Access::Other` under
  `task`, rules then mode; headless an unanswered ask refuses it). A declaration alone never allows, the
  subagent gets the parent's toolbox minus `task` (one level only), and its events arrive wrapped as
  `Event::Sub { depth, event }` — sinks must recurse (`Notes`, `Readable`) or they drop them.
- **A provider's key is bound to its host and never comes from `GLUT_API_KEY`** (`docs/plans/glut-providers.md`,
  `crates/glut/src/providers/`): providers live in `~/.glut/providers.json` (a file of their own — Glut rewrites it,
  never the owner's `settings.json`), keys in the Keychain or `~/.glut/credentials.json` (0600 from the first byte,
  `write_replacing`) under the account `<id>@<host>`, so a root changed in the file finds no key instead of sending
  the old one elsewhere. `GLUT_API_KEY` belongs to a one-off `--base-url` (and to hosts like Axiomata) only. Every
  provider key variable — the catalog's and the owner's `keyEnv` — is kept out of MCP headers' `${…}`
  (`mcp::client::header_value`); a new catalog entry gets a variable of its own (a coding plan's key is not its
  pay-as-you-go key). `ModelEndpoint.versioned`: a provider's root already holds its version.
- **The TUI never lets a key answer a question it did not show** (`crates/glut-tui/src/app.rs` `on_ask_key`): plain
  keys only, no repeat, nothing in the first 400 ms; a turn that ends drops its open card; an ask shows the head and
  the tail of a long target. Model and tool text goes through `transcript::wrap`/`clean` (control characters as
  spaces, bidi and zero-width characters out) before it reaches the terminal.
- **Anthropic's thinking blocks are signed** (PV3, `crates/glut/src/anthropic.rs`): they travel in
  `Message::reasoning_details` exactly as the API sent them and go back unchanged — a block edited, cut or merged is
  refused. The newest models also bind a block to the history it was made in; Glut's compaction changes that history,
  so every adaptive-thinking request sends `prefix_mismatch_behavior: "drop_block"` (beta
  `thinking-binding-controls-2026-08-01`). Never send `budget_tokens` or `thinking: disabled` to them, nor
  `temperature`.
- **A gateway's model is routed before its client is made** (PV3b, OpenCode Zen/Go, `Protocol::Gateway`): one root,
  each model on the part that speaks its API, told by the model list (models.dev). Every `Model::new` from a resolved
  provider goes through `providers::route` with the list — `Model::new` refuses `Gateway`. Anthropic's betas and
  fallbacks are only sent to Anthropic itself, never through the gateway.
- **What a command *is* decides beside the rules** (TUI1e, `crates/glut/src/permissions/kinds.rs`): a command whose
  every part is a listed reader (`ls`, `grep`, `sed -n 1,9p`, `git status`, …) runs in every mode like `read_file`;
  in `auto` everything sandboxed runs but a `kinds::heavy` one (`rm -rf`, `git push`, `sudo`, …). A new reader needs
  every option that writes or runs something refused (`find -exec`, `rg --pre`, `sort -o`, `git diff --output`).
  "Always" is the owner's pick of a scope (`Answer::AllowWithin`: exact, `prog sub *`, `prog *`), never a widening
  Glut chooses; reading parts ride along without a rule, except under an `allowed-tools` narrowing.
