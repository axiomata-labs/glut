//! The language server's errors after an edit (Glut 2, D, G12 — after Opencode): a lean LSP client of the harness's
//! own, so the model sees what it just broke in the edit's own result.
//!
//! * **The editor's server table** (`axiomata_files::lsp::servers`): one place says which program serves a language;
//!   the editor's override file is not read — the harness reaches nothing of the app's (harness.md, standalone).
//! * **Lean**: start the server at the first edit of its language, `initialize`, then per edit `didOpen` or
//!   `didChange` with the whole text, and wait until the server has been quiet for a moment (at most
//!   [`WAIT_LIMIT`]); the last `publishDiagnostics` for the file counts. Only errors go to the model, at most
//!   [`MAX_ERRORS`]. A server that is not installed, or slow, adds nothing — it never fails the edit.
//! * **Questions** (T62, the `code` tool): definition, references, hover and rename go to the same servers, once a
//!   server has finished indexing ([`INDEX_LIMIT`] at its first question); the `code` module shapes them.
//! * **Blocking work** (the server's pipes) runs on tokio's blocking pool. The servers live in [`LanguageServers`],
//!   which a host keeps for a whole session (a new run would index the project anew) and a run shares with its
//!   subagents; a change of the root or of the sandbox's profile starts them anew.

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
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
/// The longest a server's first question waits for its indexing to end (T62): rust-analyzer loads a workspace like
/// Glut's in some ten to forty seconds.
pub(crate) const INDEX_LIMIT: Duration = Duration::from_secs(30);
/// How long a later question waits for a server still busy (an edit made it look again).
const READY_LIMIT: Duration = Duration::from_secs(5);
/// How long a server must have reported no work under way before it counts as ready.
const READY_QUIET: Duration = Duration::from_millis(1500);
/// The longest one question waits for its answer.
const ANSWER_LIMIT: Duration = Duration::from_secs(15);
/// How long a server that has reported no work at all is given before its first answer counts: rust-analyzer begins
/// its loading report a moment after `initialized`, and asked before it, it knows no use of anything.
const FIRST_SETTLE: Duration = Duration::from_secs(3);
/// Work reported under way and unchanged this long counts as ended: its end was lost (a full channel), or it hangs.
const STALE_PROGRESS: Duration = Duration::from_secs(120);
/// How long a server silent after edits is not waited for: it still gets each edit's text.
const SILENT_PAUSE: Duration = Duration::from_secs(300);
/// How often the waits look at the server's messages.
const LOOK_EVERY: Duration = Duration::from_millis(250);

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
    /// When it fell silent the last time: it is not waited for until [`SILENT_PAUSE`] has passed.
    silent_at: Instant,
    /// The text each open document was handed over with: one changed on disk since (a command, a formatter) is
    /// handed over again before a question, or a rename would be reckoned on an old text.
    texts: HashMap<String, String>,
    /// When the server was ready to be asked (`initialized` sent).
    started: Instant,
    /// Whether it has reported any work at all.
    reported: bool,
    /// The work the server reports under way (`$/progress` begun and not ended): indexing, mostly.
    progress: HashSet<String>,
    /// When the work under way last changed.
    progress_changed: Instant,
    /// Whether the server was asked a question before: the first one waits longest for its indexing.
    asked: bool,
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
            silent_at: Instant::now(),
            texts: HashMap::new(),
            started: Instant::now(),
            reported: false,
            progress: HashSet::new(),
            progress_changed: Instant::now(),
            asked: false,
        };
        let uri = uri_of(root);
        let id = server.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": uri,
                "workspaceFolders": [{ "uri": uri, "name": "work" }],
                "capabilities": {
                    "textDocument": {
                        "publishDiagnostics": { "relatedInformation": false },
                        "definition": { "linkSupport": true },
                        "references": {},
                        "hover": { "contentFormat": ["plaintext", "markdown"] },
                        "rename": { "prepareSupport": false },
                    },
                    // Edits of documents only: no file a rename would create, move or delete (none is announced).
                    "workspace": { "configuration": true, "workspaceEdit": { "documentChanges": true } },
                    // Its indexing reported, so a question waits until the server knows the project.
                    "window": { "workDoneProgress": true },
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
        server.started = Instant::now();
        server.progress_changed = server.started;
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

    /// A message of the server's that answers nothing of ours: its work under way noted, its own request answered.
    fn note(&mut self, message: &Value) -> Result<(), String> {
        if message.get("method").and_then(Value::as_str) == Some("$/progress") {
            let token = match &message["params"]["token"] {
                Value::String(token) => token.clone(),
                other => other.to_string(),
            };
            // rust-analyzer answers while it primes its caches, only slower: that work keeps no question waiting.
            if token.contains("cachePriming") {
                return Ok(());
            }
            match message["params"]["value"]["kind"].as_str() {
                Some("begin") => {
                    self.progress.insert(token);
                }
                Some("end") => {
                    self.progress.remove(&token);
                }
                _ => return Ok(()),
            }
            self.progress_changed = Instant::now();
            self.reported = true;
            return Ok(());
        }
        self.answer_request(message)
    }

    /// Whether the server reports work under way — not counting work unchanged for [`STALE_PROGRESS`].
    fn busy(&self) -> bool {
        !self.progress.is_empty() && self.progress_changed.elapsed() < STALE_PROGRESS
    }

    /// Whether the server may be asked: no work under way for [`READY_QUIET`], and either some work reported and
    /// done, or [`FIRST_SETTLE`] passed without any.
    fn ready(&self) -> bool {
        !self.busy()
            && self.progress_changed.elapsed() >= READY_QUIET
            && (self.reported || self.started.elapsed() >= FIRST_SETTLE)
    }

    /// Takes what the server has sent meanwhile.
    fn drain(&mut self) -> Result<(), String> {
        loop {
            match self.incoming.try_recv() {
                Ok(message) => self.note(&message)?,
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => return Err("the server ended".into()),
            }
        }
    }

    /// Waits up to `limit` until the server is [`Self::ready`]: whether it got there.
    fn wait_ready(&mut self, limit: Duration) -> Result<bool, String> {
        let deadline = Instant::now() + limit;
        loop {
            self.drain()?;
            if self.ready() {
                return Ok(true);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(false);
            }
            match self.incoming.recv_timeout(left.min(LOOK_EVERY)) {
                Ok(message) => self.note(&message)?,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Err("the server ended".into()),
            }
        }
    }

    /// Hands every open document over again whose text on disk changed since (T62), so a question — a rename above all
    /// — is reckoned on the files as they are.
    fn refresh(&mut self) -> Result<(), String> {
        let open: Vec<(String, String)> = self
            .texts
            .iter()
            .map(|(uri, text)| (uri.clone(), text.clone()))
            .collect();
        for (uri, before) in open {
            let Some(path) = path_of(&uri) else { continue };
            let now = read_capped(&path).unwrap_or_default();
            if now != before {
                let language = language_of(&path).unwrap_or("plaintext");
                self.sync(&path, language, &now)?;
            }
        }
        Ok(())
    }

    /// Whether the server's process has ended.
    fn ended(&mut self) -> bool {
        self.child.try_wait().is_ok_and(|status| status.is_some())
    }

    /// Asks `method` and waits up to [`ANSWER_LIMIT`] for its answer.
    fn ask(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.request(method, params)?;
        let deadline = Instant::now() + ANSWER_LIMIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.incoming.recv_timeout(left) {
                Ok(message)
                    if message.get("id") == Some(&json!(id)) && message.get("method").is_none() =>
                {
                    if let Some(error) = message.get("error") {
                        return Err(format!(
                            "the server could not answer: {}",
                            error["message"].as_str().unwrap_or("no reason given")
                        ));
                    }
                    return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                }
                Ok(message) => self.note(&message)?,
                Err(RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "the server gave no answer within {} s",
                        ANSWER_LIMIT.as_secs()
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => return Err("the server ended".into()),
            }
        }
    }

    /// Hands the server the file's content as it is now: opened the first time, changed after.
    fn sync(&mut self, path: &Path, language: &str, text: &str) -> Result<String, String> {
        let uri = uri_of(path);
        self.texts.insert(uri.clone(), text.to_owned());
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
        Ok(uri)
    }

    /// Sends `text` as the file's content and returns the errors the server reports for it within the limit.
    fn errors_for(
        &mut self,
        path: &Path,
        language: &str,
        text: &str,
    ) -> Result<Option<Vec<String>>, String> {
        let uri = self.sync(path, language, text)?;
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
                        self.note(&message)?;
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

pub(crate) fn uri_of(path: &Path) -> String {
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

/// A file's text, refused past [`axiomata_files::MAX_READ_BYTES`] as `read_file` refuses it.
pub(crate) fn read_capped(path: &Path) -> Result<String, String> {
    let size = std::fs::metadata(path)
        .map_err(|err| err.to_string())?
        .len();
    if size > axiomata_files::MAX_READ_BYTES {
        return Err(format!("{size} bytes, over the limit"));
    }
    std::fs::read_to_string(path).map_err(|err| err.to_string())
}

/// The path a `file://` URI names, its `%XX` escapes undone; `None` for any other URI or one that is no UTF-8.
pub(crate) fn path_of(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut rest = encoded.as_bytes();
    while let Some((&first, tail)) = rest.split_first() {
        if first == b'%' && tail.len() >= 2 {
            let hex = std::str::from_utf8(&tail[..2]).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(first);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok().map(PathBuf::from)
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
    /// The servers the gate allowed to start for these servers' root and profile (T62): no other starts, whatever
    /// file an edit or a rename names.
    allowed: HashSet<&'static str>,
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
            allowed: HashSet::new(),
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

    /// The server `rel` needs that the gate has not allowed yet — for it to judge first; `None` when there is none
    /// or it is allowed.
    pub(crate) fn launch(&self, rel: &str) -> Option<Launch> {
        let (_, _, server, program, args) = self.resolve(rel)?;
        if self.allowed.contains(server) {
            return None;
        }
        let line = std::iter::once(program.display().to_string())
            .chain(args)
            .collect::<Vec<_>>()
            .join(" ");
        Some(Launch { server, line })
    }

    /// The project's folder, as the servers know it (resolved: what their answers name).
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// Asks the server of `rel` (relative to the root) `method` about it (T62): the file's content handed over as it
    /// is now, the question sent once the server knows the project, `params` built from the file's URI and text.
    ///
    /// # Errors
    ///
    /// No server for the file, one given up on in this session, one still indexing, or its answer an error.
    pub(crate) fn ask(
        &mut self,
        rel: &str,
        method: &str,
        params: impl FnOnce(&str, &str) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let Some((path, language, server, program, args)) = self.resolve(rel) else {
            return Err(format!(
                "no language server Glut may start knows {rel}; use grep or read_file"
            ));
        };
        let text = read_capped(&path).map_err(|err| format!("cannot read {rel}: {err}"))?;
        let Some(entry) = self.running(server, &program, &args) else {
            return Err(format!(
                "{server} was not allowed to start; use grep or read_file"
            ));
        };
        let Some(running) = entry.as_mut() else {
            return Err(format!(
                "{server} is off for this session (refused, broken or silent); use grep or read_file"
            ));
        };
        let answer = (|| {
            running.refresh()?;
            let uri = running.sync(&path, language, &text)?;
            let limit = if running.asked {
                READY_LIMIT
            } else {
                INDEX_LIMIT
            };
            if !running.wait_ready(limit)? {
                return Err(format!(
                    "{server} is still indexing the project; use grep for now, or ask again in a moment"
                ));
            }
            running.asked = true;
            running.ask(method, params(&uri, &text)?)
        })();
        // A server that ended or stopped listening is not asked again; a question it could not answer leaves it be.
        if answer.is_err() && running.ended() {
            *entry = None;
        }
        answer
    }

    /// Each server of the session and how it stands, for the owner (`/lsp`).
    pub(crate) fn status(&self) -> Vec<(String, ServerState)> {
        let mut all: Vec<(String, ServerState)> = self
            .servers
            .iter()
            .map(|(id, server)| {
                let state = match server {
                    Some(server) if server.busy() => ServerState::Indexing,
                    Some(_) => ServerState::Ready,
                    None => ServerState::Off,
                };
                ((*id).to_owned(), state)
            })
            .collect();
        all.sort();
        all
    }

    /// The gate allowed `server` to start.
    pub(crate) fn allow(&mut self, server: &'static str) {
        self.allowed.insert(server);
    }

    /// The running server for `server`, started where the gate allowed it and it is not given up on; `None` else.
    fn running(
        &mut self,
        server: &'static str,
        program: &Path,
        args: &[String],
    ) -> Option<&mut Option<Server>> {
        if !self.allowed.contains(server) {
            return None;
        }
        let (root, profile) = (self.root.clone(), self.profile.clone());
        Some(
            self.servers
                .entry(server)
                .or_insert_with(|| Server::start(server, program, args, &root, &profile).ok()),
        )
    }

    /// The note to append to an edit of `rel` (relative to the root): the server's errors, or `None` when there is no
    /// server, it said nothing in time, or found nothing.
    pub(crate) fn after_edit(&mut self, rel: &str) -> Option<String> {
        let (path, language, server, program, args) = self.resolve(rel)?;
        let text = read_capped(&path).ok()?;
        let entry = self.running(server, &program, &args)?;
        let running = entry.as_mut()?;
        // Silent lately: it gets the text, nobody waits for its word until the pause has passed.
        if running.silent >= MAX_SILENT && running.silent_at.elapsed() < SILENT_PAUSE {
            if running.sync(&path, language, &text).is_err() {
                *entry = None;
            }
            return None;
        }
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
            // A server still indexing says nothing yet: that is no silence to give it up for.
            Ok(None) if running.busy() => None,
            Ok(None) => {
                // Nothing within the limit: a server still loading a large project would add seconds to every edit.
                running.silent += 1;
                running.silent_at = Instant::now();
                None
            }
            Err(_) => {
                // A server that broke is not asked again in this session.
                *entry = None;
                None
            }
        }
    }
}

/// How a language server of the session stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ServerState {
    /// Answering.
    Ready,
    /// Reporting work under way — loading or indexing the project.
    Indexing,
    /// Given up on for the session: refused by the gate, broken, or silent.
    Off,
}

/// The language servers of a session (T62): kept by a host across its runs, shared by a run with its subagents. A
/// clone is the same set.
#[derive(Clone, Default)]
pub struct LanguageServers(Arc<Mutex<Option<Diagnostics>>>);

impl std::fmt::Debug for LanguageServers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LanguageServers")
    }
}

impl LanguageServers {
    /// The servers for `root`, each started in the sandbox `profile`: the ones running stay where both are the same,
    /// else they end and start anew as they are needed. Blocks while a question is under way.
    pub(crate) fn lock(&self, root: &Path, profile: &str) -> MutexGuard<'_, Option<Diagnostics>> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let root = root.canonicalize().unwrap_or_else(|_| root.to_owned());
        if held
            .as_ref()
            .is_none_or(|servers| servers.root != root || servers.profile != profile)
        {
            *held = Some(Diagnostics::new(&root, profile.to_owned()));
        }
        held
    }

    /// Each server and how it stands, for the owner; `None` while a question holds them (they are busy answering).
    pub fn status(&self) -> Option<Vec<(String, ServerState)>> {
        let held = self.0.try_lock().ok()?;
        Some(held.as_ref().map(Diagnostics::status).unwrap_or_default())
    }

    /// Ends every server: a switch turned off, a session closed. Beside the caller — a question may hold them for a
    /// while yet, and the screen must not wait for it.
    pub fn stop(&self) {
        let servers = Arc::clone(&self.0);
        std::thread::spawn(move || {
            *servers.lock().unwrap_or_else(PoisonError::into_inner) = None;
        });
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

    /// Live against rust-analyzer when it is installed (T62): a definition, the uses, a hover and a rename's edits,
    /// once it has indexed the project.
    #[test]
    #[ignore = "needs rust-analyzer"]
    fn rust_analyzer_answers_questions_about_a_symbol() {
        use crate::code::{self, Op};
        let dir = std::env::temp_dir().join(format!("glut-code-live-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname = \"d\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("src/main.rs"),
            "/// A note.\nstruct Letter;\n\nfn make() -> Letter {\n    Letter\n}\n\n\
             fn main() {\n    let _ = make();\n}\n",
        )
        .unwrap();
        let root = dir.canonicalize().unwrap();
        let home = std::path::PathBuf::from(std::env::var_os("HOME").unwrap());
        let profile = crate::sandbox::Policy::new(&root, &home, &[], &[]).profile();
        let mut servers = Diagnostics::new(&root, profile);
        servers.allow("rust-analyzer");
        let mut ask = |op: Op, line: u32, symbol: &str| {
            servers
                .ask("src/main.rs", op.method(), |uri, text| {
                    let position = code::position(text, line, symbol, 1)?;
                    Ok(code::params(op, uri, position, Some("Note")))
                })
                .unwrap()
        };
        let definition = code::places(&ask(Op::Definition, 9, "make"));
        assert_eq!(definition.len(), 1, "{definition:?}");
        assert_eq!(definition[0].line, 3);
        let uses = code::places(&ask(Op::References, 2, "Letter"));
        assert_eq!(uses.len(), 3, "{uses:?}");
        assert!(code::hover_text(&ask(Op::Hover, 4, "Letter")).contains("A note."));
        let edits = code::workspace_edits(&ask(Op::Rename, 2, "Letter")).unwrap();
        let text = std::fs::read_to_string(root.join("src/main.rs")).unwrap();
        let renamed =
            code::apply(&text, &edits[&root.join("src/main.rs")], Some("Letter")).unwrap();
        assert_eq!(renamed.matches("Note").count(), 3, "{renamed}");
        assert!(!renamed.contains("Letter"), "{renamed}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Live against rust-analyzer when it is installed: an edit with a type error gets the error back.
    #[test]
    #[ignore = "needs rust-analyzer"]
    fn rust_analyzer_reports_a_type_error() {
        let dir = std::env::temp_dir().join(format!("glut-diag-{}", std::process::id()));
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
        assert!(
            diagnostics.after_edit("src/main.rs").is_none(),
            "nothing starts unallowed"
        );
        diagnostics.allow("rust-analyzer");
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
