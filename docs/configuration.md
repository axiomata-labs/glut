# Configuring Glut

This page lists every file Glut reads or writes, every setting in them, the environment variables it reads and the
`glut` commands and flags. Everything Glut keeps lives in `~/.glut/` (the owner's folder) or, for a project, in the
project's `.glut/` folder and its `.mcp.json`.

Two rules shape the whole layout:

- **Files are storage; commands change them.** Most files below are written by a `glut` subcommand or a slash
  command of the terminal UI, and the tables name that command. You may read and edit them by hand, but the command
  is the intended way. The exception is `~/.glut/settings.json`: it is yours alone, and no command ever rewrites it.
- **A project can make Glut stricter, never looser.** A repository's `.glut/` may add deny and ask rules and secret
  paths. Everything that would widen what Glut may do (allow rules, a mode, skills, agents, command files) counts only
  in a folder you trusted, and a project's hooks never run at all.

`$GLUT_HOME` replaces your home folder for everything below: with `GLUT_HOME=/tmp/test`, Glut reads
`/tmp/test/.glut/settings.json` and so on. What is yours stays in your own home all the same: a `~` you write (in a
path below, in `/folder add`), the caches commands write and the places whose files run as programs. Plans made by
a Glut whose home is moved keep their launchd and systemd files in that home, apart from your own plans.

## Contents

- [Files at a glance](#files-at-a-glance)
- [settings.json](#settingsjson)
- [A project's .glut folder](#a-projects-glut-folder)
- [trusted.json](#trustedjson)
- [hooks.json](#hooksjson)
- [providers.json and keys](#providersjson-and-keys)
- [mcp.json, .mcp.json and mcp-approved.json](#mcpjson-mcpjson-and-mcp-approvedjson)
- [secrets.json](#secretsjson)
- [sandbox.json](#sandboxjson)
- [keychain.json](#keychainjson)
- [classifier.json](#classifierjson)
- [clean.json](#cleanjson)
- [prefs.json](#prefsjson)
- [schedules.json](#schedulesjson)
- [Skills](#skills)
- [Agents and roles](#agents-and-roles)
- [Command files](#command-files)
- [Guides, memory and the project map](#guides-memory-and-the-project-map)
- [Data Glut keeps](#data-glut-keeps)
- [Environment variables](#environment-variables)
- [Command line](#command-line)

## Files at a glance

Files you configure:

| File | What it holds | Written by |
|---|---|---|
| `~/.glut/settings.json` | Rules, mode, sandbox paths, hooks, limits | You, by hand |
| `<project>/.glut/settings.json` | A project's rules, mode, secret paths | The project |
| `<project>/.glut/settings.local.json` | Same shape, for local additions | The project |
| `~/.glut/trusted.json` | Folders whose project settings may loosen the gate | `/trust`, `glut trust` |
| `~/.glut/hooks.json` | More hooks, beside those in `settings.json` | The `make-hook` skill, or you |
| `~/.glut/providers.json` | Model providers, default provider, routing | `glut providers`, `/provider`, `/context` |
| `~/.glut/credentials.json` | Keys and sign-ins, where the file store is used | `glut providers`, `glut mcp login` |
| `~/.glut/mcp.json` | Your MCP servers | `glut mcp add`, `/mcp add` |
| `<project>/.mcp.json` | A project's MCP servers | The project |
| `~/.glut/mcp-approved.json` | Project MCP servers you approved | Glut, when you approve one |
| `~/.glut/secrets.json` | Secret masking: on/off, own formats, values let through | `/secrets` |
| `~/.glut/sandbox.json` | Git folders read-only; sandbox required | `glut sandbox`, `/sandbox` |
| `~/.glut/keychain.json` | Programs that may ask for the Keychain | `glut keychain`, `/keychain` |
| `~/.glut/classifier.json` | Which model judges read-only commands | `/classifier` |
| `~/.glut/clean.json` | How long Glut keeps its data | `glut clean keep`, `/clean keep` |
| `~/.glut/prefs.json` | Terminal UI preferences | Slash commands (see the section) |
| `~/.glut/schedules.json` | Plans that run while Glut is closed | `glut schedule`, `/schedule` |
| `~/.glut/skills/`, `<project>/.glut/skills/` | Skills (`<name>/SKILL.md`) | You, or the `make-skill` skill |
| `~/.glut/agents/`, `<project>/.glut/agents/` | Named agents (`<name>.md`) | You, or the `make-agent` skill |
| `~/.glut/commands/`, `<project>/.glut/commands/` | Slash command files for `glut run` | You |
| `~/.glut/AGENTS.md` | Your own guide, read into every session's prompt | You |

Data Glut keeps for itself is listed under [Data Glut keeps](#data-glut-keeps).

Every JSON file Glut reads is checked when it is read. A settings, hooks, providers or MCP file that is too large,
not valid JSON or holds a key Glut does not know stops the run with an error naming the file; Glut does not skip a
broken file silently. A missing file counts as empty.

## settings.json

`~/.glut/settings.json` is the owner's settings file. Glut never writes it. A project may have a file of the same
shape (see [A project's .glut folder](#a-projects-glut-folder)), but of the keys below only `permissions` and
`sandbox.denyRead` count there; everything else is read from your file only.

Unknown top-level keys are refused, so a misspelt `"denny"` stops the run instead of dropping a deny list. The file
may be at most 256 KiB and hold at most 1,000 rules.

```json
{
  "permissions": {
    "defaultMode": "default",
    "allow": ["Bash(cargo test:*)", "Edit(src/**)", "Network(crates.io)"],
    "ask": ["Bash(git push:*)"],
    "deny": ["Read(**/.env)", "Bash(rm -rf:*)", "mcp__mail__send"]
  },
  "sandbox": {
    "enabled": true,
    "allowWrite": ["~/.cache/my-tool"],
    "denyRead": ["~/private"],
    "allowLocalBinding": false
  },
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "~/bin/check-command", "timeout": 10 }] }
    ]
  },
  "effort": "medium",
  "refusals": { "repeat": 3, "total": 15 },
  "compact": { "auto": true },
  "contextWindow": { "gemma4:12b": 32768 },
  "prices": { "deepseek/deepseek-v4.1-flash": { "input_per_m": 0.1, "output_per_m": 0.4 } },
  "subagents": { "parallel": 3 },
  "guide": { "warnAt": 8 },
  "notify": { "when": "asks", "after": 20 },
  "loop": { "rounds": 50, "maxUsd": 5, "maxTokens": 5000000 },
  "nudge": true,
  "updateCheck": true,
  "lsp": true,
  "maxTurns": 100
}
```

| Key | Type | Default | Meaning | In the UI |
|---|---|---|---|---|
| `permissions` | object | empty | Rules and default mode; see below | `/gate` shows them |
| `sandbox` | object | sandbox on | Extra paths for the sandbox; see below | `/sandbox` shows it |
| `hooks` | object | none | Hooks in Claude Code's shape; see below | — |
| `effort` | string | provider's | `none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max` | `/effort` (session) |
| `refusals.repeat` | integer ≥ 2 | 3 | The same refused call this often ends the run | — |
| `refusals.total` | integer ≥ `repeat` | 15 | Refusals of any kind in one run that end it | — |
| `compact.auto` | boolean | `true` | Compact the conversation shortly before the window is full | `/compact` (once) |
| `contextWindow` | object | empty | Model id → window in tokens (at least 4,096) | `/context <size>` |
| `prices` | object | empty | Model id → USD per million tokens, for budgets in money | — |
| `subagents.parallel` | integer 1–8 | 3 | Subagents of one reply that run at once | — |
| `guide.warnAt` | integer 1–64 | 8 | KB from which a project guide is pointed out as too large | `/init` |
| `notify.when` | string | `on` | `on`, `asks`, `off` or `system` | `/notify` |
| `notify.after` | integer 0–3600 | 10 | Seconds a turn must run before its end is told | `/notify after` |
| `loop.rounds` | integer 1–1000 | 50 | Most rounds one `/loop` runs | `/loop rounds` |
| `loop.maxUsd` | number > 0 | 5 | Most a loop's session may cost, in USD | `/loop budget` |
| `loop.maxTokens` | integer 1–10⁹ | 5,000,000 | Most tokens a loop on a subscription may use | `/loop tokens` |
| `nudge` | boolean | `true` | Ask the model once to go on when it stops with its task list open | `/nudge` |
| `updateCheck` | boolean | `true` | Once a day the terminal UI reads the newest version's number from `https://glut-ai.com/latest.txt` and shows a newer one in the footer; never in a plan, a loop's round, `glut run`, `glut acp` or the arena | `/update` |
| `lsp` | boolean | `true` | Language servers: errors after an edit and the `lsp` tool | `/lsp` |
| `maxTurns` | integer 1–1000 | 100 | Most model turns a run (or a turn of the UI) takes | `/maxsteps` |

The defaults of `notify` and `loop` are the terminal UI's own. Where a slash command is named, what it sets is kept in
[`prefs.json`](#prefsjson) and takes precedence over `settings.json`; `settings.json` is the second source. `/effort`,
`/context` and `/compact` act on the session or the provider instead (see their sections).

`prices` entries take `input_per_m`, `output_per_m` and optionally `cached_input_per_m` (cache reads; unset, they
cost `input_per_m`). All must be finite and not negative. Without a price for its model, a run has no budget in
money (`glut run --max-usd` says so).

### permissions

| Key | Type | Default | Meaning |
|---|---|---|---|
| `defaultMode` | string | `default` | The mode for what no rule speaks about |
| `allow` | array of rules | `[]` | Calls that run without asking |
| `ask` | array of rules | `[]` | Calls that are asked about (refused where nobody can answer) |
| `deny` | array of rules | `[]` | Calls that are refused |

A deny rule wins over an ask rule, an ask rule over an allow rule, and a rule over the mode.

**Modes.** `defaultMode` takes `default`, `accept_edits` (also `acceptEdits` or `build`), `plan`, `dont_ask` (also
`dontAsk`) or `auto`.

| Mode | Without a rule |
|---|---|
| `plan` | Reads only: edits refused even where a rule allows them; commands refused unless a rule allows them |
| `dont_ask` | Whatever no rule allows is refused; nothing is asked |
| `default` | Reads run; edits and commands are asked about |
| `accept_edits` | Reads and edits run; commands are asked about. The UI calls this mode `build` |
| `auto` | Sandboxed commands run; heavy ones (`rm -rf`, `git push` …), the escape, and all without a sandbox ask |

In every mode a command made only of listed readers (`ls`, `grep`, `git status`, …) runs like a file read. The
terminal UI starts a session in `build` when the settings name `default` or nothing; Shift+Tab cycles the session
through `plan`, `default`, `build` and `auto`. `glut run` uses `--permission-mode`, else the settings, else
`default`; headless, an ask is refused.

**Rules** follow Claude Code's format:

| Rule | Speaks about |
|---|---|
| `Bash` | Every shell command |
| `Bash(cargo test:*)` | Commands whose first words are `cargo test`, followed by anything or nothing |
| `Bash(git * push *)` | A command matched against the pattern; `*` is any run of characters |
| `Read(src/**)` | Reading files matching the glob, relative to the working folder |
| `Edit(**/*.rs)`, `Write(…)` | Writing or editing files matching the glob (`Write` is the same as `Edit`) |
| `Network(crates.io)` | A host a sandboxed command may reach through Glut's proxy; `web_fetch` too |
| `Network(*.github.com)` | Any host below `github.com` (not `github.com` itself) |
| `WebFetch(domain:docs.rs)` | A host `web_fetch` may reach, and nothing else; `WebFetch` alone is every host |
| `WebSearch` | Glut's `web_search` tool |
| `mcp__mail` | Every tool of the MCP server `mail` |
| `mcp__mail__send` | The tool `send` of the MCP server `mail` (no pattern allowed) |
| `task`, `grep`, `memory`, … | A Glut tool by its own name |

In paths, `*` stays within one folder and `**` crosses folders; on macOS they ignore case. A command rule is matched
against every part of a chain (`a && b`) and against what wrappers run. A command that writes a file is also held
against the `Edit` rules for that file. Names are case-sensitive: `bash(rm:*)` is refused as a typo of `Bash`.

The same syntax is used by `glut run --allow/--ask/--deny`, `glut schedule add --allow/--deny`, the
`allowed-tools` of skills and command files and the `tools` of agents.

An "always" answer to a question in the UI adds a rule for that session only; `/gate` (Ctrl+G) lists those rules and
takes one back. `/rule` keeps an instruction for the model for the session; it is not a permission rule.

### sandbox

| Key | Type | Default | Meaning |
|---|---|---|---|
| `enabled` | boolean | `true` | Run commands in the OS sandbox (Seatbelt on macOS, bubblewrap on Linux) |
| `allowWrite` | array of paths | `[]` | Extra places a command may write |
| `denyRead` | array of paths | `[]` | Extra places no command may read |
| `allowLocalBinding` | boolean | `false` | Commands may listen on and connect to anything on localhost |

Paths starting with `~/` are below your home folder (also where `GLUT_HOME` moves Glut's); relative paths are below
Glut's home folder in your file and below the project in a project's file. A project's file may only add `denyRead` entries: it can neither switch the
sandbox off nor open places to write or localhost. `/sandbox` shows how commands are held.

Two further switches, read-only git folders and a required sandbox, live in [`sandbox.json`](#sandboxjson).

### hooks

Hooks use Claude Code's shape and its four events: `SessionStart`, `PreToolUse`, `PostToolUse` and `Stop`. Other
event names are kept but never run.

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash|Edit",
        "hooks": [{ "type": "command", "command": "~/bin/guard", "timeout": 30 }]
      }
    ],
    "SessionStart": [{ "hooks": [{ "type": "command", "command": "cat ~/notes/today.md" }] }]
  }
}
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `matcher` | string | every tool | Unanchored regular expression on the tool's name; ignored for `Stop` |
| `hooks[].type` | string | required | Must be `command` |
| `hooks[].command` | string | required | The shell command; receives the event as JSON on stdin |
| `hooks[].timeout` | integer | 60 | Seconds before the hook is given up |

The matcher sees Glut's tool name and its Claude Code name: `bash` is also `Bash`, `read_file` `Read`, `write_file`
`Write`, `edit_file` `Edit`, `list_dir` `LS`, `grep` `Grep`, `glob` `Glob` and `task` `Task`. MCP tools are
`mcp__<server>__<tool>`.

**Hooks can only veto.** A hook blocks a call (or the final answer, for `Stop`) by exiting with code 2 (stderr is the
reason) or by printing `{"hookSpecificOutput": {"permissionDecision": "deny"}}` or `{"decision": "block"}`.
`allow` and `ask` answers are accepted and ignored: the gate alone decides what runs. A `SessionStart` hook never
blocks; what it prints (`additionalContext`, stdout on exit 0 or stderr on exit 2) is added to the session's context.
A hook that crashes, times out or exits with another code becomes a note and the run goes on.

Only your hooks run: those in `~/.glut/settings.json` and in [`~/.glut/hooks.json`](#hooksjson). Hooks in a project's
settings files are parsed (so a broken one is still an error) and reported, but never executed.

## A project's .glut folder

A project may hold:

| Path | Counts | Notes |
|---|---|---|
| `.glut/settings.json`, `.glut/settings.local.json` | Always, in part | See below |
| `.glut/skills/` | Trusted folders only | Wins a name over yours |
| `.glut/agents/` | Trusted folders only | Wins a name over yours |
| `.glut/commands/` | Trusted folders only | Read by `glut run` |
| `.mcp.json` (project root) | Always, per server approval | See [MCP](#mcpjson-mcpjson-and-mcp-approvedjson) |

What a project's settings files can do:

| Setting | Untrusted folder | Trusted folder |
|---|---|---|
| `permissions.deny`, `permissions.ask` | Added | Added |
| `permissions.allow` | Ignored (reported) | Added; never opens the Keychain to a command |
| `permissions.defaultMode` | Ignored (reported) | Used only if no looser than yours |
| `sandbox.denyRead` | Added | Added |
| `sandbox` (other keys) | Ignored | Ignored |
| `hooks` | Parsed, never run | Parsed, never run |
| All other keys | Ignored | Ignored |

Modes from strictest to loosest: `plan`, `dont_ask`, `default`, `accept_edits`, `auto`.

Owner-only files are never read from a project: `secrets.json`, `sandbox.json`, `keychain.json`, `classifier.json`,
`providers.json`, `mcp.json`, `hooks.json` and `prefs.json` exist only in `~/.glut/`. Text from a project (guides,
skills, agents) is framed as the project's and never speaks with Glut's own voice.

## trusted.json

`~/.glut/trusted.json` lists the folders whose project settings may loosen the gate, and whose skills, agents and
command files count. A folder must match exactly, as its canonical (resolved) path. `/trust` in Glut shows whether
the working folder is trusted and what its settings would add; `/trust on`, `/trust off` and `/trust list` change and
list it, `glut trust show|add|remove [folder]` and `glut trust list` do the same outside Glut. A change counts from the folder's next
session; the home folder, the root and a folder not yours alone (another user's, or one anybody may write) are
never trusted, and no command Glut runs may change the list. `/trust` lists the project's allow rules, mode, skills,
agents and commands before you decide.

```json
{
  "folders": ["/Users/me/code/my-project"]
}
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `folders` | array of paths | `[]` | Trusted project folders, canonical paths |

## hooks.json

`~/.glut/hooks.json` holds hooks in the same shape as the `hooks` key of `settings.json`, and nothing else
(`{"hooks": {…}}`). It exists so Glut can write hooks without touching your hand-written settings: the built-in
`make-hook` skill writes here. Its hooks run after those of `settings.json`. A hook that does not compile stops the run,
as in `settings.json`.

## providers.json and keys

`~/.glut/providers.json` holds the model providers runs use. It never holds a key. Glut rewrites it whole; edit it with
`glut providers` or in the UI.

```json
{
  "default": "openrouter",
  "credentialStore": "auto",
  "providers": {
    "openrouter": {
      "kind": "openrouter",
      "model": "deepseek/deepseek-v4.1-flash",
      "windows": { "deepseek/deepseek-v4.1-flash": 65536 },
      "routing": {
        "rule": { "on": true, "margin": 0.4, "uptime": 95, "tokensPerSecond": 30, "firstTokenSeconds": 10 },
        "models": { "deepseek/deepseek-v4.1-flash": { "prefer": ["deepinfra"], "quant": ["fp8"] } }
      }
    },
    "box": { "kind": "openai-compatible", "root": "http://box:8080/v1", "model": "qwen3", "key": false }
  }
}
```

| Key | Type | Default | Meaning | Command |
|---|---|---|---|---|
| `default` | string | first added | The provider a run takes without `--provider` | `glut providers default` |
| `credentialStore` | string | `auto` | `auto`, `keyring` or `file`; see below | — |
| `providers.<id>.kind` | string | required | A catalog id, or `openai-compatible` | `glut providers add <kind>` |
| `providers.<id>.root` | string | catalog's | API root with its version; needed for `openai-compatible` | `--root` |
| `providers.<id>.model` | string | none | Model a run takes when none is named | `--model`, `/model` |
| `providers.<id>.keyEnv` | string | catalog's | Environment variable the key may come from | `--key-env` |
| `providers.<id>.key` | boolean | `true` | `false`: needs no key (`openai-compatible` only) | `--keyless` |
| `providers.<id>.windows` | object | empty | Model id → context window in tokens | `/context <size>` |
| `providers.<id>.routing` | object | default rule | OpenRouter host routing | `glut providers route`, `/route` |

Unknown keys are refused. A provider id has at most 40 characters.

**Context windows** are taken in this order: `glut run --context-window`, the provider's `windows`,
`contextWindow` in `settings.json`, the provider's model list.

**Routing** applies to OpenRouter only. `routing.rule` holds `on` (send an order at all), `margin` (how much more a
more precise quantization may cost, as a fraction), `uptime` (percent over the last day), `tokensPerSecond` and
`firstTokenSeconds`; the defaults are the values in the example. `routing.models.<model>` holds a model's pins:
`prefer` and `avoid` (host names), `quant` (`int4`, `int8`, `fp4`, `fp6`, `fp8`, `fp16`, `bf16`, `fp32`,
`unknown`) and `strict` (only the preferred hosts).

### Catalog

`glut providers catalog` lists the kinds `add` takes:

| Kind | Key variable | Kind | Key variable |
|---|---|---|---|
| `openrouter` | `OPENROUTER_API_KEY` | `moonshot` | `MOONSHOT_API_KEY` |
| `openai` | `OPENAI_API_KEY` | `kimi-code` | `KIMI_API_KEY` |
| `chatgpt` | sign-in | `zai` | `ZAI_API_KEY` |
| `anthropic` | `ANTHROPIC_API_KEY` | `zai-coding` | `ZAI_CODING_API_KEY` |
| `gemini` | `GEMINI_API_KEY` | `minimax` | `MINIMAX_API_KEY` |
| `mistral` | `MISTRAL_API_KEY` | `qwen` | `DASHSCOPE_API_KEY` |
| `xai` | `XAI_API_KEY` | `qwen-coding` | `DASHSCOPE_CODING_API_KEY` |
| `deepseek` | `DEEPSEEK_API_KEY` | `ollama-cloud` | `OLLAMA_API_KEY` |
| `groq` | `GROQ_API_KEY` | `opencode-zen` | `OPENCODE_API_KEY` |
| `together` | `TOGETHER_API_KEY` | `opencode-go` | `OPENCODE_GO_API_KEY` |
| `fireworks` | `FIREWORKS_API_KEY` | `ollama` | no key (local) |
| `cerebras` | `CEREBRAS_API_KEY` | `lmstudio` | no key (local) |
| `openai-compatible` | key, or `--keyless` | | |

`chatgpt` signs in with a ChatGPT Plus or Pro plan (`glut providers login chatgpt`). `openrouter` can also sign in
and receive a key without copying it (`glut providers login openrouter`).

### Keys

A run takes a provider's key from its key variable (`keyEnv`, else the catalog's) if that is set, else from the secret
store. `GLUT_API_KEY` is never used for a configured provider; it belongs to `glut run --base-url` alone.

Keys and sign-ins are stored under the account `<id>@<host>`, with the host of the provider's root, so a root changed
in the file finds no key instead of sending the old one to a new host. The store is chosen by `credentialStore`:

| Value | Store |
|---|---|
| `auto` | The macOS Keychain (service `glut`) where available, else `~/.glut/credentials.json` |
| `keyring` | The system's store, or an error where Glut has none (currently macOS only) |
| `file` | `~/.glut/credentials.json`, mode 0600 |

On macOS, a regular build ignores `file` and uses the Keychain; only a build with the `dev-keys` feature reads the
file store (`scripts/install-dev.sh` builds with it, and such a build adds `glut providers store file|keychain` to move
the keys). On Linux the file is the only store for now.

`credentials.json` has the shape `{"secrets": {"<account>": "<secret>"}}`; a file others may read is refused. MCP
tokens are kept in the same store (accounts `mcp-token:<name>@<host>` and `mcp-oauth:<name>@<url>`).

## mcp.json, .mcp.json and mcp-approved.json

MCP servers come from your `~/.glut/mcp.json` and the project's `.mcp.json` (in the project's root, Claude Code's
name). Both use Claude Code's shape. A name in both is yours: a repository cannot take over a server you configured.

```json
{
  "mcpServers": {
    "files": { "command": "npx", "args": ["-y", "some-mcp-server"], "env": { "LOG_LEVEL": "warn" } },
    "github": {
      "type": "http",
      "url": "https://api.githubcopilot.com/mcp/",
      "headers": { "Authorization": "Bearer ${GITHUB_TOKEN}" },
      "oauth": { "clientId": "Iv1.0123456789", "callbackPort": 8765 }
    }
  }
}
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `type` | string | `stdio` | `stdio` or `http` (Streamable HTTP); `sse` is not supported |
| `command` | string | required (stdio) | The program that starts the server |
| `args` | array of strings | `[]` | Its arguments (stdio) |
| `env` | object | `{}` | Variables set for the server process (stdio) |
| `url` | string | required (http) | `https://`, or `http://` to this machine only |
| `headers` | object | `{}` | HTTP headers; `${NAME}` is replaced from the environment (http) |
| `oauth.clientId` | string | none | A client registered beforehand, for servers without dynamic registration (http) |
| `oauth.callbackPort` | integer | any free port | The redirect port the client was registered with (http) |

Server names use `[a-zA-Z0-9_-]`, at most 32 characters, without `__`. A stdio entry with `url` or `headers`, or an
http entry with `command`, `args` or `env`, is refused, as are unknown keys. A stdio server starts with a cleaned
environment plus its `env`.

`${NAME}` in a header is never filled with a model key: `GLUT_API_KEY`, the catalog's key variables and any `keyEnv`
of your providers stay empty there.

**Project servers need approval.** A server from `.mcp.json` starts only after you approved that very server; the
approval is kept in `~/.glut/mcp-approved.json`, keyed by project path, server name and a SHA-256 of its whole config,
so any change asks again. Where nobody can answer (headless, a pipe), an unapproved server is left out with a note. A
project's stdio server may not set variables that decide which code a program loads (`PATH`, `HOME`, `NODE_*`,
`PYTHON*`, `LD_*`, `DYLD_*`, `GIT_*`, `*_PROXY` and similar); set those in your own `mcp.json`.

**Sign-in.** `glut mcp login <name>` (or `/mcp login <name>`) signs in to a hosted server: in the browser where it
speaks OAuth, else with a token typed without echo. `glut mcp logout` revokes and forgets it. Tokens go to the secret
store, never into the file.

`glut mcp add <name> <url>` or `glut mcp add <name> -- <command> [args…]` adds a server to `~/.glut/mcp.json`;
`glut mcp remove <name>` removes and signs it out; `glut mcp list` shows all servers and how each is signed in.
`--mcp-config <file>` (on `glut` and `glut run`) adds servers from another file in the same shape;
`--strict-mcp-config` uses that file alone.

## secrets.json

Glut masks secrets in every tool result before the model, a hook or a stored file sees them; a hit becomes a
placeholder such as `‹secret 1: anthropic key, 108 chars›`. `~/.glut/secrets.json` holds your additions. It is
written by `/secrets`.

```json
{
  "mask": true,
  "patterns": [{ "name": "acme token", "regex": "acme_[A-Za-z0-9]{32}" }],
  "allowed": ["9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"]
}
```

| Key | Type | Default | Meaning | Command |
|---|---|---|---|---|
| `mask` | boolean | `true` | `false`: nothing is masked | `/secrets on`, `/secrets off` |
| `patterns` | array | `[]` | Own formats: `name` (for the placeholder) and `regex` | `/secrets add\|remove` |
| `allowed` | array | `[]` | Values always let through, as hex SHA-256 (never the value) | `/secrets allow <n> always` |

`glut run --no-mask` turns masking off for one run.

## sandbox.json

`~/.glut/sandbox.json` holds two switches about how commands are contained. It is read again before every command.
A file that cannot be read counts as both switches on.

```json
{ "gitReadOnly": true, "required": false }
```

| Key | Type | Default | Meaning | Command |
|---|---|---|---|---|
| `gitReadOnly` | boolean | `false` | Git folders read-only to commands: no commit from the sandbox | `/sandbox git` |
| `required` | boolean | `false` | Without a sandbox here, no shell command or hook runs | `/sandbox required` |

Set them with `glut sandbox git read-only|writable` and `glut sandbox required on|off`, or the same words after
`/sandbox` in the UI. `glut sandbox show` and `/sandbox` show the current state.

## keychain.json

On macOS, commands Glut runs cannot reach the Keychain, except programs on a list, whose commands ask each time (even
in `auto` mode). `~/.glut/keychain.json` holds that list.

```json
{ "open": false, "programs": ["gh", "git push", "git pull", "docker"] }
```

| Key | Type | Default | Meaning | Command |
|---|---|---|---|---|
| `open` | boolean | `false` | The Keychain is open to every command, without asking | `glut keychain open\|close` |
| `programs` | array | see below | Programs (or `git <sub-command>`) that may ask for it | `/keychain add\|remove` |

Without `programs`, the list is `gh`, `glab`, `git push`, `git pull`, `git fetch`, `git clone`, `git ls-remote`,
`docker`, `codesign` and `xcodebuild`. `/keychain open|close` changes it for the current session only;
`glut keychain open|close` keeps the choice. `glut keychain list` and `/keychain` show the state.

## classifier.json

A second model may judge a command that only the mode would ask about: if it says the command only reads, the command
runs like a listed reader, in the sandbox. `~/.glut/classifier.json` names that model; `/classifier` writes it.

```json
{ "classifier": "openrouter deepseek/deepseek-v4.1-flash" }
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `classifier` | string | `session` | `off`, `session` (the session's own model), `<model>` or `<provider> <model>` |

`glut run --classifier` overrides it for one run. The judge never decides past a rule, a heavy command, the Keychain
or plan mode, and with no clear answer within 15 seconds you are asked as usual.

## clean.json

Once a day at start, Glut removes its own old data from `~/.glut` by your terms. `~/.glut/clean.json` holds the
terms in days (1 to 3650).

```json
{ "snapshots": 7, "journals": 7, "sessions": 90, "results": 30, "lean": 90 }
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `snapshots` | integer | 7 | A run's snapshots |
| `journals` | integer | 7 | An unfinished run's journal |
| `sessions` | integer | 90 | A kept session, since it was last written |
| `results` | integer | 30 | Long results of a session that is gone, or of none |
| `lean` | integer | 90 | Lines of `lean.jsonl` |

Set a term with `glut clean keep <kind> <days>d` or `/clean keep <kind> <days>d`. `glut clean` (or `/clean now`)
cleans now; `--dry-run` only counts; `--json` writes the report for a program.

## prefs.json

`~/.glut/prefs.json` holds the terminal UI's preferences. Every key is written by a slash command; keys Glut does not
know are kept as they are.

| Key | Type | Default | Meaning | Command |
|---|---|---|---|---|
| `editor` | string | none | How a file opens at a line: `nvim +{line} {file}` | `/editor` |
| `thinking` | string | `folded` | `off`, `folded` or `open` | `/thinking`, Ctrl+T |
| `panel` | boolean | — | Side panel shown | `/panel`, Ctrl+B |
| `vi` | boolean | `false` | Edit the input the vi way | `/vim` |
| `footer` | string | guessed | `powerline` or `plain` | `/footer` |
| `cache` | string | `ping` | `ping`, `1h` (Anthropic's hour) or `off` | `/cache` |
| `images` | string | `inline` | `inline`, `blocks` or `off` | `/images` |
| `links` | string | by terminal | `clickable` or `plain` | `/links` |
| `notify` | string | settings, else `on` | `on`, `asks`, `off` or `system` | `/notify` |
| `notifyAfter` | integer | settings, else 10 | Seconds before a turn's end is told | `/notify after` |
| `tiles` | boolean | `true` | Tiles at the window's top right | `/notify tiles` |
| `loopRounds` | integer | settings, else 50 | Most rounds of a loop | `/loop rounds` |
| `loopMaxUsd` | number | settings, else 5 | Most a loop may cost, in USD | `/loop budget` |
| `loopMaxTokens` | integer | settings, else 5,000,000 | Most tokens of a loop on a subscription | `/loop tokens` |
| `motion` | string | `full` | `full`, `calm` or `off` | `/motion` |
| `steps` | integer | settings, else 100 | Most steps a turn takes | `/maxsteps` |
| `nudge` | boolean | settings, else `true` | Ask the model once to go on | `/nudge` |
| `updateCheck` | boolean | settings, else `true` | Look once a day for a newer Glut | `/update` |
| `lsp` | boolean | settings, else `true` | Language servers serve the turns | `/lsp` |
| `reader` | boolean | `false` | Screen reader mode | `/reader`, `glut --reader` |
| `theme` | string | `glut` | `glut` or an app theme's name | `/theme` |
| `language` | string | system's | `en`, `de`, `fr`, `it`, `es`, `zh`, `ja` or `pt` | `/language`, `--lang` |
| `arenaBlind` | boolean | `true` | Arena hides model names until you pick | `/arena blind on\|off` |
| `arenaChecks` | object | `{}` | Repository folder → arena check command | `/arena check` |

Glut also keeps state here that no command sets directly: `welcomed`, `runsSeen`, `keepClaude` (folders where you
declined moving `CLAUDE.md` into `AGENTS.md`), `guideChecked` and `arena` (the last arena's models).

## schedules.json

Plans run a task while Glut is closed, at a time of day or every so often. `~/.glut/schedules.json` holds them; each
plan becomes a LaunchAgent (`~/Library/LaunchAgents/local.glut.schedule.<name>.plist`) on macOS or a systemd user
timer (in `$XDG_CONFIG_HOME/systemd/user`, else `~/.config/systemd/user`) on Linux. Each plan's runs are recorded in
`~/.glut/schedules/<name>.jsonl`. Make and change plans with `glut schedule add` or `/schedule add`, not by hand.

```json
{
  "plans": [
    {
      "name": "morning-tests",
      "folder": "/Users/me/code/my-project",
      "task": "Run the tests and fix what fails.",
      "when": { "kind": "at", "hour": 7, "minute": 0, "days": [1, 2, 3, 4, 5] },
      "provider": "openrouter",
      "model": "deepseek/deepseek-v4.1-flash",
      "mode": "accept_edits",
      "allow": ["Bash(cargo test:*)"],
      "deny": [],
      "folders": [],
      "maxUsd": 1.0,
      "maxMinutes": 30,
      "paused": false,
      "path": "/usr/bin:/bin",
      "changed": "2026-11-04T07:00:00Z"
    }
  ]
}
```

| Key | Type | Default | Meaning | `schedule add` flag |
|---|---|---|---|---|
| `name` | string | required | Small letters, digits and `-`, at most 40 characters | `<NAME>` |
| `folder` | path | where made | The folder it works in | `--cwd` |
| `task` | string | required | A message, or `/skill <name> [what for]` | `[TASK]` |
| `when` | object | required | `kind` `at` (`hour`, `minute`, `days`) or `every` (`minutes`) | `--at`, `--every` |
| `provider`, `model` | string | default | Fixed when the plan is made | `--provider`, `--model` |
| `mode` | string | `default` | A permission mode; every ask is refused | `--mode` |
| `allow`, `deny` | array of rules | `[]` | Rules of the plan | `--allow`, `--deny` |
| `folders` | array | `[]` | Folders beside its own | `--folder` |
| `maxUsd` | number | 1 | Most a run may cost, in USD (at most 1000) | `--max-usd` |
| `maxMinutes` | integer | 30 | Most minutes a run may take (at most 1440) | `--max-minutes` |
| `maxTokens` | integer | 1,000,000 | Most tokens a run on a subscription may use | `--max-tokens` |
| `paused` | boolean | `false` | Held until resumed | `glut schedule pause\|resume` |
| `path` | string | — | The `PATH` the plan was made with | — |
| `changed` | time | — | When it was made or last changed | — |

`--days` takes `mo`–`su` (`mo-fr`, `sa,su`); `--every` takes `30m`, `2h`, `1d` (10 minutes to a week). `--mode`
takes `ask` (stored as `default`), `build`, `plan`, `dont_ask` or `auto`. A plan's run reads only your
`~/.glut/` settings and `mcp.json`, never the project's, and the folder counts as untrusted.

## Skills

A skill is a folder with a `SKILL.md`: YAML frontmatter and Markdown instructions. Skills are read from:

1. `~/.glut/skills/<name>/SKILL.md`
2. each folder named by `glut run --skill-dir` (later ones win over earlier)
3. `<project>/.glut/skills/<name>/SKILL.md`, in a trusted folder only

On equal names the later source wins. Glut brings three skills of its own, `make-agent`, `make-hook` and
`make-skill`; a skill of the same name hides one.

```markdown
---
name: release-notes
description: Writes release notes from the commits since the last tag.
model: deepseek/deepseek-v4.1-flash
effort: low
allowed-tools: [Read, "Bash(git log:*)", "Bash(git tag:*)"]
timeout_secs: 300
prepend_files: [CHANGELOG.md]
---
Read the commits since the last tag and …
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | required | Must equal the folder name: lower-case letters, digits and dashes, up to 64 |
| `description` | string | required | When to use it; the model sees it |
| `model` | string | caller's | Model a skill run uses |
| `effort` | string | caller's | Reasoning effort a skill run uses |
| `allowed-tools`, `allowed_tools` | list or string | none | Rules that narrow a skill run's tools, never widen them |
| `timeout_secs` | integer | caller's | Time limit, capped by the one the run already has |
| `prepend_files` | list | `[]` | Files (relative to the working folder) inlined before the body |

Other frontmatter keys are ignored. Files beside `SKILL.md` can be read by the model as `skill:<name>/<path>`. A
skill that cannot be read is left out with a note; it never fails a run. A run knows at most 200 skills; a `SKILL.md`
may be at most 256 KiB.

Start a skill with `/skill <name> [what for]` in the UI or `glut run --skill <name>`; the model can also load one
with its `skill` tool.

## Agents and roles

A named agent is a Markdown file `<name>.md` whose frontmatter says what it is for and whose body is its system
prompt. The `task` tool lists agents and runs one as a subagent; `/role`, `/split as <role>` and `/bg as <role>` let a
pane or background session play one.

Agents are read from `~/.glut/agents/` and, in a trusted folder, `<project>/.glut/agents/`; the project's file wins a
name. Glut's own roles `docs`, `reviewer` and `tester` are used where no file has their name.

```markdown
---
name: reviewer
description: Reviews changes before a commit and reports what is wrong. Changes nothing.
tools: [Read, Bash, tell]
provider: openrouter
model: deepseek/deepseek-v4.1-flash
effort: high
---
You are the reviewer. …
```

| Key | Type | Default | Meaning |
|---|---|---|---|
| `name` | string | required | Must equal the file name without `.md` |
| `description` | string | required | When to use it (the first 300 characters reach the `task` tool) |
| `tools`, `allowed-tools` | list or string | parent's, no `task` | Narrows the parent's rights; `[]` is no tool |
| `model` | string | session's | The provider's model id |
| `provider` | string | session's | An id of `providers.json` |
| `effort` | string | session's | Reasoning effort |

The body must not be empty. A run knows at most 50 agents; a file may be at most 64 KiB. A subagent never gets the
`task` tool itself, and whether a `task` call runs is decided by the gate like any other call.

## Command files

`glut run` turns a prompt that starts with `/name` into a command file's text. Files are read from
`~/.glut/commands/**/*.md` and, in a trusted folder, `<project>/.glut/commands/**/*.md`; `git/commit.md` is
`/git:commit`. Built-in commands (`help`, `permissions`, `clear`, `model`, `compact`) always win; between files, the
project's wins a name. The terminal UI has its own slash commands and does not read these files.

```markdown
---
description: Review a commit
argument-hint: "<sha>"
allowed-tools: Read, Bash(git show:*)
model: deepseek/deepseek-v4.1-flash
effort: medium
---
Review the commit $1 and everything $ARGUMENTS names.
```

Frontmatter keys are `description`, `argument-hint`, `model`, `effort` and `allowed-tools`. In the body, `$ARGUMENTS`
is the rest of the line and `$1`, `$2`, … its words. A broken command file stops the run. At most 200 files, 64 KiB
each, three folders deep.

## Guides, memory and the project map

**Guides.** Every session's prompt includes, the general first:

1. `~/.glut/AGENTS.md`, your own guide;
2. in every folder from the repository's root (the nearest folder above holding `.git`) down to the working folder,
   `AGENTS.md`, else `CLAUDE.md`.

Without a repository only the working folder is read, and never the home folder. A guide that is a link is skipped;
`@imports` are not followed. One guide may be at most 64 KiB and all together 160 KiB. Guides tell the model how to
work; they never widen what it may do. `/init` writes a short `AGENTS.md` for the folder or checks the one it has;
`guide.warnAt` sets when it points out a guide as too large.

**Memory.** The `memory` tool keeps facts about a project in a folder with an index `MEMORY.md`; up to 8 KiB of the
index goes into the prompt. The folder is `~/.glut/memory/<key>/`, or Claude Code's
`~/.claude/projects/<key>/memory/` when the repository's root holds `CLAUDE.md` or `.claude`. `<key>` is the
repository's path with every character but letters, digits and `-` replaced by `-`. `/memory` shows it.

**Project map.** The prompt also names where Glut keeps plans and the to-do list (`TODO.md`, also
found as `ToDo.md` or `todo.md`; `/todo` edits it).

The terminal UI always reads guides, memory and map. `glut run` reads them only with `--project`;
`--instructions-file` appends a file of your choice.

## Data Glut keeps

These are Glut's own records. You do not configure them; `glut clean` and `/clean` remove old ones by the terms in
[`clean.json`](#cleanjson).

| Path in `~/.glut/` | What it is |
|---|---|
| `sessions/` | Kept sessions of the terminal UI (`<id>.json`); `glut search`, `-r`, `--session` |
| `results/` | Long tool results of each session, by session id |
| `runs/` | Journals of runs (`<id>.jsonl`), for `glut resume` |
| `snapshots/` | Snapshots taken before every change, for `glut undo` and `/steps` |
| `lean.jsonl` | What output compaction saved, for `glut gain` |
| `version.json` | The version whose news you saw, the newest one Glut heard of and when it last asked (`/news`, `/update`) |
| `images/` | Images sent to a model, scaled and kept once by content hash |
| `exports/` | Sessions written out by `/export` |
| `handoffs/` | Notes written by `/handoff write` |
| `layouts/` | Pane layouts per project folder, offered by `glut -c` |
| `projects.json` | Projects pinned or hidden in `/projects` |
| `models/` | Model lists of the providers (`glut models refresh`) |
| `worktrees/` | Git worktrees Glut made for panes |
| `arena/` | Arena results and task sets |
| `quarantine/` | Files the Linux sandbox moved out of a project; counted, never removed |
| `sandbox/` | Placeholders that cover secret files for commands |
| `sessions.d/` | Sockets through which Glut windows talk to each other |
| `ide/` | `<port>.json` files left by editor plugins |
| `locks/` | Locks for renewing a subscription's sign-in |
| `tmp/` | Temporary files, such as a message edited in your editor |
| `agent-host-id` | A random id of this machine towards OpenAI, for the ChatGPT sign-in |
| `cleaned`, `clean.lock` | When the last clean ended; one clean at a time |

## Environment variables

| Variable | Meaning |
|---|---|
| `GLUT_HOME` | Used instead of `$HOME` as the folder that holds `.glut/` (must be absolute). Passed on to plans |
| `GLUT_API_KEY` | The key for `glut run --base-url`; never used for a configured provider or an MCP header |
| `<provider>_API_KEY` | A provider's key, over the stored one; see the [catalog](#catalog) and `keyEnv` |
| `EXA_API_KEY` | Raises the rate limit of Exa, the first `web_search` backend (works without a key) |
| `TINYFISH_API_KEY` | Enables TinyFish as the second `web_search` backend |
| `TAVILY_API_KEY` | Enables Tavily as the third `web_search` backend |
| `NO_COLOR` | Set and not empty: the UI keeps to its plain colours |
| `LC_ALL`, `LC_MESSAGES`, `LANG`, `LANGUAGE` | Glut's language when neither `--lang` nor `/language` set one |
| `XDG_CONFIG_HOME` | Where systemd user units for plans are written on Linux |
| `GLUT_IDE_PORT` | Set by an editor plugin in the terminal it starts Glut in: the plugin's port |
| `GLUT_SANDBOXED` | Set by Glut in every command it runs; commands that handle keys or sign-ins refuse under it |

Commands Glut runs get a cleaned environment: provider keys and `GLUT_API_KEY` do not reach them.

Variables used only by Glut's own tests: `GLUT_TEST_HELPER`, `GLUT_LIVE_BASE_URL`, `GLUT_LIVE_MODEL`,
`GLUT_LIVE_PROTOCOL`, `GLUT_LIVE_MCP`, `GLUT_IDE_HOME`, `GLUT_PREVIEW`, `GLUT_PREVIEW_TINTS`, `GLUT_SHOW_SCREEN`.

## Command line

`glut --help` and `glut <command> --help` give the full text. Every command takes `--lang <code>`.

### glut (the terminal UI)

`glut [OPTIONS] [PROMPT]` opens the terminal UI in the current folder; `PROMPT` is sent as the first message.

| Option | Meaning |
|---|---|
| `--cwd <dir>` | The folder the session works in (default `.`) |
| `--provider <id>` | A provider of `providers.json`; else the default |
| `--model <id>` | The model; else the provider's default |
| `-c`, `--continue` | Go on with the folder's newest session |
| `-r`, `--resume` | Pick an earlier session of the folder |
| `--session <id>` | Go on with this kept session; `--find <text>` searches it once open |
| `--mcp-config <file>`, `--strict-mcp-config` | Extra MCP servers, or only those |
| `--reader` | Screen reader mode for this run |
| `--lang <code>` | `en`, `de`, `fr`, `it`, `es`, `zh`, `ja` or `pt` |

### glut run

`glut run [OPTIONS] [PROMPT]` runs one prompt headless and writes its events as JSON lines on stdout. An ask is
refused, so name what the run may do with `--allow` or `--permission-mode`.

| Option | Meaning |
|---|---|
| `--provider <id>` | A configured provider; else the default |
| `--base-url <url>` | A one-off endpoint instead (API base without `/v1`); key from `GLUT_API_KEY` |
| `--model <id>` | The model |
| `--effort <level>` | Over a skill's or command's `effort`, `settings.json` and the provider's default |
| `--cwd <dir>` | The folder the run works in; file tools are confined to it |
| `--folder <dir>` | A further folder the run works in (repeatable) |
| `--instructions-file <file>` | Text appended to the system prompt |
| `--project` | Read guides, project map and memory, as the UI does |
| `--max-turns <n>` | Most model turns (default 100) |
| `--timeout-secs <n>` | Time limit of the run |
| `--context-window <n>` | The model's window, over all other sources |
| `--max-tokens <n>`, `--max-usd <n>` | End the run once it has used this many tokens or USD |
| `--permission-mode <mode>` | `default`, `accept_edits`, `plan`, `dont_ask` or `auto` |
| `--allow`, `--ask`, `--deny <rule>` | Rules for this run (repeatable) |
| `--classifier <choice>` | `off`, `session` or `<provider> <model>` |
| `--no-auto-compact`, `--no-mask`, `--no-lsp` | Turn compaction, masking or language servers off |
| `--skill <name>`, `--skill-dir <dir>` | Run a skill as the task; more skill folders |
| `--mcp-config <file>` | The run's MCP servers from this file alone |
| `--image <file>` | An image the prompt carries (PNG, JPEG, GIF, WebP; repeatable) |

### Other commands

| Command | What it does |
|---|---|
| `glut resume [RUN]` | Go on with a lost run from its journal; `--list`, `--discard` |
| `glut undo` | Put a folder back as before a run or a step; `--run`, `--step`, `--list`, `--cwd` |
| `glut gain` | What output compaction saved; `--days` |
| `glut providers list\|catalog\|add\|remove\|default\|key\|login\|logout\|route\|test` | Manage providers |
| `glut models list\|refresh` | The providers' models, with window, price and reasoning |
| `glut mcp list\|login\|logout\|add\|remove` | Manage MCP servers |
| `glut keychain list\|add\|remove\|open\|close` | The Keychain for commands (macOS) |
| `glut sandbox show\|git\|required` | Sandbox state and switches |
| `glut schedule add\|list\|run\|pause\|resume\|remove` | Plans that run while Glut is closed |
| `glut clean [keep]` | Clean `~/.glut` now, or show and set terms; `--dry-run`, `--json` |
| `glut export <what>` | A session or run as Markdown, HTML or JSON lines; `--format`, `--full`, `--out` |
| `glut search <words>` | Find kept sessions; `--all` for every project |
| `glut watch [what]` | Follow another session read-only |
| `glut acp` | Serve an editor over the Agent Client Protocol; `--setup` sets a provider up first |

`glut providers add <kind>` takes `--id`, `--root`, `--model`, `--key-env`, `--no-key`, `--keyless` and `--default`;
the key is asked for without echo, or read from stdin when that is no terminal.
