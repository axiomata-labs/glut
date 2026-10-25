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
pub const COMMANDS: [Command; 49] = [
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
        "[id]",
        "switch the provider, or /provider <id>",
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
        "on|asks|off|system|after",
        "tell me when a turn ends or asks while the window is elsewhere: on, asks only, off, macOS's own (system); \
         /notify after <s>",
    ),
    command(
        Topic::Settings,
        "motion",
        "full|calm|off",
        "how much the screen moves; calm and off hold Glimm still (macOS Reduce motion counts as off)",
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
pub const KEYS: [Key; 41] = [
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
        "a word left, a word right (⌥← ⌥→ too)",
        "",
    ),
    key(Topic::Editing, "⌃W", "delete the word before (⌥⌫ too)", ""),
    key(Topic::Editing, "⌃U", "delete to the line's start", ""),
    key(Topic::Editing, "⌃J", "a new line", ""),
    key(
        Topic::Editing,
        "⌃D",
        "quit, on an empty line while Glut rests",
        "",
    ),
    key(Topic::Help, "⌃K", "this sheet of keys", "/help"),
];

/// The words a command takes when they are a fixed few (`on|off`, `full|calm|off`); `None` for anything freer.
fn choices(args: &str) -> Option<Vec<&str>> {
    let words: Vec<&str> = args.split('|').collect();
    (words.len() > 1
        && words.iter().all(|word| {
            !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        }))
    .then_some(words)
}

/// What a typed command still takes, shown faintly after the cursor (TUI9): after `/name ` its arguments as `/help`
/// writes them, and after the start of one of its fixed words the rest of that word.
pub fn ghost(text: &str) -> Option<String> {
    let (name, rest) = text.strip_prefix('/')?.split_once(' ')?;
    let command = COMMANDS.iter().find(|command| command.name == name)?;
    if command.args.is_empty() || text.contains('\n') {
        return None;
    }
    if rest.is_empty() {
        return Some(command.args.to_owned());
    }
    completion(text)
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
    // A project is a folder; every other place takes a file as well.
    let wants = if name == "projects" {
        crate::paths::Wants::Folders
    } else {
        crate::paths::Wants::Any
    };
    Some((wants, fixed))
}

/// The rest of the fixed word a typed command's argument begins (`/motion c` → `alm`), what ⇥ adds.
pub fn completion(text: &str) -> Option<String> {
    let (name, rest) = text.strip_prefix('/')?.split_once(' ')?;
    if rest.is_empty() || rest.contains(char::is_whitespace) {
        return None;
    }
    let command = COMMANDS.iter().find(|command| command.name == name)?;
    choices(command.args)?
        .into_iter()
        .find(|word| word.starts_with(rest) && *word != rest)
        .map(|word| word[rest.len()..].to_owned())
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

/// What `/help` says: the commands, one row per topic, then the keys the same way, then what is neither.
pub fn help() -> String {
    let mut rows = vec![tr!(help_commands).to_owned()];
    for topic in Topic::ALL {
        let names: Vec<String> = COMMANDS
            .iter()
            .filter(|command| command.topic == topic)
            .map(|command| match command.args {
                "" => format!("/{}", command.name),
                args => format!("/{} {args}", command.name),
            })
            .collect();
        if !names.is_empty() {
            rows.push(format!("  {}: {}", topic.label(), names.join(" · ")));
        }
    }
    rows.push(tr!(help_keys, legend = tr!(legend)));
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
        let help = help();
        assert!(help.starts_with("Befehle\n  Session: /clear"), "{help}");
        assert!(help.contains("Tasten (⌃ Control · ⇧ Umschalt"), "{help}");
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

    #[test]
    fn a_typed_command_shows_what_it_takes_and_tab_completes_a_fixed_word() {
        assert_eq!(ghost("/motion ").as_deref(), Some("full|calm|off"));
        assert_eq!(ghost("/motion c").as_deref(), Some("alm"));
        assert_eq!(completion("/motion c").as_deref(), Some("alm"));
        assert_eq!(ghost("/motion calm"), None, "the word is whole");
        assert_eq!(ghost("/motion x"), None);
        assert_eq!(
            ghost("/bg ").as_deref(),
            Some("[name] [as <role>] [here|tree]")
        );
        assert_eq!(completion("/bg h"), None, "free words are not completed");
        assert_eq!(ghost("/motion"), None, "the name still typed");
        assert_eq!(ghost("/clear "), None, "nothing to take");
        assert_eq!(ghost("hello /motion "), None);
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
    fn help_writes_every_command_and_key_by_topic() {
        let help = help();
        for command in &COMMANDS {
            assert!(
                help.contains(&format!("/{}", command.name)),
                "/{}",
                command.name
            );
        }
        for key in &KEYS {
            assert!(help.contains(key.key), "{}", key.key);
        }
        assert!(help.contains("  Model: /model [id] · /provider [id]"));
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
