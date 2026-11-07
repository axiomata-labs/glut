//! A guide kept small (T42): a project's `AGENTS.md` (or `CLAUDE.md`) rides along on every
//! turn, so one that grew past the owner's `guide.warnAt` is pointed out — at the session's start and in the Meter
//! tab, in Glut's orange — with `/init` to shorten it. A guide the owner had `/init` check stays quiet until it grew
//! [`MARGIN_BYTES`] beyond the size it had then. The owner's own `~/.glut/AGENTS.md` is never counted.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How much a guide `/init` checked may grow before it is pointed out again: about 500 tokens on every turn.
pub const MARGIN_BYTES: u64 = 2 * 1024;
/// The bytes a token stands for, roughly, as `/context` counts the guides.
const BYTES_PER_TOKEN: u64 = 4;

/// A guide past the size it may have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Large {
    pub path: PathBuf,
    pub bytes: u64,
}

/// The guides of `paths` larger than `warn` bytes, as they are now — but those `/init` checked (`checked`: the size
/// each had then, by path) that grew less than [`MARGIN_BYTES`] since.
pub fn too_large(paths: &[PathBuf], warn: u64, checked: &BTreeMap<String, u64>) -> Vec<Large> {
    paths
        .iter()
        .filter_map(|path| {
            let meta = path.symlink_metadata().ok().filter(|meta| meta.is_file())?;
            let bytes = meta.len();
            let kept = checked
                .get(&path.display().to_string())
                .is_some_and(|then| bytes <= then.saturating_add(MARGIN_BYTES));
            (bytes > warn && !kept).then(|| Large {
                path: path.clone(),
                bytes,
            })
        })
        .collect()
}

/// What the owner is told of a guide too large, its path shown below `cwd` where it is. `/init` checks the nearest
/// guide (`nearest`): a farther one is shortened by an `/init` in its own folder.
pub fn note(large: &Large, cwd: &Path, nearest: Option<&Path>) -> String {
    let shown_path = |path: &Path| {
        path.strip_prefix(cwd).map_or_else(
            |_| path.display().to_string(),
            |rel| rel.display().to_string(),
        )
    };
    let shown = shown_path(&large.path);
    let how = if nearest == Some(large.path.as_path()) {
        "/init shortens it".to_owned()
    } else {
        let folder = large.path.parent().map(shown_path).unwrap_or_default();
        format!(
            "/init in {} shortens it",
            if folder.is_empty() { "." } else { &folder }
        )
    };
    format!(
        "{shown} has {} KB, about {} tokens on every turn — {how}",
        large.bytes.div_ceil(1024),
        large.bytes / BYTES_PER_TOKEN
    )
}

/// The guides of `paths` `checked` holds that are no longer too large: shortened since, they count afresh.
pub fn shrunk(paths: &[PathBuf], warn: u64, checked: &BTreeMap<String, u64>) -> Vec<String> {
    paths
        .iter()
        .map(|path| path.display().to_string())
        .filter(|key| checked.contains_key(key))
        .filter(|key| size(Path::new(key)).is_none_or(|bytes| bytes <= warn))
        .collect()
}

/// The size of the guide at `path` now, for `/init` to keep; `None` for none or no plain file.
pub fn size(path: &Path) -> Option<u64> {
    path.symlink_metadata()
        .ok()
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_guide_is_pointed_out_until_init_checked_it_and_again_once_it_grew() {
        let dir = std::env::temp_dir().join(format!("glut-tui-guides-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let guide = dir.join("AGENTS.md");
        std::fs::write(&guide, "x".repeat(9 * 1024)).unwrap();
        let small = dir.join("CLAUDE.md");
        std::fs::write(&small, "short").unwrap();
        let paths = vec![guide.clone(), small, dir.join("missing.md")];
        let warn = 8 * 1024;
        let mut checked = BTreeMap::new();
        let found = too_large(&paths, warn, &checked);
        assert_eq!(
            found,
            [Large {
                path: guide.clone(),
                bytes: 9 * 1024
            }]
        );
        assert_eq!(
            note(&found[0], &dir, Some(&guide)),
            "AGENTS.md has 9 KB, about 2304 tokens on every turn — /init shortens it"
        );
        // A guide farther up than the one /init checks is shortened from its own folder.
        let deeper = dir.join("sub");
        assert_eq!(
            note(&found[0], &deeper, Some(&deeper.join("AGENTS.md"))),
            format!(
                "{} has 9 KB, about 2304 tokens on every turn — /init in {} shortens it",
                guide.display(),
                dir.display()
            )
        );
        checked.insert(guide.display().to_string(), 9 * 1024);
        assert!(too_large(&paths, warn, &checked).is_empty());
        std::fs::write(&guide, "x".repeat(11 * 1024 + 1)).unwrap();
        assert_eq!(too_large(&paths, warn, &checked).len(), 1);
        // Shortened below the limit, the size kept is forgotten: it counts afresh when it grows again.
        assert!(shrunk(&paths, warn, &checked).is_empty());
        std::fs::write(&guide, "short now").unwrap();
        assert_eq!(
            shrunk(&paths, warn, &checked),
            [guide.display().to_string()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
