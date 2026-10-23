//! `/memory` (`docs/plans/glut-tui.md`, T35): the project's memory as the owner reads it — the facts with the
//! description their frontmatter gives, one fact whole. Read here, no model asked; never through a link, never more
//! than a fact may hold.

use std::io::Read as _;
use std::path::Path;

/// The most bytes of a fact that are read: the `memory` tool keeps none larger.
const MAX_FACT_BYTES: u64 = 32 * 1024;

/// The facts of `dir`, by name, each with its description (empty without one); the index first.
pub fn facts(dir: &Path) -> Vec<(String, String)> {
    if !dir.symlink_metadata().is_ok_and(|meta| meta.is_dir()) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut facts: Vec<(String, String)> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let name = name.strip_suffix(".md")?.to_owned();
            let about = read(&entry.path())
                .map(|text| description(&text))
                .unwrap_or_default();
            Some((name, about))
        })
        .collect();
    facts.sort_by(|(a, _), (b, _)| {
        let index = |name: &str| name != glut::tools::memory::INDEX;
        index(a).cmp(&index(b)).then_with(|| a.cmp(b))
    });
    facts
}

/// One fact's text, read without following a link; `None` for no plain file or no text.
pub fn read(path: &Path) -> Option<String> {
    let file = glut::providers::open_no_follow(path).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut text = String::new();
    file.take(MAX_FACT_BYTES).read_to_string(&mut text).ok()?;
    Some(text)
}

/// The `description:` of a fact's frontmatter, else the first line of its own text.
fn description(text: &str) -> String {
    let mut lines = text.lines();
    if text.starts_with("---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
            if let Some(about) = line.trim().strip_prefix("description:") {
                return about.trim().trim_matches('"').to_owned();
            }
        }
    }
    lines
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_facts_come_with_their_description_the_index_first_and_no_link_is_read() {
        let dir = std::env::temp_dir().join(format!("glut-tui-memory-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("german.md"),
            "---\nname: german\ndescription: always answer in German\n---\nThe owner reads German.",
        )
        .unwrap();
        std::fs::write(dir.join("MEMORY.md"), "- [german](german.md)\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a fact").unwrap();
        std::os::unix::fs::symlink(dir.join("german.md"), dir.join("linked.md")).unwrap();
        let facts = facts(&dir);
        assert_eq!(
            facts,
            [
                ("MEMORY".to_owned(), "- [german](german.md)".to_owned()),
                ("german".to_owned(), "always answer in German".to_owned()),
            ]
        );
        assert!(read(&dir.join("linked.md")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
