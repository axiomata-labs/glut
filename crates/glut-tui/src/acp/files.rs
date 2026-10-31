//! The editor's open files for Glut's file tools (T64b, [`glut::editor`]): a read asks the editor for a file's text —
//! an open tab's unsaved changes too. The tools make every check first (the root takes the path before the editor ever
//! sees it); this only carries a path out and a text back. v1 only: v2's draft has no editor files.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

use super::Peer;

/// How long a file tool waits for the editor's answer before it goes on without it.
const EDITOR_WAIT: Duration = Duration::from_secs(10);

/// One session's way to the editor's files, as far as the editor said it offers them.
pub struct Files {
    /// The editor's end of the line.
    pub peer: Arc<Peer>,
    /// The session the editor knows the files' requests by.
    pub session: String,
    /// The runtime the editor's answers come in on: the tools call from a blocking thread and wait on it.
    pub runtime: tokio::runtime::Handle,
    /// The editor let a question go unanswered: from then on the disk alone, so no call waits for it again.
    pub silent: AtomicBool,
}

impl Files {
    fn ask(&self, method: &str, params: Value) -> Option<Value> {
        if self.silent.load(Ordering::SeqCst) {
            return None;
        }
        let (peer, method) = (Arc::clone(&self.peer), method.to_owned());
        let asked = Arc::new(AtomicU64::new(u64::MAX));
        let id = Arc::clone(&asked);
        let answer = self.runtime.block_on(async move {
            tokio::time::timeout(
                EDITOR_WAIT,
                peer.request(&method, params, |at| id.store(at, Ordering::SeqCst)),
            )
            .await
        });
        match answer {
            Ok(answer) => answer,
            Err(_) => {
                // Given up: a late answer finds no one waiting, and the next call does not wait.
                self.peer.give_up(asked.load(Ordering::SeqCst));
                self.silent.store(true, Ordering::SeqCst);
                None
            }
        }
    }
}

impl glut::editor::EditorFiles for Files {
    fn read(&self, path: &Path) -> Option<String> {
        let answer = self.ask(
            "fs/read_text_file",
            json!({"sessionId": self.session, "path": path.display().to_string()}),
        )?;
        answer.get("content")?.as_str().map(str::to_owned)
    }
}
