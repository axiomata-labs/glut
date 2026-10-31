//! Glut's side of its editor protocol (T64c, `docs/editor-protocol.md`): an editor plugin (`glut.nvim`) listens on
//! loopback and leaves `~/.glut/ide/<port>.json` with its port, folders and a fresh token; Glut finds the one its
//! folder lies in — or the one `GLUT_IDE_PORT` names — and connects as an MCP client to a server named `ide`, which no
//! `mcp.json` names and `/mcp` never lists. Through it Glut's file tools read the editor's loaded buffers, the owner's
//! messages carry where they work, the model may ask what is open and what the language servers say, and the gate's
//! questions about edits show as the editor's own diffs.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use glut::editor::{Context, Diagnostic, OpenFile, Selection};
use glut::mcp::Client;
use glut::mcp::config::ServerConfig;
use serde_json::{Value, json};

/// The server name Glut gives the editor's connection.
pub const SERVER: &str = "ide";
/// The variable a terminal the plugin starts Glut in names its port by.
pub const PORT_VARIABLE: &str = "GLUT_IDE_PORT";
/// The version of the protocol Glut speaks.
const VERSION: u64 = 1;
/// How long a question to the editor may take before Glut goes on without it.
const ASK_WAIT: Duration = Duration::from_secs(5);

/// An editor that left its file for Glut.
#[derive(Debug, Clone)]
pub struct Found {
    /// The editor's process, which must be the one listening on `port`.
    pid: i32,
    pub port: u16,
    /// The editor, as it names itself ("Neovim").
    pub ide: String,
    pub folders: Vec<PathBuf>,
    token: String,
}

/// The folder the editors leave their files in.
pub fn dir(home: &Path) -> PathBuf {
    home.join(".glut").join("ide")
}

/// Every editor whose file is there, readable as the protocol says: the folder this user's alone, each file theirs
/// and `0600`, of version 1, its process alive.
pub fn found(home: &Path) -> Vec<Found> {
    use std::os::unix::fs::MetadataExt;
    let dir = dir(home);
    let uid = rustix::process::getuid().as_raw();
    let private = std::fs::symlink_metadata(&dir)
        .is_ok_and(|meta| meta.is_dir() && meta.uid() == uid && meta.mode() & 0o077 == 0);
    if !private {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter(|entry| {
            entry
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.uid() == uid && meta.mode() & 0o077 == 0)
        })
        .filter_map(|entry| {
            let file = glut::providers::open_no_follow(&entry.path()).ok()?;
            let value: Value =
                serde_json::from_reader(std::io::Read::take(file, 64 * 1024)).ok()?;
            (value.get("version")?.as_u64()? == VERSION).then_some(())?;
            let pid = i32::try_from(value.get("pid")?.as_u64()?).ok()?;
            // Alive and this user's: a process of another user answers EPERM, and is no editor of theirs.
            let alive = rustix::process::Pid::from_raw(pid)
                .is_some_and(|pid| rustix::process::test_kill_process(pid).is_ok());
            alive.then_some(())?;
            let token = value.get("authToken")?.as_str()?.to_owned();
            // A token that could carry a second header line is no token.
            (token.len() >= 16 && token.chars().all(|c| c.is_ascii_alphanumeric())).then_some(())?;
            Some(Found {
                pid,
                port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
                ide: value
                    .get("ideName")
                    .and_then(Value::as_str)
                    .unwrap_or("editor")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == ' ')
                    .take(30)
                    .collect(),
                folders: value
                    .get("workspaceFolders")?
                    .as_array()?
                    .iter()
                    .filter_map(Value::as_str)
                    .map(PathBuf::from)
                    .filter(|folder| folder.is_absolute())
                    .collect(),
                token,
            })
        })
        .collect()
}

/// The editors Glut in `cwd` may connect to: the one `GLUT_IDE_PORT` names, else every one whose folders hold `cwd`.
pub fn fitting(home: &Path, cwd: &Path) -> Vec<Found> {
    let all = found(home);
    let named = std::env::var(PORT_VARIABLE)
        .ok()
        .and_then(|port| port.parse::<u16>().ok());
    if let Some(port) = named
        && let Some(found) = all.iter().find(|found| found.port == port)
    {
        return vec![found.clone()];
    }
    all.into_iter()
        .filter(|found| found.folders.iter().any(|folder| cwd.starts_with(folder)))
        .collect()
}

/// The process listening on `port` on loopback, by `lsof`; `None` where none is, or it cannot be told.
fn listener_of(port: u16) -> Option<i32> {
    // Its own place on macOS, not whatever a PATH names first.
    let program = if Path::new("/usr/sbin/lsof").exists() {
        "/usr/sbin/lsof"
    } else {
        "lsof"
    };
    let out = std::process::Command::new(program)
        .args([
            "-nP",
            &format!("-iTCP@127.0.0.1:{port}"),
            "-sTCP:LISTEN",
            "-Fp",
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut pids = text
        .lines()
        .filter_map(|line| line.strip_prefix('p')?.parse::<i32>().ok());
    let pid = pids.next()?;
    // Two listeners on one port is no answer to trust.
    pids.all(|other| other == pid).then_some(pid)
}

/// The connection to one editor.
pub struct Bridge {
    pub found: Found,
    /// The quick questions: context, buffers, files, diagnostics.
    asks: tokio::sync::Mutex<Client>,
    /// The diffs, which wait for the owner: a connection of their own, so a quick question never waits behind one.
    diffs: tokio::sync::Mutex<Client>,
    /// The runtime the file tools wait on from their blocking threads.
    runtime: tokio::runtime::Handle,
    /// Questions the editor left unanswered in a row: after [`MOST_SILENCES`] it is taken for gone, and asked no more.
    silences: std::sync::atomic::AtomicU32,
}

/// How many questions in a row an editor may leave unanswered before Glut stops asking it.
const MOST_SILENCES: u32 = 3;

/// How a diff in the editor ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Accepted,
    Rejected,
    /// Closed without an answer, or the editor gone.
    Closed,
}

impl Bridge {
    /// Connects to `found`: two MCP connections with its token, the handshake on each.
    ///
    /// # Errors
    ///
    /// Why the editor did not answer as the protocol says.
    pub async fn connect(found: Found) -> Result<Arc<Bridge>, String> {
        // The port of an editor that died may be another program's by now: the token and the files go only to the
        // process the file names.
        let (pid, port) = (found.pid, found.port);
        let listening = tokio::task::spawn_blocking(move || listener_of(port))
            .await
            .map_err(|err| err.to_string())?;
        if listening != Some(pid) {
            return Err(format!("port {port} is not the editor's any more"));
        }
        let config = ServerConfig::Http {
            url: format!("http://127.0.0.1:{}/mcp", found.port),
            headers: BTreeMap::from([(
                "Authorization".to_owned(),
                format!("Bearer {}", found.token),
            )]),
            oauth: None,
        };
        let asks = Client::connect(SERVER, &config)
            .await
            .map_err(|err| err.to_string())?;
        let mut diffs = Client::connect(SERVER, &config)
            .await
            .map_err(|err| err.to_string())?;
        diffs.set_call_timeout(glut::mcp::elicit::WAIT);
        Ok(Arc::new(Bridge {
            found,
            asks: tokio::sync::Mutex::new(asks),
            diffs: tokio::sync::Mutex::new(diffs),
            runtime: tokio::runtime::Handle::current(),
            silences: std::sync::atomic::AtomicU32::new(0),
        }))
    }

    /// One quick question, its answer as JSON; `None` where the editor did not answer in time or as it should.
    async fn ask(&self, tool: &str, arguments: Value) -> Option<Value> {
        use std::sync::atomic::Ordering;
        if self.gone() {
            return None;
        }
        let asked = async {
            let mut client = self.asks.lock().await;
            client.call(tool, arguments).await.ok()
        };
        let answer = tokio::time::timeout(ASK_WAIT, asked).await.ok().flatten();
        if answer.is_some() {
            self.silences.store(0, Ordering::SeqCst);
        } else {
            self.silences.fetch_add(1, Ordering::SeqCst);
        }
        serde_json::from_str(&answer?).ok()
    }

    /// Whether the editor left too many questions unanswered: gone, until `/ide on` connects anew.
    pub fn gone(&self) -> bool {
        self.silences.load(std::sync::atomic::Ordering::SeqCst) >= MOST_SILENCES
    }

    /// [`Self::ask`] from a blocking thread, as the file tools call.
    fn ask_blocking(&self, tool: &str, arguments: Value) -> Option<Value> {
        self.runtime.block_on(self.ask(tool, arguments))
    }

    /// Shows the change of `path` from `old` (`None`: a new file) to `new` as the editor's diff, and waits for the
    /// owner's answer there — or for [`Self::close_diff`] of the same `id`.
    pub async fn open_diff(
        &self,
        id: &str,
        path: &Path,
        old: Option<&str>,
        new: &str,
        title: &str,
    ) -> Outcome {
        let arguments = json!({
            "id": id, "path": path.display().to_string(), "oldText": old, "newText": new, "title": title,
        });
        let answer = {
            let mut client = self.diffs.lock().await;
            client.call("openDiff", arguments).await.ok()
        };
        let outcome = answer
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|value| {
                value
                    .get("outcome")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            });
        match outcome.as_deref() {
            Some("accepted") => Outcome::Accepted,
            Some("rejected") => Outcome::Rejected,
            _ => Outcome::Closed,
        }
    }

    /// Closes the diff `id`: the TUI's card answered first.
    pub async fn close_diff(&self, id: &str) {
        let _ = self.ask("closeDiff", json!({"id": id})).await;
    }

    /// Tells the editor Glut wrote `paths`: it reads them again where no unsaved change stands.
    pub async fn changed(&self, paths: &[PathBuf]) {
        let paths: Vec<String> = paths
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        let _ = self.ask("changed", json!({"paths": paths})).await;
    }
}

/// Whether the editor answered for the file asked about, by the path it names: an editor that matched a name loosely
/// (`.env.local` for `.env`) would hand over another file than Glut's checks approved.
fn answered_for(answer: &Value, asked: &Path) -> bool {
    let Some(named) = answer.get("path").and_then(Value::as_str) else {
        return false;
    };
    let real = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_owned());
    real(Path::new(named)) == real(asked)
}

fn number(value: &Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|number| u32::try_from(number).ok())
        .unwrap_or(1)
}

impl glut::editor::EditorFiles for Bridge {
    fn read(&self, path: &Path) -> Option<String> {
        let answer =
            self.ask_blocking("readBuffer", json!({"path": path.display().to_string()}))?;
        let loaded = answer
            .get("loaded")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !loaded || !answered_for(&answer, path) {
            return None;
        }
        answer
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn offers_tools(&self) -> bool {
        !self.gone()
    }

    fn context(&self) -> Option<Context> {
        let answer = self.ask_blocking("getContext", json!({}))?;
        let active = answer
            .get("activeFile")
            .filter(|active| active.is_object())?;
        let path = PathBuf::from(active.get("path")?.as_str()?);
        let cursor = active.get("cursor").cloned().unwrap_or(Value::Null);
        let selection = active
            .get("selection")
            .filter(|selection| selection.is_object())
            .and_then(|selection| {
                Some(Selection {
                    start_line: number(selection.get("start")?, "line"),
                    end_line: number(selection.get("end")?, "line"),
                    text: selection.get("text")?.as_str()?.to_owned(),
                })
            });
        Some(Context {
            path,
            line: number(&cursor, "line"),
            character: number(&cursor, "character"),
            selection,
        })
    }

    fn open_files(&self) -> Option<Vec<OpenFile>> {
        let answer = self.ask_blocking("openFiles", json!({}))?;
        Some(
            answer
                .get("files")?
                .as_array()?
                .iter()
                .filter_map(|file| {
                    Some(OpenFile {
                        path: PathBuf::from(file.get("path")?.as_str()?),
                        active: file.get("active").and_then(Value::as_bool).unwrap_or(false),
                        dirty: file.get("dirty").and_then(Value::as_bool).unwrap_or(false),
                    })
                })
                .collect(),
        )
    }

    fn diagnostics(&self, path: &Path) -> Option<Vec<Diagnostic>> {
        let answer =
            self.ask_blocking("diagnostics", json!({"path": path.display().to_string()}))?;
        if !answered_for(&answer, path) {
            return None;
        }
        Some(
            answer
                .get("diagnostics")?
                .as_array()?
                .iter()
                .filter_map(|found| {
                    Some(Diagnostic {
                        line: number(found, "line"),
                        character: number(found, "character"),
                        severity: found.get("severity")?.as_str()?.to_owned(),
                        message: found.get("message")?.as_str()?.to_owned(),
                        source: found
                            .get("source")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glut::editor::EditorFiles;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    /// An editor as the protocol says one answers: a token on every request, the tools on a buffer it holds.
    async fn editor(token: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut stream = BufReader::new(stream);
                    loop {
                        let (mut length, mut authorized) = (0, false);
                        let mut line = String::new();
                        if stream.read_line(&mut line).await.unwrap_or(0) == 0 {
                            return;
                        }
                        loop {
                            line.clear();
                            stream.read_line(&mut line).await.unwrap();
                            let header = line.trim().to_lowercase();
                            if header.is_empty() {
                                break;
                            }
                            if let Some(value) = header.strip_prefix("content-length:") {
                                length = value.trim().parse().unwrap();
                            }
                            authorized |= header == format!("authorization: bearer {token}");
                        }
                        let mut body = vec![0; length];
                        stream.read_exact(&mut body).await.unwrap();
                        let message: Value = serde_json::from_slice(&body).unwrap();
                        let reply = if !authorized {
                            "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\n\r\n".to_owned()
                        } else if message.get("id").is_none() {
                            "HTTP/1.1 202 Accepted\r\ncontent-length: 0\r\n\r\n".to_owned()
                        } else {
                            let tool = |text: Value| json!({"content": [{"type": "text", "text": text.to_string()}]});
                            let result = match message["method"].as_str().unwrap() {
                                "initialize" => {
                                    json!({"protocolVersion": "2025-06-18", "capabilities": {"tools": {}},
                                    "serverInfo": {"name": "Neovim", "version": "0"}})
                                }
                                "tools/list" => {
                                    json!({"tools": [{"name": "openFiles", "inputSchema": {"type": "object"}}]})
                                }
                                _ => match message["params"]["name"].as_str().unwrap() {
                                    "getContext" => tool(json!({"activeFile": {"path": "/w/a.rs",
                                        "cursor": {"line": 3, "character": 1},
                                        "selection": {"start": {"line": 2, "character": 1}, "end": {"line": 4, "character": 9},
                                        "text": "fn a()"}}})),
                                    "readBuffer" => tool(
                                        json!({"loaded": true, "path": "/w/a.rs", "text": "unsaved\n", "dirty": true}),
                                    ),
                                    "openDiff" => tool(json!({"outcome": "accepted"})),
                                    _ => tool(json!({})),
                                },
                            };
                            let body =
                                json!({"jsonrpc": "2.0", "id": message["id"], "result": result})
                                    .to_string();
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                                body.len()
                            )
                        };
                        stream.get_mut().write_all(reply.as_bytes()).await.unwrap();
                    }
                });
            }
        });
        port
    }

    fn found(port: u16, token: &str) -> Found {
        Found {
            pid: i32::try_from(std::process::id()).unwrap(),
            port,
            ide: "Neovim".into(),
            folders: vec![PathBuf::from("/w")],
            token: token.into(),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn glut_hears_the_editors_place_buffer_and_diff_with_its_token_alone() {
        let port = editor("0123456789abcdef0123").await;
        assert!(
            Bridge::connect(found(port, "ffffffffffffffffffff"))
                .await
                .is_err(),
            "a wrong token is refused"
        );
        let bridge = Bridge::connect(found(port, "0123456789abcdef0123"))
            .await
            .unwrap();
        let asked = Arc::clone(&bridge);
        let (context, read) = tokio::task::spawn_blocking(move || {
            (asked.context(), asked.read(Path::new("/w/a.rs")))
        })
        .await
        .unwrap();
        let context = context.unwrap();
        assert_eq!((context.path, context.line), (PathBuf::from("/w/a.rs"), 3));
        let selection = context.selection.unwrap();
        assert_eq!(
            (
                selection.start_line,
                selection.end_line,
                selection.text.as_str()
            ),
            (2, 4, "fn a()")
        );
        assert_eq!(read.as_deref(), Some("unsaved\n"));
        let outcome = bridge
            .open_diff("ask-1", Path::new("/w/a.rs"), Some("a"), "b", "a.rs")
            .await;
        assert_eq!(outcome, Outcome::Accepted);
    }

    #[test]
    fn only_a_private_file_of_a_live_editor_counts() {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::temp_dir().join(format!("glut-ide-{}", std::process::id()));
        let dir = dir(&home);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let write = |name: &str, pid: u32, mode: u32, token: &str| {
            let path = dir.join(name);
            std::fs::write(
                &path,
                json!({"version": 1, "port": 4000, "pid": pid, "ideName": "Neovim",
                "workspaceFolders": ["/w"], "authToken": token})
                .to_string(),
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        write(
            "live.json",
            std::process::id(),
            0o600,
            "0123456789abcdef0123",
        );
        write(
            "open.json",
            std::process::id(),
            0o644,
            "0123456789abcdef0123",
        );
        write("gone.json", 999_999, 0o600, "0123456789abcdef0123");
        write("odd.json", std::process::id(), 0o600, "0123\r\nX-Evil: 1");
        let found = found_in(&home);
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(fitting_in(&home, Path::new("/w/src")).len(), 1);
        assert!(fitting_in(&home, Path::new("/elsewhere")).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    fn found_in(home: &Path) -> Vec<Found> {
        super::found(home)
    }

    fn fitting_in(home: &Path, cwd: &Path) -> Vec<Found> {
        fitting(home, cwd)
    }
}

/// Glut against a real editor: `GLUT_IDE_HOME=<a home whose ~/.glut/ide holds a running plugin's file>`
/// `cargo test -p glut-tui --lib ide::live -- --ignored`.
#[cfg(test)]
mod live {
    use super::*;
    use glut::editor::EditorFiles;

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "needs a running editor plugin"]
    async fn glut_talks_to_a_real_plugin() {
        let home = PathBuf::from(std::env::var("GLUT_IDE_HOME").unwrap());
        let found = found(&home);
        assert_eq!(found.len(), 1, "{found:?}");
        let bridge = Bridge::connect(found[0].clone()).await.unwrap();
        let asked = Arc::clone(&bridge);
        let (context, files) =
            tokio::task::spawn_blocking(move || (asked.context(), asked.open_files()))
                .await
                .unwrap();
        println!("context {context:?}\nfiles {files:?}");
        let context = context.unwrap();
        let asked = Arc::clone(&bridge);
        let path = context.path.clone();
        let read = tokio::task::spawn_blocking(move || asked.read(&path))
            .await
            .unwrap();
        println!("read {read:?}");
        assert!(read.unwrap().contains("unsaved"));
        let diff = Arc::clone(&bridge);
        let path = context.path.clone();
        let waiting = tokio::spawn(async move {
            diff.open_diff("ask-live", &path, Some("a\n"), "b\n", "live")
                .await
        });
        tokio::time::sleep(Duration::from_millis(500)).await;
        bridge.close_diff("ask-live").await;
        assert_eq!(waiting.await.unwrap(), Outcome::Closed);
    }
}
