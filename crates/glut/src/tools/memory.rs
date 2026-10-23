//! The `memory` tool (`docs/plans/glut-tui.md`, T35): the project's memory, kept outside the repository — Glut's own
//! (`~/.glut/memory/<project>/`) or, in a repository Claude Code works in, Claude's (`~/.claude/projects/<path>/memory/`)
//! — in Claude Code's form: one Markdown file per fact with its frontmatter, and `MEMORY.md`, the index of one line
//! per fact. The file tools never reach it (they stay in the working folder, and never write `.glut` or `.claude`),
//! so this tool is the one way in, and only into that folder: a fact is named, never a path.

use std::path::{Path, PathBuf};

use axiomata_files::{
    EntryKind, LinkPolicy, MAX_WRITE_BYTES, Root, delete, list_dir, read_text, write_text,
};
use serde_json::{Value, json};

use crate::model::ToolSpec;
use crate::permissions::Access;
use crate::tools::{CallContext, Tool, ToolFuture};

/// The tool's name.
pub const NAME: &str = "memory";

/// The index's name, without its `.md`.
pub const INDEX: &str = "MEMORY";
/// The index's file.
pub const INDEX_FILE: &str = "MEMORY.md";

/// The most bytes one fact may hold: a fact is a few lines, a file this size is a document gone astray.
const MAX_FACT_BYTES: u64 = 32 * 1024;

/// The most characters of a fact's name.
const MAX_NAME_CHARS: usize = 80;

/// The project's memory folder.
pub struct Memory {
    dir: PathBuf,
}

impl Memory {
    pub fn new(dir: PathBuf) -> Self {
        Memory { dir }
    }
}

/// What a call asks for.
enum Action {
    List,
    Read(String),
    Write(String, String),
    Delete(String),
}

impl Action {
    fn of(arguments: &Value) -> Result<Self, String> {
        let name = || -> Result<String, String> {
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .ok_or("`name` is required")?;
            let name = name.strip_suffix(".md").unwrap_or(name);
            if valid_name(name) {
                Ok(name.to_owned())
            } else {
                Err(format!(
                    "a fact's name is letters, digits, `-` and `_`, at most {MAX_NAME_CHARS} characters — never a path"
                ))
            }
        };
        match arguments.get("action").and_then(Value::as_str) {
            Some("list") => Ok(Action::List),
            Some("read") => Ok(Action::Read(name()?)),
            Some("write") => {
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or("`content` is required to write")?;
                Ok(Action::Write(name()?, content.to_owned()))
            }
            Some("delete") => Ok(Action::Delete(name()?)),
            _ => Err("`action` is one of list, read, write, delete".into()),
        }
    }
}

impl Tool for Memory {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: NAME.to_owned(),
            description: format!(
                "The project's memory, which lasts between sessions: one Markdown file per fact, with frontmatter \
                 (name, description, type), and the index {INDEX}, one line per fact. list names the facts, read \
                 gives one, write keeps one (add its line to {INDEX} too), delete removes one. Keep what is not in \
                 the repository: the owner's preferences, decisions and why, where work stands."
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "read", "write", "delete"] },
                    "name": { "type": "string", "description": "The fact's name (or MEMORY), without .md." },
                    "content": { "type": "string", "description": "The whole file, to write." }
                },
                "required": ["action"]
            }),
        }
    }

    fn access(&self, arguments: &Value) -> Access {
        // Reading the project's own memory is like reading the folder: free — its index is in the prompt already.
        // A change outlives the run and reaches every later session: the owner sees what and where (`Access::Memory`).
        let place = self.dir.display();
        match Action::of(arguments) {
            Ok(Action::List | Action::Read(_)) => Access::Own,
            Ok(Action::Write(name, content)) => Access::Memory {
                change: format!("keep {name} ({} bytes) in {place}", content.len()),
            },
            Ok(Action::Delete(name)) => Access::Memory {
                change: format!("remove {name} from {place}"),
            },
            Err(_) => Access::Memory {
                change: format!("a call the tool cannot read, in {place}"),
            },
        }
    }

    fn offered_as(&self) -> Access {
        Access::Other
    }

    fn replay_safe(&self, arguments: &Value) -> bool {
        matches!(Action::of(arguments), Ok(Action::List | Action::Read(_)))
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        let dir = self.dir.clone();
        Box::pin(async move {
            let action = Action::of(&arguments)?;
            tokio::task::spawn_blocking(move || answer(&dir, action))
                .await
                .map_err(|err| format!("the memory could not be reached: {err}"))?
        })
    }
}

/// One call, on a blocking thread.
fn answer(dir: &Path, action: Action) -> Result<String, String> {
    let describe = |err: axiomata_files::FilesError| err.to_string();
    match std::fs::symlink_metadata(dir) {
        // A linked memory folder would root the tool wherever the link points.
        Ok(meta) if !meta.is_dir() => {
            return Err(format!(
                "{} is no plain folder: the memory is not used",
                dir.display()
            ));
        }
        Ok(_) => {}
        Err(_) => match &action {
            Action::Write(..) => make_private(dir)?,
            Action::List => return Ok("the memory is empty: nothing kept yet".into()),
            Action::Read(name) | Action::Delete(name) => {
                return Err(format!("no fact {name}: the memory is empty"));
            }
        },
    }
    let root = Root::dir(dir, LinkPolicy::Contained).map_err(describe)?;
    match action {
        Action::List => {
            let listing = list_dir(&root, "").map_err(describe)?;
            let facts: Vec<String> = listing
                .entries
                .into_iter()
                .filter(|entry| entry.kind == EntryKind::File)
                .filter_map(|entry| entry.name.strip_suffix(".md").map(str::to_owned))
                // Only what the tool can reach by name: another file of the folder is no fact of its.
                .filter(|name| valid_name(name))
                .collect();
            Ok(if facts.is_empty() {
                "the memory is empty: nothing kept yet".into()
            } else {
                facts.join("\n")
            })
        }
        Action::Read(name) => read_text(&root, &format!("{name}.md"), MAX_FACT_BYTES)
            .map(|file| file.content)
            .map_err(describe),
        Action::Write(name, content) => {
            if content.len() as u64 > MAX_FACT_BYTES.min(MAX_WRITE_BYTES) {
                return Err(format!(
                    "a fact holds at most {MAX_FACT_BYTES} bytes: keep it short, the details belong in the repository"
                ));
            }
            write_text(&root, &format!("{name}.md"), &content, None, MAX_FACT_BYTES)
                .map_err(describe)?;
            Ok(format!("kept {name}"))
        }
        Action::Delete(name) => {
            delete(&root, &format!("{name}.md")).map_err(describe)?;
            Ok(format!("deleted {name}"))
        }
    }
}

/// A fact's name: letters, digits, `-` and `_`, never a path nor a flag.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= MAX_NAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !name.starts_with('-')
}

/// The memory's folder, made the first time a fact is kept: the owner's alone.
fn make_private(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt as _;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|err| {
            format!(
                "the memory's folder {} could not be made: {err}",
                dir.display()
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "glut-memory-{tag}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    fn call(dir: &Path, arguments: Value) -> Result<String, String> {
        answer(dir, Action::of(&arguments)?)
    }

    #[test]
    fn facts_are_kept_listed_read_and_deleted_and_the_folder_comes_with_the_first() {
        let dir = scratch("round");
        assert!(
            call(&dir, json!({"action": "list"}))
                .unwrap()
                .contains("empty")
        );
        assert!(!dir.exists(), "nothing made for a look");
        call(&dir, json!({"action": "write", "name": "prefers-german", "content": "---\nname: x\n---\nGerman."}))
            .unwrap();
        call(
            &dir,
            json!({"action": "write", "name": "MEMORY.md", "content": "- [x](prefers-german.md)"}),
        )
        .unwrap();
        let listed = call(&dir, json!({"action": "list"})).unwrap();
        assert!(
            listed.contains("prefers-german") && listed.contains("MEMORY"),
            "{listed}"
        );
        assert!(
            call(&dir, json!({"action": "read", "name": "prefers-german"}))
                .unwrap()
                .ends_with("German.")
        );
        call(&dir, json!({"action": "delete", "name": "prefers-german"})).unwrap();
        assert!(call(&dir, json!({"action": "read", "name": "prefers-german"})).is_err());
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_name_is_never_a_path_and_reading_is_free_while_changing_is_judged() {
        for name in ["../secret", "a/b", "", "-rf", ".hidden"] {
            assert!(
                Action::of(&json!({"action": "read", "name": name})).is_err(),
                "{name:?}"
            );
        }
        let memory = Memory::new(PathBuf::from("/m"));
        assert_eq!(
            memory.access(&json!({"action": "read", "name": "x"})),
            Access::Own
        );
        assert_eq!(
            memory.access(&json!({"action": "write", "name": "x", "content": "abc"})),
            Access::Memory {
                change: "keep x (3 bytes) in /m".into()
            }
        );
        assert!(!memory.replay_safe(&json!({"action": "delete", "name": "x"})));
    }
}
