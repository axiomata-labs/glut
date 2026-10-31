//! An editor's open files (T64b): where Glut is an editor's agent, its file tools read a file as the editor holds it —
//! an open tab's unsaved changes too — and a change of a file whose tab holds unsaved changes waits until the owner
//! saved it (the owner's word: Glut writes the disk alone). Every check stays Glut's own and comes first (the gate,
//! the root's verdict on the path, the protected places, no link on the way); the editor only hands over a text.
//! Commands see the disk. What stays open: the editor opens the path itself, outside the sandbox — a link swapped in
//! between Glut's check and its read would be followed (the owner took that, 2026-10-31).
//!
//! T64c: an editor plugin of Glut's own (`docs/editor-protocol.md`) says more — the file, the cursor and the selection
//! the owner works on, the open files, a file's diagnostics — and hands over a buffer only as the editor holds it,
//! never opening a path itself.

use std::path::Path;
use std::sync::{Arc, RwLock};

/// What an editor does with its files for Glut.
pub trait EditorFiles: Send + Sync {
    /// The text of `path` (absolute) as the editor holds it, an open tab's unsaved changes too; `None` where it cannot
    /// say. Called off the runtime's workers: it may wait for the editor.
    fn read(&self, path: &Path) -> Option<String>;

    /// Where the owner works now: the active file, the cursor, a selection — `None` where the editor does not say.
    fn context(&self) -> Option<Context> {
        None
    }

    /// Whether the editor says what is open in it and a file's diagnostics: the `editor` tool is offered only then.
    /// Asked whenever the tools are listed, so it answers at once, without asking the editor.
    fn offers_tools(&self) -> bool {
        false
    }

    /// The files open in the editor, the active first — `None` where it does not say.
    fn open_files(&self) -> Option<Vec<OpenFile>> {
        None
    }

    /// The editor's diagnostics of `path` (absolute), as its language servers see the buffer — `None` where it does
    /// not say.
    fn diagnostics(&self, _path: &Path) -> Option<Vec<Diagnostic>> {
        None
    }
}

/// Where the owner works in the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// The active file, absolute.
    pub path: std::path::PathBuf,
    /// The cursor, 1-based.
    pub line: u32,
    pub character: u32,
    /// The selected lines (1-based, inclusive) and their text, cut by the editor.
    pub selection: Option<Selection>,
}

/// A selection in the active file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub start_line: u32,
    pub end_line: u32,
    pub text: String,
}

/// A file open in the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFile {
    pub path: std::path::PathBuf,
    pub active: bool,
    /// Whether its buffer holds unsaved changes.
    pub dirty: bool,
}

/// One diagnostic of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: u32,
    pub character: u32,
    pub severity: String,
    pub message: String,
    pub source: Option<String>,
}

/// The editor a toolbox's file tools ask, if any: set by the host that serves one, shared by every tool of the box
/// and its subagents.
#[derive(Clone, Default)]
pub struct Editor(Arc<RwLock<Option<Arc<dyn EditorFiles>>>>);

impl Editor {
    /// Makes `files` the editor the file tools ask from their next call on; `None`: the disk alone.
    pub fn set(&self, files: Option<Arc<dyn EditorFiles>>) {
        *self.0.write().unwrap_or_else(|poison| poison.into_inner()) = files;
    }

    /// The editor asked now, if any.
    pub fn get(&self) -> Option<Arc<dyn EditorFiles>> {
        self.0
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .clone()
    }
}
