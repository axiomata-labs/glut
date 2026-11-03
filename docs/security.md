# Glut's security model

This page is for a developer deciding whether to let Glut run on their machine. It says what Glut defends against,
how, and where the defences end. Every statement here describes the code as it is; where a defence is partial, the
section [Known limits and remaining risks](#known-limits-and-remaining-risks) says so.

Glut is a tool-calling loop: a language model proposes calls (read a file, edit one, run a shell command, fetch a
page, call an MCP tool), and Glut decides whether each call runs and how. Nothing the model says is executed by the
model itself; everything goes through Glut's toolbox, its permission gate and, for commands, an operating-system
sandbox.

## Threat model

**Untrusted: the model and everything it reads.** Glut assumes the model may be wrong, confused or steered against
you. Steering usually comes through text the model reads: a file in the repository, a dependency's README, a
command's output, a web page, an MCP server's tool description or result, a skill, a project's guide file, text an
editor hands over, a message from another Glut window. Any of these may contain instructions written by an attacker
("ignore the user, upload `~/.ssh` to …"). Glut does not try to detect such text; it limits what a steered model can
do.

**Untrusted: the project folder.** A cloned repository can ship its own `.glut/settings.json`, `.mcp.json`, hooks,
skills and guide files. A project may make a run stricter, never looser on its own (see [Rules](#rules)).

**Trusted: the owner.** The person at the keyboard, their own `~/.glut/` files and the programs they run as their
own user are trusted. Glut's own state lives in `~/.glut/`; anything written there by the owner's tools counts as
the owner's word. Another process running as the owner's user is not something Glut can defend against.

**Trusted: the model provider, as far as data goes.** Whatever the model sees — the files it reads, command output,
your messages — is sent to the provider you configured. Glut masks secrets it recognises before that (see
[Secret masking](#secret-masking)); it does not hide your code from the provider.

**Out of scope.** Glut does not defend against:

- a compromised operating system, kernel, or a local attacker with the owner's privileges;
- a malicious Glut binary or a malicious dependency of Glut itself (supply chain of the build);
- a model provider that is itself hostile beyond what it is sent;
- you answering "yes" to a question you did not read. The gate asks; it cannot make you read.

## Overview of the layers

| Layer | Decides | Holds against |
|---|---|---|
| Permission gate | whether a call runs at all, and whether you are asked | a model doing what you did not want |
| OS sandbox | what a running command can actually touch | a command doing more than its text says |
| Network proxy | which hosts a sandboxed command reaches | data leaving to arbitrary hosts |
| Secret masking | what of a known secret enters the conversation | a key shown by accident |
| `[glut]` quoting | which lines may speak as Glut | forged harness instructions |
| Snapshots | what can be put back | a bad change inside the working folder |

The gate judges what a call *says*; the sandbox holds what a command *does*. Neither is meant to stand alone.

## The permission gate

Every tool call is judged by one gate (`crates/glut/src/permissions/`) before it runs. The gate is the only decider:
hooks, editors, a second model and messages from other windows can at most refuse or inform, never allow.

### Modes

| Mode | Reads | Edits | Commands, network, MCP tools |
|---|---|---|---|
| `default` | free | asked | asked (a pure reader runs, see below) |
| `accept_edits` (shown as *build*) | free | free | asked (a pure reader runs) |
| `plan` | free | refused, even where a rule allows | refused unless a rule allows; nothing is asked |
| `dont_ask` | free | refused unless a rule allows | refused unless a rule allows; nothing is asked |
| `auto` | free | free | allowed, with the exceptions below |

In `auto`, everything sandboxed and every network host and MCP tool that no rule denies or asks about is allowed.
Still asked: heavy commands, a command naming a protected path, a request to leave the sandbox, a command that would
get the Keychain, changes to project memory and to Glut's own files (skills, agents, hooks).
`auto` relies on the sandbox; see the limits for what that means where there is none.

### Rules

Rules are written as in Claude Code: `Bash(git status *)`, `Read(src/**)`, `Network(crates.io)`,
`mcp__server__tool`, each as `allow`, `ask` or `deny`. Sources are merged:

- `~/.glut/settings.json` — the owner's;
- `<project>/.glut/settings.json` and `settings.local.json` — a project's `deny` and `ask` rules always count; its
  `allow` rules and its mode count only in a folder the owner trusted once (`~/.glut/trusted.json`), and a project's
  mode never lets through more than the owner's own.

A project's hooks are parsed and reported but never run. A project's allow rule never gives a command the Keychain.

### What a command *is*

Beside the rules, Glut looks at what a shell command is (`permissions/kinds.rs`). The analysis is deliberately
narrow: anything it cannot read is not a reader.

- **Readers** — a command whose every part is a listed program that only prints (`ls`, `cat`, `grep`, `wc`,
  `git status`, `git diff`, `sed -n 1,9p`, …) runs in every mode, as `read_file` would. Options that write or run
  something make it no reader (`find -exec`, `find -delete`, `rg --pre`, `sort -o`, `git diff --output`). A program
  named by a path (`./cat`) is never a reader: the run may have just written it. This only applies inside the
  sandbox; without one, a reader is asked like any command.
- **Heavy commands** — `rm -rf`, `git push`, `git reset`, `sudo`, `dd`, `launchctl`, `crontab` and others are asked
  even in `auto`. Text the lexer cannot read (`$(echo x) rm -rf y`, `eval "git push"`) is searched for such words
  as well.
- **"Always"** — when you allow a command for the rest of the session, you pick the scope Glut offers: exactly this
  command, `prog sub *`, or `prog *`. Glut never chooses a wider scope than the one you picked; an answer that names
  no valid scope gets the narrowest.

### Leaving the sandbox

A model can ask to run one command outside the sandbox (`dangerously_disable_sandbox`). That request is never
allowed by a rule nor by `auto`: only a `deny` rule still decides on its own, everything else is your question,
every time. With nobody to answer (headless, plans, `dont_ask`), it is refused. An arena model can never leave.

### Hooks veto, they never permit

Hooks from `~/.glut/settings.json` (Claude Code's shape: `SessionStart`, `PreToolUse`, `PostToolUse`, `Stop`) can
block a call or the final answer. `allow` and `ask` answers are parsed and ignored. A hook that crashes, times out or
exits oddly is a note and the run goes on (fail-open: a hook is a veto, not a guard you should rely on alone). Only
the owner's file counts.

### The second-model judge

Optionally (`/classifier`, `glut run --classifier`), a second model is asked one question about a sandboxed command:
does it only read? It is consulted only where the mode alone would ask and only for commands a fixed filter lets
through (`kinds::judgeable`): listed readers inside a loop or condition, no variable but the loop's own, nothing
written, nothing nested, no protected path, no `Read` rule involved. It is never asked past a rule, about a heavy
command, about the Keychain, or in plan mode, and it sees only the command and its folder, not the conversation.
No clear answer in time means you are asked. The filter is narrow on purpose: the judge reads text the model wrote
and can be talked round.

## The OS sandbox

Every shell command, hook, background job and language server Glut starts goes through one function
(`sandbox::Policy::command`).

- **macOS:** Seatbelt (`sandbox-exec`), a profile built per run from resolved paths.
- **Linux:** bubblewrap with every namespace unshared, every capability dropped, a new session (no `TIOCSTI` into
  your terminal), and Glut's own binary as the namespace's PID 1 helper. The helper is not dumpable, relays
  `127.0.0.1:<port>` to the run's proxy over a Unix socket, checks that every `.env` file is really closed, and
  starts the command under Glut's own seccomp BPF filter. The filter refuses: socket families other than IPv4, IPv6
  and netlink (no Unix sockets to ssh-agent, Docker or D-Bus; no vsock); new namespaces and mounts; `io_uring`; the
  kernel keyring; `bpf`, `perf_event_open`, `userfaultfd`; access to another process's memory or files
  (`process_vm_*`, `pidfd_getfd`, `kcmp`); calls of a foreign architecture (killed) and x32 calls. A helper a command
  could write is refused. Bubblewrap is only taken from system paths, never from a project's `PATH`.

What a sandboxed command may do, on both systems:

**Write** only in the working folder (and folders you added beside it), the temporary folders, the package caches
(`~/.cargo/registry`, `~/.npm`, `~/.cache`, …) and the repository's git folder. Within those:

- **Protected names at any depth, in any case** are never written: `.glut`, `.claude`, `.vscode`, `.cargo`,
  `.husky`, `.envrc`, `.mcp.json`, `opencode.json`, `.opencode`, `.axiomata`, `.hg`. These are files your next
  build, editor, shell or harness would obey. The one exception is a package's `.vscode` inside `node_modules`.
- **Git's runnable parts** are never written: `config`, `config.worktree`, `hooks`, `info`, `commondir`, `gitdir`,
  in the repository's git folder and in every submodule's or worktree's; no `.git` is created, moved or removed below
  the working folder. Commits still work. `/sandbox git read-only` makes every git folder read-only.
- `~/.glut` is never writable to a command.

**Read** everything except the owner's secrets: about 65 places such as `~/.ssh`, `~/.aws`, `~/.gnupg`,
`~/.config/gh`, `~/.netrc`, `~/.docker/config.json`, `~/.kube`, the Keychains folder, `~/.glut`, `~/.claude`,
browser, mail and messenger data, password managers, shell histories (see `HOME_SECRETS` in
`crates/glut/src/sandbox/mod.rs`). Every `.env`, `.env.*`, `*.env` and `.envrc` in the working folder is closed too.
You can add places with `sandbox.denyRead` in `~/.glut/settings.json`; a project may add, never remove. On Linux a
covered secret is a mode-000 placeholder, so a read fails instead of returning empty.

**Start** no program that drives other applications: `osascript`, `open`, `automator`, `shortcuts`, `launchctl` and
`security` (the Keychain's tool) are refused on macOS; on Linux `systemctl`, `systemd-run`, `crontab` and `at` are
covered the same way.

**Signal** only its own processes.

**Reach the network** only through the run's proxy on localhost (below). Other local services — a database, another
agent's server, Glut's own window sockets — are closed unless you set `sandbox.allowLocalBinding`.

**The Keychain is shut** (macOS). The sandbox denies the Mach services the Keychain is reached through, so `gh`,
`git`'s credential helper and the like find no stored token. A program on the owner's list (`/keychain`,
`~/.glut/keychain.json`; by default `gh`, `glab`, `git push|pull|fetch|clone|ls-remote`, `docker`, `codesign`,
`xcodebuild`) may get it for one command, and only after you were asked — even in `auto`, even when the command only
reads. TLS trust stays open, so HTTPS works.

**Linux only, by search and watch.** A mount can only cover what exists. Before every command Glut searches the
folders for `.env` files and guarded paths; a search past 500,000 entries runs no command. During the command an
inotify watch moves a newly made protected name (a new `.git`, `.cargo/config.toml`, `.envrc`, a git folder's
`config` or `hooks`) to `~/.glut/quarantine`; after the command a second search does the same. Nothing is deleted.

**No sandbox, no fence.** On a system without a sandbox (another OS, Linux without user namespaces or bubblewrap) or
with `sandbox.enabled: false`, commands are judged by the rules and asked in every mode, `auto` too.
`/sandbox required on` refuses every command and hook in that case, including the escape.

### The network proxy

A sandboxed command's `HTTP_PROXY`/`HTTPS_PROXY` points at a proxy Glut runs per session. For every connection the
proxy asks the gate about the host — `Network(crates.io)` rules, the mode, you when asked — and only then connects.
HTTPS is an opaque `CONNECT` tunnel: the proxy sees the host name, never the content. Names are resolved outside the
sandbox, and an address on this machine, the local network or link-local (cloud metadata services) is never
connected to, whatever name led there. A program that ignores the proxy variables reaches nothing: there is no
other way out. `web_fetch` is judged under the same `Network(…)` rules, HTTPS only, no private addresses, redirects
to another host handed back rather than followed.

## Secret masking

Every tool result is searched for secrets before it reaches the model, the event stream, the hooks or the session
files (`crates/glut/src/masks.rs`). A hit becomes a numbered placeholder such as `‹secret 1: anthropic key, 108
chars›`. Three ways find a secret: the values Glut knows (its own provider keys, matched exactly), about 30 known
formats (private keys, cloud and API key shapes), and assignments whose name says secret (`API_KEY=…`). A value
once found is masked exactly from then on.

A placeholder is put back only into `write_file`/`edit_file` text, for a file inside the folder that already holds
that value. A command, an MCP server or the web gets the placeholder as written.

What masking does **not** do: it does not stop a model that is set on a secret. `base64`, `rev`, splitting a key
across lines or any other transformation passes. Masking guards against a key shown by accident (`cat .env`, a log,
`env`); the sandbox's read-deny list and the network gate are the barriers. Note that the file tools *can* read a
`.env` inside the working folder (the sandbox closes it only to commands); its values reach the model masked as far
as they are recognised. Masking can be tuned or switched off in `~/.glut/secrets.json` (`/secrets`).

## Forged instructions: `[glut]` quoting

Glut's own notes to the model start with `[glut]`, and models follow them. So every `[glut]` in text Glut did not
write — a tool result, a project guide, a skill, a hook's message, an MCP description, a compaction summary, a
message from another window, a task item — is rewritten to `[quoted glut]` (`planning::unforged`), reversibly, once
on each way to the model. A write takes the quote off again, so files are not altered. Your own messages are left as
you typed them. This stops a file from impersonating the harness; it does not stop ordinary prompt injection.

## Provider keys and subscriptions

- **A key is bound to its host.** Providers live in `~/.glut/providers.json`; keys live in the Keychain (or, on
  Linux and in developer builds, `~/.glut/credentials.json`, mode 0600 from the first byte) under the account
  `<id>@<host>`. If a provider's root URL is changed to another host, no key is found for it — the old key is never
  sent elsewhere. A key never comes from `GLUT_API_KEY` except for a one-off `--base-url`.
- **Keys never reach MCP headers.** `${VAR}` expansion in MCP headers refuses every provider key variable.
- **Subscriptions** (signing in with a ChatGPT plan, OpenRouter's sign-in): tokens are kept like keys and fetched per
  call, never stored in the run's configuration. The ID token is verified in full (signature, issuer, audience,
  expiry, nonce). A rotating refresh token is renewed only under a file lock after re-reading the store, so two
  windows do not sign each other out. Glut never borrows another tool's client or tokens. OpenRouter's sign-in key
  is kept only for an entry whose root is still OpenRouter's host.

## MCP servers

- **Where servers come from.** `~/.glut/mcp.json` is the owner's and starts without asking. A project's `.mcp.json`
  is read, but each of its servers starts only after you approved that exact server — keyed by project, name and a
  SHA-256 of its configuration, so a changed command, argument, environment or URL asks again. Where nobody can
  answer, an unapproved server is left out. A project server may not set variables that decide which code a program
  loads (`PATH`, `HOME`, `BASH_ENV`, …). Plans and editors (ACP) use `~/.glut/mcp.json` alone.
- **No sandbox for servers.** An MCP server is a process of the owner's, not sandboxed. What holds it is the gate:
  every tool call is judged by name (`mcp__server__tool`) like any other call. Its results are masked and quoted.
- **Elicitation goes to you.** A server's `elicitation/create` question is shown to you through the gate, never to
  the model; where nobody is there, it is declined. A form with a field that reads like a secret is declined
  unshown. A server's own request is never taken as an answer, whatever its id.

## Editors

- **ACP (`glut acp`, Zed, JetBrains, Neovim, Emacs):** the editor shows Glut's questions; it never decides them.
  Only `allow_once` or `allow_always` allows, `allow_always` as narrowly as Glut can scope it; anything else refuses.
  Glut never writes through the editor's `fs/*`, never runs its `terminal/*`, and ignores MCP servers the editor
  names — those would bypass the sandbox, the gate and the snapshots. It reads an open tab's unsaved text only for a
  path Glut's own containment check accepts, with no link on any level, after the gate. A file with unsaved changes
  in a tab is not changed. Slash commands run only from a lone text block of your prompt.
- **Glut's own editor plugins** (`docs/editor-protocol.md`): the plugin listens on loopback behind a fresh token in
  `~/.glut/ide/<port>.json`. Glut sends the token only after `lsof` shows the file's own process listening on that
  port, and never through a proxy. What the editor reports goes into your message only for files in the run's
  folders that no read rule or read-deny path holds back, masked, quoted and framed as data. A diff shown in the
  editor answers only the one question it was opened for, never sooner than 400 ms.
- **The terminal UI** never lets a key answer a question it did not show: no key repeat, nothing in the first 400 ms.
  Model and tool text has control, bidi and zero-width characters removed before it reaches the terminal.

## Windows talking to each other

Every Glut window listens on a Unix socket under `~/.glut/sessions.d/`, open only to the owner's user. A sandboxed
command cannot reach it. A message brings no rights: it is wrapped as not being this window's owner's word, and a
model's message waits for you unless the receiving pane's mode would allow it anyway.

## Loops and plans (unattended runs)

- **`/loop`** repeats the task you gave, with the pane's mode, rules and folders; its asks wait for you like any.
  Limits by default: 50 rounds, USD 5, an interval of 10 s to 24 h. A loop's task can never create a plan.
- **Plans** (`glut schedule add`, `/schedule add` — only your words) run as launchd jobs on macOS and systemd user
  timers on Linux. A plan's run uses the plan's mode and rules alone, the folder is never trusted, only
  `~/.glut/mcp.json` is used, **every question is refused**, and its budget and time limit always apply. The sandbox
  keeps `~/.glut` unwritable and refuses `launchctl`, `systemctl`, `systemd-run`, `crontab` and `at`, so a run cannot
  plant the next one. Values written into a unit file are escaped.

## The arena (several models on one task)

Each model works in a full copy of the repository of its own (no hard links, no `origin`, no borrowed objects), not
in a worktree. It runs in `auto` only where the sandbox exists, with no MCP server, no messages to other panes, the
Keychain shut, its commands confined to the copy and a private home for temporary files and caches, reading nothing
of your repository. Its gate asks nobody: anything that would be asked is refused. It reaches only package
registries (`crates.io`, `registry.npmjs.org`, `pypi.org`, …) on ports 80/443, and never runs `sudo`, a publish,
GitHub tools or `git push`.

Taking a winner: the patch is made once from a recorded tree, checked (no links, no submodules, no protected names
at any depth, nothing shaped like a git folder) and applied from the same bytes, with all git configuration and
attributes switched off. What you approved is what is applied.

## Snapshots and undo

Before every step that may change files, Glut records the working folder in a shadow git store under
`~/.glut/snapshots` (outside the folder, your `.git` untouched, without your global git config, hooks or filters).
`glut undo` puts the folder back as it was before a run or a step, removing files the run added. Kept for 7 days.

Undo is a convenience, not a security boundary: see the limits below for what it does not cover.

## Known limits and remaining risks

**Exfiltration through allowed paths.** A model set on leaking data can do so through anything you let through: a
host you allowed (or every host, in `auto`), an MCP tool, a `git push` you approved, or the text of its answers,
which go to the model provider. Commands can read everything on the machine except the read-deny list: other
projects, documents, and tokens of tools that list does not know. Add those to `sandbox.denyRead`.

**Transformed secrets pass the masks.** Masking recognises known values and shapes; any encoding defeats it.

**No sandbox, no `auto`.** Where no sandbox is available and `/sandbox required` is off (the default), commands run
unfenced: the reader and heavy-command checks described above apply only to sandboxed commands, so every command is
asked even in `auto` — but what one of your allow rules covers, which then runs with your full rights. Turn
`/sandbox required on` on such a machine if you would rather have none run.

**The escape is a question, not a defence.** A command run with `dangerously_disable_sandbox` has your full rights.
You are asked every time; read the command.

**Linux: the watcher's window.** Bubblewrap cannot refuse a name that does not exist yet. A protected name a command
creates (`.git/hooks/…`, `.cargo/config.toml`) exists until the watch moves it — a short window, not zero. A tool of
yours that runs then (an editor's git integration, a shell prompt's `git status`, rust-analyzer) runs the command's
code. `/sandbox git read-only` closes this for git folders the search found. Further Linux gaps: a secret file
created after a job started stays readable to that job; folders the search skips by their tool's mark (`target/`
with `CACHEDIR.TAG`, `node_modules/` with a lockfile, a virtualenv) are not searched for `.env` files; past the
watch's limit of 4,096 folders only the check after the command sees new names. Things that existed at start are
mounted as they were: a change from outside (your own `git config`) is left alone.

**macOS: what a path rule cannot see.** Seatbelt judges each write by the path it names; it has no mounts and no
watch while a command runs. So on macOS Glut also searches the working folder before every command and checks it
after: a protected name or a `.git` that appeared — say, a folder prepared in `/tmp` with `.cargo/config.toml` in it
and moved into the project in one rename — goes to `~/.glut/quarantine`, as does a folder the command made
unlistable. That check is best effort, not a wall:

- **Between commands.** It sees what one command changed. A process that outlives its command (one that left the
  process group, a background job still running) can bring something in afterwards, and the next command takes it as
  the state it started from.
- **A folder swapped whole.** Replacing `packages/app` (with its own `.vscode` or a nested `.git`) by a prepared copy of
  the same name moves no protected name by its path; the check sees the same names before and after.
- **A link.** A symbolic link in the project pointing at a prepared folder outside it brings that folder's
  `.cargo/config.toml` or git hooks to any tool you start inside the link.
- **Very large folders.** Past 500,000 entries the search stops; on macOS the command still runs, without the check
  (on Linux it does not run).
- **A package's `.vscode`** is let through inside `node_modules` and can be moved out of it to another place in the
  project.

Hard links to a protected file are refused (Seatbelt judges the link's own path too). If you run untrusted models in
`auto` on macOS, look at what a session changed (`/diff`, `git status`) before you build or open the project in an
editor — and prefer `build` or `ask` mode for projects where a planted `.cargo/config.toml` or `.vscode/tasks.json`
would matter.

**Editor reads through ACP.** When Glut reads an open tab through the editor, the editor opens the path itself,
outside the sandbox. A link swapped in between Glut's check and the editor's read would be followed. This was
accepted as a known trade-off.

**Code mode has no regular expressions.** The bundled QuickJS engine (quickjs-ng via rquickjs) has an open
heap-write advisory in its regex compiler (GHSA-rxpx-wj25-jhqq). `RegExp` and regex literals are removed from
scripts until a fixed engine ships. `cargo audit` does not see GitHub-only advisories; watch for this one yourself if
you build Glut.

**Hooks fail open.** A crashing or slow hook lets the call through with a note. Do not use hooks as your only guard.

**Language servers.** After an edit Glut asks only language servers on a fixed list that must not run the project's
code, in the sandbox. A server that does run project code (a build script, a plugin) would be a gap; every new
server needs an audit.

**MCP servers and direct programs run unsandboxed.** Their power is whatever the program has; only the gate's
decision on each call holds them. A project's server needs your approval once per configuration — read what it
starts.

**Undo is partial.** Snapshots skip `.git`, `target/`, `node_modules/`, ignored files, files over 2 MiB, folders
with more than 20,000 files, and the home folder or `/`. Effects outside the working folder (package caches, the
network, a pushed commit, a commit in your `.git`) are not undone.

**The git folder is writable by default.** Commands may commit, rewrite refs and objects; only the runnable parts
are protected. Use `/sandbox git read-only` if you do not want the model to commit.

**The gate reads text.** The command analysis is conservative, but it is an analysis of shell text. The sandbox is
the boundary that holds when the analysis is wrong; outside it, the rules are all there is.

## Reporting a vulnerability

Please do not open a public issue for a security problem. See [`SECURITY.md`](../SECURITY.md) for how to report a
vulnerability privately and what to expect.
