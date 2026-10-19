//! Old tool results shrink in the conversation (Glut 2, B, G6): once the model has answered after a result, a long one
//! is replaced by its head and a note, and its whole text stays with the run — `read_full_result` gives it back.
//!
//! Measured on Glut rund's news-web run: every fetched page rode along in every later step (1.7 M input tokens for 18
//! steps). A result the model has already worked with is rarely needed whole again; when it is, one call fetches it.
//!
//! * **Relocation, never loss**: the whole text is kept in the run's [`ResultStore`], the note names its id.
//! * **Each result shrinks once**: the conversation before it stays byte for byte the same afterwards, so a provider's
//!   prompt cache breaks once at that point and holds from there on — not on every turn.
//! * **The newest results stay whole** until the model has answered on them.
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

/// Shrinks the long tool results in `messages[from..]` that an assistant message already follows, keeping their whole
/// text in `store`; returns the index up to which the conversation is done — the next call starts there, so no result
/// shrinks twice and none is looked at again.
pub(crate) fn shrink_answered(messages: &mut [Message], from: usize, store: &ResultStore) -> usize {
    let Some(last_answer) = messages.iter().rposition(|m| m.role == Role::Assistant) else {
        return from;
    };
    if last_answer <= from {
        return from;
    }
    for message in &mut messages[from..last_answer] {
        if message.role != Role::Tool {
            continue;
        }
        let Some(text) = message.content.as_deref() else {
            continue;
        };
        let total = text.chars().count();
        if total <= SHRINK_OVER_CHARS {
            continue;
        }
        let head: String = text.chars().take(KEEP_HEAD_CHARS).collect();
        let id = store.put(message.content.take().unwrap_or_default());
        let removed = total - KEEP_HEAD_CHARS;
        message.content = Some(format!(
            "{head}\n\n[shortened: {removed} more characters were taken out of the conversation after you used this \
             result; `{NAME}` with id \"{id}\" gives the whole text]"
        ));
    }
    last_answer
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
    fn answered_long_results_shrink_once_and_the_newest_stay_whole() {
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
        let done = shrink_answered(&mut messages, 0, &store);
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
        assert_eq!(shrink_answered(&mut messages, done, &store), done);
        assert_eq!(messages, before);

        messages.push(assistant());
        assert_eq!(shrink_answered(&mut messages, done, &store), 7);
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
