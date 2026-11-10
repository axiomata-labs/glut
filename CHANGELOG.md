# Changelog

All notable changes to Glut. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [Semantic Versioning](https://semver.org/) — while Glut is at 0.x, a minor version may change what it does.

## [Unreleased]

### Added

- **What is new, and when a newer Glut is out**: after an update the welcome shows once what is new since the
  version you saw last (`/news` shows it again), in your language; once a day the terminal UI reads the newest
  version's number from `glut-ai.com/latest.txt` and shows a newer one quietly in the footer (`↑ 0.1.2`) — `/update`
  says the command that updates this Glut by how it was installed, `/update off` or `"updateCheck": false` stops the
  look. Never in a plan, a loop's round, `glut run`, `glut acp` or the arena; Glut never updates itself.
- **The install script on glut-ai.com**: `curl -fsSL https://glut-ai.com/install.sh | sh`; the binaries still come
  from GitHub's releases, and the script beside each release stays as the second way.
- **Who is here**: the models get a tool `who` beside `tell` — every other agent of this window and of the other Glut
  windows, with its role, model, mode, folder, branch, whether it is busy, and the line it set for what it works on
  (its own words). A model sets its own line with `who {"doing": …}`.
- **Requests with answers**: `tell` with `answer` asks another agent for something, in this window or another; the
  turn it starts there answers — its last reply comes back to the asker as a message marked as that request's
  answer, while the asker works on. A busy agent takes it after its turn, and the asker hears where it stands in line;
  one that cannot do it says so. Where nobody will answer (set aside with Esc, a turn refused or one before it
  unfinished, the pane closed, the window ended, an hour gone) Glut tells the asker. Between windows a request
  travels by a random number, so no window answers what it was not asked.

- **The Team tab** (⌃X T): every agent of this window and of the other Glut windows — busy, free or waiting for
  you, its branch and its own line — and what passed between them: messages, requests, answers, Glut's word that
  none comes, who came and went. Its shapes are the library's (`glut::team::Profile`, `Note`), for any screen.

- **Overlaps told**: an agent at work in the same working tree and branch as another at work there, or one that
  changes a file another changed in the last quarter hour, hears it once before its next step as Glut's own note; the
  owner sees it in the stream and the Team tab.
- **A lead and its helpers**: a session the owner started can lead — `start_helpers` starts helpers in the
  background, each with a role, a model, its first task (a request whose answer comes back), in the lead's folder or
  in a copy of the repository of its own (which reads nothing of the lead's repository); `take_work` brings a
  helper's work over from its copy, checked as an arena's winner is, every file judged as an edit of the lead's
  folder with a snapshot before, uncommitted, and closes it. A helper has its lead's mode and "always" rules, plays
  its lead's role if it has one, and starts no helpers; one in a copy gets no MCP server, no folder beside it and no
  Keychain, and runs only where the sandbox does. Build mode asks once for the whole team, auto starts it, ask and plan mode only
  propose; past six agents in the background the owner decides.
- `RunConfig::notes`: an inbox for the host's own `[glut]` notes, taken before the run's next model call beside the
  owner's steering, never shown as the owner's word.

### Changed

- **An answer always gets through** (agents that work together): an answer, or Glut's word that none comes, starts
  the asker's next turn even after a long exchange without the owner, and even when the owner allowed the asker
  something "always" while it waited — it asked. A model that asked another agent, or started helpers, is no longer
  urged to go on by its open task list, and is told to end its turn rather than wait, sleep or check on the answer.
  A lead starts no more helpers on its own after a long exchange without the owner.
- **Fewer questions when agents work together**: `xargs` running a plain reader (`find … -print0 | xargs -0 wc -l`,
  `… | xargs cat`) counts as reading, like the reader itself; a helper's work taken with `take_work` leaves out what a
  build left (`__pycache__`, `*.pyc`, `node_modules`, `target`, `.pytest_cache`, `dist`) where the repository keeps
  nothing of that kind, and says so; and two agents on the same branch of one working tree hear of each other only
  once one of them changed a file there.
- New public fields a host building these structs must fill: `Settings::update_check`, `Loaded::update_check` and
  `BuiltGate::update_check` (the owner's `updateCheck`); `glut clean --json` lists a new item `version`.
- New public items a host matching or building them exhaustively must add: `RunConfig::notes`,
  `RunConfig::team_tools` and `glut::TEAM_TOOLS`, and
  `glut::team::NoteKind::Overlap` in the new `glut::team`.
- `glut::PANE_TOOLS` holds two names now, `tell` and `who` (`[&str; 2]`): a host that adds the screen's tools adds
  both.

## [0.1.0] — 2026-11-08

The first public release. Glut grew for about a month inside a larger project before it became an application of
its own; this release brings together:

- **The loop**: a tool-calling loop over OpenAI-compatible, Anthropic and Responses APIs, with two dozen providers in
  its catalog, keys in the Keychain bound to their host, ChatGPT and OpenRouter sign-in, prices, budgets and
  per-provider routing; compaction ahead of time, refusal limits, results kept whole in a store, compact command
  output, journals that let a lost run resume, snapshots before every change and `glut undo`, `/rewind` of the
  conversation with the files, what it cuts off kept as a branch.
- **The tools**: guarded file tools, a shell whose every command runs in an OS sandbox (macOS Seatbelt; Linux
  bubblewrap with Glut's own seccomp filter) behind a network proxy, code mode (QuickJS), language-server errors after
  an edit and an `lsp` tool, MCP (stdio and HTTP, OAuth, forms), skills, hooks that can veto, subagents.
- **The gate**: modes (plan, ask, build, auto), rules, commands judged by what they are, scopes for "always", a second
  model that may only say a reader reads, secrets masked where results enter the conversation.
- **The terminal UI**: up to seven panes, agents in the background and messages between them and across windows,
  loops, plans that run while Glut is closed (launchd, systemd), the arena and its test bench, side questions, search
  over sessions, images, export, eight languages, a screen-reader mode.
- **Editors**: plugins for Neovim, VS Code and JetBrains, and the Agent Client Protocol.

[0.1.0]: https://github.com/axiomata-labs/glut/releases/tag/v0.1.0
