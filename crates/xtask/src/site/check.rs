//! Whether every link of the built site leads somewhere: each `href` and `src` to a file of the site, each `#anchor`
//! to an `id` on its page. Links to other sites are counted, not fetched — a check that needs the network would fail
//! for reasons not the site's.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::links::{is_external, normalise};

/// What the check found.
pub struct Report {
    /// Links between the site's own files.
    pub internal: usize,
    /// Links elsewhere.
    pub external: usize,
    /// `page: link — why`, one per link that leads nowhere.
    pub broken: Vec<String>,
}

/// Checks the site built in `out`.
///
/// # Errors
///
/// A page that cannot be read.
pub fn check_built(out: &Path) -> Result<Report, String> {
    let files = all_files(out)?;
    let pages: Vec<String> = files
        .iter()
        .filter(|path| path.ends_with(".html"))
        .cloned()
        .collect();
    let files: HashSet<String> = files.into_iter().collect();
    let mut ids: HashMap<String, HashSet<String>> = HashMap::new();
    let mut texts = Vec::new();
    for path in &pages {
        let text = std::fs::read_to_string(out.join(path))
            .map_err(|err| format!("cannot read {path}: {err}"))?;
        ids.insert(path.clone(), attributes(&text, "id").into_iter().collect());
        texts.push((path.clone(), text));
    }
    let mut report = Report {
        internal: 0,
        external: 0,
        broken: Vec::new(),
    };
    for (page, text) in &texts {
        if !text.contains("<title>") || text.contains("<title></title>") {
            report.broken.push(format!("{page}: no title"));
        }
        for link in attributes(text, "href")
            .into_iter()
            .chain(attributes(text, "src"))
        {
            if is_external(&link) {
                report.external += 1;
                continue;
            }
            report.internal += 1;
            if let Some(why) = leads_nowhere(page, &link, &files, &ids) {
                report.broken.push(format!("{page}: {link} — {why}"));
            }
        }
    }
    Ok(report)
}

/// Why `link` on `page` leads nowhere, or `None` when it leads somewhere.
fn leads_nowhere(
    page: &str,
    link: &str,
    files: &HashSet<String>,
    ids: &HashMap<String, HashSet<String>>,
) -> Option<String> {
    let (path, anchor) = link.split_once('#').unwrap_or((link, ""));
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    let base = page.rsplit_once('/').map_or("", |(dir, _)| dir);
    let target = if path.is_empty() {
        page.to_owned()
    } else {
        let Some(mut target) = normalise(base, path) else {
            return Some("climbs out of the site".to_owned());
        };
        if target.is_empty() || target.ends_with('/') {
            target.push_str("index.html");
        }
        target
    };
    // Against the files the build wrote, not the disk: macOS finds `Docs/A.html` where a host will not.
    if !files.contains(&target) {
        return Some(format!("no file {target}"));
    }
    if anchor.is_empty() {
        return None;
    }
    match ids.get(&target) {
        Some(found) if found.contains(&decoded(anchor)) => None,
        Some(_) => Some(format!("no #{anchor} on {target}")),
        None => Some(format!("#{anchor} on {target}, which is not a page")),
    }
}

/// An anchor as the page's `id` holds it: `%xx` read back as the bytes they stand for.
fn decoded(anchor: &str) -> String {
    let bytes = anchor.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = bytes
            .get(at + 1..at + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok());
        match (
            bytes[at],
            hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()),
        ) {
            (b'%', Some(byte)) => {
                out.push(byte);
                at += 3;
            }
            (byte, _) => {
                out.push(byte);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Every value of the attribute `name` in `html`, written `name="…"` as the site's pages write them, the entities
/// the renderer writes read back.
fn attributes(html: &str, name: &str) -> Vec<String> {
    let needle = format!(" {name}=\"");
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find(&needle) {
        rest = &rest[at + needle.len()..];
        let Some(end) = rest.find('"') else { break };
        values.push(
            rest[..end]
                .replace("&quot;", "\"")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&amp;", "&"),
        );
        rest = &rest[end..];
    }
    values
}

/// Every file below `out`, as paths from it.
fn all_files(out: &Path) -> Result<Vec<String>, String> {
    let mut found = Vec::new();
    let mut stack: Vec<PathBuf> = vec![out.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|err| format!("cannot list {}: {err}", dir.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(inside) = path.strip_prefix(out) {
                found.push(inside.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_link_to_a_missing_page_or_anchor_is_found() {
        let out = std::env::temp_dir().join(format!("glut-site-check-{}", std::process::id()));
        std::fs::create_dir_all(out.join("docs")).unwrap();
        let page =
            |links: &str| format!("<html><title>t</title><h2 id=\"rules\">R</h2>{links}</html>");
        std::fs::write(
            out.join("index.html"),
            page("<a href=\"docs/a.html#rules\">a</a><a href=\"docs/b.html\">b</a><a href=\"https://x.org\">x</a>"),
        )
        .unwrap();
        std::fs::write(
            out.join("docs/a.html"),
            page(
                "<a href=\"../index.html#nope\">i</a><a href=\"#rules\">r</a><a href=\"../\">t</a>",
            ),
        )
        .unwrap();
        std::fs::write(out.join("docs/c.html"), page("<a href=\"A.html\">case</a>")).unwrap();
        let report = check_built(&out).unwrap();
        assert_eq!(report.external, 1);
        assert_eq!(report.internal, 6);
        assert_eq!(
            report.broken,
            [
                "docs/a.html: ../index.html#nope — no #nope on index.html",
                "docs/c.html: A.html — no file docs/A.html",
                "index.html: docs/b.html — no file docs/b.html"
            ]
        );
        std::fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn an_anchor_may_be_escaped() {
        assert_eq!(decoded("donn%C3%A9es"), "données");
        assert_eq!(decoded("a%2"), "a%2");
    }
}
