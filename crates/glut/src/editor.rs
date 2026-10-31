//! An editor's open files (T64b): where Glut is an editor's agent, its file tools read a file as the editor holds it —
//! an open tab's unsaved changes too — and a change of a file whose tab holds unsaved changes waits until the owner
//! saved it (the owner's word: Glut writes the disk alone). Every check stays Glut's own and comes first (the gate,
//! the root's verdict on the path, the protected places, no link on the way); the editor only hands over a text.
//! Commands see the disk. What stays open: the editor opens the path itself, outside the sandbox — a link swapped in
//! between Glut's check and its read would be followed (the owner took that, 2026-10-31).
//!
//! v1 only: the protocol's draft v2 drops the editor's files, and with it this way.

use std::path::Path;
use std::sync::{Arc, RwLock};

/// What an editor does with its files for Glut.
pub trait EditorFiles: Send + Sync {
    /// The text of `path` (absolute) as the editor holds it, an open tab's unsaved changes too; `None` where it cannot
    /// say. Called off the runtime's workers: it may wait for the editor.
    fn read(&self, path: &Path) -> Option<String>;
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
