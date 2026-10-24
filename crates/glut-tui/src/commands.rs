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
    Changes,
    Project,
    Settings,
    /// Keys only: what the input does.
    Input,
    Help,
}

impl Topic {
    pub const ALL: [Topic; 8] = [
        Topic::Session,
        Topic::Model,
        Topic::View,
        Topic::Changes,
        Topic::Project,
        Topic::Settings,
        Topic::Input,
        Topic::Help,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Topic::Session => "Session",
            Topic::Model => "Model",
            Topic::View => "View",
            Topic::Changes => "Changes",
            Topic::Project => "Project",
            Topic::Settings => "Settings",
            Topic::Input => "Input",
            Topic::Help => "Help",
        }
    }
}

/// A `/` command: its name, what it takes as `/help` writes it, what it does, and its topic.
#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub name: &'static str,
    pub args: &'static str,
    pub what: &'static str,
    pub topic: Topic,
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
pub const COMMANDS: [Command; 28] = [
    command(Topic::Session, "clear", "", "start a new session"),
    command(
        Topic::Session,
        "resume",
        "",
        "open an earlier session of this folder — the forks under theirs",
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
        "the session's subagents: open one's own stream (a click on its tile does too; ⌃A: the Agents tab)",
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

const fn key(topic: Topic, key: &'static str, what: &'static str, command: &'static str) -> Key {
    Key {
        key,
        what,
        command,
        topic,
    }
}

/// Every key Glut has, grouped as [`COMMANDS`] are; a topic without keys shows no heading.
pub const KEYS: [Key; 25] = [
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
        "⌃A",
        "the Agents tab: every subagent, live",
        "/agents",
    ),
    key(Topic::View, "PgUp PgDn", "scroll the stream", ""),
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
    key(Topic::Input, "↑", "the messages sent before", ""),
    key(Topic::Input, "⇧⏎", "a new line (⌥⏎ too)", ""),
    key(
        Topic::Input,
        "⌃X E",
        "edit the input in your editor",
        "/editor",
    ),
    key(Topic::Help, "⌃K", "this sheet of keys", "/help"),
];

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
    if typed.chars().count() >= 3 {
        let in_what = listed.filter(|command| {
            !command.name.starts_with(&typed)
                && command
                    .what
                    .to_lowercase()
                    .split(|c: char| !c.is_alphanumeric())
                    .any(|word| word.starts_with(&typed))
        });
        fits.extend(in_what);
    }
    fits
}

/// What `/help` says: the commands, one row per topic, then the keys the same way, then what is neither.
pub fn help() -> String {
    let mut rows = vec!["Commands".to_owned()];
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
    rows.push(format!("Keys ({LEGEND}; ⌃K keeps them in the side panel)"));
    for topic in Topic::ALL {
        let keys: Vec<String> = KEYS
            .iter()
            .filter(|key| key.topic == topic)
            .map(|key| format!("{} {}", key.key, key.what))
            .collect();
        if !keys.is_empty() {
            rows.push(format!("  {}: {}", topic.label(), keys.join(" · ")));
        }
    }
    rows.push(
        "Also: !cmd runs a command and shows Glut its output · !!cmd runs it for you alone · a large paste stands \
         as [pasted N lines] · drag to copy · a click on a subagent's tile opens it"
            .to_owned(),
    );
    rows.push(
        "/vim on: ⎋ to Normal mode (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y with a motion or doubled · \
         counts · p P · u ⌃R), i a I A o O to Insert; ⎋ in Normal mode stops Glut"
            .to_owned(),
    );
    rows.join("\n")
}

/// What the key symbols stand for (T39), atop the Keys sheet and in `/help`: read once, then the short forms are
/// plain.
pub const LEGEND: &str = "⌃ Ctrl · ⇧ Shift · ⌥ Alt · ⏎ Enter · ⇥ Tab · ⎋ Esc";

/// The panel width from which a key's row also names its `/` command.
pub const SHEET_WITH_COMMANDS: usize = 46;

/// The Keys sheet `width` cells wide: each topic's heading, then its keys — the key in a column of its own, what it
/// does wrapped beside it, the `/` command after it where the sheet is wide enough.
pub fn sheet(width: usize) -> Vec<Line<'static>> {
    let key_width = KEYS.iter().map(|key| key.key.width()).max().unwrap_or(0) + 2;
    let what_width = width.saturating_sub(key_width + 1).max(8);
    let commands = width >= SHEET_WITH_COMMANDS;
    let mut lines: Vec<Line<'static>> = wrap(LEGEND, width.saturating_sub(1).max(8))
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
                command if commands && !command.is_empty() => format!("{} · {command}", key.what),
                _ => key.what.to_owned(),
            };
            for (at, row) in wrap(&what, what_width).into_iter().enumerate() {
                let head = if at == 0 { key.key } else { "" };
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

    fn names(commands: &[&Command]) -> Vec<&'static str> {
        commands.iter().map(|command| command.name).collect()
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
        assert_eq!(names(&fitting("c")), ["clear", "compact", "context"]);
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
