//! Where a link of a repository file leads on the site: to the page made from the file it names, or to the file on
//! GitHub when the site has no page of it.

use std::collections::HashMap;
use std::path::Path;

/// Where the repository is read by people without a checkout.
pub const GITHUB: &str = "https://github.com/axiomata-labs/glut";

/// The owner's account on X, where Glut's news are posted.
pub const X: &str = "https://x.com/GlutDev";

/// The manual's sources, one folder per language, in the repository.
pub const MANUAL_DIR: &str = "crates/glut-tui/manual";

/// The site's pages by the repository file they are made from.
pub struct Linker<'a> {
    /// Repository path (`docs/install.md`) → the page (`docs/install.html`).
    pages: HashMap<String, String>,
    /// The manual's languages the site has, each with its pages' folder (`manual`, `de/manual`).
    manuals: HashMap<String, String>,
    repo: &'a Path,
}

impl<'a> Linker<'a> {
    /// A linker over the documents' pages and the manual's folders, `repo` the repository's top.
    pub fn new(
        repo: &'a Path,
        pages: HashMap<String, String>,
        manuals: HashMap<String, String>,
    ) -> Self {
        Linker {
            pages,
            manuals,
            repo,
        }
    }

    /// The `href` of `dest` written in `source` (a repository path), seen from the page `page`.
    ///
    /// # Errors
    ///
    /// A relative path that climbs out of the repository.
    pub fn href(&self, source: &str, page: &str, dest: &str) -> Result<String, String> {
        if dest.is_empty() || dest.starts_with('#') || is_external(dest) {
            return Ok(dest.to_owned());
        }
        let (path, anchor) = match dest.split_once('#') {
            Some((path, anchor)) => (path, format!("#{anchor}")),
            None => (dest, String::new()),
        };
        let base = source.rsplit_once('/').map_or("", |(dir, _)| dir);
        let target = normalise(base, path)
            .ok_or_else(|| format!("{source}: the link {dest} leaves the repository"))?;
        if let Some(target_page) = self.pages.get(&target) {
            return Ok(format!("{}{anchor}", relative(page, target_page)));
        }
        if let Some(rest) = target.strip_prefix(&format!("{MANUAL_DIR}/")) {
            let (lang, file) = rest.split_once('/').unwrap_or((rest, ""));
            // The language's folder is the manual's index, a page its page; any other file is GitHub's.
            let name = if file.is_empty() {
                Some("index")
            } else {
                file.strip_suffix(".md")
            };
            if let (Some(folder), Some(name)) = (self.manuals.get(lang.trim_end_matches('/')), name)
            {
                return Ok(format!(
                    "{}{anchor}",
                    relative(page, &format!("{folder}/{name}.html"))
                ));
            }
        }
        let kind = if path.ends_with('/') || self.repo.join(&target).is_dir() {
            "tree"
        } else {
            "blob"
        };
        Ok(format!(
            "{GITHUB}/{kind}/main/{}{anchor}",
            target.trim_end_matches('/')
        ))
    }
}

impl Linker<'_> {
    /// The `src` of an image `dest` written in `source`: like [`Linker::href`], but a file of the repository is
    /// fetched from GitHub's raw files — its `blob` page is HTML, which an `<img>` cannot show.
    ///
    /// # Errors
    ///
    /// A relative path that climbs out of the repository.
    pub fn image(&self, source: &str, page: &str, dest: &str) -> Result<String, String> {
        let href = self.href(source, page, dest)?;
        Ok(match href.strip_prefix(&format!("{GITHUB}/blob/")) {
            Some(rest) => format!("{GITHUB}/raw/{rest}"),
            None => href,
        })
    }
}

/// Whether `dest` names a place of its own (a scheme, or `//host`), not a file of the repository.
pub fn is_external(dest: &str) -> bool {
    dest.starts_with("//")
        || dest.split_once(':').is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '+')
        })
}

/// `path` read from the folder `base`, both relative to the repository's top, without `.` and `..`; `None` above the
/// top. A trailing `/` stays.
pub fn normalise(base: &str, path: &str) -> Option<String> {
    let mut parts: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        base.split('/').filter(|part| !part.is_empty()).collect()
    };
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    let mut joined = parts.join("/");
    if path.ends_with('/') && !joined.is_empty() {
        joined.push('/');
    }
    Some(joined)
}

/// The link from the page `from` to the page `to`, both paths from the site's top.
pub fn relative(from: &str, to: &str) -> String {
    let from_dirs: Vec<&str> = from.split('/').collect();
    let from_dirs = &from_dirs[..from_dirs.len().saturating_sub(1)];
    let to_parts: Vec<&str> = to.split('/').collect();
    let common = from_dirs
        .iter()
        .zip(&to_parts[..to_parts.len().saturating_sub(1)])
        .take_while(|(a, b)| a == b)
        .count();
    let mut link = "../".repeat(from_dirs.len() - common);
    link.push_str(&to_parts[common..].join("/"));
    link
}

/// The way from the page `page` up to the site's top (`""`, `../`, `../../`).
pub fn root(page: &str) -> String {
    "../".repeat(page.matches('/').count())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linker(repo: &Path) -> Linker<'_> {
        let pages = HashMap::from([
            (
                "docs/security.md".to_owned(),
                "docs/security.html".to_owned(),
            ),
            ("README.md".to_owned(), "docs/index.html".to_owned()),
        ]);
        let manuals = HashMap::from([
            ("en".to_owned(), "manual".to_owned()),
            ("de".to_owned(), "de/manual".to_owned()),
        ]);
        Linker::new(repo, pages, manuals)
    }

    #[test]
    fn a_link_leads_to_its_page_or_to_github() {
        let repo = Path::new("/nowhere");
        let links = linker(repo);
        let href = |source: &str, page: &str, dest: &str| links.href(source, page, dest).unwrap();
        assert_eq!(
            href("docs/install.md", "docs/install.html", "security.md#rules"),
            "security.html#rules"
        );
        assert_eq!(
            href("README.md", "docs/index.html", "docs/security.md"),
            "security.html"
        );
        assert_eq!(
            href(
                "docs/install.md",
                "docs/install.html",
                "../crates/glut-tui/manual/en/"
            ),
            "../manual/index.html"
        );
        assert_eq!(
            href(
                "docs/a.md",
                "de/docs/a.html",
                "../crates/glut-tui/manual/de/arena.md"
            ),
            "../manual/arena.html"
        );
        assert_eq!(
            href("docs/a.md", "docs/a.html", "../AGENTS.md"),
            format!("{GITHUB}/blob/main/AGENTS.md")
        );
        assert_eq!(
            href("docs/a.md", "docs/a.html", "../crates/glut/"),
            format!("{GITHUB}/tree/main/crates/glut")
        );
        assert_eq!(
            href("docs/a.md", "docs/a.html", "https://x.org/a.md"),
            "https://x.org/a.md"
        );
        assert_eq!(href("docs/a.md", "docs/a.html", "#here"), "#here");
        assert!(
            links
                .href("docs/a.md", "docs/a.html", "../../outside.md")
                .is_err()
        );
        assert_eq!(
            href(
                "docs/a.md",
                "docs/a.html",
                "../crates/glut-tui/manual/en/notes.txt"
            ),
            format!("{GITHUB}/blob/main/crates/glut-tui/manual/en/notes.txt")
        );
        assert_eq!(
            links.image("docs/a.md", "docs/a.html", "shot.png").unwrap(),
            format!("{GITHUB}/raw/main/docs/shot.png")
        );
    }

    #[test]
    fn paths_between_pages() {
        assert_eq!(relative("index.html", "docs/a.html"), "docs/a.html");
        assert_eq!(relative("docs/a.html", "docs/b.html"), "b.html");
        assert_eq!(
            relative("de/manual/x.html", "manual/y.html"),
            "../../manual/y.html"
        );
        assert_eq!(relative("docs/a.html", "index.html"), "../index.html");
        assert_eq!(root("de/manual/x.html"), "../../");
        assert_eq!(root("index.html"), "");
        assert!(is_external("mailto:a@b.c") && is_external("https://x") && !is_external("a.md"));
    }
}
