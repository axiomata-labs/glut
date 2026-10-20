//! The run's store of whole texts (Glut lean, N2): a tool result too long for the conversation is never cut — the model
//! gets its head and tail and a note, the whole text stays here, and `read_full_result` reads it on in parts.
//!
//! * **Relocation, never loss**: every character of a relocated result can be read back.
//! * **Once, at the moment it arrives**: a result is bounded before it enters the conversation and never touched
//!   again, so the conversation before it stays byte for byte the same and a provider's prompt cache holds.
//!
//! Like `task`, `read_full_result` has no [`crate::tools::Tool`] impl: the store belongs to the loop, which answers
//! the call itself (`run.rs`). Code mode bounds a script's output the same way.

use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::model::ToolSpec;
use crate::tools::MAX_TOOL_OUTPUT_CHARS;

/// What a run's store keeps of relocated results and script outputs, in bytes; past it the oldest are dropped.
const MAX_STORED_BYTES: usize = 32 * 1024 * 1024;

/// The tool's name.
pub(crate) const NAME: &str = "read_full_result";

/// The whole texts of relocated results. Shared, so code mode can put an over-long output here too.
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
        // Past the store's limit the oldest texts go: their ids then answer "dropped", the newest stay readable.
        let mut held: usize = texts.iter().map(String::len).sum();
        for old in texts.iter_mut() {
            if held <= MAX_STORED_BYTES {
                break;
            }
            held -= old.len();
            *old = String::new();
        }
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
        if text.is_empty() {
            return Err(format!(
                "result {id} was dropped to keep the run's memory small; call its tool again"
            ));
        }
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
        description: "Read the whole text of a tool result that was too long for the conversation (its note names \
                      the id). Long texts come in parts: read on with the offset the answer names."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "id": { "type": "string", "description": "The id the note names." },
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

/// `text` as the conversation gets it: whole up to `max` characters; beyond, its head and tail with a note between them,
/// and the whole text in `store`.
pub(crate) fn bounded(text: String, store: &ResultStore, max: usize) -> String {
    let total = text.chars().count();
    if total <= max {
        return text;
    }
    let half = max / 2;
    let head: String = text.chars().take(half).collect();
    let tail: String = text.chars().skip(total - half).collect();
    let id = store.put(text);
    format!(
        "{head}\n\n[… {} of {total} characters left out of the middle; `{NAME}` with id \"{id}\" and an offset reads \
         the whole text …]\n\n{tail}",
        total - 2 * half
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
mod store_tests {
    use super::*;

    #[test]
    fn a_full_store_drops_its_oldest_texts_and_says_so() {
        let store = ResultStore::new("f");
        let big = "x".repeat(MAX_STORED_BYTES / 2 + 1);
        let first = store.put(big.clone());
        let second = store.put(big.clone());
        let third = store.put(big);
        assert!(store.read(&first, 0).unwrap_err().contains("dropped"));
        assert!(store.read(&second, 0).unwrap_err().contains("dropped"));
        assert!(store.read(&third, 0).is_ok());
    }

    #[test]
    fn an_over_long_result_keeps_head_and_tail_and_every_character_comes_back() {
        let store = ResultStore::new("b");
        assert_eq!(bounded("short".into(), &store, 100), "short");
        let text: String = (0..5_000)
            .map(|n| char::from(b'a' + (n % 26) as u8))
            .collect();
        let shown = bounded(text.clone(), &store, 1_000);
        assert!(shown.starts_with(&text[..500]) && shown.ends_with(&text[4_500..]));
        assert!(shown.contains("4000 of 5000 characters left out") && shown.contains("\"b-1\""));
        // Read back in parts, the whole text is there: nothing was lost.
        let mut back = String::new();
        let mut offset = 0;
        while offset < 5_000 {
            let part = store.read("b-1", offset).unwrap();
            let part = part.split("\n\n[characters").next().unwrap().to_owned();
            offset += part.chars().count();
            back.push_str(&part);
        }
        assert_eq!(back, text);
    }
}
