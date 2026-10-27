//! The session's commands and keys by topic (`docs/plans/glut-tui.md`, T38): one order for the `/` list, the Keys
//! sheet (Ctrl+K, a tab of the side panel) and `/help`, kept here once — each entry carries its topic, and the topics
//! follow [`Topic::ALL`].

use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::theme;
use crate::transcript::wrap;

/// What a command or a key is about, in the order the lists show them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Session,
    Model,
    View,
    /// Several sessions in one window (T36a).
    Panes,
    Changes,
    Project,
    Settings,
    /// Keys only: what the input does.
    Input,
    /// Keys only: moving and deleting in the line typed, as in the shell (TUI9a).
    Editing,
    Help,
}

impl Topic {
    pub const ALL: [Topic; 10] = [
        Topic::Session,
        Topic::Model,
        Topic::View,
        Topic::Panes,
        Topic::Changes,
        Topic::Project,
        Topic::Settings,
        Topic::Input,
        Topic::Editing,
        Topic::Help,
    ];

    /// Its heading in the language read now (T48).
    pub fn label(self) -> &'static str {
        let at = Topic::ALL
            .iter()
            .position(|topic| *topic == self)
            .unwrap_or(0);
        crate::texts::t()
            .topics
            .get(at)
            .copied()
            .unwrap_or_default()
    }
}

/// `name`'s row of `rows` in the language read now, else `english`.
fn said(
    rows: &'static [(&'static str, &'static str)],
    name: &str,
    english: &'static str,
) -> &'static str {
    rows.iter()
        .find(|(row, _)| *row == name)
        .map_or(english, |(_, text)| *text)
}

/// A `/` command: its name, what it takes as `/help` writes it, what it does, and its topic.
#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub name: &'static str,
    pub args: &'static str,
    pub what: &'static str,
    pub topic: Topic,
}

impl Command {
    /// What it does, in the language read now (T48).
    pub fn what(&self) -> &'static str {
        said(crate::texts::t().commands, self.name, self.what)
    }
}

const fn command(
    topic: Topic,
    name: &'static str,
    args: &'static str,
    what: &'static str,
) -> Command {
    Command {
        name,
        args,
        what,
        topic,
    }
}

/// Every command, grouped by topic in [`Topic::ALL`]'s order, the most used first inside a topic.
pub const COMMANDS: [Command; 55] = [
    command(Topic::Session, "clear", "", "start a new session"),
    command(
        Topic::Session,
        "resume",
        "",
        "open an earlier session of this folder — the forks under theirs",
    ),
    command(
        Topic::Session,
        "search",
        "<text>",
        "find in the kept conversations: /resume's list with the text typed — ⌃A every project",
    ),
    command(
        Topic::Session,
        "btw",
        "[name] <question>",
        "a side question, answered in a card and never part of the conversation; the turn under way goes on — \
         /btw reviewer how far?",
    ),
    command(
        Topic::Session,
        "projects",
        "[path]",
        "go on in another project: the folders Glut worked in, or one by its path",
    ),
    command(
        Topic::Session,
        "folder",
        "[add|remove] [path]",
        "folders beside the working one for this session: the file tools reach them by their full path, commands write \
         there — /folder add <path>, /folder remove <path>",
    ),
    command(
        Topic::Session,
        "compact",
        "",
        "summarise the conversation so far",
    ),
    command(
        Topic::Session,
        "handoff",
        "write|read",
        "/handoff write: what was done and what comes next; /handoff read in a new session",
    ),
    command(
        Topic::Session,
        "export",
        "[md|html|jsonl|copy] [full] [path]",
        "write the session out to read or share: Markdown, one HTML file or JSON lines, into ~/.glut/exports or a \
         path; copy puts the Markdown on the clipboard; full adds the whole results and the system prompt",
    ),
    command(Topic::Session, "quit", "", "end the session"),
    command(Topic::Session, "exit", "", "end the session, as /quit does"),
    command(
        Topic::Model,
        "model",
        "[id]",
        "pick a model of the provider in use, or /model <id>",
    ),
    command(
        Topic::Model,
        "provider",
        "[id|key [id]]",
        "switch the provider, or /provider <id>; /provider key changes only its key",
    ),
    command(
        Topic::Model,
        "login",
        "[provider]",
        "sign in instead of giving a key — your ChatGPT plan, or OpenRouter for a key of its own; /login alone lists \
         them with how each stands, ⏎ signs in or out",
    ),
    command(
        Topic::Model,
        "effort",
        "[level]",
        "how hard the model thinks, or /effort <level>",
    ),
    command(
        Topic::Model,
        "context",
        "[tokens]",
        "how full the window is; /context 64k sets it",
    ),
    command(
        Topic::Model,
        "thinking",
        "off|folded|open",
        "the model's thinking: /thinking off, folded or open",
    ),
    command(
        Topic::View,
        "panel",
        "",
        "the side panel on the plan, or hide it (⌃B; ⌃G the gate)",
    ),
    command(
        Topic::View,
        "meter",
        "",
        "context, tokens and cost per turn and tool, lean's savings (⌃N)",
    ),
    command(
        Topic::View,
        "agents",
        "",
        "the session's subagents: open one's own stream (a click on its tile does too; ⌃X A: the Agents tab)",
    ),
    command(
        Topic::View,
        "plan",
        "",
        "the plan Glut sent from plan mode, whole, with its task list",
    ),
    command(
        Topic::View,
        "scrollback",
        "",
        "the conversation into the terminal's own scrollback (F3)",
    ),
    command(
        Topic::Panes,
        "split",
        "[resume] [as <role>] [here|tree [branch]]",
        "a new session beside the main pane (up to six, two rows of three); in a git repository here or in a worktree",
    ),
    command(
        Topic::Panes,
        "swap",
        "[n]",
        "the main pane and pane n (or this one) trade places",
    ),
    command(
        Topic::Panes,
        "close",
        "[name]",
        "close this pane (not the main one) — or end a background agent; its session stays for /resume",
    ),
    command(
        Topic::Panes,
        "send",
        "[n] <text>|plan",
        "a message to another pane (its number first, with three or more), taken there as if typed; /send plan \
         hands it your plan to build",
    ),
    command(
        Topic::Panes,
        "role",
        "[name|none]",
        "the role this pane plays: a named agent's prompt and tools, not its model — reviewer, tester, docs or yours",
    ),
    command(
        Topic::Panes,
        "bg",
        "[name] [as <role>] [here|tree]",
        "an agent in the background: a session with no pane, reached by name — /bg as reviewer",
    ),
    command(
        Topic::Panes,
        "show",
        "<name>",
        "bring an agent from the background forward as a pane",
    ),
    command(
        Topic::Panes,
        "hide",
        "",
        "send this pane to the background, its memory kept (⌃X H)",
    ),
    command(
        Topic::Panes,
        "peek",
        "<name>",
        "read a background agent's stream over this pane; Esc closes it",
    ),
    command(
        Topic::Panes,
        "loop",
        "<interval> [fresh] [as <role>] [notify] <task>",
        "run a task again and again in a background agent of its own, the interval counted from a round's end — /loop \
         10m run the tests; /loop alone lists them, /loop stop [name] ends one, /loop rounds|budget|tokens set where \
         one ends",
    ),
    command(
        Topic::Panes,
        "schedule",
        "[list|add|run|pause|resume|remove] [name]",
        "plans that run while Glut is closed, at a time or every so often — /schedule add tests --at 07:00 --days \
         mo-fr run the tests; /schedule alone opens the Runs tab (⌃X R)",
    ),
    command(
        Topic::Changes,
        "diff",
        "[session]",
        "review what the last turn changed, hunk by hunk; /diff session (⌃R)",
    ),
    command(
        Topic::Changes,
        "steps",
        "",
        "the steps that changed files: what each changed, or the files back",
    ),
    command(
        Topic::Project,
        "todo",
        "[text|open|clean]",
        "the project's TODO.md (⌃Y): /todo <text> adds one, /todo open edits it, /todo clean drops the done",
    ),
    command(
        Topic::Project,
        "memory",
        "[open]",
        "what the project's memory keeps: ⏎ shows a fact; /memory open edits its index",
    ),
    command(
        Topic::Project,
        "skill",
        "[name]",
        "start a skill: /skill <name> [what for] — /skill alone lists them (make-agent, make-skill, make-hook, …)",
    ),
    command(
        Topic::Project,
        "init",
        "",
        "a short AGENTS.md for this folder, or check the guide it has (/init --migrate asks about CLAUDE.md again)",
    ),
    command(
        Topic::Settings,
        "gate",
        "",
        "the rules in force; take an \"always\" back (⌃G: the gate, live)",
    ),
    command(
        Topic::Settings,
        "mcp",
        "[login|logout|add|remove]",
        "the MCP servers and how each stands: /mcp login <name> signs in to a hosted one, /mcp add <name> <url> adds \
         one to ~/.glut/mcp.json",
    ),
    command(
        Topic::Settings,
        "images",
        "inline|blocks|off",
        "how images are drawn: the real one where the terminal can, true-colour half blocks, or their line alone",
    ),
    command(
        Topic::Settings,
        "links",
        "clickable|plain",
        "a link's text is the link where the terminal speaks OSC 8, or the URL written after it",
    ),
    command(
        Topic::Settings,
        "notify",
        "on|asks|off|system|after|tiles",
        "tell me when a turn ends or asks while the window is elsewhere: on, asks only, off, macOS's own (system); \
         /notify after <s>; /notify tiles on|off — the tiles at the top right",
    ),
    command(
        Topic::Settings,
        "secrets",
        "[on|off|allow|add|remove]",
        "the keys hidden from the model in this session; /secrets allow <n> [always] lets one through, \
         /secrets add <name> <regex> hides a format of your own, /secrets remove <name>, /secrets on|off",
    ),
    command(
        Topic::Settings,
        "motion",
        "full|calm|off",
        "how much the screen moves; calm and off hold Glimm still (macOS Reduce motion counts as off)",
    ),
    command(
        Topic::Settings,
        "nudge",
        "on|off",
        "whether Glut asks the model once to go on when it ends with its task list open or announcing a next step",
    ),
    command(
        Topic::Settings,
        "reader",
        "on|off",
        "for a screen reader: keys in words, a plain screen, nothing moving, a line at each change of state",
    ),
    command(
        Topic::Settings,
        "language",
        "[code]",
        "the language Glut speaks to you: en de fr it es zh ja pt — the model answers in the one you write",
    ),
    command(
        Topic::Settings,
        "theme",
        "[name]",
        "Glut's colours, or an Axiomata theme's: /theme <name>",
    ),
    command(
        Topic::Settings,
        "editor",
        "[command]",
        "pick your editor, or /editor <command>",
    ),
    command(
        Topic::Settings,
        "cache",
        "ping|1h|off",
        "how a resting session keeps its cache: read again (ping), Anthropic's hour (1h), or not at all",
    ),
    command(
        Topic::Settings,
        "footer",
        "powerline|plain",
        "the footer's joins: Powerline's arrows or plain blocks (guessed from the terminal)",
    ),
    command(
        Topic::Settings,
        "vim",
        "on|off",
        "edit the input the vi way: /vim on or off",
    ),
    command(Topic::Help, "help", "", "commands and keys"),
];

/// A key of the Keys sheet: the key as written, what it does, the `/` command that does the same (or `""`), and its
/// topic.
#[derive(Debug, PartialEq, Eq)]
pub struct Key {
    pub key: &'static str,
    pub what: &'static str,
    pub command: &'static str,
    pub topic: Topic,
}

impl Key {
    /// What it does, in the language read now (T48).
    pub fn what(&self) -> &'static str {
        said(crate::texts::t().keys, self.key, self.what)
    }
}

const fn key(topic: Topic, key: &'static str, what: &'static str, command: &'static str) -> Key {
    Key {
        key,
        what,
        command,
        topic,
    }
}

/// Every key Glut has, grouped as [`COMMANDS`] are; a topic without keys shows no heading.
pub const KEYS: [Key; 44] = [
    key(
        Topic::Session,
        "⇧⇥",
        "the next mode: plan → ask → build → auto",
        "",
    ),
    key(Topic::Session, "⎋", "stop Glut", ""),
    key(
        Topic::Session,
        "⏎",
        "while Glut works: steer it, taken in before its next step",
        "",
    ),
    key(
        Topic::Session,
        "⇥",
        "while Glut works: queue the message for after",
        "",
    ),
    key(
        Topic::Session,
        "⌥↑",
        "take the last waiting message back",
        "",
    ),
    key(Topic::Session, "⌃C ⌃C", "quit", "/quit"),
    key(Topic::View, "⌃F", "search the stream", ""),
    key(Topic::View, "⌃O", "unfold or fold every tool result", ""),
    key(Topic::View, "⌃T", "open or fold every thought", "/thinking"),
    key(
        Topic::View,
        "⌃B",
        "the side panel on the plan, or hide it",
        "/panel",
    ),
    key(
        Topic::View,
        "⌃G",
        "the gate, live: every judgement, the hosts, the refusals",
        "/gate",
    ),
    key(
        Topic::View,
        "⌃N",
        "the meter: context, tokens and cost per turn and tool",
        "/meter",
    ),
    key(
        Topic::View,
        "⌃X A",
        "the Agents tab: every subagent and the agents in the background, live",
        "/agents",
    ),
    key(
        Topic::View,
        "⌃X R",
        "the Runs tab: every project's plans, when they run next and how they ended",
        "/schedule",
    ),
    key(
        Topic::View,
        "⌃X ↑",
        "read the stream block by block: ⏎ folds, c copies, ⎋ back",
        "",
    ),
    key(
        Topic::View,
        "⌃X ⇥",
        "the side panel takes the keys: ←→ the tabs, ↑↓ within, ⎋ back",
        "",
    ),
    key(
        Topic::View,
        "PgUp PgDn",
        "scroll the stream (⇧↑ ⇧↓ a line)",
        "",
    ),
    key(
        Topic::View,
        "F3",
        "the conversation into the terminal's scrollback",
        "/scrollback",
    ),
    key(
        Topic::View,
        "F2",
        "hand the mouse to the terminal and back",
        "",
    ),
    key(
        Topic::Panes,
        "⌃P",
        "the next pane, round in reading order (⌃] too: ⌃6 on a German layout)",
        "",
    ),
    key(
        Topic::Panes,
        "⌃X 2",
        "a new pane beside the main one (⌃X 3 too)",
        "/split",
    ),
    key(
        Topic::Panes,
        "⌃X }",
        "the main pane wider (the mouse drags its border too)",
        "",
    ),
    key(Topic::Panes, "⌃X {", "the main pane narrower", ""),
    key(Topic::Panes, "⌃X 0", "close this pane", "/close"),
    key(
        Topic::Panes,
        "⌃X B",
        "an agent in the background, its role picked",
        "/bg",
    ),
    key(Topic::Panes, "⌃X H", "this pane to the background", "/hide"),
    key(
        Topic::Changes,
        "⌃R",
        "review what the last turn changed, hunk by hunk",
        "/diff",
    ),
    key(
        Topic::Changes,
        "⎋⎋",
        "rewind or fork to before a message of yours",
        "",
    ),
    key(
        Topic::Project,
        "⌃Y",
        "the To-dos tab: Space ticks, a adds",
        "/todo",
    ),
    key(Topic::Input, "/", "the commands", ""),
    key(Topic::Input, "@", "mention a file", ""),
    key(
        Topic::Input,
        "⌃V",
        "the clipboard's image into the message (a dropped image file too)",
        "",
    ),
    key(Topic::Input, "↑", "the messages sent before", ""),
    key(Topic::Input, "⇧⏎", "a new line (⌥⏎ too)", ""),
    key(
        Topic::Input,
        "⌃X E",
        "edit the input in your editor",
        "/editor",
    ),
    key(
        Topic::Editing,
        "⌃A ⌃E",
        "the line's start, its end (Home End too)",
        "",
    ),
    key(
        Topic::Editing,
        "⌥B ⌥F",
        "a word left, a word right (⌥← ⌥→ ⌃← ⌃→ too)",
        "",
    ),
    key(
        Topic::Editing,
        "⇧← ⇧→",
        "select a character left, right — ⇧Home ⇧End up to the input's start, its end; ⌫ deletes the selection, \
         typing replaces it",
        "",
    ),
    key(
        Topic::Editing,
        "⌃W",
        "delete the word before (⌥⌫ ⌃⌫ too)",
        "",
    ),
    key(Topic::Editing, "⌃U", "delete to the line's start", ""),
    key(
        Topic::Editing,
        "⌦",
        "delete the character after the cursor",
        "",
    ),
    key(Topic::Editing, "⌃J", "a new line", ""),
    key(
        Topic::Editing,
        "⌃D",
        "quit, on an empty line while Glut rests",
        "",
    ),
    key(Topic::Help, "⌃K", "this sheet of keys", "/help"),
];

/// Where the values of a command's argument come from (T71): what only the running Glut knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The themes' names (`/theme`).
    Themes,
    /// Glut's languages' codes (`/language`).
    Languages,
    /// The effort levels and `default` (`/effort`).
    Efforts,
    /// The owner's providers' ids (`/provider`).
    Providers,
    /// The skills' names (`/skill`).
    Skills,
    /// The roles' names and `none` (`/role`, after `as` in `/bg` and `/split`).
    Roles,
    /// The agents in the background, by name (`/show`, `/peek`, `/close`, `/btw`, `/send`).
    Agents,
    /// The MCP servers (`/mcp login|logout|remove`).
    Servers,
    /// The plans' names (`/schedule run|pause|resume|remove`, T60b).
    Plans,
    /// The subscriptions one signs in to (`/login`, PV4).
    Subscriptions,
}

/// The catalog's words for something to be given, never one to complete to: `[name]`, `[path]`, `[n]` …
const PLACEHOLDERS: [&str; 13] = [
    "name", "id", "path", "n", "level", "code", "text", "tokens", "command", "branch", "question",
    "role", "provider",
];

/// Where the values for the argument after `before` come from, for command `name`.
fn source(name: &str, before: &[&str]) -> Option<Source> {
    Some(match (name, before) {
        ("theme", []) => Source::Themes,
        ("language", []) => Source::Languages,
        ("effort", []) => Source::Efforts,
        ("provider", [] | ["key"]) => Source::Providers,
        ("login", []) => Source::Subscriptions,
        ("skill", []) => Source::Skills,
        ("role", []) => Source::Roles,
        ("close" | "show" | "peek" | "btw" | "send", []) => Source::Agents,
        ("mcp", ["login" | "logout" | "remove"]) => Source::Servers,
        ("bg" | "split", [.., "as"]) => Source::Roles,
        // Only among the loop's own words, before its task begins.
        ("loop", [_, words @ .., "as"])
            if words.iter().all(|word| matches!(*word, "fresh" | "notify")) =>
        {
            Source::Roles
        }
        ("loop", ["stop"]) => Source::Agents,
        ("schedule", ["run" | "pause" | "resume" | "remove"]) => Source::Plans,
        _ => return None,
    })
}

/// A command's arguments as `/help` writes them, cut into their places: `[md|html] [full] [path]` → three, a
/// bracket's inside (`[as <role>]`, `[here|tree [branch]]`) kept whole.
fn slots(args: &str) -> Vec<&str> {
    let mut slots = Vec::new();
    let (mut depth, mut from) = (0usize, 0);
    for (at, c) in args.char_indices() {
        match c {
            '[' | '<' => depth += 1,
            ']' | '>' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => {
                if at > from {
                    slots.push(&args[from..at]);
                }
                from = at + 1;
            }
            _ => {}
        }
    }
    if from < args.len() {
        slots.push(&args[from..]);
    }
    slots
}

/// A place's first part and the rest, its brackets taken off: `[as <role>]` → `as`, `<role>`.
fn lead(slot: &str) -> (&str, &str) {
    let inner = slot
        .strip_prefix('[')
        .and_then(|inner| inner.strip_suffix(']'))
        .unwrap_or(slot);
    slots(inner).first().map_or((inner, ""), |first| {
        (first, inner[first.len()..].trim_start())
    })
}

/// The fixed words a part of the arguments names, in their order: never a placeholder, nothing inside `<…>`.
fn fixed_words(args: &str) -> Vec<&str> {
    let mut words = Vec::new();
    let mut angle = 0usize;
    let mut from = None;
    for (at, c) in args.char_indices().chain([(args.len(), ' ')]) {
        if c.is_ascii_alphanumeric() || c == '-' {
            from.get_or_insert(at);
            continue;
        }
        if let Some(start) = from.take() {
            let word = &args[start..at];
            if angle == 0 && !PLACEHOLDERS.contains(&word) && !words.contains(&word) {
                words.push(word);
            }
        }
        match c {
            '<' => angle += 1,
            '>' => angle = angle.saturating_sub(1),
            _ => {}
        }
    }
    words
}

/// Whether a place must be given a word of the owner's before any later one: its first part is a placeholder.
fn free_lead(slot: &str) -> bool {
    let (first, _) = lead(slot);
    first.starts_with('<') || first.split('|').any(|word| PLACEHOLDERS.contains(&word))
}

/// The places still open after the words typed, as far as the words tell: each word fills the first place from here on
/// that names it, else the next one as its value; a place led by the word goes on with its rest (`as` → `<role>`).
fn open_slots<'a>(args: &'a str, typed: &[&str]) -> Vec<&'a str> {
    let mut open = slots(args);
    let mut at = 0;
    for word in typed {
        let named = (at..open.len()).find(|&slot| fixed_words(lead(open[slot]).0).contains(word));
        match named {
            Some(slot) => {
                // The rest belongs to the lead's last word: `tree` takes a `[branch]`, `here` none.
                let (first, rest) = lead(open[slot]);
                at = slot + 1;
                if !rest.is_empty() && first.rsplit('|').next() == Some(*word) {
                    open.insert(at, rest);
                }
            }
            None => at += 1,
        }
        if at >= open.len() {
            return Vec::new();
        }
    }
    open.split_off(at.min(open.len()))
}

/// What the word being typed at the end of `text`, a command's argument, may become (T71): the fixed words of the
/// places it may fill — up to the first that wants a word of the owner's — and the values `values` knows for it.
/// A fixed word given before is not offered again; the case of what is typed does not count. `None` where nothing
/// fits; an empty word only when `empty_too` (⇥ asks).
pub fn offer(text: &str, values: impl Fn(Source) -> Vec<String>, empty_too: bool) -> Option<Offer> {
    if text.contains('\n') {
        return None;
    }
    let (name, rest) = text.strip_prefix('/')?.split_once(' ')?;
    let command = COMMANDS.iter().find(|command| command.name == name)?;
    let (before, start) = rest.rsplit_once(' ').unwrap_or(("", rest));
    let before: Vec<&str> = before.split_whitespace().collect();
    if start.is_empty() && !empty_too {
        return None;
    }
    let open = open_slots(command.args, &before);
    let mut words: Vec<String> = Vec::new();
    for slot in &open {
        words.extend(
            fixed_words(lead(slot).0)
                .into_iter()
                .filter(|word| !before.contains(word))
                .map(str::to_owned),
        );
        if free_lead(slot) {
            break;
        }
    }
    // A value may be a word given before: `/bg reviewer as reviewer`.
    if let Some(source) = source(name, &before) {
        words.extend(values(source));
    }
    let begun = start.to_lowercase();
    let mut seen = std::collections::HashSet::new();
    words.retain(|word| {
        word.to_lowercase().starts_with(&begun) && word != start && seen.insert(word.clone())
    });
    (!words.is_empty()).then(|| Offer {
        start: start.to_owned(),
        words,
        title: format!("/{name}"),
    })
}

/// What a typed command's argument may become, as [`offer`] finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    /// The word typed so far, which a pick replaces.
    pub start: String,
    /// What it may become, the command's fixed words first.
    pub words: Vec<String>,
    /// The command, for the list's title.
    pub title: String,
}

/// What a typed command still takes, shown faintly after the cursor (TUI9, T71): after a space the places still
/// open, as `/help` writes them; inside a word the rest of the one word it can become.
pub fn ghost(text: &str, values: impl Fn(Source) -> Vec<String>) -> Option<String> {
    let (name, rest) = text.strip_prefix('/')?.split_once(' ')?;
    let command = COMMANDS.iter().find(|command| command.name == name)?;
    if command.args.is_empty() || text.contains('\n') {
        return None;
    }
    if rest.is_empty() || rest.ends_with(' ') {
        let typed: Vec<&str> = rest.split_whitespace().collect();
        let open = open_slots(command.args, &typed);
        return (!open.is_empty()).then(|| open.join(" "));
    }
    let (start, word) = completion(text, values)?;
    Some(word.chars().skip(start.chars().count()).collect())
}

/// Whether command `name` takes a path (T68: its arguments name `[path]`), what that place takes, and its fixed words
/// (`/export`'s `html`, `full`), which a word they begin is completed to instead.
pub fn takes_path(name: &str) -> Option<(crate::paths::Wants, Vec<&'static str>)> {
    let command = COMMANDS.iter().find(|command| command.name == name)?;
    if !command.args.contains("[path]") {
        return None;
    }
    let fixed = command
        .args
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty() && *word != "path")
        .collect();
    // A project is a folder, and so is one beside the working folder (T60c); every other place takes a file as well.
    let wants = if name == "projects" || name == "folder" {
        crate::paths::Wants::Folders
    } else {
        crate::paths::Wants::Any
    };
    Some((wants, fixed))
}

/// The one word a typed command's argument can become, beside the start typed that it replaces (`/motion c` → `c`,
/// `calm`); `None` where none or several fit.
pub fn completion(text: &str, values: impl Fn(Source) -> Vec<String>) -> Option<(String, String)> {
    let mut offer = offer(text, values, false)?;
    (offer.words.len() == 1).then(|| (offer.start, offer.words.remove(0)))
}

/// The commands a typed `/` part fits: with nothing typed every one, by topic; else those whose name starts with it,
/// then — from three characters on, so `to` or `or` match no filler — those with a word in what they do starting with
/// it (`/hook` finds `/skill`).
pub fn fitting(typed: &str) -> Vec<&'static Command> {
    let listed = COMMANDS.iter();
    if typed.is_empty() {
        return listed.collect();
    }
    let typed = typed.to_lowercase();
    let mut fits: Vec<&Command> = listed
        .clone()
        .filter(|command| command.name.starts_with(&typed))
        .collect();
    // Chinese and Japanese put no spaces between words, and two characters make one: found anywhere in the text.
    let wide = typed.width() > typed.chars().count();
    if typed.chars().count() >= if wide { 2 } else { 3 } {
        // In the words read now, and in English's: a term the owner knows from the docs finds its command too.
        let in_what = listed.filter(|command| {
            !command.name.starts_with(&typed)
                && [command.what(), command.what].iter().any(|what| {
                    let what = what.to_lowercase();
                    if wide {
                        return what.contains(&typed);
                    }
                    what.split(|c: char| !c.is_alphanumeric())
                        .any(|word| word.starts_with(&typed))
                })
        });
        fits.extend(in_what);
    }
    fits
}

/// The help box's last entry (`/help`): the keys by topic, then what is neither a command nor a key — the commands
/// have entries of their own.
pub fn help_more() -> String {
    let mut rows = vec![tr!(help_keys, legend = tr!(legend))];
    for topic in Topic::ALL {
        let keys: Vec<String> = KEYS
            .iter()
            .filter(|key| key.topic == topic)
            .map(|key| format!("{} {}", key.key, key.what()))
            .collect();
        if !keys.is_empty() {
            rows.push(format!("  {}: {}", topic.label(), keys.join(" · ")));
        }
    }
    rows.push(tr!(help_also).to_owned());
    rows.push(tr!(help_vim).to_owned());
    rows.join("\n")
}

/// The panel width from which a key's row also names its `/` command.
pub const SHEET_WITH_COMMANDS: usize = 46;

/// The Keys sheet `width` cells wide: each topic's heading, then its keys — the key in a column of its own, what it
/// does wrapped beside it, the `/` command after it where the sheet is wide enough.
pub fn sheet(width: usize) -> Vec<Line<'static>> {
    let key_width = KEYS
        .iter()
        .map(|key| crate::motion::spoken(key.key).width())
        .max()
        .unwrap_or(0)
        + 2;
    let what_width = width.saturating_sub(key_width + 1).max(8);
    let commands = width >= SHEET_WITH_COMMANDS;
    // What the key symbols stand for (T39), read once, then the short forms are plain.
    let mut lines: Vec<Line<'static>> = wrap(tr!(legend), width.saturating_sub(1).max(8))
        .into_iter()
        .map(|row| Line::from(Span::styled(format!(" {row}"), theme::quiet())))
        .collect();
    for topic in Topic::ALL {
        let keys: Vec<&Key> = KEYS.iter().filter(|key| key.topic == topic).collect();
        if keys.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::from(Span::styled(
            format!(" {}", topic.label()),
            theme::glow().add_modifier(Modifier::BOLD),
        )));
        for key in keys {
            let what = match key.command {
                command if commands && !command.is_empty() => format!("{} · {command}", key.what()),
                _ => key.what().to_owned(),
            };
            // The reader mode says the keys in words (TUI9b): the column takes the longest.
            let spoken = crate::motion::spoken(key.key);
            for (at, row) in wrap(&crate::motion::spoken(&what), what_width)
                .into_iter()
                .enumerate()
            {
                let head: &str = if at == 0 { &spoken } else { "" };
                let pad = key_width - head.width();
                lines.push(Line::from(vec![
                    Span::styled(format!(" {head}{}", " ".repeat(pad)), theme::strong()),
                    Span::styled(row, theme::quiet()),
                ]));
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use glut::lang::Lang;

    #[test]
    fn help_the_keys_and_the_suggestions_speak_the_language_read_now() {
        glut::lang::set_for_thread(Some(Lang::De));
        let help = help_more();
        assert!(help.starts_with("Tasten (⌃ Control · ⇧ Umschalt"), "{help}");
        let sheet: Vec<String> = sheet(60).iter().map(ToString::to_string).collect();
        assert!(
            sheet.iter().any(|line| line.contains("Glut anhalten")),
            "{sheet:?}"
        );
        assert_eq!(
            names(&fitting("zusammenfassen")),
            ["compact"],
            "found in the words read now"
        );
        assert!(
            names(&fitting("summarise")).contains(&"compact"),
            "and in English's"
        );
        glut::lang::set_for_thread(Some(Lang::Ja));
        assert!(
            names(&fitting("要約")).contains(&"compact"),
            "two characters, anywhere"
        );
        glut::lang::set_for_thread(None);
    }

    #[test]
    fn every_language_says_what_each_command_and_key_does_keeping_their_words() {
        for lang in Lang::ALL.into_iter().filter(|lang| *lang != Lang::En) {
            let texts = crate::texts::of(lang);
            let named: Vec<&str> = texts.commands.iter().map(|(name, _)| *name).collect();
            let commands: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
            assert_eq!(named, commands, "{}: every command, in order", lang.code());
            let written: Vec<&str> = texts.keys.iter().map(|(key, _)| *key).collect();
            let keys: Vec<&str> = KEYS.iter().map(|key| key.key).collect();
            assert_eq!(written, keys, "{}: every key, in order", lang.code());
            let pairs = COMMANDS
                .iter()
                .map(|command| command.what)
                .zip(texts.commands.iter().map(|(_, what)| *what))
                .chain(
                    KEYS.iter()
                        .map(|key| key.what)
                        .zip(texts.keys.iter().map(|(_, what)| *what)),
                );
            for (english, translated) in pairs {
                let lost = crate::texts::check::kept_in(english, translated);
                assert!(
                    lost.is_empty(),
                    "{}: {lost:?} lost in {translated}",
                    lang.code()
                );
            }
            assert_eq!(texts.topics.len(), Topic::ALL.len());
            assert!(
                texts.topics.iter().all(|topic| !topic.is_empty()),
                "{}",
                lang.code()
            );
        }
    }

    fn names(commands: &[&Command]) -> Vec<&'static str> {
        commands.iter().map(|command| command.name).collect()
    }

    /// The values a test's Glut knows: two themes, one role, two agents in the background.
    fn known(source: Source) -> Vec<String> {
        let names: &[&str] = match source {
            Source::Themes => &["glut", "github-light"],
            Source::Roles => &["none", "reviewer"],
            Source::Agents => &["critic", "coder"],
            _ => &[],
        };
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_typed_command_shows_what_it_takes_and_tab_completes_a_fixed_word() {
        let whole = |start: &str, word: &str| Some((start.to_owned(), word.to_owned()));
        assert_eq!(ghost("/motion ", known).as_deref(), Some("full|calm|off"));
        assert_eq!(ghost("/motion c", known).as_deref(), Some("alm"));
        assert_eq!(completion("/motion c", known), whole("c", "calm"));
        assert_eq!(ghost("/motion calm", known), None, "the word is whole");
        assert_eq!(ghost("/motion x", known), None);
        assert_eq!(
            ghost("/bg ", known).as_deref(),
            Some("[name] [as <role>] [here|tree]")
        );
        assert_eq!(
            completion("/bg h", known),
            None,
            "a name is the owner's to give"
        );
        assert_eq!(ghost("/motion", known), None, "the name still typed");
        assert_eq!(ghost("/clear ", known), None, "nothing to take");
        assert_eq!(ghost("hello /motion ", known), None);
    }

    #[test]
    fn every_word_in_the_catalogs_arguments_is_a_placeholder_or_a_word_to_complete_to() {
        // A new placeholder (`[mode]`) missing from PLACEHOLDERS would be offered as the word "mode": named here, a
        // change of the catalog has to say which it is.
        let mut words: Vec<&str> = COMMANDS
            .iter()
            .flat_map(|command| fixed_words(command.args))
            .collect();
        words.sort_unstable();
        words.dedup();
        assert_eq!(
            words,
            [
                "1h",
                "add",
                "after",
                "allow",
                "as",
                "asks",
                "blocks",
                "calm",
                "clean",
                "clickable",
                "copy",
                "folded",
                "fresh",
                "full",
                "here",
                "html",
                "inline",
                "jsonl",
                "key",
                "list",
                "login",
                "logout",
                "md",
                "none",
                "notify",
                "off",
                "on",
                "open",
                "pause",
                "ping",
                "plain",
                "plan",
                "powerline",
                "read",
                "remove",
                "resume",
                "run",
                "session",
                "system",
                "tiles",
                "tree",
                "write",
            ]
        );
    }

    #[test]
    fn every_place_of_a_command_completes_its_fixed_words_and_the_values_glut_knows() {
        // A bracket's words, at a later place too; what is still open shows after a space.
        assert_eq!(ghost("/secrets al", known).as_deref(), Some("low"));
        assert_eq!(ghost("/export html f", known).as_deref(), Some("ull"));
        assert_eq!(
            ghost("/export html ", known).as_deref(),
            Some("[full] [path]")
        );
        assert_eq!(
            ghost("/bg critic as ", known).as_deref(),
            Some("<role> [here|tree]")
        );
        assert_eq!(
            ghost("/bg critic as reviewer ", known).as_deref(),
            Some("[here|tree]")
        );
        assert_eq!(
            ghost("/split here ", known),
            None,
            "only tree takes a branch"
        );
        assert_eq!(ghost("/split tree ", known).as_deref(), Some("[branch]"));
        // Values: themes — whatever the case typed —, roles after `as`, the agents by name.
        assert_eq!(ghost("/theme gith", known).as_deref(), Some("ub-light"));
        assert_eq!(ghost("/theme GITH", known).as_deref(), Some("ub-light"));
        assert_eq!(ghost("/bg critic as rev", known).as_deref(), Some("iewer"));
        assert_eq!(
            ghost("/bg reviewer as rev", known).as_deref(),
            Some("iewer"),
            "a role may be the name"
        );
        let agents = offer("/show c", known, false).expect("two agents fit");
        assert_eq!(
            (agents.start.as_str(), agents.words),
            ("c", vec!["critic".to_owned(), "coder".to_owned()])
        );
        assert_eq!(
            completion("/show c", known),
            None,
            "two fit: the list shows them"
        );
        // Nothing begun: only when ⇥ asks; a fixed word given is not offered again; free text ends the offers.
        assert!(offer("/theme ", known, false).is_none());
        assert_eq!(
            offer("/theme ", known, true).map(|offer| offer.words.len()),
            Some(2)
        );
        assert!(
            offer("/export html h", known, false).is_none(),
            "html is given already"
        );
        assert!(
            offer("/todo buy op", known, false).is_none(),
            "a to-do's text is free"
        );
        assert_eq!(ghost("/todo op", known).as_deref(), Some("en"));
    }

    #[test]
    fn every_topic_stands_together_in_one_order_and_every_name_once() {
        let mut seen = std::collections::BTreeSet::new();
        for command in &COMMANDS {
            assert!(seen.insert(command.name), "/{} twice", command.name);
        }
        let order = |topic: Topic| Topic::ALL.iter().position(|t| *t == topic).unwrap();
        let listed = fitting("");
        assert!(
            listed
                .windows(2)
                .all(|pair| order(pair[0].topic) <= order(pair[1].topic)),
            "the list is grouped"
        );
        assert!(
            KEYS.windows(2)
                .all(|pair| order(pair[0].topic) <= order(pair[1].topic)),
            "the keys are grouped"
        );
        assert!(names(&listed).contains(&"exit"), "/exit beside /quit");
    }

    #[test]
    fn a_typed_part_takes_the_names_first_then_what_they_do() {
        assert_eq!(
            names(&fitting("c")),
            ["clear", "compact", "context", "close", "cache"]
        );
        assert_eq!(names(&fitting("hook")), ["skill"]);
        let con = fitting("con");
        assert_eq!(names(&con[..1]), ["context"], "the names first");
        assert!(
            names(&con).contains(&"compact"),
            "then a word of what it does: conversation"
        );
        assert!(fitting("or").is_empty(), "two letters match no description");
        assert!(fitting("zz").is_empty());
    }

    #[test]
    fn the_help_boxs_keys_entry_writes_every_key_by_topic() {
        let help = help_more();
        for key in &KEYS {
            assert!(help.contains(key.key), "{}", key.key);
        }
        assert!(help.contains("  View: ⌃F search the stream"), "{help}");
    }

    #[test]
    fn the_sheet_names_the_commands_only_where_there_is_room() {
        let text = |lines: Vec<Line>| {
            lines
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let narrow = text(sheet(28));
        assert!(narrow.contains(" Session") && narrow.contains(" Help"));
        assert!(!narrow.contains("/gate"));
        assert!(narrow.lines().all(|row| row.width() <= 28), "{narrow}");
        let wide = text(sheet(60));
        assert!(wide.contains("· /gate"));
    }
}
