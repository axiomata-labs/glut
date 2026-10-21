//! The run's store of whole texts (Glut lean, N2): a tool result too long for the conversation is never cut — the model
//! gets its head and tail and a note, the whole text stays here, and `read_full_result` reads it on in parts.
//!
//! * **Relocation, never loss**: every character of a relocated result can be read back.
//! * **Once, at the moment it arrives**: a result is bounded before it enters the conversation and never touched
//!   again, so the conversation before it stays byte for byte the same and a provider's prompt cache holds.
//!
//! Like `task`, `read_full_result` has no [`crate::tools::Tool`] impl: the store belongs to the loop, which answers
//! the call itself (`run.rs`). Code mode bounds a script's output the same way.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::model::ToolSpec;
use crate::tools::MAX_TOOL_OUTPUT_CHARS;

/// What a run's store keeps of relocated results and script outputs, in bytes; past it the oldest are dropped.
const MAX_STORED_BYTES: usize = 32 * 1024 * 1024;

/// The tool's name.
pub(crate) const NAME: &str = "read_full_result";

/// Where a run's store keeps its texts beyond the run (Glut lean, N2): a chat's next turn and a resumed run read the
/// ids its notes name from there. `tag` is what the ids start with — the conversation's or the run's own id, so it is
/// the same in every turn.
#[derive(Debug, Clone)]
pub struct StoreSpec {
    pub dir: PathBuf,
    pub tag: String,
}

/// The whole texts of relocated results. Shared, so code mode can put an over-long output here too.
///
/// An id is `<tag>-<n>`. In memory only, the tag is the run's own, so a note from an earlier turn of a chat is
/// answered "no longer available" — never with another run's text under the same number. With a [`StoreSpec`], the
/// texts are also written to its folder and the numbering goes on from what is there, so later turns read them back.
#[derive(Debug, Clone)]
pub struct ResultStore {
    tag: String,
    texts: Arc<Mutex<Vec<String>>>,
    /// The folder the texts are also written to, and how many ids it held when this store opened.
    disk: Option<(PathBuf, usize)>,
}

impl ResultStore {
    /// An empty store whose ids carry `tag`.
    pub fn new(tag: impl Into<String>) -> Self {
        Self {
            tag: tag.into(),
            texts: Arc::default(),
            disk: None,
        }
    }

    /// A store that also keeps its texts in `spec.dir`, numbering on from the texts already there.
    pub fn persistent(spec: StoreSpec) -> Self {
        let first = std::fs::read_dir(&spec.dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| number_of(&entry.ok()?.path()))
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        Self {
            tag: spec.tag,
            texts: Arc::default(),
            disk: Some((spec.dir, first)),
        }
    }

    /// Keeps `text`; returns its id.
    pub fn put(&self, text: String) -> String {
        let mut texts = self
            .texts
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let first = self.disk.as_ref().map_or(0, |(_, first)| *first);
        let number = first + texts.len() + 1;
        if let Some((dir, _)) = &self.disk {
            // Best effort: a text that cannot be written stays readable from memory for the rest of this run.
            if write_text(dir, number, &text).is_ok() {
                prune(dir);
            }
        }
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
        format!("{}-{number}", self.tag)
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
        let dropped = || {
            format!("result {id} was dropped to keep the run's memory small; call its tool again")
        };
        let first = self.disk.as_ref().map_or(0, |(_, first)| *first);
        let text = if number <= first {
            let (dir, _) = self
                .disk
                .as_ref()
                .ok_or_else(|| format!("no result with id {id}"))?;
            std::fs::read_to_string(text_path(dir, number)).map_err(|_| dropped())?
        } else {
            let texts = self
                .texts
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let text = texts
                .get(number - first - 1)
                .ok_or_else(|| format!("no result with id {id}"))?;
            if text.is_empty() {
                // Dropped from memory: the folder may still hold it.
                match &self.disk {
                    Some((dir, _)) => {
                        std::fs::read_to_string(text_path(dir, number)).map_err(|_| dropped())?
                    }
                    None => return Err(dropped()),
                }
            } else {
                text.clone()
            }
        };
        part_of(&text, offset)
    }
}

/// Up to [`MAX_TOOL_OUTPUT_CHARS`] characters of `text` from `offset` on, with a note when more follows.
fn part_of(text: &str, offset: usize) -> Result<String, String> {
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

fn text_path(dir: &Path, number: usize) -> PathBuf {
    dir.join(format!("{number}.txt"))
}

/// The number of a store file (`<n>.txt`).
fn number_of(path: &Path) -> Option<usize> {
    path.file_name()?
        .to_str()?
        .strip_suffix(".txt")?
        .parse()
        .ok()
}

/// Writes text `number` into `dir`, readable by the owner alone: the texts are tool output — files, commands' output.
fn write_text(dir: &Path, number: usize, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut folder = std::fs::DirBuilder::new();
    folder.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut folder, 0o700);
    folder.create(dir)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options
        .open(text_path(dir, number))?
        .write_all(text.as_bytes())
}

/// Keeps a store's folder under [`MAX_STORED_BYTES`]: the oldest texts go first.
fn prune(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<(usize, PathBuf, u64)> = entries
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let size = std::fs::metadata(&path).ok()?.len();
            Some((number_of(&path)?, path, size))
        })
        .collect();
    files.sort_by_key(|(number, ..)| *number);
    let mut held: u64 = files.iter().map(|(.., size)| size).sum();
    for (_, path, size) in files {
        if held <= MAX_STORED_BYTES as u64 {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            held -= size;
        }
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

/// `text` as the conversation gets it: whole up to `max` characters; beyond, its head and tail with a note between
/// them, and the whole text in `store`.
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

    #[test]
    fn a_persistent_store_answers_a_later_turn_and_numbers_on() {
        let dir = std::env::temp_dir().join(format!("axh-results-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let spec = StoreSpec {
            dir: dir.clone(),
            tag: "axh-chat".into(),
        };
        let first_turn = ResultStore::persistent(spec.clone());
        assert_eq!(first_turn.put("one".into()), "axh-chat-1");
        assert_eq!(first_turn.put("two".into()), "axh-chat-2");
        // The next turn is a new process' store: it reads the earlier ids and goes on numbering.
        let next_turn = ResultStore::persistent(spec);
        assert_eq!(next_turn.read("axh-chat-1", 0).unwrap(), "one");
        assert_eq!(next_turn.put("three".into()), "axh-chat-3");
        assert_eq!(next_turn.read("axh-chat-3", 0).unwrap(), "three");
        assert!(next_turn.read("axh-chat-9", 0).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("1.txt"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
