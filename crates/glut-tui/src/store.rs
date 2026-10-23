//! Sessions kept to open again (T25): every session is `~/.glut/sessions/<id>.json` — the folder it worked in, a
//! title, when, what it cost, and its conversation — written after every turn, readable by the owner alone (a
//! conversation can hold anything the session read). `glut -c` goes on with the folder's newest, `glut -r` and
//! `/resume` pick one.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use glut::Message;
use glut::model::Role;
use serde::{Deserialize, Serialize};

use crate::transcript::{Kind, Transcript};

/// The folder of the sessions, below `~/.glut`.
const DIR: &str = "sessions";
/// The most sessions a list shows.
const LISTED: usize = 50;
/// A session file larger than this is not read.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// How long a title is.
const TITLE_CHARS: usize = 72;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub cwd: PathBuf,
    pub title: String,
    pub created: DateTime<Utc>,
    pub updated: DateTime<Utc>,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub cost: Option<f64>,
    pub conversation: Vec<Message>,
    /// Where each of the owner's messages began, for rewinding to before it (T11, TUI4a).
    #[serde(default)]
    pub marks: Vec<Mark>,
    /// The session this one was forked from (T14, TUI4b): the sessions are a tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

/// One of the owner's messages that started a turn: how long the conversation was before it, its first line, and the
/// run it started — whose first snapshot is the folder as it was before the message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mark {
    /// `None` once a compaction rewrote the conversation before it.
    #[serde(default)]
    pub at: Option<usize>,
    pub prompt: String,
    #[serde(default)]
    pub run: Option<String>,
}

impl Session {
    /// A new session in `cwd`.
    pub fn new(cwd: &Path, provider: &str, model: &str) -> Self {
        /// Sessions made by this process so far: two in one millisecond (a fork right after its parent) still get
        /// ids of their own.
        static MADE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let now = Utc::now();
        let made = MADE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let again = if made == 0 {
            String::new()
        } else {
            format!("-{made}")
        };
        Session {
            id: format!(
                "{}-{}{again}",
                now.format("%Y%m%d-%H%M%S%3f"),
                std::process::id()
            ),
            cwd: cwd.to_path_buf(),
            title: String::new(),
            created: now,
            updated: now,
            provider: provider.to_owned(),
            model: model.to_owned(),
            cost: None,
            conversation: Vec::new(),
            marks: Vec::new(),
            parent: None,
        }
    }

    fn dir(home: &Path) -> PathBuf {
        home.join(glut::providers::DIR).join(DIR)
    }

    /// Keeps the session; a session without a message yet is not worth a file — unless it had one and was rewound
    /// to its start.
    ///
    /// # Errors
    ///
    /// A message when it cannot be written.
    pub fn save(&mut self, home: &Path) -> Result<(), String> {
        let dir = Self::dir(home);
        let path = dir.join(format!("{}.json", self.id));
        if self.conversation.is_empty() && !path.exists() {
            return Ok(());
        }
        self.updated = Utc::now();
        let text = serde_json::to_string(self).map_err(|err| err.to_string())?;
        glut::providers::make_private_dir(&dir)
            .and_then(|()| glut::providers::write_replacing(&dir, &path, text.as_bytes(), 0o600))
            .map_err(|err| format!("{}: {err}", path.display()))
    }

    /// The session `id`.
    ///
    /// # Errors
    ///
    /// A message when there is no such session or it cannot be read.
    pub fn load(home: &Path, id: &str) -> Result<Self, String> {
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return Err(format!("no session {id}"));
        }
        read(&Self::dir(home).join(format!("{id}.json")))
    }

    /// The sessions of folder `cwd`, newest first.
    pub fn list(home: &Path, cwd: &Path) -> Vec<Session> {
        let Ok(entries) = std::fs::read_dir(Self::dir(home)) else {
            return Vec::new();
        };
        let mut sessions: Vec<Session> = entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|entry| read(&entry.path()).ok())
            .filter(|session| session.cwd == cwd)
            .collect();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated));
        sessions.truncate(LISTED);
        sessions
    }

    /// `sessions` as a tree: each session followed by its forks, indented by how deep they are; roots (and forks
    /// whose parent is not among them) newest first.
    pub fn as_tree(sessions: Vec<Session>) -> Vec<(usize, Session)> {
        let ids: Vec<String> = sessions.iter().map(|session| session.id.clone()).collect();
        let (roots, mut forks): (Vec<Session>, Vec<Session>) =
            sessions.into_iter().partition(|session| {
                session
                    .parent
                    .as_ref()
                    .is_none_or(|parent| !ids.contains(parent))
            });
        let mut out = Vec::new();
        fn walk(
            session: Session,
            depth: usize,
            forks: &mut Vec<Session>,
            out: &mut Vec<(usize, Session)>,
        ) {
            let id = session.id.clone();
            out.push((depth, session));
            let (children, rest): (Vec<Session>, Vec<Session>) = std::mem::take(forks)
                .into_iter()
                .partition(|fork| fork.parent.as_deref() == Some(id.as_str()));
            *forks = rest;
            for child in children {
                walk(child, depth + 1, forks, out);
            }
        }
        for root in roots {
            walk(root, 0, &mut forks, &mut out);
        }
        // A parent chain that runs in a circle (a file edited by hand) has no root: its sessions stand as roots.
        for left in std::mem::take(&mut forks) {
            out.push((0, left));
        }
        out
    }

    /// The stream as it was: the owner's messages, the answers, a line for each tool call with its result.
    pub fn transcript(&self, transcript: &mut Transcript) {
        for message in &self.conversation {
            match message.role {
                Role::User => {
                    if let Some(text) = &message.content {
                        transcript.push(Kind::Owner(text.clone()));
                    }
                }
                Role::Assistant => {
                    if let Some(text) = message
                        .content
                        .as_deref()
                        .filter(|text| !text.trim().is_empty())
                    {
                        transcript.push(Kind::Answer {
                            text: text.to_owned(),
                            streaming: false,
                        });
                    }
                    for call in &message.tool_calls {
                        transcript.take(&glut::Event::ToolCall {
                            turn: 0,
                            id: call.id.clone(),
                            name: call.function.name.clone(),
                            arguments: call.function.arguments.clone(),
                        });
                    }
                }
                Role::Tool => {
                    if let Some(id) = &message.tool_call_id {
                        transcript.take(&glut::Event::ToolResult {
                            turn: 0,
                            id: id.clone(),
                            name: String::new(),
                            ok: true,
                            output: message.content.clone().unwrap_or_default(),
                            truncated: false,
                        });
                    }
                }
                Role::System => {}
            }
        }
        // A call whose answer the conversation does not hold was cut off — a subagent's tile too.
        transcript.settle();
    }
}

fn read(path: &Path) -> Result<Session, String> {
    let file = glut::providers::open_no_follow(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => "no such session".to_owned(),
        _ => format!("{}: {err}", path.display()),
    })?;
    let mut text = String::new();
    file.take(MAX_BYTES)
        .read_to_string(&mut text)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let session: Session =
        serde_json::from_str(&text).map_err(|err| format!("{}: {err}", path.display()))?;
    // The id names the file a later save writes: one that could step out of the folder is refused.
    if session.id.is_empty()
        || !session
            .id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(format!("{}: no session id", path.display()));
    }
    Ok(session)
}

/// A title from the first message: its first line, cut short.
pub fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    if line.chars().count() <= TITLE_CHARS {
        return line.to_owned();
    }
    let mut short: String = line.chars().take(TITLE_CHARS - 1).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transcript::ToolState;

    #[test]
    fn forks_hang_under_their_session_and_an_orphan_is_a_root() {
        let session = |id: &str, parent: Option<&str>| {
            let mut session = Session::new(Path::new("/w"), "p", "m");
            session.id = id.into();
            session.parent = parent.map(str::to_owned);
            session
        };
        let tree = Session::as_tree(vec![
            session("b", None),
            session("a1", Some("a")),
            session("a", None),
            session("a1x", Some("a1")),
            session("gone", Some("deleted")),
        ]);
        let shown: Vec<(usize, &str)> = tree
            .iter()
            .map(|(depth, s)| (*depth, s.id.as_str()))
            .collect();
        assert_eq!(
            shown,
            [(0, "b"), (0, "a"), (1, "a1"), (2, "a1x"), (0, "gone")]
        );
        let circle = Session::as_tree(vec![
            session("x", Some("y")),
            session("y", Some("x")),
            session("z", Some("z")),
        ]);
        assert_eq!(circle.len(), 3, "a circle loses no session");
    }

    fn home() -> PathBuf {
        let home = std::env::temp_dir().join(format!(
            "glut-sessions-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&home).unwrap();
        home
    }

    #[test]
    fn a_session_is_kept_listed_by_its_folder_and_read_back_owner_only() {
        let home = home();
        let mut empty = Session::new(Path::new("/w"), "openrouter", "m");
        empty.save(&home).unwrap();
        assert!(
            Session::list(&home, Path::new("/w")).is_empty(),
            "nothing said yet: no file"
        );
        let mut session = Session::new(Path::new("/w"), "openrouter", "m");
        session.title = title_of("Fix the parser\nplease");
        session.conversation = vec![
            Message::user("Fix the parser\nplease"),
            Message::user("and more"),
        ];
        session.save(&home).unwrap();
        let listed = Session::list(&home, Path::new("/w"));
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "Fix the parser");
        assert!(Session::list(&home, Path::new("/other")).is_empty());
        let back = Session::load(&home, &session.id).unwrap();
        assert_eq!(back.conversation.len(), 2);
        use std::os::unix::fs::PermissionsExt as _;
        let file = home
            .join(".glut/sessions")
            .join(format!("{}.json", session.id));
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(Session::load(&home, "../../etc/passwd").is_err());
    }

    #[test]
    fn a_kept_conversation_comes_back_as_the_stream_it_was() {
        let call: glut::ToolCall = serde_json::from_value(serde_json::json!({
            "id": "c1", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"a.rs\"}"}
        }))
        .unwrap();
        let mut answer = Message::user("");
        answer.role = Role::Assistant;
        answer.content = Some("Let me look.".into());
        answer.tool_calls = vec![call];
        let mut result = Message::user("fn main() {}");
        result.role = Role::Tool;
        result.tool_call_id = Some("c1".into());
        let mut session = Session::new(Path::new("/w"), "p", "m");
        session.conversation = vec![Message::user("Look at a.rs"), answer, result];
        let mut transcript = Transcript::default();
        session.transcript(&mut transcript);
        let kinds: Vec<&str> = transcript
            .blocks
            .iter()
            .map(|block| match &block.kind {
                Kind::Owner(_) => "owner",
                Kind::Answer { .. } => "answer",
                Kind::Tool {
                    state: ToolState::Done,
                    ..
                } => "tool done",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["owner", "answer", "tool done"]);
    }
}
