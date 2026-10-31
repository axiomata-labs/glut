//! The owner's kept sessions as an editor lists them (T64b): `session/list`, a page at a time, newest first — each by
//! its id, folder, title and last save, read from the files' heads alone. v2 keeps it as it is.

use std::path::Path;

use serde_json::{Value, json};

use crate::store::Session;

/// How many sessions one page lists.
const PAGE: usize = 50;

/// One page of the kept sessions, of `cwd` alone when the editor names one, from `cursor` (an offset Glut handed
/// out) on; the next page's cursor where there is one.
pub fn page(home: &Path, cwd: Option<&Path>, cursor: Option<&str>) -> Result<Value, String> {
    let from = match cursor {
        None => 0,
        Some(cursor) => cursor
            .parse::<usize>()
            .map_err(|_| format!("no cursor {cursor} of Glut's"))?,
    };
    // A head whose id is no session file's name is a copy beside one: listed once, under the file it is.
    let named = |id: &str| id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let mut heads: Vec<_> = Session::heads(home)
        .into_iter()
        .filter(|head| head.id.as_deref().is_some_and(named))
        .filter(|head| cwd.is_none_or(|cwd| head.cwd == cwd))
        .collect();
    // Newest first, and the same order every time: a page follows the one before.
    heads.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.id.cmp(&b.id)));
    let listed: Vec<Value> = heads
        .iter()
        .skip(from)
        .take(PAGE)
        .map(|head| {
            let title = head
                .title
                .as_deref()
                .filter(|title| !title.is_empty())
                .map(|title| crate::transcript::clean(title).to_string());
            let mut entry = json!({
                "sessionId": head.id,
                "cwd": crate::transcript::clean(&head.cwd.display().to_string()),
                "updatedAt": head.updated.to_rfc3339(),
            });
            if let Some(title) = title {
                entry["title"] = json!(title);
            }
            entry
        })
        .collect();
    let next = from.saturating_add(PAGE);
    let next = (next < heads.len()).then(|| next.to_string());
    Ok(json!({"sessions": listed, "nextCursor": next}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sessions_come_a_page_at_a_time_of_the_folder_named() {
        let home = std::env::temp_dir().join(format!("glut-acp-list-{}", std::process::id()));
        for at in 0..(PAGE + 2) {
            let mut session = Session::new(Path::new("/w/a"), "p", "m");
            session.title = format!("task {at}");
            session.conversation = vec![glut::Message::user("x")];
            session.save(&home).unwrap();
        }
        let mut other = Session::new(Path::new("/w/b"), "p", "m");
        other.conversation = vec![glut::Message::user("x")];
        other.save(&home).unwrap();
        let first = page(&home, Some(Path::new("/w/a")), None).unwrap();
        assert_eq!(first["sessions"].as_array().unwrap().len(), PAGE);
        assert_eq!(first["nextCursor"], PAGE.to_string());
        let second = page(&home, Some(Path::new("/w/a")), first["nextCursor"].as_str()).unwrap();
        assert_eq!(second["sessions"].as_array().unwrap().len(), 2);
        assert!(second["nextCursor"].is_null());
        assert_eq!(
            page(&home, None, None).unwrap()["sessions"]
                .as_array()
                .unwrap()
                .len(),
            PAGE
        );
        assert_eq!(
            page(&home, Some(Path::new("/w/b")), None).unwrap()["sessions"][0].get("title"),
            None
        );
        assert!(page(&home, None, Some("x")).is_err());
        assert!(page(&home, None, Some(&usize::MAX.to_string())).unwrap()["nextCursor"].is_null());
        let _ = std::fs::remove_dir_all(&home);
    }
}
