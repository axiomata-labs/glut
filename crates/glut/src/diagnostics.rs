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

/// The programs asked after an edit, by file name — never by the table's server id, which also stands for fallbacks
/// (`pylsp` for "pyright", with plugins that run the project's config). A language server reads files the agent wrote
/// and may run programs because of them (rust-analyzer: `cargo metadata`, which obeys the project's
/// `.cargo/config.toml`; pyright: the interpreter a project's config names). So a server starts only where the run's
/// gate would allow its command line, and only in the run's sandbox — writes inside the folder, no network — as a
/// command of the run would ([`Diagnostics::launch`], `run.rs` `diagnostics_after`).
const SAFE_PROGRAMS: [&str; 9] = [
    "rust-analyzer",
    "pyright-langserver",
    "basedpyright-langserver",
    "taplo",
    "yaml-language-server",
    "vscode-json-language-server",
    "vscode-css-language-server",
    "vscode-html-language-server",
    "marksman",
];

/// Most messages a server may have waiting between two edits; past it, the rest is dropped (progress reports, mostly).
const MAX_WAITING: usize = 1024;
/// A server that has said nothing within the limit this many times in a row is not asked again in the run.
const MAX_SILENT: u32 = 2;

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
    match server {
        "rust-analyzer" => rust_analyzer_settings(),
        // No schema fetched from a URL a file names: a way out past the web gate (the sandbox has no network anyway).
        "json" => json!({ "json": { "schemaDownload": { "enable": false } } }),
        "yaml" => json!({ "yaml": { "schemaStore": { "enable": false }, "schemas": {} } }),
        "taplo" => json!({ "schema": { "enabled": false } }),
        _ => Value::Null,
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
    /// Edits in a row the server said nothing about within the limit.
    silent: u32,
}

impl Drop for Server {
    fn drop(&mut self) {
        // The server leads its own process group: its children (cargo, node workers) go with it.
        if let Some(pid) = rustix::process::Pid::from_raw(self.child.id() as i32) {
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        }
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
        profile: &str,
    ) -> Result<Self, String> {
        use std::os::unix::process::CommandExt;
        // In the run's sandbox, as a command of the run; with a cleared environment.
        let mut command = Command::new(crate::sandbox::SANDBOX_EXEC);
        command
            .arg("-p")
            .arg(profile)
            .arg(program)
            .args(args)
            .env_clear();
        for key in ["PATH", "HOME", "TMPDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut child = command
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .map_err(|err| format!("cannot start {}: {err}", program.display()))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let (tx, incoming) = mpsc::sync_channel(MAX_WAITING);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(text)) = framing::read_message(&mut reader) {
                let Ok(message) = serde_json::from_str::<Value>(&text) else {
                    continue;
                };
                match tx.try_send(message) {
                    Ok(()) | Err(mpsc::TrySendError::Full(_)) => {}
                    Err(mpsc::TrySendError::Disconnected(_)) => break,
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
            silent: 0,
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

/// A server that would start for an edit: its id and the command line the gate judges.
pub(crate) struct Launch {
    pub server: &'static str,
    pub line: String,
}

/// The servers of one run, by server id; a server that would not start, was refused, or fell silent is remembered as
/// such (`None`).
pub(crate) struct Diagnostics {
    root: PathBuf,
    search: Vec<PathBuf>,
    /// The run's sandbox profile, which every server runs in.
    profile: String,
    servers: HashMap<&'static str, Option<Server>>,
}

/// What [`Diagnostics::resolve`] finds for a file: its path, language, server id, program and arguments.
type Resolved = (PathBuf, &'static str, &'static str, PathBuf, Vec<String>);

impl Diagnostics {
    /// The servers for a run in `root`, each started in the sandbox `profile`.
    pub(crate) fn new(root: &Path, profile: String) -> Self {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        Self {
            root: root.canonicalize().unwrap_or_else(|_| root.to_owned()),
            search: servers::search_path(std::env::var_os("PATH").as_deref(), home.as_deref()),
            profile,
            servers: HashMap::new(),
        }
    }

    /// The file `rel` names inside the root, its language and its server — `None` for a file outside, a language
    /// without a server, or a program not on [`SAFE_PROGRAMS`].
    fn resolve(&self, rel: &str) -> Option<Resolved> {
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
        let name = program.file_name()?.to_str()?;
        SAFE_PROGRAMS
            .contains(&name)
            .then_some((path, language, server, program, args))
    }

    /// The server an edit of `rel` would start now — for the gate to judge first; `None` when none would start (no
    /// server, already running, or given up on in this run).
    pub(crate) fn launch(&self, rel: &str) -> Option<Launch> {
        let (_, _, server, program, args) = self.resolve(rel)?;
        if self.servers.contains_key(server) {
            return None;
        }
        let line = std::iter::once(program.display().to_string())
            .chain(args)
            .collect::<Vec<_>>()
            .join(" ");
        Some(Launch { server, line })
    }

    /// The gate refused to start `server`: it is not asked about again in this run.
    pub(crate) fn refuse(&mut self, server: &'static str) {
        self.servers.insert(server, None);
    }

    /// The note to append to an edit of `rel` (relative to the root): the server's errors, or `None` when there is no
    /// server, it said nothing in time, or found nothing.
    pub(crate) fn after_edit(&mut self, rel: &str) -> Option<String> {
        let (path, language, server, program, args) = self.resolve(rel)?;
        let text = std::fs::read_to_string(&path).ok()?;
        let (root, profile) = (self.root.clone(), self.profile.clone());
        let entry = self
            .servers
            .entry(server)
            .or_insert_with(|| Server::start(server, &program, &args, &root, &profile).ok());
        let running = entry.as_mut()?;
        match running.errors_for(&path, language, &text) {
            Ok(Some(errors)) => {
                running.silent = 0;
                (!errors.is_empty()).then(|| {
                    format!(
                        "{server} reports {} error(s) in {rel} now:\n{}",
                        errors.len(),
                        errors.join("\n")
                    )
                })
            }
            Ok(None) => {
                // Nothing within the limit: a server still loading a large project would add seconds to every edit.
                running.silent += 1;
                if running.silent >= MAX_SILENT {
                    *entry = None;
                }
                None
            }
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
        for program in [
            "typescript-language-server",
            "svelteserver",
            "sourcekit-lsp",
            "pylsp",
            "sqls",
        ] {
            assert!(!SAFE_PROGRAMS.contains(&program), "{program}");
        }
        assert_eq!(
            settings_for("json")["json"]["schemaDownload"]["enable"],
            false
        );
        assert_eq!(settings_for("yaml")["yaml"]["schemaStore"]["enable"], false);
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
        let root = dir.canonicalize().unwrap();
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let profile = crate::sandbox::Policy::new(&root, &home, &[], &[]).profile();
        let mut diagnostics = Diagnostics::new(&root, profile);
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
