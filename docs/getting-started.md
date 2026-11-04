# Getting started

This walks through a first session: a provider, a folder, a task, and the few keys that matter on the first day.
Everything else is in the [manual](../crates/glut-tui/manual/en/) — and in Glut itself, behind `/help`.

## 1. A provider

Glut talks to a model through a provider. Add one; the key is asked without echo and kept in the macOS Keychain (on
Linux in `~/.glut/credentials.json`, readable only by you), bound to the provider's host:

```sh
glut providers add openrouter --model anthropic/claude-sonnet-5-5
```

Other ways in:

```sh
glut providers catalog                 # every provider Glut knows by name
glut providers login chatgpt           # your ChatGPT Plus or Pro plan, signed in in the browser
glut providers login openrouter        # an OpenRouter key without copying it
glut providers add ollama              # models on this machine, no key
glut providers add openai-compatible --root http://box:8080/v1 --keyless
glut providers test openrouter         # one tiny call: endpoint, key and model work
```

The first provider added is the default; `glut providers default <id>` changes it, and `/model` in Glut picks another
model or provider for the session.

## 2. A folder

```sh
cd your-project
glut
```

Glut works in the folder it was started in. It reads the project's guide — `AGENTS.md` (or `CLAUDE.md`) — into its
prompt; `/init` writes one for you from what it finds in the folder. A git repository helps: Glut shows the branch,
and every change it makes can be seen as a diff.

A project may bring settings of its own in `.glut/settings.json`. Its deny and ask rules always count; if it brings
allow rules or a mode, those count only in a folder you trust and are left out everywhere else: `/trust` shows what
they would add, `/trust on` trusts the folder from its next session on. A project's hooks never run — only yours, from
`~/.glut/settings.json`, do.

## 3. A task

Type what you want, as you would tell a colleague:

```
the tests in parser.rs fail since yesterday's change — find out why and fix it
```

Glut plans its steps (the task list stands in the side panel), reads, edits and runs commands. Each edit appears as a
diff in the stream; `⌃O` unfolds every tool's result, `⌃T` every thought.

While it works:

| key | |
|---|---|
| `⏎` | steer it: your words are taken in before its next step |
| `⇥` | queue a message for when it is done |
| `⌥↑` | take the last waiting message back |
| `⎋` | stop it |

When it asks — before a change, a command, a new host on the network — a card says exactly what it wants to do.
Answer with a key; "always" offers the scope, and you pick it: this exact command, `cargo test *`, or `cargo *`.

## 4. Modes

`⇧⇥` goes round the modes; the footer shows the one in force.

| mode | reads | edits files | runs commands |
|---|---|---|---|
| **plan** | yes | no — it writes a plan and asks you to approve it | readers only |
| **ask** | yes | asks | readers run, everything else asks |
| **build** | yes | yes | readers run, everything else asks |
| **auto** | yes | yes | everything sandboxed runs; heavy commands (`rm -rf`, `git push`, `sudo` …) ask |

Every command runs in the sandbox, in every mode: it writes your project and the usual caches, reads no `.env` file,
leaves git's hooks and the like alone, and reaches the network only through Glut's proxy. See
[the security model](security.md).

## 5. When something went wrong

- `/steps` — Glut snapshots your files before every change; this lists the steps that changed files, and puts them
  back as they were before one of them (`glut undo` does the same outside Glut, for a whole run or one step).
- `⎋ ⎋` — rewind the conversation to an earlier message and go on from there (or fork it).
- `/diff` — what changed in this session, file by file; a hunk can go back alone.
- `glut -c` — the newest session of this folder, as you left it; `glut -r` picks an older one, `/search` finds one by
  what was said.

## 6. The screen

- **The header**: the folder, the branch, the session.
- **The stream**: your messages, Glut's answers, its tool calls and results.
- **The side panel** (`⌃B`): the plan's task list; `⌃G` the gate's judgements live, `⌃N` the meter (context, tokens,
  cost per turn and tool), `⌃X A` the agents.
- **The footer**: the mode, the model, the context used, the cost, the branch.

`⌃K` lists every key, `/` every command, `/help <command>` opens its page. `/language` switches among the eight
languages; `--reader` (or `/reader`) is a mode for screen readers.

## Where next

- More agents: `⌃X 2` splits a second pane, `/bg` starts an agent in the background, `/send` writes to another pane.
- `/arena` gives one task to several models and lets you take the best result.
- `/loop` repeats a task until it is done; `/schedule` runs one while Glut is closed.
- `/mcp`, `/skill`, `/role` and [configuration](configuration.md) for tools, skills and agents of your own.
- [Editor plugins](editor-protocol.md) for Neovim, VS Code and JetBrains.
