//! `editor` (T64c): what the owner's editor shows — the files open in it, and a file's diagnostics as its language
//! servers see the buffer, unsaved changes too. Offered only while an editor that says so is connected
//! ([`crate::editor::EditorFiles::offers_tools`]); a file is named only where it lies in the run's folders, and its
//! diagnostics are a read of it, judged by the gate like `read_file`.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Access, CallContext, Tool, ToolFuture, ToolSpec};
use crate::editor::Editor;
use crate::folders::Folders;

/// The tool's name.
pub const NAME: &str = "editor";

/// The owner's editor, read through the toolbox's [`Editor`].
pub struct EditorTool {
    cwd: PathBuf,
    beside: Folders,
    editor: Editor,
}

impl EditorTool {
    pub fn new(cwd: &Path, beside: Folders, editor: Editor) -> Self {
        Self {
            cwd: cwd.to_owned(),
            beside,
            editor,
        }
    }

    /// `path` as the run names it — below the working folder relative, in a folder beside it by its full path — where
    /// it lies in a folder of the run, judged where it really leads (links and `..` resolved); `None` elsewhere, which
    /// the model is not told of.
    fn named(&self, path: &Path) -> Option<String> {
        let real = path.canonicalize().ok()?;
        let cwd = self.cwd.canonicalize().ok()?;
        if let Ok(inside) = real.strip_prefix(&cwd) {
            return Some(inside.display().to_string());
        }
        self.beside
            .list()
            .iter()
            .any(|folder| real.starts_with(&folder.real))
            .then(|| real.display().to_string())
    }
}

impl Tool for EditorTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: NAME.into(),
            description: concat!(
                "What the owner's editor shows: action \"open_files\" lists the files open in it (the active one ",
                "first; \"unsaved\" marks one with changes not saved yet), action \"diagnostics\" gives a file's ",
                "errors and warnings as the editor's language servers see it. The owner's message already says ",
                "which file is active and what is selected."
            )
            .into(),
            parameters: json!({"type": "object", "properties": {
                "action": {"type": "string", "enum": ["open_files", "diagnostics"]},
                "path": {"type": "string", "description": "the file, for diagnostics"}
            }, "required": ["action"]}),
        }
    }

    fn access(&self, arguments: &Value) -> Access {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .to_owned();
        Access::Read { path }
    }

    fn offered_now(&self) -> bool {
        self.editor.get().is_some_and(|files| files.offers_tools())
    }

    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_> {
        // What a read rule holds back is left out, as a search leaves it out.
        let gate = context.gate;
        Box::pin(async move {
            let files = self.editor.get().ok_or("no editor is connected")?;
            let action = arguments
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let path = arguments
                .get("path")
                .and_then(Value::as_str)
                .map(|path| self.cwd.join(path));
            let inside = path
                .as_deref()
                .is_some_and(|path| self.named(path).is_some());
            // The editor may take a moment; the runtime's workers do not wait for it.
            let asked = tokio::task::spawn_blocking(move || match (action.as_str(), path) {
                ("open_files", _) => Ok(Asked::Open(files.open_files())),
                ("diagnostics", Some(path)) if inside => {
                    Ok(Asked::Diagnostics(files.diagnostics(&path)))
                }
                ("diagnostics", Some(_)) => {
                    Err("that file lies outside the run's folders".to_owned())
                }
                ("diagnostics", None) => Err("diagnostics needs a path".to_owned()),
                (other, _) => Err(format!("no action {other:?}: open_files or diagnostics")),
            })
            .await
            .map_err(|err| err.to_string())??;
            match asked {
                Asked::Open(None) | Asked::Diagnostics(None) => {
                    Err("the editor did not answer".to_owned())
                }
                Asked::Open(Some(open)) => {
                    let lines: Vec<String> = open
                        .iter()
                        .filter_map(|file| {
                            let name = self.named(&file.path)?;
                            if gate.as_ref().is_some_and(|gate| gate.read_held_back(&name)) {
                                return None;
                            }
                            let mut line = name;
                            if file.active {
                                line.push_str(" (active)");
                            }
                            if file.dirty {
                                line.push_str(" (unsaved)");
                            }
                            Some(line)
                        })
                        .collect();
                    Ok(if lines.is_empty() {
                        "no file of the run's folders is open in the editor".to_owned()
                    } else {
                        lines.join("\n")
                    })
                }
                Asked::Diagnostics(Some(found)) if found.is_empty() => {
                    Ok("no diagnostics".to_owned())
                }
                Asked::Diagnostics(Some(found)) => Ok(found
                    .iter()
                    .map(|diagnostic| {
                        let source = diagnostic
                            .source
                            .as_deref()
                            .map(|source| format!(" ({source})"))
                            .unwrap_or_default();
                        format!(
                            "{}:{} {}{source}: {}",
                            diagnostic.line,
                            diagnostic.character,
                            diagnostic.severity,
                            diagnostic.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")),
            }
        })
    }
}

/// What the editor answered.
enum Asked {
    Open(Option<Vec<crate::editor::OpenFile>>),
    Diagnostics(Option<Vec<crate::editor::Diagnostic>>),
}
