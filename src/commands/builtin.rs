//! The built-in commands (`docs/plans/harness.md`, HS4, C5): written in Rust, not read from a file, always there
//! and always before a file's command of the same name — and answered without a model, so their text goes to the
//! front end and costs nothing.
//!
//! The names `clear`, `model` and `compact` are reserved although they answer nothing yet: they come with the TUI,
//! and naming them now stops a file from taking one (C5).

use super::{Command, PermissionsView, Registry, Source};
use crate::permissions::printable;

/// What a built-in answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// `/help`: the commands of this run.
    Help,
    /// `/permissions`: the run's mode, its rules and the sandbox.
    Permissions,
    /// Reserved for a later step: it answers that it is not available in a single run yet.
    Unavailable,
}

/// One built-in command.
struct Builtin {
    /// The name as it is typed after the `/`.
    name: &'static str,
    description: &'static str,
    kind: Kind,
}

/// Every built-in, in the order `/help` lists them.
const BUILTINS: [Builtin; 5] = [
    Builtin {
        name: "help",
        description: "Lists every command with its source, argument hint and description.",
        kind: Kind::Help,
    },
    Builtin {
        name: "permissions",
        description: "Shows this run's mode, its allow, ask and deny rules, and the sandbox.",
        kind: Kind::Permissions,
    },
    Builtin {
        name: "clear",
        description: "Starts a fresh conversation (not in a single run yet).",
        kind: Kind::Unavailable,
    },
    Builtin {
        name: "model",
        description: "Shows or changes the model (not in a single run yet).",
        kind: Kind::Unavailable,
    },
    Builtin {
        name: "compact",
        description: "Summarises the conversation so far (not in a single run yet).",
        kind: Kind::Unavailable,
    },
];

/// The built-ins as commands, first in the registry.
pub(super) fn builtins() -> Vec<Command> {
    BUILTINS
        .iter()
        .map(|builtin| Command {
            name: builtin.name.to_owned(),
            source: Source::Builtin,
            description: Some(builtin.description.to_owned()),
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            body: String::new(),
        })
        .collect()
}

/// Whether a name belongs to a built-in: a command file of that name is skipped, never an error (C5).
pub(super) fn holds(name: &str) -> bool {
    BUILTINS.iter().any(|builtin| builtin.name == name)
}

/// What the built-in `name` answers with; [`None`] when `name` is no built-in.
pub(super) fn text(name: &str, registry: &Registry) -> Option<String> {
    let builtin = BUILTINS.iter().find(|builtin| builtin.name == name)?;
    Some(match builtin.kind {
        Kind::Help => help(registry),
        Kind::Permissions => permissions(registry.permissions_view()),
        Kind::Unavailable => format!("/{name} is not available in a single run yet"),
    })
}

/// `/help`: one line per command — name, source, argument hint and description (C5).
fn help(registry: &Registry) -> String {
    registry
        .commands()
        .iter()
        .map(|command| {
            let mut line = format!("/{} ({})", command.name, command.source.as_str());
            if let Some(hint) = &command.argument_hint {
                line.push_str(&format!(" [{hint}]"));
            }
            if let Some(description) = &command.description {
                line.push_str(" — ");
                line.push_str(description);
            }
            printable(&line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `/permissions`: the mode, the three rule lists and the sandbox, one line each (C5).
fn permissions(view: &PermissionsView) -> String {
    let mut lines = vec![format!("mode: {}", view.mode)];
    for (name, rules) in [
        ("allow", &view.allow),
        ("ask", &view.ask),
        ("deny", &view.deny),
    ] {
        lines.push(format!("{name}:"));
        if rules.is_empty() {
            lines.push("  (none)".to_owned());
        } else {
            lines.extend(rules.iter().map(|rule| format!("  {rule}")));
        }
    }
    let sandbox = &view.sandbox;
    lines.push(format!("sandbox.enabled: {}", sandbox.enabled));
    lines.push(format!(
        "sandbox.allowWrite: {}",
        joined(&sandbox.allow_write)
    ));
    lines.push(format!("sandbox.denyRead: {}", joined(&sandbox.deny_read)));
    lines.push(format!(
        "sandbox.allowLocalBinding: {}",
        sandbox.allow_local_binding
    ));
    lines
        .into_iter()
        .map(|line| printable(&line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Paths as one line: the list of writable or unreadable places, or nothing at all.
fn joined(paths: &[std::path::PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
