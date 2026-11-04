//! `glut trust`: the folders whose own `.glut/` settings count in full — the same as `/trust` in the terminal UI. A
//! command Glut runs is refused every change: trust is the owner's word alone.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Subcommand;

pub use crate::permissions::settings::{Refused, may_trust};
use crate::permissions::settings::{Trust, project_summary};

/// What `glut trust` does.
#[derive(Subcommand)]
pub enum TrustCommand {
    /// Shows whether a folder is trusted, and what its own .glut settings add once it is
    Show {
        /// The folder; the current one when left out
        folder: Option<PathBuf>,
    },
    /// Lists the trusted folders
    List,
    /// Trusts a folder: its own allow rules, mode, skills, agents and commands count from its next session on
    Add {
        /// The folder; the current one when left out
        folder: Option<PathBuf>,
    },
    /// Trusts a folder no more: only its deny and ask rules count from its next session on
    Remove {
        /// The folder; the current one when left out
        folder: Option<PathBuf>,
    },
}

/// What a folder's own `.glut` adds once it is trusted, a line each: its allow rules, its mode (which counts only
/// where it is no looser than the owner's), and the names of its skills, agents and commands. Text from the project's
/// files is shown with its control characters as spaces — a folder's file may hold anything.
///
/// # Errors
///
/// A settings file of the project that cannot be read.
pub fn offered(folder: &Path) -> Result<Vec<String>, String> {
    let (allow, mode) = project_summary(folder).map_err(|err| err.to_string())?;
    let mut lines: Vec<String> = allow
        .iter()
        .map(|rule| format!("allow   {}", printable(rule)))
        .collect();
    if let Some(mode) = mode {
        lines.push(format!("mode    {} (if no looser than yours)", mode.name()));
    }
    let own = folder.join(".glut");
    for name in named(&own.join("skills"), Shape::SkillFolder) {
        lines.push(format!("skill   {name}"));
    }
    for name in named(&own.join("agents"), Shape::MarkdownFile) {
        lines.push(format!("agent   {name}"));
    }
    for name in named(&own.join("commands"), Shape::MarkdownTree) {
        lines.push(format!("command /{name}"));
    }
    Ok(lines)
}

/// How a kind of the project's files is laid out.
#[derive(Clone, Copy)]
enum Shape {
    /// `<name>/SKILL.md`.
    SkillFolder,
    /// `<name>.md`.
    MarkdownFile,
    /// `<path>/<name>.md` at any depth, named `path:name`.
    MarkdownTree,
}

/// The names below `dir` in `shape`, sorted, printable. Links are not followed: what is shown is what lies there.
fn named(dir: &Path, shape: Shape) -> Vec<String> {
    let mut names = Vec::new();
    walk(dir, "", shape, &mut names);
    names.sort();
    names
}

fn walk(dir: &Path, prefix: &str, shape: Shape, names: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let name = printable(&entry.file_name().to_string_lossy());
        match shape {
            Shape::SkillFolder if kind.is_dir() && entry.path().join("SKILL.md").is_file() => {
                names.push(name)
            }
            Shape::MarkdownFile | Shape::MarkdownTree if kind.is_file() => {
                if let Some(stem) = name.strip_suffix(".md") {
                    names.push(format!("{prefix}{stem}"));
                }
            }
            Shape::MarkdownTree if kind.is_dir() => {
                walk(&entry.path(), &format!("{prefix}{name}:"), shape, names);
            }
            _ => {}
        }
    }
}

/// `text` with every control character as a space and every character that hides or reorders text (bidi marks and
/// overrides, zero-width characters, line and paragraph separators, a soft hyphen, the blank braille cell) taken out —
/// a project's rule must look like what it is.
pub fn printable(text: &str) -> String {
    text.chars()
        .filter(|c| !hides(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn hides(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}' | '\u{2800}' | '\u{FEFF}'
    )
}

/// Runs one `glut trust` command.
pub fn run(command: TrustCommand) -> ExitCode {
    let home = match crate::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(&err.to_string()),
    };
    let changes = matches!(
        command,
        TrustCommand::Add { .. } | TrustCommand::Remove { .. }
    );
    if changes && std::env::var_os(crate::tools::shell::SANDBOXED_ENV).is_some() {
        return fail(say!(trust_owners_only));
    }
    let mut trust = match Trust::load(&home) {
        Ok(trust) => trust,
        Err(err) => return fail(&err.to_string()),
    };
    match command {
        TrustCommand::List => {
            if trust.folders().is_empty() {
                println!("{}", say!(trust_none));
            }
            for folder in trust.folders() {
                println!("{}", printable(&folder.display().to_string()));
            }
            ExitCode::SUCCESS
        }
        TrustCommand::Show { folder } => {
            let folder = match folder_of(folder) {
                Ok(folder) => folder,
                Err(err) => return fail(&err),
            };
            let shown = printable(&folder.display().to_string());
            if trust.contains(&folder) {
                println!("{}", say!(trust_is, folder = shown));
            } else {
                println!("{}", say!(trust_is_not, folder = shown));
            }
            match offered(&folder) {
                Ok(lines) if lines.is_empty() || trust.contains(&folder) => {}
                Ok(lines) => {
                    println!("{}", say!(trust_adds));
                    for line in lines {
                        println!("  {line}");
                    }
                }
                Err(err) => return fail(&err),
            }
            ExitCode::SUCCESS
        }
        TrustCommand::Add { folder } => {
            let folder = match folder_of(folder) {
                Ok(folder) => folder,
                Err(err) => return fail(&err),
            };
            match may_trust(&folder, &home) {
                Err(Refused::Home) => return fail(say!(trust_not_home)),
                Err(Refused::Root) => return fail(say!(trust_not_root)),
                Err(Refused::Shared) => return fail(say!(trust_not_shared)),
                Ok(()) => {}
            }
            match trust.add(&home, &folder) {
                Ok(()) => {
                    println!(
                        "{}",
                        say!(
                            trust_added,
                            folder = printable(&folder.display().to_string())
                        )
                    );
                    ExitCode::SUCCESS
                }
                Err(err) => fail(&err.to_string()),
            }
        }
        TrustCommand::Remove { folder } => {
            // A folder gone from the disk is still taken off the list by the path it was kept under.
            let folder = match folder {
                Some(folder) => match folder.canonicalize() {
                    Ok(real) => real,
                    Err(_) => std::path::absolute(&folder).unwrap_or(folder),
                },
                None => match folder_of(None) {
                    Ok(folder) => folder,
                    Err(err) => return fail(&err),
                },
            };
            let shown = printable(&folder.display().to_string());
            match trust.remove(&home, &folder) {
                Ok(true) => {
                    println!("{}", say!(trust_removed, folder = shown));
                    ExitCode::SUCCESS
                }
                Ok(false) => {
                    println!("{}", say!(trust_was_not, folder = shown));
                    ExitCode::SUCCESS
                }
                Err(err) => fail(&err.to_string()),
            }
        }
    }
}

/// The folder named, or the current one, canonical — the form the list keeps.
fn folder_of(folder: Option<PathBuf>) -> Result<PathBuf, String> {
    let folder = match folder {
        Some(folder) => folder,
        None => std::env::current_dir().map_err(|err| err.to_string())?,
    };
    folder
        .canonicalize()
        .map_err(|err| say!(cannot_use, path = folder.display(), err = err))
}

/// Says why it failed — an error may quote a project's file, so it is made printable too.
fn fail(message: &str) -> ExitCode {
    eprintln!("glut trust: {}", printable(message));
    ExitCode::from(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neither_the_home_folder_nor_the_root_is_trusted() {
        let home = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("glut-trust-home-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(may_trust(&home, &home), Err(Refused::Home));
        assert_eq!(may_trust(Path::new("/"), &home), Err(Refused::Root));
        let project = home.join(format!("glut-trust-may-{}", std::process::id()));
        std::fs::create_dir_all(&project).unwrap();
        assert_eq!(may_trust(&project, &home), Ok(()));
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&project, std::fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            may_trust(&project, &home),
            Err(Refused::Shared),
            "anybody may write it"
        );
        std::fs::remove_dir_all(&project).unwrap();
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn what_a_folder_adds_is_shown_without_control_characters() {
        let dir = std::env::temp_dir().join(format!("glut-trust-offered-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".glut")).unwrap();
        std::fs::write(
            dir.join(".glut/settings.json"),
            r#"{"permissions": {"allow": ["Bash(make \u001b[2J \u202e\u200b*)"], "defaultMode": "acceptEdits"}}"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.join(".glut/skills/deploy")).unwrap();
        std::fs::write(dir.join(".glut/skills/deploy/SKILL.md"), "x").unwrap();
        std::fs::create_dir_all(dir.join(".glut/agents")).unwrap();
        std::fs::write(dir.join(".glut/agents/helper.md"), "x").unwrap();
        std::fs::create_dir_all(dir.join(".glut/commands/git")).unwrap();
        std::fs::write(dir.join(".glut/commands/git/commit.md"), "x").unwrap();
        let lines = offered(&dir).unwrap();
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert_eq!(lines[0], "allow   Bash(make  [2J *)", "{lines:?}");
        assert!(lines[1].starts_with("mode    "), "{lines:?}");
        assert_eq!(
            lines[2..],
            ["skill   deploy", "agent   helper", "command /git:commit"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
