# Glut and other coding agents

How Glut compares with nine other terminal coding agents, **as of 2026-11-05**. Every statement about another tool
comes from its own documentation, repository or release notes, linked where it is made; "not documented" means we
looked there and did not find it, not that it cannot be done. These tools change every week — if something here is
wrong or out of date, please [open an issue](https://github.com/axiomata-labs/glut/issues) and we correct it.

The versions looked at: Claude Code 2.1.318, Codex CLI 0.166.0, Gemini CLI 0.67.0, opencode 2.2.5 (and 1.19.5),
Aider 0.86 (last tagged release; its main branch has moved on since), Cursor CLI (the `agent` command, changelog of
2026-10), Goose 1.57.0, Crush 0.99.0, pi 1.5.0.

## At a glance

| | Glut | Claude Code | Codex CLI | Gemini CLI | opencode | Aider | Cursor CLI | Goose | Crush | pi |
|---|---|---|---|---|---|---|---|---|---|---|
| Licence | MIT / Apache-2.0 | proprietary | Apache-2.0 | Apache-2.0 | MIT | Apache-2.0 | proprietary | Apache-2.0 | FSL-1.1-MIT ¹ | MIT |
| Written in | Rust, one binary | native binary | Rust | TypeScript (Node) | TypeScript | Python | not stated | Rust | Go, one binary | TypeScript (Node) |
| Models | any vendor, local | Claude only | OpenAI, local | Gemini only | any vendor, local | any vendor, local | Cursor's catalogue | any vendor, local | any vendor, local | any vendor, local |
| OS sandbox for commands | on by default | opt-in | on by default | opt-in | none | none | on by default ² | none | none | none ⁸ |
| Network of commands | proxy asks per host | proxy, allow-list | off; optional proxy | partly | none | none | blocked; allow-list | none | name list only | none ⁸ |
| Files put back | before every change ³ | per prompt ⁴ | git advised | opt-in ⁴ | per step ⁵ | a git commit per edit | `/rewind` | not documented | not documented | an example extension |
| Tool output | never cut ⁶ | cut, rest in a file | token budget | cut at 40k chars | cut, rest in a file | asks before adding | not documented | cut, rest in a file | cut, rest in a file | cut, rest in a file |
| Several agents | panes, background agents, subagents — each its own model and provider | subagents; teams (experimental) | subagents | subagents | subagents | one | subagents | subagents | task agents | one (tmux; an example extension) |
| Models side by side | arena, test bench | — | — | — | — | — | best-of-n (IDE) | — | — | — |
| Recurring work, local | `/loop`, plans (launchd, systemd) | `/loop`, desktop tasks | — (app only) | — | schedules (experimental) | — | — (cloud) | cron recipes | — | — |
| Editors | Neovim, VS Code, JetBrains, ACP | VS Code, JetBrains | VS Code, JetBrains, Xcode | VS Code, ACP | VS Code, ACP | file watching | ACP | ACP, VS Code (exp.) | — | — |
| Language servers | errors after an edit, `lsp` tool | plugins | not documented | not documented | v1 only, opt-in | lint and tests | not documented | not documented | yes | not documented |
| Secrets in output masked | yes | not documented | variables kept out ⁷ | variables (opt-in) | not documented | not documented | not documented | not documented | not documented | not documented |
| Interface languages | 8 | English | English | English | English (app: many) | English | English | English (app: 16) | English | English |

¹ Source-available, not an OSI licence; it turns into MIT later. ² In its recommended Auto-review mode. ³ Glut keeps
the files before every step that may change them, a command's too. ⁴ Edits of the agent's own edit tools only, not
changes made by commands. ⁵ Needs a git repository; a command's side effects are not put back. ⁶ Glut bounds a result
as it enters the conversation and keeps the whole in a store the model pages through. ⁷ Variables named like keys
are kept out of commands' environment. ⁸ pi leaves the boundary to a container and documents recipes for one.

## While you work

What the terminal shows and lets you do around the conversation. "—" means the tool's documentation and source say
nothing of it; [Around the conversation](#around-the-conversation) names the sources.

| | Glut | Claude Code | Codex CLI | Gemini CLI | opencode | Aider | Cursor CLI | Goose | Crush | pi |
|---|---|---|---|---|---|---|---|---|---|---|
| Side panel | tabs: plan, gate, meter, agents, to-dos, runs, keys | task list, agent view | status line, `/agents` | to-dos above the prompt | sidebar: cost, to-dos, files | — | jobs pager | — | sidebar: cost, agents, files | extension widgets |
| Take back an "always" | `/gate` | `/permissions` | — | — | — | — | — | — | — | — (no permissions) |
| To-do list | the model's task list; the project's `TODO.md`, shared with you | task list | plan updates | per session | experimental | — | yes | extension, off | yes | example extension |
| Plan mode | the plan's task list ticked off as it is built; `/send plan` hands it on | plan files | yes | plan file | plan agent | architect mode | yes | — | yes | example extension |
| Side question outside the conversation | `/btw`, to any pane or agent too | `/btw` | `/side` | — | `/btw` | — | `/btw` | — | — | — |
| Agents talk to each other | panes, background agents, other Glut windows — across providers, lending no right | agent teams (Claude only) | parent → subagent | remote agents (A2A) | parent → child | — | parent → subagent | parent → subagent | parent → subagent | — |
| Model **and** provider per agent | yes: each pane, background agent and subagent | model only | model only | model only | yes | main and editor model | model only | yes | yes | model (example) |
| Background agents you address | `/bg`, `/send`, `/peek`, `/show` | agent view, `/bg` | `/agents`, `codex queue` | — | background subagents | — | background subagents | — | the model's tools only | tmux |
| Panes inside the app | up to seven sessions in one window | tmux or iTerm2 | — | — | session tabs | — | — | — | — | — |
| Cost of each turn; budgets | every turn and tool; budgets for runs, loops, plans | session cost; budget with `-p` | tokens | tokens; turn limit | session cost; monthly (Zen) | each reply | plan spend | estimate; turn limit | session cost | session cost |
| Images | pasted; drawn in the terminal | pasted | pasted; previews | pasted | pasted | pasted | pasted; previews | — | pasted; picker preview | pasted; drawn inline |
| Rules for this session | `/rule` | — | — | — | — | `/read` a conventions file | — | a file read every turn | — | — |
| Hand over to a later session | `/handoff` | — | — | — | — | — | — | — | — | example extension |
| A lost run resumed | from its journal, `glut resume` | saved sessions | saved sessions | saved sessions | saved sessions | chat history | saved sessions | saved sessions | saved sessions | session file; `pi-durable` (exp.) |

## Where others are ahead

Glut is young (0.1.0) and has one maintainer. Things the others do that Glut does not:

- **Windows.** Codex CLI has a native Windows sandbox, Gemini CLI, Crush and pi run on Windows, Claude Code and Cursor
  CLI install there. Glut runs on macOS and Linux, on Windows only in WSL2 (the Linux build and its sandbox).
  ([Codex](https://learn.chatgpt.com/docs/windows/windows-sandbox.md),
  [Crush](https://github.com/charmbracelet/crush/blob/main/README.md),
  [pi](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/windows.md))
- **Rewinding the conversation with the files.** Claude Code, Gemini CLI, opencode and Cursor CLI take the
  conversation back to a point together with the files; pi keeps the conversation as a tree you branch from any point.
  Glut's `/steps` puts files back; the conversation goes on.
  ([Claude Code](https://code.claude.com/docs/en/checkpointing), [opencode](https://opencode.ai/v2/docs/snapshots/),
  [pi](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sessions.md))
- **Hooks that allow or rewrite.** Claude Code, Codex CLI, Cursor CLI and Crush let a hook approve a call or change its
  input. Glut's hooks may only veto — on purpose (the gate stays the one place that allows), but less flexible.
  ([Claude Code](https://code.claude.com/docs/en/hooks), [Crush](https://github.com/charmbracelet/crush/blob/main/docs/hooks/README.md))
- **Cloud and desktop.** Claude Code, Codex, Cursor and opencode run tasks in the cloud or in a desktop app, and
  scheduled runs that need no machine of yours. Glut is a terminal program; its plans run on your machine.
  ([Claude Code](https://code.claude.com/docs/en/scheduled-tasks), [Cursor](https://cursor.com/docs/cli/overview))
- **Policies for organisations.** Gemini CLI's policy engine has admin tiers; Claude Code and Cursor have managed
  settings and enterprise allow-lists. Glut has the owner's settings and a project's, nothing an administrator
  enforces. ([Gemini CLI](https://geminicli.com/docs/reference/policy-engine/),
  [Cursor](https://cursor.com/docs/agent/security/run-modes))
- **Ecosystem.** Plugins, marketplaces and many hook events in Claude Code, Cursor, opencode and Goose; pi's
  TypeScript extensions, deep enough to add plan mode or subagents; Aider's repo map and leaderboards; Crush's and
  opencode's breadth of install channels.
- **A Claude plan.** Claude Code uses a Claude Pro or Max plan; pi offers the sign-in too and warns that it is billed
  as extra usage. Anthropic does not permit third-party tools to offer a Claude.ai login, so Glut uses Anthropic's API
  with a key (and a ChatGPT plan by sign-in).
  ([Anthropic](https://code.claude.com/docs/en/legal-and-compliance))

## The details

### Licence and how it installs

- **Glut**: MIT or Apache-2.0. One Rust program, under 50 MB with everything inside; no Node, no Python. Install
  script, Homebrew, `cargo install`. ([install](install.md))
- **Claude Code**: proprietary ("All rights reserved"); the public repository holds plugins and documentation. A
  native binary; npm only as a way to fetch it.
  ([licence](https://raw.githubusercontent.com/anthropics/claude-code/main/LICENSE.md),
  [setup](https://code.claude.com/docs/en/setup))
- **Codex CLI**: Apache-2.0, Rust. Release archives of about 103–108 MiB, compressed.
  ([repository](https://github.com/openai/codex), [releases](https://api.github.com/repos/openai/codex/releases/latest))
- **Gemini CLI**: Apache-2.0, TypeScript on Node 20+ (99 MB unpacked from npm); standalone archives of about 37 MB.
  ([repository](https://github.com/google-gemini/gemini-cli),
  [npm](https://registry.npmjs.org/@google/gemini-cli/latest))
- **opencode**: MIT, TypeScript, shipped as a native binary per platform (106 MB for macOS arm64), a desktop app too.
  ([repository](https://github.com/anomalyco/opencode), [docs](https://opencode.ai/v2/docs/))
- **Aider**: Apache-2.0, a Python package (Python 3.10–3.12). ([install](https://aider.chat/docs/install.html))
- **Cursor CLI**: closed source, installed by a script; language and size not stated.
  ([installation](https://cursor.com/docs/cli/installation))
- **Goose**: Apache-2.0, Rust; a CLI, a desktop app and an API, part of the Agentic AI Foundation.
  ([repository](https://github.com/aaif-goose/goose))
- **Crush**: FSL-1.1-MIT (source-available), Go, one binary for many systems.
  ([licence](https://github.com/charmbracelet/crush/blob/main/LICENSE.md))
- **pi**: MIT, TypeScript on Node 22.19+, from Earendil (formerly `badlogic/pi-mono`); an install script, npm, Nix.
  ([repository](https://github.com/earendil-works/pi), [site](https://pi.dev))

### Models

- **Glut**: two dozen providers in its catalogue and any OpenAI-compatible, Anthropic or Responses API; local models
  through Ollama or LM Studio; a ChatGPT plan by sign-in; prices, budgets and the cost of every turn.
  ([configuration](configuration.md))
- **Claude Code**: Claude models only, through Anthropic, Bedrock, Foundry or Vertex; a Claude Pro, Max, Team or
  Enterprise plan. ([models](https://code.claude.com/docs/en/model-config),
  [authentication](https://code.claude.com/docs/en/authentication))
- **Codex CLI**: OpenAI's models; other providers if they speak the Responses API; local models with `--oss`
  (Ollama, LM Studio); a ChatGPT plan. ([configuration](https://learn.chatgpt.com/docs/config-file/config-reference.md))
- **Gemini CLI**: Gemini only, by API key, Vertex or a Code Assist licence; the free tier and Google AI Pro/Ultra moved
  to a separate product on 2026-06-18.
  ([announcement](https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/))
- **opencode**: 75+ providers, local models, sign-in for ChatGPT, Copilot, GitLab Duo and SuperGrok.
  ([providers](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/web/src/content/docs/providers.mdx))
- **Aider**: many providers and any OpenAI-compatible API, local models; GitHub Copilot by a token copied by hand.
  ([models](https://aider.chat/docs/llms.html))
- **Cursor CLI**: the models of a Cursor plan; local models not documented.
  ([models and pricing](https://cursor.com/docs/models-and-pricing))
- **Goose**: about fifty providers, many local runners, ChatGPT and Copilot sign-in.
  ([providers](https://github.com/aaif-goose/goose/blob/main/documentation/docs/getting-started/providers.md))
- **Crush**: about twenty providers plus its own, local models found automatically, sign-in for Copilot, ChatGPT and
  Grok. ([login](https://github.com/charmbracelet/crush/blob/main/internal/cmd/login.go))
- **pi**: about 35 providers, local models through Ollama, LM Studio, vLLM and llama.cpp; sign-in for ChatGPT, Copilot
  and others, Claude Pro/Max too (billed as extra usage, it warns). ([providers](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/providers.md),
  [models](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/models.md))

### The commands the agent runs

- **Glut**: every command in the system's sandbox, on by default — macOS Seatbelt, Linux bubblewrap with Glut's own
  seccomp filter. Commands write the project and the usual caches; `.env` files cannot be read; git hooks and
  similar files stay untouched at any depth; the network goes through a proxy that asks before each new host. Where no
  sandbox can run, every command asks first. ([security model](security.md))
- **Claude Code**: Seatbelt or bubblewrap, **opt-in**; shell commands only; without a sandbox it runs commands
  unsandboxed unless told to fail; with it, a proxy checks each host against an allow-list.
  ([sandboxing](https://code.claude.com/docs/en/sandboxing))
- **Codex CLI**: on by default (workspace writes only), Seatbelt, bubblewrap and a native Windows sandbox; network off
  by default, an optional proxy with domain rules. ([sandboxing](https://learn.chatgpt.com/docs/sandboxing.md),
  [approvals](https://learn.chatgpt.com/docs/agent-approvals-security.md))
- **Gemini CLI**: **opt-in**; Seatbelt (its default profile allows the network), containers, gVisor, Windows.
  ([sandbox](https://geminicli.com/docs/cli/sandbox/))
- **opencode**: no sandbox — commands run "with the host user's filesystem, process, and network authority".
  ([permissions](https://opencode.ai/v2/docs/permissions/))
- **Aider**: no sandbox documented. ([commands](https://aider.chat/docs/usage/commands.html))
- **Cursor CLI**: Seatbelt, or Landlock and seccomp on Linux, on in its recommended mode; network blocked except an
  allow-list. Its own documentation calls the run modes "best-effort guardrails".
  ([run modes](https://cursor.com/docs/agent/security/run-modes))
- **Goose**: no sandbox ("permission callbacks are not a sandbox"); optional prompt-injection detection.
  ([ACP](https://github.com/aaif-goose/goose/blob/main/documentation/docs/gdk/acp/index.md))
- **Crush**: no sandbox; commands it can prove read-only run, others ask, a list of dangerous commands is refused.
  ([bash tool](https://github.com/charmbracelet/crush/blob/main/internal/agent/tools/bash.go))
- **pi**: no sandbox and, on purpose, no approvals; its documentation recommends a container and gives recipes.
  ([security](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/security.md), [containers](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/containerization.md))

### Asking and allowing

- **Glut**: modes plan, ask, build and auto; reading is free, a command that only reads runs at once, `rm -rf`,
  `git push` and `sudo` always ask; "always" means the scope you pick (this command, `git status *`, `git *`).
  ([security model](security.md))
- **Claude Code**: Manual, acceptEdits, Plan, Auto (a classifier; the default since 2.1.283), dontAsk,
  bypassPermissions; "don't ask again" saves a prefix rule. ([permissions](https://code.claude.com/docs/en/permissions))
- **Codex CLI**: sandbox modes times approval policies, prefix rules with allow, prompt and forbidden, an optional
  model reviewer. ([rules](https://learn.chatgpt.com/docs/agent-configuration/rules.md))
- **Gemini CLI**: default, autoEdit, plan, yolo; a policy engine with priority tiers and prefix or regex rules.
  ([policy engine](https://geminicli.com/docs/reference/policy-engine/))
- **opencode**: allow, ask and deny rules with wildcards, every part of a compound command checked; most actions are
  allowed by default. ([permissions](https://opencode.ai/v2/docs/permissions/))
- **Aider**: yes/no questions, or `--yes-always`. ([options](https://aider.chat/docs/config/options.html))
- **Cursor CLI**: Auto-review (a classifier model), Allowlist, Run Everything; rules per tool.
  ([permissions](https://cursor.com/docs/cli/reference/permissions))
- **Goose**: Autonomous (the default, no approvals), Manual, Smart, Chat only; per tool, not per command.
  ([permissions](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/managing-tools/goose-permissions.md))
- **Crush**: asks by default; a session's "allow" for a command holds for that exact command.
  ([permission](https://github.com/charmbracelet/crush/blob/main/internal/permission/permission.go))
- **pi**: none built in ("No permission popups"); example extensions add a permission gate and protected paths.
  ([site](https://pi.dev), [examples](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/README.md))

### Putting files back, and long work

- **Glut**: a snapshot before every step that may change files, a command's included; `/steps` and `glut undo` put
  them back. Compaction ahead of time keeps whole steps; no tool result is ever cut — the whole stays in a store the
  model pages through; journals let a lost run resume. ([architecture](architecture.md))
- **Claude Code**: a checkpoint before every prompt, `/rewind` for files, conversation or both — the edit tools' changes
  only; Bash output beyond about 30,000 characters goes to a file.
  ([checkpointing](https://code.claude.com/docs/en/checkpointing),
  [tools](https://code.claude.com/docs/en/tools-reference))
- **Codex CLI**: no undo documented; it advises git. ([CLI](https://learn.chatgpt.com/docs/codex/cli.md))
- **Gemini CLI**: opt-in checkpoints in a shadow git repository and `/rewind` (edit tools only); tool output cut at
  40,000 characters. ([checkpointing](https://geminicli.com/docs/cli/checkpointing/),
  [settings](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/cli/settings.md))
- **opencode**: snapshots on by default, `/undo` and `/redo`; output cut at 2,000 lines or 50 KiB, the rest kept in a
  file for seven days. ([snapshots](https://opencode.ai/v2/docs/snapshots/),
  [compaction](https://opencode.ai/v2/docs/compaction/))
- **Aider**: a git commit for every edit, `/undo` takes back the last. ([git](https://aider.chat/docs/git.html))
- **Cursor CLI**: `/rewind` of files and conversation per turn. ([changelog](https://cursor.com/docs/cli/changelog))
- **Goose**: no undo documented; compaction on by default, shell output cut at 2,000 lines or 50 KB.
  ([context](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/sessions/smart-context-management.md))
- **Crush**: no undo documented; output beyond 30,000 characters cut to its head and tail, the whole in a file.
  ([agent](https://github.com/charmbracelet/crush/blob/main/internal/agent/agent.go))
- **pi**: the conversation is a tree you branch and go back in (`/tree`, `/fork`); files only through an example
  extension; shell output cut to 2,000 lines or 50 KB, the whole in a file. ([sessions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sessions.md))

### Several agents, and models against each other

- **Glut**: up to seven panes in a window, each its own session with its own provider, model and mode; agents in the
  background, addressed by name (`/bg`, `/send`, `/peek`, `/show`); subagents from agent files, each with its own
  provider and model too. Panes, background agents and other Glut windows write to each other, whatever provider each
  runs on — a model's message waits for you unless it comes from a pane of the same mode, and never lends a right. The
  **arena** runs one task on two to four models, blind, each in a confined copy of the repository, and takes the
  winner; a test bench scores models over fixed tasks. ([agents and roles](configuration.md#agents-and-roles))
- **Claude Code**: subagents (up to 20 at once, worktrees); agent teams with tmux or iTerm2 panes are experimental and
  off by default. ([subagents](https://code.claude.com/docs/en/sub-agents),
  [teams](https://code.claude.com/docs/en/agent-teams))
- **Codex CLI**: subagents on by default, in parallel.
  ([subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents.md))
- **Gemini CLI**: subagents, remote agents over A2A. ([subagents](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/core/subagents.md))
- **opencode**: subagents in child sessions, in the background. ([agents](https://opencode.ai/v2/docs/agents/))
- **Aider**: one agent; an architect model hands to an editor model. ([modes](https://aider.chat/docs/usage/modes.html))
- **Cursor CLI**: subagents with a model each and worktree isolation; `/best-of-n` runs one prompt on several models
  (documented for the IDE). ([subagents](https://cursor.com/docs/subagents),
  [worktrees](https://cursor.com/docs/configuration/worktrees))
- **Goose**: subagents in parallel, up to five.
  ([subagents](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/subagents.mdx))
- **Crush**: a task-agent tool. ([agent tool](https://github.com/charmbracelet/crush/blob/main/internal/agent/agent_tool.go))
- **pi**: one agent by design ("run Pi instances in tmux"); an example extension starts subagents as processes.
  ([site](https://pi.dev))

### Work that comes back

- **Glut**: `/loop` repeats a task until it is done; `/schedule` runs a plan while Glut is closed (launchd, systemd),
  with its own rules, budget and time limit; `glut run` writes its events as JSON lines.
- **Claude Code**: `/loop`, desktop scheduled tasks, cloud routines; `claude -p`.
  ([scheduled tasks](https://code.claude.com/docs/en/scheduled-tasks))
- **Codex CLI**: `codex exec`; recurring automations in the app, not the CLI.
  ([automations](https://learn.chatgpt.com/docs/automations.md))
- **Gemini CLI**: `-p` and JSON output; no scheduler. ([headless](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/cli/headless.md))
- **opencode**: `opencode run`, a server and SDK; schedules, experimental.
  ([CLI](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/web/src/content/docs/cli.mdx))
- **Aider**: `--message` for one task. ([scripting](https://aider.chat/docs/scripting.html))
- **Cursor CLI**: `agent -p`; scheduled runs as cloud automations. ([headless](https://cursor.com/docs/cli/headless))
- **Goose**: `goose run`, recipes on a cron schedule.
  ([CLI](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/goose-cli-commands.md))
- **Crush**: `crush run`, `crush serve`. ([run](https://github.com/charmbracelet/crush/blob/main/internal/cmd/run.go))
- **pi**: `-p`, JSON and RPC modes, an SDK; an experimental package resumes work after a crash.
  ([RPC](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/rpc.md))

### Editors, language servers, extensions

- **Glut**: plugins for Neovim, VS Code and JetBrains — the change shows as a diff in the editor, your answer there is
  Glut's —, and the Agent Client Protocol; a language server's errors after every edit and an `lsp` tool (rename as
  one change); MCP over stdio and HTTP with OAuth and forms; skills, agents, commands, hooks that veto.
  ([editor protocol](editor-protocol.md))
- **Claude Code**: VS Code and JetBrains; ACP through Zed's adapter; language servers by plugins; MCP with OAuth; 33
  hook events; skills, plugins. ([IDEs](https://code.claude.com/docs/en/ide-integrations),
  [code intelligence](https://code.claude.com/docs/en/plugins/code-intelligence))
- **Codex CLI**: VS Code (and forks), JetBrains, Xcode; ACP through an adapter; MCP with OAuth; hooks, skills.
  ([IDE](https://learn.chatgpt.com/docs/codex/ide.md), [MCP](https://learn.chatgpt.com/docs/extend/mcp.md))
- **Gemini CLI**: a VS Code companion, ACP; MCP with OAuth; hooks, skills, extensions.
  ([ACP](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/cli/acp-mode.md))
- **opencode**: ACP, a VS Code extension; language servers in version 1 only (off by default); MCP with OAuth;
  plugins. ([ACP](https://raw.githubusercontent.com/anomalyco/opencode/dev/packages/web/src/content/docs/acp.mdx),
  [LSP](https://opencode.ai/docs/lsp/))
- **Aider**: watches files for instructions written in comments; built-in linting and tests.
  ([watch](https://aider.chat/docs/usage/watch.html))
- **Cursor CLI**: ACP; MCP with OAuth; hooks, skills. ([ACP](https://cursor.com/docs/cli/acp),
  [hooks](https://cursor.com/docs/hooks))
- **Goose**: ACP, an experimental VS Code extension; MCP with OAuth and forms; hooks, skills, recipes.
  ([Zed](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/zed.md))
- **Crush**: language servers you configure, with tools for definition, references and rename; MCP with OAuth;
  hooks, skills. ([tools](https://github.com/charmbracelet/crush/tree/main/internal/agent/tools))
- **pi**: no editor integration documented; MCP with OAuth and a code mode; TypeScript extensions, skills, prompt
  templates, themes. ([MCP](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/mcp.md), [extensions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md))

### Around the conversation

- **Glut**: the side panel's tabs (`/panel`): the plan with its task list, every judgement of the gate, the meter
  (context, tokens and cost of each turn and tool), agents, the project's to-dos, scheduled runs and keys. `/gate`
  lists every "always" and takes one back; `/btw` asks a side question of any pane or agent; `/rule` keeps a rule for
  the session; `/handoff` writes what a later session reads; a run's journal lets `glut resume` take up a lost run.
  Budgets in money for `glut run`, loops and plans. ([configuration](configuration.md))
- **Claude Code**: a task list (⌃T), the agent view with peek and reply, `/permissions` lists rules and removes them,
  `/btw`, plan files, session cost in `/usage`, a budget only with `-p`, pasted images; agent teams message each other.
  ([interactive mode](https://code.claude.com/docs/en/interactive-mode), [agent view](https://code.claude.com/docs/en/agent-view), [permissions](https://code.claude.com/docs/en/permissions),
  [costs](https://code.claude.com/docs/en/costs), [agent teams](https://code.claude.com/docs/en/agent-teams))
- **Codex CLI**: a status line, `/agents` and `/subagents`, `codex queue`, `/side`, plan mode, `/status` tokens,
  pasted images with previews; a subagent's file cannot change the provider.
  ([commands](https://learn.chatgpt.com/docs/developer-commands?surface=cli), [subagents](https://learn.chatgpt.com/docs/agent-configuration/subagents.md))
- **Gemini CLI**: to-dos above the prompt, plan mode with a plan file, `/policies list` (no removal), `/stats` tokens,
  a turn limit, remote agents over A2A. ([to-dos](https://github.com/google-gemini/gemini-cli/blob/main/docs/tools/todos.md), [plan mode](https://github.com/google-gemini/gemini-cli/blob/main/docs/cli/plan-mode.md),
  [commands](https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/commands.md))
- **opencode**: a sidebar with the session's cost, to-dos and files; session tabs; `/btw`; background subagents; a
  provider and model per agent; monthly budgets for its Zen service. ([TUI](https://github.com/anomalyco/opencode/blob/dev/services/www/src/docs/content/cli/tui.mdx),
  [agents](https://github.com/anomalyco/opencode/blob/dev/services/www/src/docs/content/agents.mdx), [sidebar](https://github.com/anomalyco/opencode/blob/dev/packages/tui/src/feature-plugins/sidebar/context.tsx))
- **Aider**: the cost of each reply, `/paste` for images, `/read` a conventions file, `--restore-chat-history`.
  ([commands](https://github.com/Aider-AI/aider/blob/main/aider/website/docs/usage/commands.md), [conventions](https://github.com/Aider-AI/aider/blob/main/aider/website/docs/usage/conventions.md))
- **Cursor CLI**: a jobs pager for shells and background subagents, `/btw`, `/plan`, image previews, plan spend in
  `/usage`. ([changelog](https://cursor.com/docs/cli/changelog), [slash commands](https://cursor.com/docs/cli/reference/slash-commands))
- **Goose**: subagents on another provider (`GOOSE_SUBAGENT_PROVIDER`), an estimated cost, a turn limit, an
  instruction file read again every turn. ([subagents](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/subagents.mdx),
  [persistent instructions](https://github.com/aaif-goose/goose/blob/main/documentation/docs/guides/context-engineering/using-persistent-instructions.md))
- **Crush**: a sidebar with model, cost, subagents and changed files; to-do pills; plan mode that hands over to
  coding; a subagent on another provider. ([sidebar](https://github.com/charmbracelet/crush/blob/main/internal/ui/model/sidebar.go),
  [dispatch](https://github.com/charmbracelet/crush/blob/main/internal/agent/templates/agent_dispatch_tool.md))
- **pi**: cost in the footer, images drawn inline, `/tree` and `/fork`; to-dos, plan mode, subagents and a handoff as
  example extensions; steering while it works (⏎ after the current tool). ([site](https://pi.dev),
  [examples](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/examples/extensions/README.md), [sessions](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/sessions.md))

### Secrets and languages

- **Glut**: a key that appears in a tool's output is masked before the model, the hooks or the logs see it; it goes
  back only into a file that already holds it. The interface and the manual in eight languages.
- **Codex CLI** keeps variables named like keys out of commands' environment
  ([configuration](https://learn.chatgpt.com/docs/config-file/config-reference.md)); **Gemini CLI** can redact such
  variables, off by default ([configuration](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/reference/configuration.md)).
  Masking of tool output is not documented for the others.
- The terminal interfaces of the others are in English; Goose's desktop app has 16 languages and opencode's app many
  ([Goose](https://github.com/aaif-goose/goose/blob/main/I18N.md)).
