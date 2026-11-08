# Changelog

All notable changes to Glut. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions
follow [Semantic Versioning](https://semver.org/) — while Glut is at 0.x, a minor version may change what it does.

## [Unreleased]

### Added

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

### Changed

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
