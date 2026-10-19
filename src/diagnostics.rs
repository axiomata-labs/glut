//! The language server's errors after an edit (Glut 2, D, G12 — after Opencode): a lean LSP client of the harness's
//! own, so the model sees what it just broke in the edit's own result.
//!
//! * **The editor's server table** (`axiomata_files::lsp::servers`): one place says which program serves a language;
//!   the editor's override file is not read — the harness reaches nothing of the app's (harness.md, standalone).
//! * **Lean**: start the server at the first edit of its language, `initialize`, then per edit `didOpen` or
//!   `didChange` with the whole text, and wait until the server has been quiet for a moment (at most
//!   [`WAIT_LIMIT`]); the last `publishDiagnostics` for the file counts. Only errors go to the model, at most
//!   [`MAX_ERRORS`]. A server that is not installed, or slow, adds nothing — it never fails the edit.
//! * **Blocking work** (the server's pipes) runs on tokio's blocking pool; the servers live until the run ends.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use axiomata_files::lsp::framing;
use axiomata_files::lsp::servers::{self, Overrides, Resolution};
use serde_json::{Value, json};

/// The longest an edit waits for the server's verdict.
const WAIT_LIMIT: Duration = Duration::from_secs(4);
/// How long the server must stay silent after a message before its last word counts.
const QUIET: Duration = Duration::from_millis(700);
/// How long `initialize` may take before the server counts as unusable for this run.
const INIT_LIMIT: Duration = Duration::from_secs(20);
/// Most errors handed to the model per edit.
const MAX_ERRORS: usize = 10;
/// LSP's severity of an error.
const ERROR: u64 = 1;

/// The servers asked after an edit: the ones that never run the project's own code. An agent writes the files these
/// servers read, so a server that runs a build script, a proc macro or a config file (`svelte.config.js`, a tsserver
/// plugin from `node_modules`, a Swift build) would run what the agent wrote — outside the sandbox, past the gate.
/// rust-analyzer is here only with its build scripts, proc macros and check-on-save off ([`rust_analyzer_settings`]).
const SAFE_SERVERS: [&str; 8] = [
    "rust-analyzer",
    "pyright",
    "taplo",
    "yaml",
    "json",
    "css",
    "html",
    "marksman",
];

/// rust-analyzer without anything that runs the project's code: build scripts, proc macros, `cargo check`.
fn rust_analyzer_settings() -> Value {
    json!({
        "cargo": { "buildScripts": { "enable": false } },
        "procMacro": { "enable": false },
        "checkOnSave": false,
        // Without `cargo check` only rust-analyzer's own diagnostics remain; type errors are among its experimental ones.
        "diagnostics": { "experimental": { "enable": true } },
    })
}

/// What a server is started with and answers `workspace/configuration` with.
fn settings_for(server: &str) -> Value {
    if server == "rust-analyzer" {
        rust_analyzer_settings()
    } else {
        Value::Null
    }
}

/// The editor language of a file, by its extension — the languages the server table knows.
pub(crate) fn language_of(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "rs" => "rust",
        "py" | "pyi" => "python",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "svelte" => "svelte",
        "css" => "css",
        "html" | "htm" => "html",
        "json" => "json",
        "sh" | "bash" => "bash",
        "lua" => "lua",
        "swift" => "swift",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "md" => "markdown",
        "sql" => "sql",
        _ => return None,
    })
}

/// The `languageId` LSP wants where it differs from the editor's.
fn lsp_language(language: &str, path: &Path) -> &'static str {
    match (language, path.extension().and_then(|e| e.to_str())) {
        ("tsx", _) => "typescriptreact",
        ("javascript", Some("jsx")) => "javascriptreact",
        ("bash", _) => "shellscript",
        _ => servers::spec_for(language).map_or("plaintext", |spec| {
            spec.languages
                .iter()
                .find(|l| **l == language)
                .copied()
                .unwrap_or("plaintext")
        }),
    }
}

/// One running server.
struct Server {
    /// The server's id in the table, which decides its settings.
    id: &'static str,
    child: Child,
    stdin: ChildStdin,
    incoming: Receiver<Value>,
    next_id: u64,
    /// The version each open document is at.
    versions: HashMap<String, i64>,
    /// The last errors the server reported per document: a server that finds nothing new in a change says nothing,
    /// and its last word still stands.
    known: HashMap<String, Vec<String>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Server {
    fn start(
        id: &'static str,
        program: &Path,
        args: &[String],
        root: &Path,
    ) -> Result<Self, String> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|err| format!("cannot start {}: {err}", program.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, incoming) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(text)) = framing::read_message(&mut reader) {
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        let mut server = Self {
            id,
            child,
            stdin,
            incoming,
            next_id: 1,
            versions: HashMap::new(),
            known: HashMap::new(),
        };
        let uri = uri_of(root);
        let id = server.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri,
                "workspaceFolders": [{ "uri": uri, "name": "work" }],
                "capabilities": {
                    "textDocument": { "publishDiagnostics": { "relatedInformation": false } },
                    "workspace": { "configuration": true },
                },
                "initializationOptions": settings_for(id),
            }),
        )?;
        let deadline = Instant::now() + INIT_LIMIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match server.incoming.recv_timeout(left) {
                Ok(message) if message.get("id") == Some(&json!(id)) => break,
                Ok(message) => server.answer_request(&message)?,
                Err(_) => return Err("the server did not answer initialize".into()),
            }
        }
        server.notify("initialized", json!({}))?;
        Ok(server)
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        let bytes = framing::encode(&message.to_string());
        self.stdin
            .write_all(&bytes)
            .and_then(|()| self.stdin.flush())
            .map_err(|err| format!("the server stopped listening: {err}"))
    }

    fn request(&mut self, method: &str, params: Value) -> Result<u64, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))?;
        Ok(id)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.send(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    /// A request of the server's own (`workspace/configuration`, `window/workDoneProgress/create`), answered so it
    /// does not wait on us: a configuration pull gets the same settings it started with — a server must not fall back
    /// to its defaults, which for rust-analyzer run build scripts.
    fn answer_request(&mut self, message: &Value) -> Result<(), String> {
        if let (Some(id), Some(method)) = (
            message.get("id"),
            message.get("method").and_then(Value::as_str),
        ) {
            let result = if method == "workspace/configuration" {
                let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                Value::Array(vec![settings_for(self.id); items])
            } else {
                Value::Null
            };
            self.send(&json!({ "jsonrpc": "2.0", "id": id, "result": result }))?;
        }
        Ok(())
    }

    /// Sends `text` as the file's content and returns the errors the server reports for it within the limit.
    fn errors_for(
        &mut self,
        path: &Path,
        language: &str,
        text: &str,
    ) -> Result<Option<Vec<String>>, String> {
        let uri = uri_of(path);
        match self.versions.get_mut(&uri) {
            None => {
                self.versions.insert(uri.clone(), 1);
                self.notify(
                    "textDocument/didOpen",
                    json!({ "textDocument": {
                        "uri": uri, "languageId": lsp_language(language, path), "version": 1, "text": text
                    }}),
                )?;
            }
            Some(version) => {
                *version += 1;
                let version = *version;
                self.notify(
                    "textDocument/didChange",
                    json!({ "textDocument": { "uri": uri, "version": version }, "contentChanges": [{ "text": text }] }),
                )?;
            }
        }
        let deadline = Instant::now() + WAIT_LIMIT;
        let mut last: Option<Vec<String>> = None;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let wait = if last.is_some() {
                QUIET.min(left)
            } else {
                left
            };
            match self.incoming.recv_timeout(wait) {
                Ok(message) => {
                    if message.get("method").and_then(Value::as_str)
                        == Some("textDocument/publishDiagnostics")
                        && message["params"]["uri"].as_str() == Some(uri.as_str())
                    {
                        last = Some(errors_in(&message["params"]["diagnostics"]));
                    } else {
                        self.answer_request(&message)?;
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Ok(self.settle(&uri, last)),
                Err(RecvTimeoutError::Disconnected) => return Err("the server ended".into()),
            }
            if Instant::now() >= deadline {
                return Ok(self.settle(&uri, last));
            }
        }
    }
}

impl Server {
    /// The server's word on `uri`: what it just said (kept for next time), else what it said last.
    fn settle(&mut self, uri: &str, said: Option<Vec<String>>) -> Option<Vec<String>> {
        match said {
            Some(errors) => {
                self.known.insert(uri.to_owned(), errors.clone());
                Some(errors)
            }
            None => self.known.get(uri).cloned(),
        }
    }
}

/// The errors of a `publishDiagnostics` list as lines: `line:column: message`, 1-based.
fn errors_in(diagnostics: &Value) -> Vec<String> {
    diagnostics
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["severity"].as_u64().unwrap_or(ERROR) == ERROR)
        .take(MAX_ERRORS)
        .map(|d| {
            let line = d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1;
            let column = d["range"]["start"]["character"].as_u64().unwrap_or(0) + 1;
            let message = d["message"]
                .as_str()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or("");
            format!("{line}:{column}: {message}")
        })
        .collect()
}

fn uri_of(path: &Path) -> String {
    let encoded: String = path
        .to_string_lossy()
        .split('/')
        .map(|part| {
            part.bytes()
                .map(|b| match b {
                    b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                        (b as char).to_string()
                    }
                    _ => format!("%{b:02X}"),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/");
    format!("file://{encoded}")
}

/// The servers of one run, by server id; a server that would not start is remembered as such.
pub(crate) struct Diagnostics {
    root: PathBuf,
    search: Vec<PathBuf>,
    servers: HashMap<&'static str, Option<Server>>,
}

impl Diagnostics {
    pub(crate) fn new(root: &Path) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            root: root.to_owned(),
            search: servers::search_path(std::env::var_os("PATH").as_deref(), home.as_deref()),
            servers: HashMap::new(),
        }
    }

    /// The note to append to an edit of `rel` (relative to the root): the server's errors, or `None` when there is no
    /// server, it said nothing in time, or found nothing.
    pub(crate) fn after_edit(&mut self, rel: &str) -> Option<String> {
        let path = self.root.join(rel).canonicalize().ok()?;
        if !path.starts_with(&self.root) {
            return None;
        }
        let language = language_of(&path)?;
        let Resolution::Found {
            server,
            program,
            args,
        } = servers::resolve(language, &Overrides::default(), &self.search)
        else {
            return None;
        };
        if !SAFE_SERVERS.contains(&server) {
            return None;
        }
        let text = std::fs::read_to_string(&path).ok()?;
        let entry = self
            .servers
            .entry(server)
            .or_insert_with(|| Server::start(server, &program, &args, &self.root).ok());
        let running = entry.as_mut()?;
        match running.errors_for(&path, language, &text) {
            Ok(Some(errors)) if !errors.is_empty() => Some(format!(
                "{server} reports {} error(s) in {rel} now:\n{}",
                errors.len(),
                errors.join("\n")
            )),
            Ok(_) => None,
            Err(_) => {
                // A server that broke is not asked again in this run.
                *entry = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_errors_and_uris() {
        assert_eq!(language_of(Path::new("src/main.rs")), Some("rust"));
        assert_eq!(language_of(Path::new("App.TSX")), Some("tsx"));
        assert_eq!(language_of(Path::new("notes.txt")), None);
        assert_eq!(lsp_language("tsx", Path::new("a.tsx")), "typescriptreact");
        assert_eq!(lsp_language("rust", Path::new("a.rs")), "rust");
        let diagnostics = json!([
            { "severity": 1, "message": "mismatched types\nexpected i32", "range": { "start": { "line": 4, "character": 2 } } },
            { "severity": 2, "message": "unused", "range": { "start": { "line": 0, "character": 0 } } }
        ]);
        assert_eq!(errors_in(&diagnostics), ["5:3: mismatched types"]);
        assert_eq!(uri_of(Path::new("/a b/c.rs")), "file:///a%20b/c.rs");
        // The project's own code never runs: no build scripts or proc macros for rust-analyzer, no server that loads
        // a project's config or plugins.
        let ra = settings_for("rust-analyzer");
        assert_eq!(ra["cargo"]["buildScripts"]["enable"], false);
        assert_eq!(ra["procMacro"]["enable"], false);
        for unsafe_server in ["typescript", "svelte", "sourcekit", "lua", "sqls", "bash"] {
            assert!(!SAFE_SERVERS.contains(&unsafe_server), "{unsafe_server}");
        }
    }

    /// Live against rust-analyzer when it is installed: an edit with a type error gets the error back.
    #[test]
    #[ignore = "needs rust-analyzer"]
    fn rust_analyzer_reports_a_type_error() {
        let dir = std::env::temp_dir().join(format!("axh-diag-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "fn main() { let x: i32 = \"no\"; }\n",
        )
        .unwrap();
        let mut diagnostics = Diagnostics::new(&dir.canonicalize().unwrap());
        // Its own diagnostics (no `cargo check`): the type error among them; asked again, the last word stands.
        let first = diagnostics.after_edit("src/main.rs");
        let note = diagnostics.after_edit("src/main.rs");
        assert_eq!(first, note);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            note.as_deref().is_some_and(|n| n.contains("expected i32")),
            "{note:?}"
        );
    }
}
