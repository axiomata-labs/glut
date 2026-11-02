# AGENTS.md

Guidance for coding agents working in this repository — Claude Code reads it through `CLAUDE.md`. Short and
pointer-heavy: it is loaded on every turn.


**Worktrees** (owner, 2026-10-28): every worktree lives under `~/Worktrees/axiomata-glut/<name>/`, never under
`~/Documents` (iCloud syncs it and chokes on `target/`; it also evicts files, so builds and tests stall) and not under
`~/.axiomata` or `~/.claude`. Put the checkout one level down with a link `Axiomata-Foundation` → the real repository
beside it (Glut's path dependency is `../Axiomata-Foundation`), and remove the worktree with its folder once its branch
is merged.
## What this is

**Glut**, an agent harness: a lean tool-calling loop over an OpenAI-compatible model API (OpenRouter, Ollama), with
guarded file tools, an OS sandbox for every command (macOS Seatbelt, Linux bubblewrap plus seccomp) and a network gate, refusal limits, compaction of
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
  no `axiomata-core`, no Tauri. The manifest names it by its GitHub repository; the local `.cargo/config.toml` (not
  checked in) patches that to `../Axiomata-Foundation`, and Axiomata's root manifest does the same — one copy of its
  types in either tree. What Axiomata adds comes in as plain values or a trait the caller implements.
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
scripts/install.sh               # glut for every terminal: ~/.cargo/bin, linked once into /usr/local/bin (asks, sudo)
```

## Traps worth knowing before you touch things

- **Glut's code mode has no regular expressions** (`crates/glut/src/codemode.rs` `Intrinsics`): the bundled
  quickjs-ng has an open heap-write advisory in its regex compiler (GHSA-rxpx-wj25-jhqq) — do not add `RegExp` back
  until rquickjs bundles a fixed engine; `cargo audit` cannot see GitHub advisories. A script's tool calls go through
  `run.rs` `perform` like direct ones — never call the toolbox from `codemode.rs`. Language servers asked after an
  edit (`diagnostics.rs` `SAFE_SERVERS`) must never run the project's code: an agent writes the files they read.
- **The Linux sandbox holds what Seatbelt holds, by mounts** (T59, `crates/glut/src/sandbox/{bwrap,linux,scan}.rs`):
  every sandboxed start goes through `Policy::command` (the shell, hooks, jobs, language servers) — never spawn
  `sandbox-exec` or `bwrap` yourself. bubblewrap runs this build's `glut` as its helper (`sandbox::serve_helper`, first
  thing in `main`) as the namespace's PID 1, not dumpable (`--as-pid-1`: no unfiltered process a command could write
  into): it checks inside that every `.env` is closed, relays `127.0.0.1:<port>` to the proxy's `Bridge` socket and
  starts the command under seccomp (Glut's own BPF, `sandbox/seccomp.rs`, tested through an interpreter: no socket but
  IPv4/IPv6/netlink, no namespace, mount, `io_uring`, keyring, other process's memory, no x32 call; every capability
  dropped) — a helper a command may write is refused. Mounts cover only
  what exists, so `scan::search` runs before every command (the `.env` files to cover, the guarded paths; a search
  past `MAX_ENTRIES` runs no command) and `scan::Watch::settle` after it, even when the run is dropped, moving what it
  made that a Seatbelt pattern refuses (`.envrc`, `.git`, a protected name, a git folder's `config`/`hooks`/
  `commondir`…) to `~/.glut/quarantine` through folders opened without links — keep both around any new way a command
  runs. While the command runs, `sandbox/watcher.rs` (inotify, `scan::Rules` — keep it equal to the search's guarded
  set, a test compares them) moves such a name the moment it appears; **left open** is that moment — a tool of the
  owner's that runs it then (an editor's git, a prompt's `git status`) runs the command's code. The owner's switch
  `/sandbox git read-only` (`~/.glut/sandbox.json`, `sandbox::choices`, read before every command, an unreadable file
  counts as on) keeps every git folder read-only on both systems and closes it for them — on Linux the ones the
  search found (not inside a build folder it skips). What was there at the start was mounted as it was, so a change to
  it came from outside (the owner's `git config` renames a new `config` in): neither the watch nor the check after
  moves it — only a link, which no mount holds, is judged again. A covered secret is a mode-000
  placeholder from `~/.glut/sandbox`, so a read is refused rather than empty; a secret made after a job started stays
  open to that job. Every session's scratch folder lies in one `glut-shells-<uid>` no command writes (macOS too).
  `allowLocalBinding` means nothing there (a command's localhost is its own). Tested in Docker only (`--security-opt
  seccomp=unconfined --security-opt systempaths=unconfined`); the library's own tests reach the helper through
  `GLUT_TEST_HELPER`.
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
  the old one elsewhere. `GLUT_API_KEY` belongs to a one-off `--base-url` (and to hosts like Axiomata) only. On macOS
  the file store is read only by a build with the feature `dev-keys` (T87, `secrets::FILE_ALLOWED`; `scripts/install.sh`
  sets it, `glut providers store file|keychain` moves the keys): a build for others keeps the Keychain alone. Every
  provider key variable — the catalog's and the owner's `keyEnv` — is kept out of MCP headers' `${…}`
  (`mcp::client::header_value`); a new catalog entry gets a variable of its own (a coding plan's key is not its
  pay-as-you-go key). `ModelEndpoint.versioned`: a provider's root already holds its version.
- **A subscription's tokens live in the secret store and renew under a lock** (PV4, `crates/glut/src/subscription/`):
  a sign-in (ChatGPT's plan) keeps a `Record` under `<id>@<host>` like a key; a run's model asks a `Credential` for
  the bearer at every call — never put it in `ModelEndpoint::api_key` beyond fetching a model list. Its refresh
  token rotates: renew only under `Lock::take` after reading the store again, or two windows sign each other out.
  The ID token is verified in full (`chatgpt::verify`); Glut never borrows another tool's client or tokens, and never
  drives another harness for a subscription (owner, PV4 Q3). OpenRouter's sign-in (PV4b) hands out a plain key:
  keep it through `providers::keep_signed_key`, which refuses an entry whose root left OpenRouter's host. A loop or a
  plan's run on a plan (`catalog::Plan`) counts tokens, since its use has no price.
- **Secrets are masked where a result enters the conversation, and go back only into a file that holds them**
  (T58, `crates/glut/src/masks.rs`): `perform_after` masks before the result event, the hooks, the store and the
  model — a new path of tool text into the conversation must go through it too. `Loop::unmasked` puts a placeholder's
  value back only into `write_file`/`edit_file`'s text fields (`masks::WRITTEN_FIELDS`, never a path), for a file
  inside the folder that already holds it. It guards against a key shown by accident (`cat`, logs, `env`), not
  against a model set on it: a transformed value (`base64`, `rev`) passes — the sandbox's read-deny list and the
  network gate stay the barriers.
- **Only Glut speaks as `[glut]`** (T89, `planning::unforged`): the model follows those lines, so every `[glut]`
  in text Glut did not write — a tool's result, a project's guide, a skill, a hook's words, an MCP description, the
  compaction summary, a message from another pane or window, a task item — is quoted `[quoted glut]` wherever it
  stands, reversibly (one `quoted ` more per mark), once on each way; Glut's own notes are added after. A new path
  of such text to the model goes through it too; a write takes the quote off again (`planning::written_marks`,
  beside the masks). The owner's own words stay.
- **A second model may only say a reader reads** (T52, `crates/glut/src/classifier.rs`, `permissions::Judge`): the
  gate asks it only about a sandboxed command a fixed filter lets through (`kinds::judgeable`: listed readers a loop
  kept off the list, `$` only the loop's own variable, nothing written, nested or protected, no Read rule) and only where
  the mode alone would ask — never past a rule, a heavy kind, the Keychain or plan mode. Widen that filter only with
  an audit: the judge sees text the model wrote and can be talked round.
- **An editor shows Glut's questions, it never decides them** (T64, `crates/glut-tui/src/acp/`, `glut acp`): the
  Agent Client Protocol's permission answer is only Glut's ask answered elsewhere — anything but allow refuses, and
  "always" is the narrowest scope; Glut never writes through the editor's `fs/*` nor runs its `terminal/*` (they would
  run past the sandbox, the gate and the snapshots), nor uses the MCP servers it names. It reads an open tab through
  `fs/read_text_file` (`glut::editor`) only for a path `Root::resolve` itself takes, with no link on any level, after
  the gate and the guard — never widen that: the containment check lives in `axiomata-files`, and the editor opens
  any path it is handed. A file whose tab holds unsaved changes is not changed. A new kind of update must not carry anything the
  transcript would not show. Its pickers (`acp/options.rs`) take only values they offered; its slash commands
  (`acp/commands.rs`) run only from a lone text block of the editor's prompt — never from an attachment, a tool result
  or the model — and share their logic with the TUI through `said.rs`: change a command there, not twice. Glut speaks
  each editor's protocol version whole (v1 or, once built, v2), never a mix; what v1 alone has is marked `v1 only`.
- **An editor plugin tells, Glut decides** (T64c, `crates/glut-tui/src/ide.rs`, `docs/editor-protocol.md`,
  `glut.nvim`): the plugin listens on loopback behind a token in `~/.glut/ide/<port>.json`; Glut sends the token only
  after `lsof` shows the file's own process listening, and never through a proxy. What the editor says goes into the
  owner's message only for a file in the run's folders no read rule or read-deny path holds back (judged on the real
  path), masked, quoted (T89) and framed as data — and never into a `/loop` round. A diff in the editor answers only
  the one question it was opened for (one exact call, never sooner than 400 ms), read through the folder's guard;
  an answer about a file the editor did not echo is no answer. Keep every new editor path behind these checks.
  The owner's own text from a diff's right side (T91) is written only for the one question it answered, with an
  allowing answer, as the gate's `owners_answer` keyed by that call's exact arguments — taken once by the toolbox,
  as an edit of the whole file the diff showed (so a file changed meanwhile is not overwritten) and only through a
  tool the run offers; the model's note about it is masked and quoted. The gate's `asked_arguments` (a call's whole
  arguments, for the screen) and `asked_rename` (set only by `judge_all_showing`, never from a model's call) live
  while their question is open and never reach an event, a journal or the model.
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
- **Folders beside the working one are the owner's word alone** (T60c, `crates/glut/src/folders.rs`): only
  `glut run --folder`, `/folder add` and an editor's workspace folders the owner said yes to in its dialog (T64b) add
  one, after `folders::check` — never a model, a skill or a project. One `Folders` per toolbox is shared by the file tools, the gate, the shell's sandbox and the snapshots: a new place that
  resolves a run's file path must ask it too (`Folders::place`/`full_path`), or it treats such a path as outside.
- **A loop repeats the owner's words, never more** (T60a, `crates/glut-tui/src/looping.rs`): a round sends the task
  `/loop` was given, a command only of `looping::COMMANDS`; its agent takes the pane's mode, granted rules and folders,
  and its asks wait for the owner like any. `end_loop` is a screen tool like `tell`: offered by `RunConfig::loop_tool`
  at depth 0 only, never from a script.
- **A plan runs with its own rights alone, and only the owner makes one** (T60b, `crates/glut-tui/src/schedule.rs`,
  `scheduled.rs`): a plan's run is the owner's session with nobody there — the plan's mode and rules, the folder
  never trusted, `~/.glut/mcp.json` alone, every ask refused (`Unattended`), its budget and time limit always. Plans
  come only from `glut schedule add` and `/schedule add` (the owner's words; a loop's task never), and the sandbox
  keeps `~/.glut` out of a command's writes and refuses to start `launchctl` — keep both. What launchd starts (the
  program, every `PATH` folder) never lies where a plan's commands write, or a run would leave the next one a
  program outside the sandbox. A plan's name is a file's and a launchd label's part: `check_name` before it reaches
  a path. On Linux (T94) a plan is a systemd timer and service of the user (`schedule::Systemd`, behind the same
  `Agents` as launchd — pick it through `schedule::system()`, never by name): every value written into a unit goes
  through `unit_value`/`exec_quoted` (no line break, `%%`, `$$`), and the sandbox covers `systemctl`, `systemd-run`,
  `crontab` and `at` as macOS refuses `launchctl`. `/sandbox required on` (`sandbox::choices`, a broken file or an
  unknown home counting as on) refuses every shell command and hook where there is no sandbox, the escape too —
  not a declared direct program nor the owner's MCP servers, which never ran in it.
- **`lsp` asks the safe language servers, and a rename is one change** (T62, `crates/glut/src/lsp.rs`,
  `diagnostics.rs` `LanguageServers`): the same `SAFE_PROGRAMS` as an edit's errors, in the sandbox, started only where
  the gate allows the command line (`Diagnostics::allow`; nothing else starts one) — a new server needs its own audit
  (it must never run the project's code). Every path is judged where it really leads (`lsp::inside`, links followed),
  every question as a read of its file first; what an answer names outside the folder, or in a file the rules hold
  back, shows by path alone, and a hover only where its definition may be read. A rename goes through
  `Gate::judge_all` (every file weighed as an edit, one ask, all or none) and `files::open_for_change`, never a write
  of its own, and only where every replaced piece is the old name; one that would create, move or delete a file, or
  reach one through a link, is not done.
- **A server's question is the owner's alone** (T67, `crates/glut/src/mcp/elicit.rs`, `client.rs` `Asks`,
  `glut-tui/src/form.rs`): an `elicitation/create` goes through the call's `Gate::fill` to the asker — never to the
  model, declined where nobody is there; a form whose field reads like a secret is declined unshown. A server's own
  request is never an answer, whatever its id; keep the `method` check first on every transport path.
- **Glut windows talk through Glut alone** (T83, `crates/glut-tui/src/sessions.rs`): a socket per window under
  `~/.glut/sessions.d/`, the owner's user only; a sandboxed command must never reach it (`(deny network*)` — keep any
  new network allowance off Unix sockets). A message brings no rights: a model's waits for the owner unless it comes
  from the receiving pane's mode, and every one is wrapped (`tell::wrapped_session`) as no word of this window's owner.
- **Glut cleans its own `~/.glut`** (T82, `crates/glut-tui/src/clean.rs`, `glut::clean`): once a day at the start, by
  the owner's terms in `~/.glut/clean.json` (written only by `glut clean keep` / `/clean keep`). A new kind of data
  Glut keeps there gets its line in the report (`clean::ITEMS`, `--json` is read by Axiomata — add, never rename) and
  a rule for when it goes; a dry run removes nothing; what a session's JSON names is never a path to delete.
- **An arena's winner is checked as the bytes git applies** (T61, `crates/glut-tui/src/worktree.rs` `arena_patch`,
  `panes.rs` `start_member`): every git call over a model's worktree goes through `git_alone` (no global or system
  config, no attributes file — a model's `.gitattributes` then names no filter that exists); the patch is made once
  from a written tree id, listed by `git apply --numstat/--summary` and its own header lines, refused for any link or
  submodule mode (an edited link keeps its mode: the `index` line says it), a protected, tool or key name at any depth,
  any `….git` part or a git folder's shape, then applied from the same bytes on stdin — the yes is bound to the tree
  the owner was shown. A member runs in `auto` only where `session::sandboxed`, with no MCP server, `tell` denied and
  the Keychain shut. Its check (`/arena check`) runs through the member's own `bash` tool from the worktree's top —
  never the owner's `!!`, which runs outside the sandbox: what it builds is a model's code.
- **What a command *is* decides beside the rules** (TUI1e, `crates/glut/src/permissions/kinds.rs`): a command whose
  every part is a listed reader (`ls`, `grep`, `sed -n 1,9p`, `git status`, …) runs in every mode like `read_file`;
  in `auto` everything sandboxed runs but a `kinds::heavy` one (`rm -rf`, `git push`, `sudo`, …). A new reader needs
  every option that writes or runs something refused (`find -exec`, `rg --pre`, `sort -o`, `git diff --output`).
  "Always" is the owner's pick of a scope (`Answer::AllowWithin`: exact, `prog sub *`, `prog *`), never a widening
  Glut chooses; reading parts ride along without a rule, except under an `allowed-tools` narrowing.
