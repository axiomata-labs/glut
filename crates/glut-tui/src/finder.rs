//! The search across kept sessions (T55): `/resume`'s filter finds in the conversations
//! too, and `glut search` prints what it finds. What is searched is the owner's messages, the model's answers and the
//! tool calls' names and arguments — never a tool's result (a file read would answer every search) nor the thinking.
//! There is no index: the sessions are read when the list opens, kept as their searchable text alone while it is
//! open, and let go with it.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use glut::Message;
use glut::model::Role;

use crate::store::Session;

/// How many characters a query needs before the conversations are searched: one letter would find every session.
pub const CONTENT_FROM: usize = 2;
/// How many characters of a hit's message stand before the word in its snippet, and how many in all.
const SNIPPET_BEFORE: usize = 24;
const SNIPPET_CHARS: usize = 80;

/// The kept sessions as the search needs them, newest first.
#[derive(Debug, Default)]
pub struct Corpus {
    sessions: Vec<Kept>,
}

/// One kept session: where and when it worked, and its messages' searchable text.
#[derive(Debug)]
struct Kept {
    id: String,
    cwd: PathBuf,
    title: String,
    updated: DateTime<Utc>,
    /// Each searchable message after the part a fork shares with its parent (that part is the parent's to find): its
    /// place in the conversation, its text, and the text lowered character for character — a place in the one is the
    /// same place in the other.
    texts: Vec<(usize, String, String)>,
}

/// A session the search found.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: String,
    pub cwd: PathBuf,
    pub title: String,
    pub updated: DateTime<Utc>,
    /// The newest message that holds every word, a short part of it around the first; `None` for a title's hit.
    pub snippet: Option<String>,
    /// How many more messages hold every word.
    pub more: usize,
    /// What the stream's search (⌃F) looks for once the session is open: the query where the newest hit holds it as
    /// typed, else its longest word — the stream's search finds one run of characters.
    pub find: String,
}

impl Corpus {
    /// Every kept session in `home`, read one after the other and kept as its searchable text alone.
    pub fn read(home: &Path) -> Self {
        let mut read: Vec<(Kept, Option<String>, Vec<u64>)> = Vec::new();
        Session::each(home, |session| {
            let prints = session.conversation.iter().map(print_of).collect();
            let texts = session
                .conversation
                .iter()
                .enumerate()
                .filter_map(|(at, message)| Some((at, searchable(message)?)))
                .map(|(at, text)| {
                    let lowered = text.chars().map(crate::select::lower).collect();
                    (at, text, lowered)
                })
                .collect();
            let kept = Kept {
                id: session.id,
                cwd: session.cwd,
                title: session.title,
                updated: session.updated,
                texts,
            };
            read.push((kept, session.parent, prints));
        });
        // What a fork took over from its parent is the parent's to find.
        let shares: Vec<usize> = read
            .iter()
            .map(|(_, parent, prints)| {
                parent
                    .as_ref()
                    .and_then(|parent| read.iter().find(|(other, _, _)| &other.id == parent))
                    .map_or(0, |(_, _, theirs)| shared(theirs, prints))
            })
            .collect();
        let mut sessions: Vec<Kept> = read
            .into_iter()
            .zip(shares)
            .map(|((mut kept, _, _), shared)| {
                kept.texts.retain(|(at, _, _)| *at >= shared);
                kept
            })
            .collect();
        sessions.sort_by_key(|kept| std::cmp::Reverse(kept.updated));
        Corpus { sessions }
    }

    /// The sessions of `cwd` (every folder's for `None`) that hold every word of `query`, newest first: those whose
    /// title holds them, then those whose conversation does — each session once. The titles of `listed` are not
    /// looked at (the list shows those itself), nor anything of `shown`.
    pub fn find(
        &self,
        query: &str,
        cwd: Option<&Path>,
        listed: &[&str],
        shown: &[&str],
    ) -> Vec<Hit> {
        let words: Vec<String> = query
            .split_whitespace()
            .map(|word| word.chars().map(crate::select::lower).collect())
            .collect();
        let phrase: String = query.trim().chars().map(crate::select::lower).collect();
        let longest = query
            .split_whitespace()
            .max_by_key(|word| word.chars().count())
            .unwrap_or_default();
        let holds = |text: &str| words.iter().all(|word| text.contains(word.as_str()));
        let candidates = || {
            self.sessions
                .iter()
                .filter(|kept| cwd.is_none_or(|cwd| kept.cwd == cwd))
                .filter(|kept| !shown.contains(&kept.id.as_str()))
        };
        let hit = |kept: &Kept, snippet, more, find: &str| Hit {
            id: kept.id.clone(),
            cwd: kept.cwd.clone(),
            title: kept.title.clone(),
            updated: kept.updated,
            snippet,
            more,
            find: find.to_owned(),
        };
        // Nothing typed lists every project's sessions (⌃A), the folder's being the list's own.
        if words.is_empty() {
            return match cwd {
                Some(_) => Vec::new(),
                None => candidates().map(|kept| hit(kept, None, 0, "")).collect(),
            };
        }
        let mut hits: Vec<Hit> = Vec::new();
        hits.extend(
            candidates()
                .filter(|kept| !listed.contains(&kept.id.as_str()))
                .filter(|kept| {
                    holds(
                        &kept
                            .title
                            .chars()
                            .map(crate::select::lower)
                            .collect::<String>(),
                    )
                })
                .map(|kept| hit(kept, None, 0, query.trim())),
        );
        if query.chars().filter(|c| !c.is_whitespace()).count() < CONTENT_FROM {
            return hits;
        }
        for kept in candidates() {
            if hits.iter().any(|found| found.id == kept.id) {
                continue;
            }
            let mut found = kept.texts.iter().filter(|(_, _, lowered)| holds(lowered));
            let Some(newest) = found.next_back() else {
                continue;
            };
            let more = found.count();
            let find = if newest.2.contains(&phrase) {
                query.trim()
            } else {
                longest
            };
            hits.push(hit(
                kept,
                Some(snippet(&newest.1, &newest.2, &words[0])),
                more,
                find,
            ));
        }
        hits
    }
}

/// How many messages a fork shares with its parent from the start, by their prints.
fn shared(parent: &[u64], fork: &[u64]) -> usize {
    parent.iter().zip(fork).take_while(|(a, b)| a == b).count()
}

/// A message's print: what it said and called, to tell a fork's taken-over messages from its own without keeping
/// them whole.
fn print_of(message: &Message) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (message.role as u8).hash(&mut hasher);
    message.content.hash(&mut hasher);
    message.tool_call_id.hash(&mut hasher);
    for call in &message.tool_calls {
        (&call.id, &call.function.name, &call.function.arguments).hash(&mut hasher);
    }
    hasher.finish()
}

/// The text of `message` the search looks in: the owner's and the model's words and the tools called with their
/// arguments; `None` for a tool's result and the system prompt.
fn searchable(message: &Message) -> Option<String> {
    if !matches!(message.role, Role::User | Role::Assistant) {
        return None;
    }
    let mut text = message.content.clone().unwrap_or_default();
    for call in &message.tool_calls {
        text.push('\n');
        text.push_str(&call.function.name);
        text.push(' ');
        text.push_str(&call.function.arguments);
    }
    (!text.trim().is_empty()).then_some(text)
}

/// A short part of a hit's message around `word`, on one line, cleaned for the terminal.
fn snippet(text: &str, lowered: &str, word: &str) -> String {
    let at = lowered
        .find(word)
        .map_or(0, |byte| lowered[..byte].chars().count());
    let start = at.saturating_sub(SNIPPET_BEFORE);
    let total = text.chars().count();
    let part: String = text.chars().skip(start).take(SNIPPET_CHARS).collect();
    let part = crate::transcript::clean(&part)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let before = if start > 0 { "…" } else { "" };
    let after = if start + SNIPPET_CHARS < total {
        "…"
    } else {
        ""
    };
    format!("{before}{part}{after}")
}

/// `glut search`: the sessions of `cwd` (every project's when `all`) whose title or conversation holds every word
/// of `words`, newest first — a line each: its id, its title and the part of the conversation it was found in.
/// Ends with 1 when nothing was found, as `grep` does.
pub fn headless(words: &[String], all: bool, cwd: &Path) -> std::process::ExitCode {
    use std::io::Write as _;
    let home = match glut::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => {
            eprintln!("glut: {err}");
            return std::process::ExitCode::from(1);
        }
    };
    // Sessions are kept by their folder's full path, as a session started here names it.
    let cwd = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let hits =
        Corpus::read(&home).find(&words.join(" "), (!all).then_some(cwd.as_path()), &[], &[]);
    let mut out = std::io::stdout().lock();
    for hit in &hits {
        // The id as the file says it, not as its name: cleaned like the title.
        let mut line = format!(
            "{}\t{}",
            crate::transcript::clean(&hit.id),
            crate::transcript::clean(&hit.title)
        );
        if all {
            line.push_str(&format!(" · {}", project_of(&hit.cwd)));
        }
        if let Some(snippet) = &hit.snippet {
            line.push('\t');
            line.push_str(snippet);
            if hit.more > 0 {
                line.push_str(&format!("  {}", tr!(found_more, count = hit.more)));
            }
        }
        // A reader that stopped early (`| head`) took what it wanted.
        if writeln!(out, "{line}").is_err() {
            break;
        }
    }
    if hits.is_empty() {
        std::process::ExitCode::from(1)
    } else {
        std::process::ExitCode::SUCCESS
    }
}

/// A project's name as a hit across every folder says it: its folder's last part.
pub fn project_of(cwd: &Path) -> String {
    crate::transcript::clean(&cwd.file_name().map_or_else(
        || cwd.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glut::model::{FunctionCall, ToolCall};

    fn said(role: Role, text: &str) -> Message {
        Message {
            role,
            content: Some(text.into()),
            ..Message::user("")
        }
    }

    fn kept(
        home: &Path,
        id: &str,
        cwd: &str,
        title: &str,
        conversation: Vec<Message>,
        parent: Option<&str>,
    ) {
        let mut session = Session::new(Path::new(cwd), "p", "m");
        session.id = id.into();
        session.title = title.into();
        session.conversation = conversation;
        session.parent = parent.map(Into::into);
        session.save(home).unwrap();
        // A little apart: the newest is the last saved.
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    #[test]
    fn titles_first_then_conversations_every_word_a_fork_only_after_it_forked_never_a_result() {
        let home = std::env::temp_dir().join(format!("glut-finder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let early = vec![
            said(Role::System, "You are Glut; the sandbox is Seatbelt."),
            said(Role::User, "Explain the Sandbox to me"),
            said(
                Role::Assistant,
                "The sandbox wraps each command in Seatbelt's profile.",
            ),
        ];
        let mut asked = early.clone();
        asked.push(Message {
            role: Role::Assistant,
            content: None,
            tool_calls: vec![ToolCall {
                id: "c1".into(),
                kind: "function".into(),
                function: FunctionCall {
                    name: "read_file".into(),
                    arguments: r#"{"path":"sandbox.rs"}"#.into(),
                },
            }],
            ..Message::user("")
        });
        asked.push(Message::tool(
            "c1",
            "fn seatbelt_profile() — the whole file",
        ));
        kept(
            &home,
            "a",
            "/w/glut",
            "Sandbox questions",
            early.clone(),
            None,
        );
        kept(&home, "b", "/w/glut", "Other work", asked, Some("a"));
        kept(
            &home,
            "c",
            "/w/os",
            "Seatbelt in the OS",
            vec![said(Role::User, "seatbelt please")],
            None,
        );
        let corpus = Corpus::read(&home);
        let glut = Path::new("/w/glut");

        let found = corpus.find("sandbox", Some(glut), &[], &[]);
        let ids: Vec<&str> = found.iter().map(|hit| hit.id.as_str()).collect();
        assert_eq!(
            ids,
            ["a", "b"],
            "the title's hit first, then the fork's own call"
        );
        assert_eq!(found[0].snippet, None);
        assert_eq!(
            found[1].snippet.as_deref(),
            Some(r#"read_file {"path":"sandbox.rs"}"#)
        );

        let found = corpus.find("seatbelt sandbox", Some(glut), &["a", "b"], &[]);
        assert_eq!(
            found.len(),
            1,
            "the fork shares the answer, which is the parent's"
        );
        assert_eq!((found[0].id.as_str(), found[0].more), ("a", 0));
        assert_eq!(found[0].find, "seatbelt", "not as typed: the longest word");
        assert!(
            corpus.find("whole file", None, &[], &[]).is_empty(),
            "a result is not searched"
        );
        assert!(
            corpus.find("glut", None, &[], &[]).is_empty(),
            "nor the system prompt"
        );

        let everywhere = corpus.find("seatbelt", None, &[], &["a"]);
        let ids: Vec<&str> = everywhere.iter().map(|hit| hit.id.as_str()).collect();
        assert_eq!(ids, ["c"], "every folder's, the one left out not");
        assert_eq!(
            corpus.find("s", None, &["a", "b", "c"], &[]),
            Vec::new(),
            "one letter searches titles alone"
        );
        assert_eq!(
            corpus.find(" ", None, &[], &[]).len(),
            3,
            "nothing typed lists every project's"
        );
        assert!(
            corpus.find("", Some(glut), &[], &[]).is_empty(),
            "the folder's are the list's own"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_snippet_is_one_cleaned_line_around_the_word() {
        let text = format!(
            "{} needle\u{202e} in\nthe {}",
            "hay ".repeat(20),
            "stack ".repeat(20)
        );
        let lowered: String = text.chars().map(crate::select::lower).collect();
        let shown = snippet(&text, &lowered, "needle");
        assert!(shown.starts_with("…") && shown.ends_with("…"), "{shown}");
        assert!(shown.contains("needle in the stack"), "{shown}");
        assert!(!shown.contains('\u{202e}'));
    }
}
