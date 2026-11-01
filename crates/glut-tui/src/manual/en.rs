//! The commands' manual pages in English (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 65] = [
    (
        "clear",
        "Start a new, empty session in this folder.\n\
         \n\
         /clear — the conversation, the plan's task list, the cost and the meter start from nothing\n\
         \n\
         The session you leave is kept: /resume opens it again. Your model, mode and settings stay as they are.",
    ),
    (
        "resume",
        "Open an earlier session of this folder and go on with it.\n\
         \n\
         /resume — the list of this folder's kept sessions, the newest first; a fork stands under the session it came \
         from\n\
         \n\
         In the list: type to filter, ⏎ opens one, ⌫ deletes one (asked first; its forks stay as sessions of their \
         own), ⌃E writes one out as Markdown into `~/.glut/exports`. A session open in another pane is reached there \
         (⌃P).\n\
         \n\
         /search finds a session by what was said in it.",
    ),
    (
        "search",
        "Find a kept session by its words: /resume's list, filtered by the text in its conversation.\n\
         \n\
         /search <text> — the sessions of this folder whose conversation holds the text\n\
         \n\
         In the list, ⌃A switches between this folder and every project. ⏎ opens a session of this folder; one of \
         another project names its folder — start Glut there to go on with it.",
    ),
    (
        "btw",
        "Ask a side question without disturbing the conversation: the answer comes in a window above the input and \
         never becomes part of the session.\n\
         \n\
         /btw <question> — asks about this pane's session, even while its turn goes on\n\
         /btw <name> <question> — asks about another pane or a background agent, by its number or name\n\
         /btw — opens the last answer again\n\
         \n\
         The model answers from the conversation as it stands and runs no tool: if it would need to look something up, \
         ask it as a message instead. Its cost counts in the session's cost. ⎋ closes the window, PgUp PgDn scroll \
         it.\n\
         \n\
         /btw reviewer how far are you?",
    ),
    (
        "projects",
        "Go on in another project: one of the folders Glut worked in, or any folder by its path.\n\
         \n\
         /projects — the list: ★ pinned, ● this one; ⏎ opens one, p pins or unpins, x hides one until it is worked in \
         again\n\
         /projects <path> — opens that folder (⇥ completes a folder's name)\n\
         \n\
         While a turn or an agent still works, Glut asks before it stops them and switches. The sessions and this \
         window's panes are kept. In a window that a host app opened, the folder is the host's to pick.",
    ),
    (
        "folder",
        "Give this session folders beside the working one, so Glut can work across projects.\n\
         \n\
         /folder — the folders beside the working one\n\
         /folder add <path> — adds one for this session\n\
         /folder remove <path> — takes one away\n\
         \n\
         The file tools reach such a folder by its full path, commands may write there inside the sandbox, the Gate \
         judges it like the working folder and undo puts its files back too. Only you add a folder, never a model, a \
         Skill or a project. Glut refuses the home folder, a folder that holds the project or lies inside it, one with \
         secrets, and two folders inside each other.\n\
         \n\
         The folders hold for this session only; `glut run --folder <path>` gives one to a headless run.",
    ),
    (
        "compact",
        "Summarise the conversation so far, so it takes less of the model's window.\n\
         \n\
         /compact — the model writes a summary that takes the place of the older messages\n\
         \n\
         The system prompt, your latest message and whole steps stay; what the summary replaces is kept in the \
         session's result store, never thrown away. Glut also compacts by itself shortly before the window is full \
         (`compact` in `~/.glut/settings.json`). /context shows how full the window is.",
    ),
    (
        "handoff",
        "Hand the work over to a later session of this folder.\n\
         \n\
         /handoff write — the model writes what was done, how things stand and what comes next\n\
         /handoff read — takes the folder's handoff into this session\n\
         \n\
         A handoff is kept in `~/.glut/handoffs`, one per folder. Once read it moves aside, so no later session reads \
         it twice. Writing one is a paid call of its own; the conversation stays as it is.",
    ),
    (
        "export",
        "Write the session out to read or share.\n\
         \n\
         /export — Markdown into `~/.glut/exports`\n\
         /export html — one HTML file, its styles inside, no script\n\
         /export jsonl — the messages as JSON lines\n\
         /export copy — the Markdown onto the clipboard instead of a file\n\
         /export full — adds the whole tool results and the system prompt\n\
         /export <format> <path> — into a file of your own (`~/` is your home, a relative path lies in the working \
         folder)\n\
         \n\
         The words go in any order, the path last. Without full the export shows what the model saw: long results \
         shortened. An existing file is never written over. The keys Glut hid stay hidden, but what you typed yourself \
         may still hold one: read an export before you share it. In /resume, ⌃E exports a listed session.",
    ),
    (
        "quit",
        "End Glut, with every pane and background agent.\n\
         \n\
         /quit — while another pane or agent still works, Glut says so first: /quit again ends it too\n\
         \n\
         ⌃C ⌃C does the same, and ⌃D on an empty line while Glut rests. The sessions are kept for /resume.",
    ),
    ("exit", "End Glut, as /quit does."),
    (
        "model",
        "Pick the model this session talks to.\n\
         \n\
         /model — the models of the provider in use, from its kept list\n\
         /model <id> — switches to that model by its id\n\
         \n\
         The conversation goes on with the new model, and the next start begins with it. A model of another provider \
         is picked after /provider. `glut models refresh` renews the lists.",
    ),
    (
        "provider",
        "Switch to another model provider.\n\
         \n\
         /provider — the providers you set up\n\
         /provider <id> — switches to that one, on its default model\n\
         /provider key [id] — changes only that provider's key (the default provider without an id)\n\
         \n\
         Providers are kept in `~/.glut/providers.json`, their keys in the Keychain (or `~/.glut/credentials.json`); \
         `glut providers add <id>` adds one. The conversation goes on, and the next start begins with the provider you \
         picked.\n\
         \n\
         The key is typed hidden and tried against the provider's model list before it replaces the old one; if that \
         fails you choose whether to keep it anyway, and otherwise the old key stays. The session takes a kept key at \
         once. A provider that signs in, needs no key or takes its key from an environment variable is told so. In the \
         shell: `glut providers key <id>`.",
    ),
    (
        "login",
        "Sign in to a subscription instead of a key — today your ChatGPT Plus or Pro plan.\n\
         \n\
         /login — the subscriptions, each with how it stands; ⏎ signs in, or out\n\
         /login <provider> — signs in to that one at once, as /login chatgpt\n\
         /login <provider> <address> — over SSH: the address the browser ended on, pasted back\n\
         /login openrouter — OpenRouter hands Glut a key of its own, kept like one typed; limit or delete it at \
         openrouter.ai/settings/keys\n\
         \n\
         The address is shown first; ⏎ opens it in your browser. Glut keeps the sign-in in the Keychain (or \
         `~/.glut/credentials.json`) and renews it by itself; signing out revokes it at OpenAI.\n\
         \n\
         On the plan a call costs no money, but the plan's usage limits hold: when one is reached the turn ends and \
         Glut points to ChatGPT Settings → Usage, where you also limit what each app may use. In the shell: glut \
         providers login|logout <provider>.",
    ),
    (
        "effort",
        "Set how hard the model thinks before it answers.\n\
         \n\
         /effort — the levels to pick from\n\
         /effort <level> — none, minimal, low, medium, high, xhigh or max\n\
         /effort default — the provider decides\n\
         \n\
         More effort usually means better answers on hard work, slower and at a higher cost. Not every model knows \
         every level. The meter (⌃N) shows the level in use.",
    ),
    (
        "route",
        "Which hosts serve the model on OpenRouter (others serve each model from one host).\n\
         \n\
         /route — the order Glut sends now, each host with why, its price and speed\n\
         /route prefer <hosts> — these first, in this order (xiaomi deepinfra)\n\
         /route avoid <hosts> — these never\n\
         /route quant <levels> — only these quantizations (fp8 bf16)\n\
         /route strict on|off — only the preferred hosts, no other as a fallback\n\
         /route off — this model's pins go\n\
         /route rule on|off — the rule for every model\n\
         /route rule margin|uptime|speed|wait <n> — its numbers\n\
         \n\
         OpenRouter serves one model from many hosts, at different quantizations, prices and speeds. Glut reads a \
         model's hosts once a day and ranks them by its rule: full precision (bf16, fp16) before fp8 unless it costs \
         more than 40 % more, a host that names no quantization between fp8 and fp4 — the model's maker's own as full \
         —, fp4 only where nothing else serves; a host under 95 % uptime, under 30 tokens a second or over 10 seconds \
         to the first token goes to the end. The order goes with every request with fallbacks allowed, so a request \
         never fails for the rule. A model's pins come before the rule. Everything is kept in \
         `~/.glut/providers.json`.",
    ),
    (
        "context",
        "See how full the model's window is, or tell Glut how large it is.\n\
         \n\
         /context — the tokens used, the window and where its size comes from, and what the guides (`AGENTS.md`, \
         `CLAUDE.md`) cost on every turn\n\
         /context <tokens> — sets this model's window: 65536, 64k or 1M\n\
         \n\
         A window you set is kept in `~/.glut/providers.json` for this model and counts before the provider's list. \
         Glut compacts by itself shortly before the window is full.",
    ),
    (
        "thinking",
        "Choose how the model's thinking shows in the stream.\n\
         \n\
         /thinking off — hidden\n\
         /thinking folded — a line per thought; a click opens one\n\
         /thinking open — every thought shown whole\n\
         \n\
         /thinking alone folds them. ⌃T opens or folds every thought at once. Your choice is kept in \
         `~/.glut/prefs.json`.",
    ),
    (
        "panel",
        "Show the side panel on its Plan tab, or hide it.\n\
         \n\
         /panel — the side panel with its tabs: Plan, Gate, Meter, Agents, To-dos, Runs and Keys\n\
         \n\
         ⌃B does the same, ⌃X ⇥ hands the keys to the panel (←→ the tabs, ↑↓ within, ⎋ back). The panel needs a window \
         at least 100 columns wide. Whether it shows is kept for the next start.",
    ),
    (
        "meter",
        "See what the session uses: the context, the tokens and the cost.\n\
         \n\
         /meter — the Meter tab of the side panel\n\
         \n\
         It shows how full the window is and when Glut compacts, the session's tokens in, cached and out with their \
         price, each turn and each tool, and what Glut's lean output spared. ⌃N opens it too.",
    ),
    (
        "agents",
        "See the subagents Glut started in this session.\n\
         \n\
         /agents — the list of subagents, each under the one that started it; ⏎ opens one's own stream\n\
         \n\
         A click on a subagent's tile opens it too. ⌃X A opens the Agents tab: every subagent and every background \
         agent, live — ⏎ opens one, x stops it.",
    ),
    (
        "plan",
        "Read the plan Glut sent from plan mode, whole, with its task list.\n\
         \n\
         /plan — the last plan of this session\n\
         \n\
         In plan mode (⇧⇥) Glut reads and plans without changing anything, then sends its plan for you to approve or \
         send back. /send plan hands it to another pane to build.",
    ),
    (
        "scrollback",
        "Put the conversation into the terminal's own scrollback, to scroll, search and copy it with the terminal's \
         tools.\n\
         \n\
         /scrollback — prints the conversation; ⏎ or ⌃C goes back to Glut\n\
         \n\
         F3 does the same.",
    ),
    (
        "split",
        "Open a new session in a pane beside the main one.\n\
         \n\
         /split — a new pane; in a git repository Glut asks where it works\n\
         /split here — it works in this folder, beside you\n\
         /split tree [branch] — it works in a new Worktree on a branch of its own\n\
         /split resume — it opens an earlier session, picked from the list\n\
         /split as <role> — it plays a role (/role lists them)\n\
         \n\
         Up to six panes stand beside the main one, in two rows of three. ⌃P goes round the panes, ⌃X 2 splits too, ⌃X \
         } and ⌃X { make the main pane wider or narrower. Two panes here change the same files; a Worktree keeps them \
         apart until /close asks how it ends.\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "swap",
        "Let the main pane and another trade places.\n\
         \n\
         /swap — this pane becomes the main one\n\
         /swap <n> — pane n becomes the main one",
    ),
    (
        "close",
        "Close a pane, or end a background agent.\n\
         \n\
         /close — closes this pane (never the main one); while it works, a second /close stops it and closes it\n\
         /close <name> — ends that background agent, its turn stopped\n\
         /close <n> — closes pane n from where you are, by its number, name or role; you stay in your pane\n\
         /close alone, with more than this pane to close — a list of them, this pane first: ⏎ closes the one chosen\n\
         \n\
         The session is kept for /resume. A pane in a Worktree asks how the Worktree ends: keep it and its branch, \
         take it over (merge it into the branch you are on) or discard it. ⌃X 0 closes this pane too.",
    ),
    (
        "send",
        "Send a message to another pane or background agent, taken there as if you had typed it.\n\
         \n\
         /send — a list of whom to write to: the panes and agents here, then the other Glut windows; ⏎ puts /send \
         <who> into the input\n\
         /send <text> — to the only other pane\n\
         /send <n|name> <text> — to pane n or the agent of that name (needed with three or more)\n\
         /send plan — hands this pane's plan to the other pane to build\n\
         /send <n|name> plan — hands it to that one\n\
         \n\
         The message starts at once where the session rests, otherwise after its turn. The keys stay in this pane.",
    ),
    (
        "role",
        "Let this pane play a role: a named agent's prompt and tools, never its model.\n\
         \n\
         /role — the roles to pick from\n\
         /role <name> — plays that role from now on\n\
         /role none — an all-rounder again, with every tool\n\
         \n\
         Glut brings reviewer, tester and docs; your own roles live in `~/.glut/agents`, a project's in its \
         `.glut/agents` (read only in a trusted folder). A role's tools take effect at once, its prompt from the next \
         turn. A role that narrows the tools needs the Gate.",
    ),
    (
        "bg",
        "Start an agent in the background: a session with no pane, reached by its name.\n\
         \n\
         /bg — an agent named agent (or agent-2, …); in a git repository Glut asks where it works\n\
         /bg <name> — an agent of that name (small letters, digits and dashes)\n\
         /bg as <role> — it plays a role, and is named after it\n\
         /bg here — it works in this folder\n\
         /bg tree [branch] — it works in a new Worktree\n\
         \n\
         /send <name> gives it work, /peek <name> reads its stream, /show <name> brings it forward, /close <name> ends \
         it. Each agent works and costs on its own: Glut asks once a window before a seventh. ⌃X B starts one with its \
         role picked, ⌃X A lists them.\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "Bring a background agent forward as a pane.\n\
         \n\
         /show <name> — the agent becomes the last pane of the grid\n\
         \n\
         When the window has no room left, /hide or /close a pane first. ⌃X A lists the agents in the background.",
    ),
    (
        "hide",
        "Send this pane to the background, with its memory kept.\n\
         \n\
         /hide — the pane leaves the screen and works on as a background agent\n\
         \n\
         It keeps a name to be reached by (its role, or pane-<n>): /show <name> brings it back, /send <name> and /peek \
         <name> reach it. The main pane and the last one stay. ⌃X H does the same.",
    ),
    (
        "peek",
        "Read a background agent's stream over this pane, without bringing it forward.\n\
         \n\
         /peek <name> — its stream, live, only to read\n\
         /peek — closes it again\n\
         \n\
         ⎋ closes it too; /show <name> brings the agent forward.",
    ),
    (
        "loop",
        "Run a task again and again in a background agent of its own, while Glut is open.\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — starts a loop; the first round at once\n\
         /loop — lists the loops: interval, rounds, the next round and the cost so far\n\
         /loop stop [name] — ends that loop, or the only one\n\
         /loop rounds [n] — the most rounds of a loop; alone it shows the limit now\n\
         /loop budget [usd] — the most a loop may cost; alone it shows the limit now\n\
         /loop tokens [n] — the most tokens a loop on a subscription may use (5M, 500k); alone it shows the limit now\n\
         \n\
         Rounds never overlap: the interval is counted from a round's end, as 30s, 5m, 2h or 1h30m (from 10s to 24h). \
         A round goes on with the conversation of the round before; with fresh each round starts a new one. The task \
         is a message, or /skill <name> [what for], /compact or /init — no other command.\n\
         \n\
         The agent is named loop (or after its role) and takes this pane's mode, the \"always\" you gave here, its \
         folders beside and its role; as <role> may only name the role this pane plays. Its asks wait for you. A tile \
         comes when a round asks or the loop ends; with notify after every round.\n\
         \n\
         A loop ends after its rounds or once its rounds cost its budget (by default 50 rounds and 5 $, else \
         `loop.rounds` and `loop.maxUsd` of `~/.glut/settings.json`; a change counts for loops started after it), when \
         a round fails or you stop it (⎋ in its round), or when its model calls `end_loop` because the end your task \
         named is reached. When the model's prices are unknown, only the rounds end it. Its agent stays in the Agents \
         tab afterwards, to read or /close. x on its card in the Agents tab (⌃X A) ends the loop.\n\
         \n\
         /loop 10m run the tests and fix what fails; stop when all pass\n\
         /loop 1h fresh as reviewer notify review the last commit",
    ),
    (
        "schedule",
        "Plans that run while Glut is closed: a task at a time of day or every so often.\n\
         \n\
         /schedule — the Runs tab (⌃X R): every plan with its next time and its last result\n\
         /schedule list — the plans in the stream\n\
         /schedule add <name> [flags] <task> — makes a plan, or changes the one of that name\n\
         /schedule run <name> — runs it now, beside Glut\n\
         /schedule pause <name> — holds it until resume\n\
         /schedule resume <name> — it runs at its times again\n\
         /schedule remove <name> — the plan and its record go; its sessions stay\n\
         \n\
         The flags are those of `glut schedule add`:\n\
         - `--at 07:00` a time of day, with `--days mo-fr` or `sa,su` (every day without)\n\
         - `--every 2h` an interval from 10m to 7d, instead of --at\n\
         - `--mode ask|build|plan|dont_ask|auto` what it may do without a rule; ask unless named\n\
         - `--allow <rule>`, `--deny <rule>` rules in Claude Code's form, such as `Bash(cargo test:*)`; repeatable\n\
         - `--folder <path>` a folder beside its own; repeatable\n\
         - `--max-usd 1`, `--max-minutes 30` the most a run may cost and take: 1 $ and 30 min unless named\n\
         - `--max-tokens 1M` on a subscription, the most tokens a run may use: 1M unless named\n\
         - `--replace` the rules and folders given take the place of the plan's instead of joining them\n\
         - `--cwd <path>`, `--provider <id>`, `--model <id>` its folder (this one otherwise), provider and model, \
         fixed when it is made\n\
         \n\
         The task is a message, or /skill <name> [what for]. launchd starts each run, only while you are logged in; a \
         time missed asleep is made up once on waking. Nobody is there to answer: every ask is refused and noted, so \
         allow what it needs. A run is a kept session of its own, told by a macOS notification. In the Runs tab: ⏎ \
         opens a plan or a run's session, r runs it now, p pauses or resumes, d deletes it.\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" run the tests and report what \
         fails",
    ),
    (
        "sessions",
        "Write to the other Glut windows on this machine.\n\n/sessions — their panes and agents: address, project, \
        model, mode, working or waiting; ⏎ puts /send <address> into the input\n/sessions off — this window takes no \
        messages from them\n/sessions on — and again\n\nA message from the owner there goes in at once; one from a \
        model there only when it runs in the mode of the pane it reaches, else it waits above the input: ⏎ lets it \
        in, Esc drops it. Neither is your word to the model here. Only Glut talks to the other windows — a command of \
        the model's never reaches them.",
    ),
    (
        "name",
        "Name this window among the Glut windows on this machine.\n\n/name — the name this window has now\n/name \
        <new> — a new one: letters, digits, - _ and .\n\nOther windows reach this one by its name, a pane by name/2 \
        or name/reviewer. A name you give is kept with the session and comes back with /resume.",
    ),
    (
        "diff",
        "Review what the last turn changed, hunk by hunk.\n\
         \n\
         /diff — the changes of the last turn that changed files\n\
         /diff session — every change of this session\n\
         \n\
         In the review: ↑↓ a hunk, ←→ a file, a accepts, r rejects (the hunk goes back, and Glut reads why with your \
         next message), c comments (sent with your next message), v marks a file reviewed, s switches between turn and \
         session, ⎋ closes. ⌃R opens it too; /review is the same command.",
    ),
    (
        "steps",
        "See the steps that changed files, and put files back.\n\
         \n\
         /steps — every step that changed a file; ⏎ on one shows what it changed, or puts the files back to before \
         it\n\
         \n\
         Putting files back takes your edits since with them; the conversation stays. Glut keeps a snapshot before \
         each changing step; what the files were just before is kept too, so `glut undo` brings it back. ⎋⎋ rewinds \
         the conversation instead.",
    ),
    (
        "todo",
        "Keep the project's to-dos in its `TODO.md`, a checklist you and Glut both use.\n\
         \n\
         /todo — the To-dos tab: Space ticks an item, a adds one, o opens the editor\n\
         /todo <text> — adds an item at the list's end, while Glut works too\n\
         /todo open — the list in your editor\n\
         /todo clean — drops the done items, asked first\n\
         \n\
         The rest of the file is never touched. ⌃Y shows or hides the tab.",
    ),
    (
        "memory",
        "See what the project's memory keeps: the facts Glut noted for later sessions.\n\
         \n\
         /memory — the facts with their descriptions; ⏎ shows one whole\n\
         /memory open — the index (`MEMORY.md`) in your editor\n\
         \n\
         The memory lies outside the repository: in `~/.glut/memory/<project>/`, or Claude Code's memory where Claude \
         Code works in the repository. Glut writes it with its `memory` tool; reading it here asks no model.",
    ),
    (
        "skill",
        "Start a Skill: instructions for a kind of task, which the model loads and follows.\n\
         \n\
         /skill — the Skills to pick from\n\
         /skill <name> [what for] — runs that Skill on what follows its name\n\
         \n\
         Glut brings make-agent, make-skill and make-hook to write your own. Yours live in `~/.glut/skills`, a \
         project's in its `.glut/skills` (read only in a trusted folder); one of the same name hides Glut's.\n\
         \n\
         /skill make-hook block git push on main",
    ),
    (
        "init",
        "Write a short `AGENTS.md` for this folder, or check and shorten the guide it has.\n\
         \n\
         /init — without a guide Glut writes `AGENTS.md`; with one it checks it and keeps it short\n\
         /init --migrate — asks again whether `CLAUDE.md` moves into `AGENTS.md`\n\
         \n\
         With only a `CLAUDE.md`, Glut asks once whether it moves into `AGENTS.md` (`CLAUDE.md` then imports it, one \
         guide for every harness); a no is kept. The guides ride along on every turn: /context shows what they cost.",
    ),
    (
        "gate",
        "See the rules the Gate decides by, and take an \"always\" back.\n\
         \n\
         /gate — the rules in force: your \"always\" of this session, then those of `~/.glut/settings.json` and the \
         project's `.glut/`; ⏎ on an \"always\" takes it back, and Glut asks again next time\n\
         \n\
         ⌃G opens the Gate tab: every judgement live, the hosts reached and the refusals. ⇧⇥ changes the mode.",
    ),
    (
        "mcp",
        "See the MCP servers and how each stands, sign in to a hosted one, add or remove one.\n\
         \n\
         /mcp — every server: connected with its tools, waiting for a sign-in, or failed\n\
         /mcp login <name> — signs in to a hosted server; its address opens in your browser on ⏎\n\
         /mcp login <name> <address> — over SSH: the address the browser ended on, pasted back\n\
         /mcp logout <name> — signs out and forgets its token\n\
         /mcp add <name> <url> — adds a hosted server to `~/.glut/mcp.json` and connects it\n\
         /mcp add <name> -- <command> [args] — adds a server Glut starts itself\n\
         /mcp remove <name> — takes it out of `~/.glut/mcp.json` and out of the session\n\
         \n\
         A server that offers no sign-in Glut can use asks for a token, typed hidden. A project's own servers are the \
         project's to change: /mcp remove takes only yours. `glut mcp` does the same outside a session.\n\
         \n\
         While one of its tools runs, a server may ask you: a form (⇥ ↑↓ between the fields, space ticks, ⏎ sends, \
         Esc closes, ⌃D declines) or a page to open (o). Only you answer it, never the model; a form that asks for a \
         password or a key is declined, and with nobody there every question is declined.",
    ),
    (
        "images",
        "Choose how images are drawn in the stream.\n\
         \n\
         /images inline — the real image where the terminal can show one\n\
         /images blocks — true-colour half blocks\n\
         /images off — only the image's line\n\
         \n\
         /images alone shows the choice now. By default Glut draws inline where the terminal can and in blocks \
         elsewhere. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "links",
        "Choose how links show in the stream.\n\
         \n\
         /links clickable — the link's text is the link, where the terminal speaks OSC 8\n\
         /links plain — the URL is written after the text\n\
         \n\
         /links alone shows the choice now. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "notify",
        "Be told when a turn ends or Glut asks, while the window is not in front.\n\
         \n\
         /notify on — a turn's end and every ask\n\
         /notify asks — only the asks\n\
         /notify off — nothing\n\
         /notify system — through macOS's own notifications (a click on one opens Script Editor, not the terminal)\n\
         /notify after <s> — a turn's end is told only once it ran this many seconds (10 by default, up to 3600)\n\
         /notify tiles on|off — the tiles at the window's top right\n\
         \n\
         /notify alone shows what holds now. Glut uses the terminal's own notifications where it knows it has them, \
         and the bell. A notification names Glut, the project, the pane and what happened, never the model's words. \
         Kept in `~/.glut/prefs.json`, over `notify` in `~/.glut/settings.json`.",
    ),
    (
        "secrets",
        "See and set the keys hidden from the model in this session.\n\
         \n\
         /secrets — every value hidden so far, by number, kind, length and where it was seen (never the value)\n\
         /secrets allow <n> — lets secret n through for this session\n\
         /secrets allow <n> always — lets it through always (its fingerprint is kept, never the value)\n\
         /secrets add <name> <regex> — hides a format of your own, from the next result on\n\
         /secrets remove <name> — drops a format of yours\n\
         /secrets on|off — masking on or off\n\
         \n\
         A key in what a tool answers reaches the model as ‹secret n: …›; Glut puts its value back only into a file \
         that already holds it. A message of yours that looks like it holds a key is asked about before it goes. The \
         model hears of a change with your next message. Your formats and choices are kept in \
         `~/.glut/secrets.json`.\n\
         \n\
         Masking guards against a key shown by accident; the sandbox and the network Gate stay the barriers.",
    ),
    (
        "keychain",
        "Keep the Keychain out of reach of the commands Glut runs.\n\
         \n\
         /keychain — whether it is open, and the programs that may ask for it\n\
         /keychain add <program> — puts a program on the list (gh, or git and a sub-command: git push)\n\
         /keychain remove <program> — takes it off the list\n\
         /keychain open — opens it to every command, until this session ends\n\
         /keychain close — shuts it again\n\
         \n\
         A key or a sign-in in the Keychain is reached past every file rule of the sandbox, so a command Glut runs \
         finds the Keychain shut. A command of a listed program asks first — in every mode, `auto` too — and runs \
         with it open once you allow it; an \"always\" makes it a rule of yours. With nobody there (a plan, \
         `glut run`) only such a rule opens it. When a command fails on the shut Keychain, Glut says so under its \
         output.\n\
         \n\
         Allowing a command gives the Keychain to everything it starts as well: a git hook, a gh extension or a build \
         script can then read any key in it.\n\
         \n\
         Only your own rules give the Keychain: a project's `.glut/` allow rule lets a command run, never with it \
         open. A chain gets it only when every part is listed or only reads. What `glut keychain open` keeps counts \
         only here, where you sit — never in `glut run` or a host's run.\n\
         \n\
         A rule of yours opens it unasked only when it names a program and its sub-command (`gh pr *`, `git push *`) \
         — never a whole program (`gh *`). A 🔑 card shows the whole command, ↑↓ through it, and allows only once you \
         have read it to its end.\n\
         \n\
         The list is kept in `~/.glut/keychain.json`; `glut keychain open` keeps the Keychain open across sessions, \
         `glut keychain close` shuts it for good.",
    ),
    (
        "classifier",
        "Let a second model judge the commands only the mode would ask you about.\n\
         \n\
         /classifier — which model judges, or that none does\n\
         /classifier session — the session's own model judges (as long as you set none)\n\
         /classifier <provider> <model> — a model of its own, a quick one is enough (openrouter deepseek/deepseek-v4.1-flash)\n\
         /classifier off — none judges: you are asked each time\n\
         \n\
         Glut knows many commands that only read (ls, grep, git status, …) and runs them in every mode. A loop or a pipeline of them (for f in *.md; do grep -c x \"$f\"; done) it cannot judge from its text alone, so it asks. The second model sees that command and its folder — never the conversation, a file or an output — and says READS or CHANGES. Only on READS does the command run as a listed reader, in the sandbox; anything else, or no clear answer within seconds, asks you as before, its word under the command. A rule of yours, a heavy command, the Keychain and the sandbox stay as they are; the same command again is not judged twice in a session.\n\
         \n\
         A command it let run says so beside it. The choice is kept in `~/.glut/classifier.json` and counts from the next turn; `glut run --classifier` sets it for one run.",
    ),
    (
        "ide",
        "Your editor beside Glut: Neovim with the plugin glut.nvim, VS Code (and Cursor, Windsurf) or a JetBrains IDE with Glut's own plugin.\n\
         \n\
         /ide — which editor Glut is connected to, and which fit\n\
         /ide on — connect (asks when several fit)\n\
         /ide off — disconnect for this session\n\
         \n\
         Connected, every message of yours carries the active file and the selection (⧉ under it), as long as no read \
         rule holds the file back; the model can ask which files are open and what the language servers say; Glut \
         reads a buffer as the editor holds it, unsaved changes too, and a question of the gate about an edit shows \
         as a diff in the editor too — the first answer counts. Glut connects by itself as it starts where one editor \
         fits: the one that started Glut, else the one whose folder holds Glut's. \"⧉\" in the footer says it is \
         connected.",
    ),
    (
        "clean",
        "Keep Glut's own data in ~/.glut from growing without end.\n\
         \n\
         /clean — what Glut keeps, kind by kind, and what of it can go by your terms\n\
         /clean now — cleans at once, in the background\n\
         /clean keep — the terms\n\
         /clean keep <kind> <days>d — sets one: snapshots, journals, sessions, results or lean\n\
         \n\
         What goes: the snapshots of a run older than its term (7 days) — a folder's whole store once none is left, \
         the folder there or not; an unfinished run's journal (7 days); a session not written for 90 days, with its \
         event log and its long results; the long results of no session after 30 days; the images no kept session \
         holds; the lines of lean.jsonl older than 90 days. Exports are only counted: they are your files.\n\
         \n\
         Once a day Glut cleans on its own as it starts — here and in `glut run` — and says what it freed. Build \
         output and caches (a folder with a CACHEDIR.TAG, below node_modules/ or a Cargo project's target/) get no \
         snapshots at all.\n\
         \n\
         The terms are kept in `~/.glut/clean.json`. On the command line: `glut clean`, `glut clean --dry-run`, `glut \
         clean keep sessions 30d`; `glut clean --dry-run --json` writes the report for a program to read.",
    ),
    (
        "motion",
        "Choose how much the screen moves.\n\
         \n\
         /motion full — Glimm breathes, sparks rise, the flame flickers\n\
         /motion calm — everything holds still; a frame is drawn only when something changed\n\
         /motion off — as calm\n\
         \n\
         /motion alone shows the choice now. macOS's Reduce motion and the reader mode hold the screen still whatever \
         you choose. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "rule",
        "A rule for this session that the model keeps at every turn.\n\
         \n\
         /rule <text> — keep it, as \"update the task list at every step\"\n\
         /rule — the rules now, numbered\n\
         /rule remove <n> — take rule n back\n\
         \n\
         The rules go with every turn's instructions, after the guides, until you take them back; /clear keeps them, \
         a new session starts without. At most 20, of 500 characters each. For a rule of every session, write it into \
         `~/.glut/AGENTS.md` or the project's guide.",
    ),
    (
        "maxsteps",
        "How many steps a turn takes at most.\n\
         \n\
         /maxsteps — the limit now\n\
         /maxsteps <n> — a turn stops after n steps, from 1 to 1000\n\
         \n\
         A step is one call of the model. At the limit the turn stops and says so; ⏎ on the empty input sends \"Go on \
         where you stopped.\" and the work goes on. Budgets and the time limit stay the real brakes. Kept in \
         `~/.glut/prefs.json`; `maxTurns` in `~/.glut/settings.json` sets it for every run, `glut run --max-turns` \
         for one.",
    ),
    (
        "nudge",
        "Whether Glut asks the model to go on at the end of a turn, to keep to its task list and to write one when it \
         works without one.\n\
         \n\
         /nudge on — when a turn ends with items of the task list open, or with words that announce a next step \
         (\"next I'll…\", a final colon), Glut sends the model one line of its own to go on or say why it stops; the \
         stream shows it quietly. While the model works, a task list left unchanged brings Glut's reminder, and if \
         that changes nothing, once a message of its own, and a run that has never written one brings its request for \
         one, once a turn\n\
         /nudge off — a turn ends where the model ends it: no reminder of the task list, no request for one\n\
         \n\
         The question at the end comes once per turn, never in plan mode, after a question to you, in a loop's round \
         or a plan's run; the reminder and the request follow the same choice, but in a plan's run Glut always \
         reminds. /nudge alone shows the choice now. Kept in `~/.glut/prefs.json`; `\"nudge\": false` in \
         `~/.glut/settings.json` turns it off for every run.",
    ),
    (
        "lsp",
        "Whether the language servers help the model.\n\
         \n\
         /lsp on — after an edit the file's errors come with the result, and the model has the lsp tool: where a \
         symbol is defined, every place it is used, its type and docs, and a rename across the project\n\
         /lsp off — no server runs: their processes end at once, no errors after an edit, no lsp tool\n\
         /lsp — the choice now, and each server of this session: ready, indexing the project, or off for the session\n\
         \n\
         Glut starts only servers that never run the project's code (rust-analyzer without build scripts and proc \
         macros, pyright, and the servers of TOML, YAML, JSON, CSS, HTML and Markdown), only in the sandbox without \
         network, and only where the gate allows its command line. A server starts at the first edit or question of \
         its language and stays for the session; rust-analyzer needs some seconds to index a large project, meanwhile \
         lsp says so and the model uses grep. A rename is one change: asked once for all its files, all or none, a \
         snapshot before it. Kept in `~/.glut/prefs.json`; `\"lsp\": false` in `~/.glut/settings.json` turns it off \
         for every run, `glut run --no-lsp` for one.",
    ),
    (
        "reader",
        "Set Glut up for a screen reader.\n\
         \n\
         /reader on — keys in words, a plain footer, no Glimm, plain frames, nothing moving and a line of text at each \
         change of state\n\
         /reader off — the usual screen\n\
         \n\
         /reader alone shows the choice now. Kept in `~/.glut/prefs.json`; `glut --reader` holds it on for one run.",
    ),
    (
        "language",
        "Choose the language Glut speaks to you.\n\
         \n\
         /language — the languages to pick from\n\
         /language <code> — en, de, fr, it, es, zh, ja or pt\n\
         \n\
         Without a choice Glut speaks your system's language. The model answers in the language you write in, whatever \
         you choose here. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "theme",
        "Choose Glut's colours.\n\
         \n\
         /theme — the themes to pick from\n\
         /theme <name> — glut, Glut's own, or one of Axiomata's themes (catppuccin-mocha, github-dark, …)\n\
         \n\
         Glut colours only the text and keeps the terminal's background: a light theme wants a light terminal. Kept in \
         `~/.glut/prefs.json`; in a window a host app opened, the choice holds for that session.\n\
         \n\
         Each theme gives the footer's parts, the tools' names by their kind (reading, writing, commands, what \
         reaches out), the panes and agents and the line above the input its own muted colours; what needs you stays \
         bright. The reader mode and `NO_COLOR` keep it plain.",
    ),
    (
        "editor",
        "Choose the editor Glut opens files and the input in.\n\
         \n\
         /editor — the editors found on your system\n\
         /editor <command> — your own command, with `{file}` and `{line}` where they go\n\
         \n\
         Without `{file}` the file is added at the end. A windowed editor must wait until the file is closed (`code \
         --wait`). ⌃X E edits the input in it; /todo open and /memory open use it too. Kept in `~/.glut/prefs.json`.\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "Choose how a resting session keeps the provider's prompt cache, so the next turn starts warm.\n\
         \n\
         /cache ping — Glut reads the cache again every few minutes while the session rests, for up to 30 minutes\n\
         /cache 1h — Anthropic keeps it an hour (each write costs twice); other providers are read to keep it\n\
         /cache off — not kept: a pause past the provider's few minutes starts cold\n\
         \n\
         /cache alone shows the choice now; ping is the default. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "footer",
        "Choose how the footer joins its parts.\n\
         \n\
         /footer powerline — Powerline's arrows\n\
         /footer plain — plain blocks\n\
         \n\
         /footer alone shows the choice now and why. Without a choice Glut guesses from the terminal whether it draws \
         Powerline's glyphs. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "vim",
        "Edit the input the vi way.\n\
         \n\
         /vim on — ⎋ goes to Normal mode (motions, operators, counts, u and ⌃R), i a I A o O back to Insert\n\
         /vim off — the usual editing keys\n\
         \n\
         /vim alone turns it on. ⎋ in Normal mode stops Glut. Kept in `~/.glut/prefs.json`.",
    ),
    (
        "help",
        "Every command and every key, in a box of their own.\n\
         \n\
         /help — the commands with a line each; typing filters them, ↑↓ choose, ⏎ opens the whole page of one\n\
         /help <command> — that command's page at once, as /help loop\n\
         \n\
         On a page ↑↓ and PgUp PgDn scroll, ⎋ goes back to the list and closes the box from there. The last entry \
         holds every key by topic, !cmd and the vi mode; ⌃K keeps the keys in the side panel.",
    ),
];
