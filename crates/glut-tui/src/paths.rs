//! Paths completed wherever one is typed (T68): the argument of a command whose catalog
//! entry takes `[path]`, and in a message a word that starts like a path (`~/`, `/`, `./`, `../`) — a bare word stays
//! `@`'s. The folder typed in is read while typing, its names only: no file is opened, nothing is kept beyond the list
//! on the screen, and Glut's `credentials.json` is never offered. The model sees none of it.

use std::path::{Path, PathBuf};

/// The most names a list shows; past it the list counts the rest.
pub const MOST: usize = 200;
/// The most entries of a folder read while typing: the screen must not wait for a huge one.
const MOST_READ: usize = 5000;
/// Glut's file of keys, never offered.
const SECRET: &str = "credentials.json";

/// What a path's place takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wants {
    /// Folders alone (`/projects`).
    Folders,
    /// Folders and files.
    Any,
}

/// A name the list offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// The names that fit the path being typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
    /// The part of the path's last name typed so far, which a pick replaces.
    pub start: String,
    pub entries: Vec<Entry>,
    /// How many more fit than are shown.
    pub more: usize,
    /// The folder read: a list of the same folder and start keeps the owner's place in it.
    pub folder: PathBuf,
    /// A list of a command's words and values instead of a folder's names (T71): the command, for its title.
    pub values: Option<String>,
}

impl Listing {
    /// What ⇥ adds without a pick: the names' common start beyond what is typed, where it is longer.
    pub fn common(&self) -> Option<String> {
        let first = &self.entries.first()?.name;
        let mut common: Vec<char> = first.chars().collect();
        for entry in &self.entries[1..] {
            let same = common
                .iter()
                .zip(entry.name.chars())
                .take_while(|(a, b)| **a == *b)
                .count();
            common.truncate(same);
        }
        let common: String = common.into_iter().collect();
        (common.chars().count() > self.start.chars().count()).then_some(common)
    }
}

/// The path being typed at the end of `text`, and what its place takes; `None`: no path is typed there. `fixed`
/// names a command's fixed words for `name` (`/export`'s `html`, `full`): a word that begins one is that word's.
pub fn typed(
    text: &str,
    takes_path: impl Fn(&str) -> Option<(Wants, Vec<&'static str>)>,
) -> Option<Typed<'_>> {
    if text.ends_with(char::is_whitespace) {
        return None;
    }
    // A command's path is the rest of its line after its fixed words, spaces and all (`/projects ~/My Docs`).
    if let Some(command) = text.strip_prefix('/')
        && let Some((name, rest)) = command.split_once(' ')
        && let Some((wants, fixed)) = takes_path(name)
    {
        let mut path = rest.trim_start();
        while let Some((word, after)) = path.split_once(' ')
            && fixed.contains(&word)
        {
            path = after.trim_start();
        }
        let is_fixed =
            !path.contains(['/', ' ']) && fixed.iter().any(|fixed| fixed.starts_with(path));
        return (!path.is_empty() && !is_fixed).then_some(Typed {
            word: path,
            wants,
            spaces: true,
        });
    }
    let word = text.split_whitespace().last()?;
    // A message (or a first word with a slash in it, which is one): only what starts like a path.
    let pathlike = ["~/", "/", "./", "../"]
        .iter()
        .any(|start| word.starts_with(start));
    // A lone command being typed (`/mod`) is the command list's.
    let command_typed =
        text.starts_with('/') && !text.contains(char::is_whitespace) && !text[1..].contains('/');
    (pathlike && !command_typed).then_some(Typed {
        word,
        wants: Wants::Any,
        spaces: false,
    })
}

/// The path being typed, as [`typed`] finds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typed<'a> {
    pub word: &'a str,
    pub wants: Wants,
    /// Names with spaces fit: a command's path is the rest of its line, a message's word ends at a space.
    pub spaces: bool,
}

/// The names in the folder `word` points into that fit its last part, folders first; `None` where that folder cannot
/// be read or nothing fits.
pub fn list(typed: Typed, home: &Path, cwd: &Path) -> Option<Listing> {
    let Typed {
        word,
        wants,
        spaces,
    } = typed;
    let (parent, start) = match word.rfind('/') {
        Some(at) => (&word[..=at], &word[at + 1..]),
        None => ("", word),
    };
    let folder = match parent.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if parent.starts_with('/') => PathBuf::from(parent),
        None => cwd.join(parent),
    };
    let secret = home.join(glut::providers::DIR).join(SECRET);
    let secret = secret.canonicalize().unwrap_or(secret);
    let lower = start.to_lowercase();
    let mut entries: Vec<Entry> = std::fs::read_dir(&folder)
        .ok()?
        .flatten()
        // A folder of a million files is read no further than this: the screen must not wait for it.
        .take(MOST_READ)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            // A name with control characters, or bidi and zero-width marks that make it look like another, is no name
            // to put into the input; one hidden waits for a typed `.`.
            if name
                .chars()
                .any(|c| c.is_control() || crate::transcript::hides(c))
                || (name.starts_with('.') && !start.starts_with('.'))
                || (!spaces && name.contains(char::is_whitespace))
            {
                return None;
            }
            if !name.to_lowercase().starts_with(&lower) {
                return None;
            }
            let path = entry.path();
            // Glut's keys under whatever name a link gives them: only a name like theirs, or a link, is looked at
            // closer.
            let link = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            if (link || name.eq_ignore_ascii_case(SECRET))
                && path.canonicalize().is_ok_and(|real| real == secret)
            {
                return None;
            }
            // The entry's own type costs nothing; only a link is followed to see what it is.
            let dir = match entry.file_type() {
                Ok(kind) if kind.is_symlink() => path.is_dir(),
                Ok(kind) => kind.is_dir(),
                Err(_) => false,
            };
            (dir || wants == Wants::Any).then_some(Entry { name, dir })
        })
        .collect();
    // A file typed whole has nothing left to complete.
    if let [only] = entries.as_slice()
        && !only.dir
        && only.name == start
    {
        return None;
    }
    if entries.is_empty() {
        return None;
    }
    entries.sort_by(|a, b| {
        b.dir
            .cmp(&a.dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    let more = entries.len().saturating_sub(MOST);
    entries.truncate(MOST);
    Some(Listing {
        start: start.to_owned(),
        entries,
        more,
        folder,
        values: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("glut-paths-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["src", "Scripts", ".git", "docs", ".glut"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in [
            "Cargo.toml",
            "src/main.rs",
            "src/menu.rs",
            ".glut/credentials.json",
            ".glut/settings.json",
        ] {
            std::fs::write(root.join(file), "").unwrap();
        }
        root
    }

    fn t(word: &str, wants: Wants) -> Typed<'_> {
        Typed {
            word,
            wants,
            spaces: false,
        }
    }

    fn names(listing: &Listing) -> Vec<&str> {
        listing
            .entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect()
    }

    fn takes(name: &str) -> Option<(Wants, Vec<&'static str>)> {
        match name {
            "projects" => Some((Wants::Folders, Vec::new())),
            "export" => Some((Wants::Any, vec!["md", "html", "jsonl", "copy", "full"])),
            _ => None,
        }
    }

    #[test]
    fn a_path_is_typed_after_a_command_that_takes_one_or_as_a_pathlike_word_in_a_message() {
        assert_eq!(
            typed("/projects Axi", takes).map(|t| (t.word, t.wants)),
            Some(("Axi", Wants::Folders))
        );
        assert_eq!(
            typed("/export html ~/no", takes).map(|t| (t.word, t.wants)),
            Some(("~/no", Wants::Any))
        );
        assert_eq!(
            typed("/export ht", takes).map(|t| (t.word, t.wants)),
            None,
            "the fixed word's"
        );
        assert_eq!(
            typed("/export full", takes).map(|t| (t.word, t.wants)),
            None
        );
        assert_eq!(typed("/model gpt", takes).map(|t| (t.word, t.wants)), None);
        assert_eq!(
            typed("look at ./sr", takes).map(|t| (t.word, t.wants)),
            Some(("./sr", Wants::Any))
        );
        assert_eq!(
            typed("look at src", takes).map(|t| (t.word, t.wants)),
            None,
            "a bare word is @'s"
        );
        assert_eq!(
            typed("/usr/bin is mis", takes).map(|t| (t.word, t.wants)),
            None
        );
        assert_eq!(
            typed("/usr/bi", takes).map(|t| (t.word, t.wants)),
            Some(("/usr/bi", Wants::Any))
        );
        assert_eq!(
            typed("/mod", takes).map(|t| (t.word, t.wants)),
            None,
            "the command list's"
        );
        assert_eq!(
            typed("see ~/ ", takes).map(|t| (t.word, t.wants)),
            None,
            "a space ended the word"
        );
        assert_eq!(
            typed("/projects ~/My Docs/Gl", takes).map(|t| (t.word, t.spaces)),
            Some(("~/My Docs/Gl", true)),
            "a command's path keeps its spaces"
        );
        assert_eq!(
            typed("/export html full ~/a b", takes).map(|t| t.word),
            Some("~/a b")
        );
        assert_eq!(
            typed("see ~/a b", takes).map(|t| t.spaces),
            None,
            "a message's word ends at a space"
        );
    }

    #[test]
    fn the_folder_is_listed_folders_first_hidden_ones_after_a_dot_and_never_the_credentials() {
        let root = tree("list");
        let all = list(t("./", Wants::Any), &root, &root).unwrap();
        assert_eq!(names(&all), ["docs", "Scripts", "src", "Cargo.toml"]);
        assert_eq!(
            names(&list(t("./s", Wants::Any), &root, &root).unwrap()),
            ["Scripts", "src"]
        );
        assert_eq!(
            names(&list(t("./", Wants::Folders), &root, &root).unwrap()),
            ["docs", "Scripts", "src"]
        );
        assert_eq!(
            names(&list(t("./.", Wants::Any), &root, &root).unwrap()),
            [".git", ".glut"]
        );
        let glut = list(t("~/.glut/", Wants::Any), &root, &root).unwrap();
        assert_eq!(
            names(&glut),
            ["settings.json"],
            "Glut's keys are never offered"
        );
        std::os::unix::fs::symlink(root.join(".glut/credentials.json"), root.join("docs/keys"))
            .unwrap();
        std::fs::write(root.join("docs/notes\u{202e}txt.sh"), "").unwrap();
        std::fs::write(root.join("docs/plan.md"), "").unwrap();
        assert_eq!(
            names(&list(t("docs/", Wants::Any), &root, &root).unwrap()),
            ["plan.md"],
            "no link to the keys, no name that hides what it is"
        );
        assert_eq!(
            list(t("./src/main.rs", Wants::Any), &root, &root),
            None,
            "typed whole"
        );
        assert_eq!(list(t("./nothing/", Wants::Any), &root, &root), None);
        let src = list(t("src/m", Wants::Any), &root, &root).unwrap();
        assert_eq!(
            src.common().as_deref(),
            None,
            "main and menu share only the m"
        );
        let menu = list(t("src/me", Wants::Any), &root, &root).unwrap();
        assert_eq!(menu.common().as_deref(), Some("menu.rs"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
