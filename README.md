# Glut

**A coding agent for your terminal that runs every command in a sandbox, asks before it changes what matters, and
works with whatever model you choose.**

Glut is a lean tool-calling loop over any OpenAI-compatible, Anthropic or Responses API — OpenRouter, OpenAI, a
ChatGPT plan, Anthropic, Gemini, DeepSeek, Mistral, local models through Ollama or LM Studio, and two dozen more. It
reads and edits your project, runs your tests and builds, and shows you every step in a terminal UI made for long
sessions: several agents side by side, an arena that pits models against each other, plans that run while you
sleep, and your editor at hand.

> **Status: 0.1.0, early.** Glut is used every day by its author and is solid on macOS; Linux is tested in
> containers. Expect rough edges, and please [report them](https://github.com/NamorX68/glut/issues).

![Glut after a first task: the failing tests found, two bugs fixed, the steps it took on the left, its side panel on the right](docs/images/scene-1.png)

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
  a store the model can page through), snapshots your files before every change (`/steps` puts them back), and resumes a run that
  was lost midway.
- **Your editor, not instead of it.** Neovim, VS Code and JetBrains plugins, and the Agent Client Protocol for Zed and
  others: diffs open in your editor, your answer there is Glut's answer.
- **One program, nothing else.** Written in Rust, a single binary under 50 MB with everything above inside — no
  Node, no Python, no runtime to install.
- **Eight languages.** English, German, French, Italian, Spanish, Chinese, Japanese and Portuguese — the whole
  interface and the manual.

## A look

| The gate | The arena | Neovim |
|---|---|---|
| ![A `.env` read refused by the sandbox and a `git push` refused at the gate, the judgements in the side panel](docs/images/scene-2.png) | ![Three models' results side by side in the arena's card, the chosen one's diff below](docs/images/scene-5.png) | ![Glut's edit as a Neovim diff, the owner's own line on its right side, Glut's question beside it](docs/images/scene-8.png) |
| `.env` refused, `git push` asked and refused | three models blind, their time, cost and diff | an edit as a diff in your editor, your change in it |

Every scene is a real run; the website replays them as text.

## Install

```sh
# macOS and Linux, a prebuilt program (checksums verified):
curl -fsSL https://github.com/NamorX68/glut/releases/latest/download/install.sh | sh

# Homebrew:
brew install NamorX68/tap/glut

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
| [Glut and other coding agents](docs/comparison.md) | eight others compared, dated and sourced — and where they are ahead |

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md). A vulnerability goes to
[SECURITY.md](SECURITY.md), not to an issue. The repository's [AGENTS.md](AGENTS.md) is what a coding agent (Glut,
or another) should read before it touches the code.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
Unless you explicitly state otherwise, any contribution you submit for inclusion in Glut shall be dual licensed as
above, without any additional terms or conditions.
