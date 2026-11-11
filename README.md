<p align="center">
  <img src="docs/images/glimm.svg" width="88" alt="Glimm, Glut's ember">
</p>

<h1 align="center">Glut</h1>

<p align="center">
  <b>A coding agent that doesn't trust its model.</b><br>
  Every command in an OS sandbox · any model, local ones too · tool output never cut · one Rust binary
</p>

<p align="center">
  <a href="https://github.com/axiomata-labs/glut/actions/workflows/ci.yml"><img src="https://github.com/axiomata-labs/glut/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://crates.io/crates/glut-tui"><img src="https://img.shields.io/crates/v/glut-tui?color=ff7a1a" alt="crates.io"></a>
  <a href="https://github.com/axiomata-labs/glut/releases/latest"><img src="https://img.shields.io/github/v/release/axiomata-labs/glut?color=ffa040" alt="Release"></a>
  <img src="https://img.shields.io/badge/macOS%20%C2%B7%20Linux-1a1a1a" alt="macOS · Linux">
  <img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-b8360f" alt="MIT / Apache-2.0">
</p>

<p align="center">
  <a href="https://glut-ai.com">Website</a> ·
  <a href="docs/getting-started.md">Getting started</a> ·
  <a href="crates/glut-tui/manual/en/">Manual</a> ·
  <a href="docs/security.md">Security model</a> ·
  <a href="docs/comparison.md">Compared with others</a>
</p>

```sh
curl -fsSL https://glut-ai.com/install.sh | sh
```

![A real run: asked to read .env and push, Glut fixes the failing tests; the sandbox refuses the .env read, the gate stops the push, and every judgement shows in the side panel](docs/images/demo.gif)

<p align="center"><sub>A real run with Claude Sonnet, time shortened while the model works. The task asks for
<code>cat .env</code> and a push: the sandbox refuses the read, the gate stops the push, the tests get fixed anyway.</sub></p>

Glut assumes the model can be steered by anything it reads — a README, a web page, a command's output — and limits
what it can *do* instead of trying to detect it. It is a lean tool-calling loop over any OpenAI-compatible, Anthropic
or Responses API — OpenRouter, OpenAI, a ChatGPT plan, Anthropic, Gemini, DeepSeek, Mistral, local models through
Ollama or LM Studio, and two dozen more. It reads and edits your project, runs your tests and builds, and shows you
every step in a terminal UI made for long sessions: several agents side by side, an arena that pits models against
each other, plans that run while you sleep, and your editor at hand.

> **Status: 0.1.2, early.** Glut is used every day by its author and is solid on macOS; Linux is tested in
> containers. Expect rough edges, and please [report them](https://github.com/axiomata-labs/glut/issues).

## Why Glut

- **Every command in an OS sandbox.** macOS Seatbelt, Linux bubblewrap with Glut's own seccomp filter. Commands
  write your project and the usual caches, nothing else; `.env` files cannot be read; git hooks,
  `.cargo/config.toml`, `.vscode/tasks.json` and the like stay untouched at any depth; the network goes through a
  proxy that asks before a new host. See [the security model](docs/security.md).
- **A gate you can read.** Reading is free, writing asks — or not, in the mode you choose. A command that only reads
  (`ls`, `grep`, `git status` …) runs at once; `rm -rf`, `git push` and `sudo` always ask. "Always allow" means the
  scope *you* pick: this exact command, `git status *`, or `git *`.
- **Any model, honestly priced.** Two dozen providers out of the box, keys in the macOS Keychain and bound to their
  host. The footer shows tokens, cache hits and cost as they happen; budgets stop a run before it overspends.
- **Several agents at once.** Up to seven panes, agents in the background, agents that message each other — within one
  window and across Glut windows — without ever lending each other rights.
- **An arena for models.** The same task on two to four models, blind by default, each in a confined copy of your
  repository; compare time, cost, tokens and diffs, take the winner. A test bench of fixed tasks scores models over
  repeated runs.
- **Built for long work.** It compacts its own context ahead of time, never cuts a tool's output (the whole stays in
  a store the model can page through), snapshots your files before every change (`/steps` puts them back, `/rewind`
  the conversation with them), and resumes a run that was lost midway.
- **Your editor, not instead of it.** Neovim, VS Code and JetBrains plugins, and the Agent Client Protocol for Zed and
  others: diffs open in your editor, your answer there is Glut's answer.
- **One program, nothing else.** Written in Rust, a single binary under 50 MB with everything above inside — no
  Node, no Python, no runtime to install.
- **Eight languages.** English, German, French, Italian, Spanish, Chinese, Japanese and Portuguese — the whole
  interface and the manual.

## A look

| A first task | The arena | Neovim |
|---|---|---|
| ![Glut after a first task: the failing tests found, two bugs fixed, the steps it took on the left, its side panel on the right](docs/images/scene-1.png) | ![Three models' results side by side in the arena's card, the chosen one's diff below](docs/images/scene-5.png) | ![Glut's edit as a Neovim diff, the owner's own line on its right side, Glut's question beside it](docs/images/scene-8.png) |
| failing tests found and fixed, every step shown | three models blind, their time, cost and diff | an edit as a diff in your editor, your change in it |

Every scene is a real run; [the website](https://glut-ai.com) replays ten of them as text.

## At a glance

As of 2026-11-05, from each tool's own documentation — the [full comparison](docs/comparison.md) has nine tools, the
sources, and where the others are ahead.

| | Glut | Claude Code | Codex CLI | opencode | pi |
|---|---|---|---|---|---|
| Models | any vendor, local | Claude only | OpenAI, local | any vendor, local | any vendor, local |
| OS sandbox for commands | on by default | opt-in | on by default | none | none, a container advised |
| Network of commands | proxy asks per host | proxy, allow-list | off; optional proxy | none | none |
| Tool output | never cut | cut, rest in a file | token budget | cut, rest in a file | cut, rest in a file |
| Models side by side | arena, test bench | — | — | — | — |
| Licence | MIT / Apache-2.0 | proprietary | Apache-2.0 | MIT | MIT |

## Install

```sh
# macOS and Linux, a prebuilt program (checksums verified):
curl -fsSL https://glut-ai.com/install.sh | sh

# Homebrew:
brew install axiomata-labs/tap/glut

# From source (a current stable Rust):
cargo install glut-tui
```

More, including what macOS asks the first time and how to build from a checkout: [docs/install.md](docs/install.md).

## Start

```sh
glut providers add openrouter        # or anthropic, openai, ollama … — `glut providers catalog` lists them
cd your-project
glut
```

Type what you want done. `⇧⇥` switches the mode (plan, ask, build, auto), `/` lists every command, `⌃K` shows every
key, `/help <command>` opens its page. A walk through the first session:
[docs/getting-started.md](docs/getting-started.md).

## Documentation

| | |
|---|---|
| [Getting started](docs/getting-started.md) | the first session, modes, the screen |
| [Manual](crates/glut-tui/manual/en/) | every command, one page each — also in Glut with `/help` |
| [Configuration](docs/configuration.md) | `~/.glut/` and `.glut/`: settings, providers, MCP, hooks, skills, agents |
| [Security model](docs/security.md) | what the gate and the sandbox hold back, and what they do not |
| [Architecture](docs/architecture.md) | how a turn runs, the crates, where to start reading |
| [Glut as a library](docs/library.md) | embedding the loop in your own program; `glut run`'s JSON events |
| [Editor protocol](docs/editor-protocol.md) | what an editor plugin and Glut say to each other |
| [Glut and other coding agents](docs/comparison.md) | nine others compared, dated and sourced — and where they are ahead |

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). Found a way past the sandbox or the
gate? That is the most useful thing you can tell us. A vulnerability goes to
[SECURITY.md](SECURITY.md), not to an issue. The repository's [AGENTS.md](AGENTS.md) is what a coding agent (Glut,
or another) should read before it touches the code.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
Unless you explicitly state otherwise, any contribution you submit for inclusion in Glut shall be dual licensed as
above, without any additional terms or conditions.
