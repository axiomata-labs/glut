//! Old tool results shrink in the conversation (Glut 2, B, G6): once the conversation grows past a budget, the oldest
//! long results the model has answered on are replaced by their head and a note, and their whole text stays with the
//! run — `read_full_result` gives it back.
//!
//! Measured on Glut rund's news-web run: every fetched page rode along in every later step (1.7 M input tokens for 18
//! steps). A result the model has already worked with is rarely needed whole again; when it is, one call fetches it.
//!
//! * **Relocation, never loss**: the whole text is kept in the run's [`ResultStore`], the note names its id.
//! * **Each result shrinks once**: the conversation before it stays byte for byte the same afterwards, so a provider's
//!   prompt cache breaks once at that point and holds from there on — not on every turn.
//! * **The newest results stay whole**, and nothing shrinks while the conversation fits the budget.
//!
//! Like `task`, `read_full_result` has no [`crate::tools::Tool`] impl: the store belongs to the loop, which answers
//! the call itself (`run.rs`).

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::model::{Message, Role, ToolSpec};
use crate::tools::MAX_TOOL_OUTPUT_CHARS;

/// A result longer than this many characters shrinks once the model has answered on it (G6).
pub const SHRINK_OVER_CHARS: usize = 2_000;
/// What stays of a shrunk result: its first characters (G6).
pub const KEEP_HEAD_CHARS: usize = 1_200;
/// Results shrink only while the conversation is longer than this, in characters (~20k tokens): a data-gathering
/// task's results stay whole as long as they fit.
pub const SHRINK_BUDGET_CHARS: usize = 80_000;

/// The tool's name.
pub(crate) const NAME: &str = "read_full_result";

/// The whole texts of the results that shrank. Shared, so code mode can put an over-long output here too.
///
/// An id is `<tag>-<n>`: the tag is the run's own, so a note from an earlier turn of a chat (whose run, and store, is
/// gone) is answered "no longer available" — never with another run's text under the same number.
#[derive(Debug, Clone)]
pub struct ResultStore {
    tag: String,
    texts: Arc<Mutex<Vec<String>>>,
}

impl ResultStore {
    /// An empty store whose ids carry `tag`.
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            texts: Arc::default(),
        }
    }

    /// Keeps `text`; returns its id.
    pub fn put(&self, text: String) -> String {
        let mut texts = self
            .texts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        texts.push(text);
        format!("{}-{}", self.tag, texts.len())
    }

    /// Up to [`MAX_TOOL_OUTPUT_CHARS`] characters of result `id` from character `offset` on, with a note when more
    /// follows.
    ///
    /// # Errors
    ///
    /// A text the model reads: an id of another run, no such id, or an offset past the end.
    pub fn read(&self, id: &str, offset: usize) -> Result<String, String> {
        let number = id
            .strip_prefix(self.tag.as_str())
            .and_then(|rest| rest.strip_prefix('-'))
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| {
                format!("result {id} is no longer available (it is from an earlier run); call its tool again")
            })?;
        let texts = self
            .texts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let text = number
            .checked_sub(1)
            .and_then(|index| texts.get(index))
            .ok_or_else(|| format!("no result with id {id}"))?;
        let total = text.chars().count();
        if offset > total || (offset == total && total > 0) {
            return Err(format!(
                "offset {offset} is past the end ({total} characters)"
            ));
        }
        let part: String = text
            .chars()
            .skip(offset)
            .take(MAX_TOOL_OUTPUT_CHARS)
            .collect();
        let end = offset + part.chars().count();
        Ok(if end < total {
            format!("{part}\n\n[characters {offset}–{end} of {total}; read on with offset {end}]")
        } else {
            part
        })
    }
}

/// What the model is shown.
pub(crate) fn spec() -> ToolSpec {
    ToolSpec {
        name: NAME.into(),
        description: "Read the whole text of an earlier tool result that was shortened in the conversation (its note \
                      names the id). Long texts come in parts: read on with the offset the answer names."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "The id the shortening note names." },
                "offset": { "type": "integer", "description": "Character to start at; default 0." }
            },
            "required": ["id"]
        }),
    }
}

/// Answers a `read_full_result` call from its JSON arguments.
pub(crate) fn answer(store: &ResultStore, arguments: &str) -> (bool, String) {
    let parsed: Result<Value, _> = serde_json::from_str(arguments);
    let Ok(Value::Object(args)) = parsed else {
        return (false, "error: the arguments must be a JSON object".into());
    };
    // A model may send the id as a number when it has only the counter in mind; the tag then decides.
    let id = match args.get("id") {
        Some(Value::String(id)) => id.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => return (false, "error: `id` is required".into()),
    };
    let offset = args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
    match store.read(&id, offset) {
        Ok(text) => (true, text),
        Err(reason) => (false, format!("error: {reason}")),
    }
}

/// Shrinks long tool results the model has already answered on, oldest first, while the conversation is over
/// `budget` characters; their whole text goes into `store`. Returns the index before which every result has been
/// dealt with — the next call starts there, so no result shrinks twice.
///
/// Over a budget, not as soon as the model answered (measured 2026-10-20, Glut 2 CP2): a task that gathers data for
/// its final answer — a calendar digest writes every event out again — still needs results it has "answered on" with
/// another call; shrinking them sent the model fetching them back until the turn limit. Under the budget nothing
/// shrinks; above it the oldest results go first, the newest stay whole.
pub(crate) fn shrink_over_budget(
    messages: &mut [Message],
    from: usize,
    store: &ResultStore,
    budget: usize,
) -> usize {
    let mut total: usize = messages.iter().map(size_of).sum();
    if total <= budget {
        return from;
    }
    let Some(last_answer) = messages.iter().rposition(|m| m.role == Role::Assistant) else {
        return from;
    };
    let mut done = from;
    for (index, message) in messages.iter_mut().enumerate().take(last_answer).skip(from) {
        if total <= budget {
            break;
        }
        done = index + 1;
        if message.role != Role::Tool {
            continue;
        }
        let Some(text) = message.content.as_deref() else {
            continue;
        };
        let chars = text.chars().count();
        if chars <= SHRINK_OVER_CHARS {
            continue;
        }
        let head: String = text.chars().take(KEEP_HEAD_CHARS).collect();
        let id = store.put(message.content.take().unwrap_or_default());
        let removed = chars - KEEP_HEAD_CHARS;
        let shrunk = format!(
            "{head}\n\n[shortened: {removed} more characters were taken out of the conversation to keep it small; \
             `{NAME}` with id \"{id}\" gives the whole text]"
        );
        total = total - chars + shrunk.chars().count();
        message.content = Some(shrunk);
    }
    done
}

/// What a message adds to the prompt, in characters: its text and its tool calls' arguments.
fn size_of(message: &Message) -> usize {
    message.content.as_deref().map_or(0, |t| t.chars().count())
        + message
            .tool_calls
            .iter()
            .map(|c| c.function.arguments.chars().count())
            .sum::<usize>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant() -> Message {
        Message {
            role: Role::Assistant,
            content: Some("thinking".into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
        }
    }

    #[test]
    fn over_the_budget_answered_long_results_shrink_once_and_the_newest_stay_whole() {
        let store = ResultStore::new("r");
        let long = "a".repeat(SHRINK_OVER_CHARS + 500);
        let mut messages = vec![
            Message::system("s"),
            Message::user("u"),
            assistant(),
            Message::tool("1", long.clone()),
            Message::tool("2", "short"),
            assistant(),
            Message::tool("3", long.clone()),
        ];
        // Under a generous budget nothing shrinks.
        let untouched = messages.clone();
        assert_eq!(shrink_over_budget(&mut messages, 0, &store, usize::MAX), 0);
        assert_eq!(messages, untouched);
        let done = shrink_over_budget(&mut messages, 0, &store, 0);
        assert_eq!(done, 5);
        let shrunk = messages[3].content.as_deref().unwrap();
        assert!(shrunk.starts_with(&"a".repeat(KEEP_HEAD_CHARS)));
        assert!(shrunk.contains("id \"r-1\"") && shrunk.contains("1300 more characters"));
        assert_eq!(messages[4].content.as_deref(), Some("short"));
        // The newest result has no answer after it yet.
        assert_eq!(messages[6].content.as_deref(), Some(long.as_str()));
        assert_eq!(store.read("r-1", 0).unwrap(), long);

        // Nothing new answered: nothing changes, and what shrank is not touched again.
        let before = messages.clone();
        assert_eq!(shrink_over_budget(&mut messages, done, &store, 0), done);
        assert_eq!(messages, before);

        messages.push(assistant());
        assert_eq!(shrink_over_budget(&mut messages, done, &store, 0), 7);
        assert!(
            messages[6]
                .content
                .as_deref()
                .unwrap()
                .contains("id \"r-2\"")
        );
        assert_eq!(messages[3], before[3]);
    }

    #[test]
    fn the_whole_text_comes_back_in_parts_and_only_from_this_run() {
        let store = ResultStore::new("run7");
        let text: String = "é".repeat(MAX_TOOL_OUTPUT_CHARS + 10);
        let id = store.put(text);
        assert_eq!(id, "run7-1");
        let first = store.read(&id, 0).unwrap();
        assert!(first.contains(&format!("read on with offset {MAX_TOOL_OUTPUT_CHARS}")));
        assert_eq!(
            store.read(&id, MAX_TOOL_OUTPUT_CHARS).unwrap(),
            "é".repeat(10)
        );
        assert!(store.read(&id, MAX_TOOL_OUTPUT_CHARS + 10).is_err());
        assert!(store.read("run7-9", 0).is_err());
        assert!(store.read("run7-0", 0).is_err());
        // An earlier run's note: its text is gone, and no other text answers in its place.
        assert!(
            store
                .read("run6-1", 0)
                .unwrap_err()
                .contains("no longer available")
        );
        assert!(
            answer(&store, r#"{"id": 1}"#)
                .1
                .contains("no longer available")
        );

        assert!(!answer(&store, r#"{"id": "run7-1", "offset": 30010}"#).0);
        assert_eq!(
            answer(&store, r#"{"id": "run7-1", "offset": 30000}"#),
            (true, "é".repeat(10))
        );
        assert!(answer(&store, "[]").1.contains("JSON object"));
        assert!(answer(&store, r#"{"offset": 1}"#).1.contains("`id`"));
    }
}

#[cfg(test)]
mod budget_tests {
    use super::*;

    #[test]
    fn the_oldest_shrink_first_and_only_until_the_conversation_fits() {
        let store = ResultStore::new("b");
        let long = || "z".repeat(10_000);
        let assistant = || Message {
            role: Role::Assistant,
            content: None,
            tool_calls: Vec::new(),
            tool_call_id: None,
        };
        let mut messages = vec![
            Message::user("u"),
            assistant(),
            Message::tool("1", long()),
            assistant(),
            Message::tool("2", long()),
            assistant(),
            Message::tool("3", long()),
        ];
        // ~30k characters, a budget of 25k: shrinking the oldest one is enough.
        let done = shrink_over_budget(&mut messages, 0, &store, 25_000);
        assert_eq!(done, 3);
        assert!(
            messages[2]
                .content
                .as_deref()
                .unwrap()
                .contains("id \"b-1\"")
        );
        assert_eq!(messages[4].content.as_deref().unwrap().len(), 10_000);
        assert_eq!(messages[6].content.as_deref().unwrap().len(), 10_000);
    }
}
