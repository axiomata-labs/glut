//! The website, built from what the repository already holds: the documents in `docs/` and beside the README, the
//! terminal UI's manual, and the hand-written pages, layout and files of `site/`. No framework: Markdown through
//! pulldown-cmark, one layout with a few `{{…}}` places, every link between pages relative, so the site works from
//! any folder of a host and from the disk.

mod check;
mod deploy;
mod glimm;
mod links;
mod manual;
mod markdown;
mod serve;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use check::check_built as check_folder;
pub use deploy::deploy;
pub use serve::serve;

use links::{Linker, MANUAL_DIR, relative, root};

/// The repository's documents the site shows, by their path from the top, each with its page.
const DOCUMENTS: [(&str, &str); 12] = [
    ("docs/install.md", "docs/install.html"),
    ("docs/getting-started.md", "docs/getting-started.html"),
    ("docs/configuration.md", "docs/configuration.html"),
    ("docs/security.md", "docs/security.html"),
    ("docs/architecture.md", "docs/architecture.html"),
    ("docs/library.md", "docs/library.html"),
    ("docs/editor-protocol.md", "docs/editor-protocol.html"),
    ("docs/comparison.md", "docs/comparison.html"),
    ("CHANGELOG.md", "docs/changelog.html"),
    ("CONTRIBUTING.md", "docs/contributing.html"),
    ("SECURITY.md", "docs/security-policy.html"),
    ("CODE_OF_CONDUCT.md", "docs/code-of-conduct.html"),
];

/// The site's languages: English at the top, the others in a folder of their own.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    De,
}

impl Lang {
    const ALL: [Lang; 2] = [Lang::En, Lang::De];

    fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// The folder its pages lie in, from the site's top (`""` or `de/`).
    fn folder(self) -> &'static str {
        match self {
            Lang::En => "",
            Lang::De => "de/",
        }
    }

    fn words(self) -> &'static Words {
        match self {
            Lang::En => &EN,
            Lang::De => &DE,
        }
    }
}

/// The few words of the site's own frame, per language.
struct Words {
    home: &'static str,
    docs: &'static str,
    manual: &'static str,
    manual_title: &'static str,
    manual_lead: &'static str,
    other_language: &'static str,
    source: &'static str,
    /// The manual's topics in the order of `commands.rs`'s `Topic`, as the terminal UI names them.
    topics: [&'static str; 8],
    /// The side navigation's name for a screen reader.
    side_label: &'static str,
    home_getting_started: &'static str,
    /// The footer's headings and links.
    project: &'static str,
    legal: &'static str,
    legal_notice: &'static str,
    privacy: &'static str,
    /// The footer's last line.
    fine: &'static str,
    /// The footer's links to documents and the skip link.
    install: &'static str,
    security: &'static str,
    changelog: &'static str,
    contributing: &'static str,
    security_policy: &'static str,
    skip: &'static str,
    /// The top navigation's name for a screen reader.
    site_label: &'static str,
    /// The home page's description for search engines and link previews.
    description: &'static str,
}

const EN: Words = Words {
    home: "Home",
    docs: "Docs",
    manual: "Manual",
    manual_title: "Manual",
    manual_lead: "Every command of Glut's terminal UI, as /help shows it.",
    other_language: "Deutsch",
    source: "Source of this page",
    topics: [
        "Session", "Model", "View", "Panes", "Changes", "Project", "Settings", "Help",
    ],
    side_label: "Documentation",
    home_getting_started: "Getting started",
    project: "Project",
    legal: "Legal",
    legal_notice: "Legal notice",
    privacy: "Privacy",
    fine: "MIT OR Apache-2.0 · no cookies, no tracking, no fonts from elsewhere",
    install: "Install",
    security: "Security model",
    changelog: "Changelog",
    contributing: "Contributing",
    security_policy: "Security policy",
    skip: "Skip to the content",
    site_label: "Site",
    description: "Glut, a coding agent for your terminal: every command in an OS sandbox, a gate you can read, any \
                  model you choose.",
};

const DE: Words = Words {
    home: "Start",
    // The documents are English alone for now.
    docs: "Doku (englisch)",
    manual: "Handbuch",
    manual_title: "Handbuch",
    manual_lead: "Jeder Befehl der Terminal-Oberfläche von Glut, so wie /help ihn zeigt.",
    other_language: "English",
    source: "Quelle dieser Seite",
    topics: [
        "Session",
        "Modell",
        "Ansicht",
        "Panes",
        "Änderungen",
        "Projekt",
        "Einstellungen",
        "Hilfe",
    ],
    side_label: "Dokumentation",
    home_getting_started: "Erste Schritte",
    project: "Projekt",
    legal: "Rechtliches",
    legal_notice: "Impressum",
    privacy: "Datenschutz",
    fine: "MIT OR Apache-2.0 · keine Cookies, kein Tracking, keine fremden Schriften",
    // The documents behind these links are English.
    install: "Installation (englisch)",
    security: "Sicherheitsmodell (englisch)",
    changelog: "Änderungen (englisch)",
    contributing: "Mitwirken (englisch)",
    security_policy: "Sicherheitslücken melden (englisch)",
    skip: "Zum Inhalt springen",
    site_label: "Website",
    description: "Glut, ein Coding-Agent für dein Terminal: jeder Befehl in einer Betriebssystem-Sandbox, ein Tor, \
                  das du lesen kannst, jedes Modell, das du willst.",
};

/// One page to write: where it goes, its title and body, and the page of the same content in the other language.
struct Page {
    path: String,
    lang: Lang,
    title: String,
    body: String,
    /// The same page in the other language, when there is one.
    other: Option<String>,
    /// The repository file it is made from, for the link to its source.
    source: Option<String>,
    /// The layout it goes into: `site/layout.html`, or `site/layout-<name>.html` for a hand-written page that names
    /// one in its first line (`<!-- layout: home -->`).
    layout: Option<String>,
}

/// Builds every page into `out` (emptied first); how many pages.
///
/// # Errors
///
/// A source that cannot be read, a link that leaves the repository, a page that cannot be written.
pub fn build(repo: &Path, out: &Path) -> Result<usize, String> {
    let layout = read(&repo.join("site/layout.html"))?;
    let commands = manual_order(repo)?;
    let topics = topics(repo)?;
    let mut pages = Vec::new();

    let documents: HashMap<String, String> = DOCUMENTS
        .iter()
        .map(|(source, page)| ((*source).to_owned(), (*page).to_owned()))
        .collect();
    let manuals: HashMap<String, String> = Lang::ALL
        .iter()
        .map(|lang| (lang.code().to_owned(), format!("{}manual", lang.folder())))
        .collect();
    let linker = Linker::new(repo, documents, manuals);

    // The documents are English; the German site links them as they are.
    for (source, path) in DOCUMENTS {
        let text = read(&repo.join(source))?;
        let rendered = markdown::render(&text, source, path, &linker)?;
        pages.push(Page {
            path: path.to_owned(),
            lang: Lang::En,
            title: rendered.title,
            body: rendered.body,
            other: None,
            source: Some(source.to_owned()),
            layout: None,
        });
    }

    for lang in Lang::ALL {
        let words = lang.words();
        let folder = format!("{}manual", lang.folder());
        let other = |name: &str| {
            Lang::ALL
                .into_iter()
                .find(|other| *other != lang)
                .map(|other| format!("{}manual/{name}.html", other.folder()))
        };
        let mut index = format!(
            "<h1 id=\"manual\">{}</h1>\n<p class=\"lead\">{}</p>\n",
            words.manual_title,
            manual::escape(words.manual_lead)
        );
        for (at, topic) in words.topics.iter().enumerate() {
            let named: Vec<&String> = commands
                .iter()
                .filter(|name| topics.get(name.as_str()) == Some(&at))
                .collect();
            if named.is_empty() {
                continue;
            }
            index.push_str(&format!(
                "<h2 id=\"{}\">{topic}</h2>\n<dl class=\"commands\">\n",
                markdown::slug(topic)
            ));
            for name in named {
                let source = format!("{MANUAL_DIR}/{}/{name}.md", lang.code());
                let text = read(&repo.join(&source))?;
                let lead = text.lines().next().unwrap_or_default();
                index.push_str(&format!(
                    "<dt><a href=\"{name}.html\"><code>/{name}</code></a></dt><dd>{}</dd>\n",
                    manual::escape(lead)
                ));
                let names: Vec<&str> = commands.iter().map(String::as_str).collect();
                pages.push(Page {
                    path: format!("{folder}/{name}.html"),
                    lang,
                    title: format!("/{name} · {}", words.manual),
                    body: format!(
                        "<h1 id=\"{name}\"><code>/{name}</code></h1>\n{}",
                        manual::render(name, &text, &names)
                    ),
                    other: other(name),
                    source: Some(source),
                    layout: None,
                });
            }
            index.push_str("</dl>\n");
        }
        pages.push(Page {
            path: format!("{folder}/index.html"),
            lang,
            title: words.manual_title.to_owned(),
            body: index,
            other: other("index"),
            source: None,
            layout: None,
        });

        // The hand-written pages of `site/pages/<lang>/`, each with its twin in the other language when it has one.
        let hand = repo.join("site/pages").join(lang.code());
        let version = manifest_version(&read(&repo.join("crates/glut-tui/Cargo.toml"))?)
            .ok_or("crates/glut-tui/Cargo.toml names no version")?;
        for name in files(&hand, "html")? {
            let path = format!("{}{name}", lang.folder());
            // A hand-written page links the site's files from its top through `{{root}}`, and names Glut's version
            // through `{{version}}` — the manifest's, so no release leaves an old number on the home page.
            let body = read(&hand.join(&name))?
                .replace("{{root}}", &root(&path))
                .replace("{{version}}", &version);
            let twin = Lang::ALL
                .into_iter()
                .find(|other| *other != lang)
                .filter(|other| {
                    repo.join("site/pages")
                        .join(other.code())
                        .join(&name)
                        .is_file()
                })
                .map(|other| format!("{}{name}", other.folder()));
            let layout = named_layout(&body)
                .map_err(|err| format!("site/pages/{}/{name}: {err}", lang.code()))?;
            pages.push(Page {
                layout,
                title: title_of(&body).unwrap_or_else(|| "Glut".to_owned()),
                path,
                lang,
                body,
                other: twin,
                source: None,
            });
        }
    }

    empty_output(out)?;
    for page in &pages {
        let html = match &page.layout {
            Some(name) => framed(
                &read(&repo.join(format!("site/layout-{name}.html")))?,
                page,
                &pages,
            ),
            None => framed(&layout, page, &pages),
        };
        write(&out.join(&page.path), &html)?;
    }
    copy_tree(&repo.join("site/static"), &out.join("static"))?;
    copy_recordings(&repo.join("site/recordings"), &out.join("recordings"))?;
    // #145: `curl -fsSL https://glut-ai.com/install.sh | sh` — the script itself; the binaries stay on GitHub.
    write(
        &out.join("install.sh"),
        &read(&repo.join("scripts/install.sh"))?,
    )?;
    // Glimm on the site is the terminal's Glimm: its frames and colours read from the program's own sources.
    write(&out.join("static/glimm.js"), &glimm::script(repo)?)?;
    write(&out.join("static/glimm.svg"), &glimm::svg(repo)?)?;
    Ok(pages.len())
}

/// The file the builder leaves in its output, so it only ever empties a folder it made itself.
const MARKER: &str = ".glut-site";

/// Makes `out` an empty folder holding [`MARKER`]: a missing or empty one is made so, one this tool built is
/// emptied, any other is refused — `--out .` or `--out docs` must never wipe the repository.
fn empty_output(out: &Path) -> Result<(), String> {
    let refuse = || {
        format!(
            "{} is not a folder this tool built (no {MARKER}): not emptied",
            out.display()
        )
    };
    if out.exists() {
        let empty = std::fs::read_dir(out)
            .map_err(|err| format!("cannot list {}: {err}", out.display()))?
            .next()
            .is_none();
        if !empty && !out.join(MARKER).is_file() {
            return Err(refuse());
        }
        std::fs::remove_dir_all(out)
            .map_err(|err| format!("cannot empty {}: {err}", out.display()))?;
    }
    write(
        &out.join(MARKER),
        "built by cargo run -p xtask -- site build; emptied on every build\n",
    )
}

/// Builds into `out`, then checks every link and anchor between its pages.
///
/// # Errors
///
/// What [`build`] refuses, and every link that leads nowhere, one per line.
pub fn check(repo: &Path, out: &Path) -> Result<(), String> {
    let count = build(repo, out)?;
    let report = check_folder(out)?;
    println!(
        "site: {count} pages, {} links between them and {} to elsewhere (not fetched)",
        report.internal, report.external
    );
    if report.broken.is_empty() {
        return Ok(());
    }
    Err(format!(
        "{} links lead nowhere:\n{}",
        report.broken.len(),
        report.broken.join("\n")
    ))
}

/// `page` in the layout: its words, the navigation and the way back to the site's top filled in.
fn framed(layout: &str, page: &Page, pages: &[Page]) -> String {
    let words = page.lang.words();
    let home = format!("{}index.html", page.lang.folder());
    let manual = format!("{}manual/index.html", page.lang.folder());
    let docs: Vec<&Page> = pages
        .iter()
        .filter(|other| other.path.starts_with("docs/"))
        .collect();
    let mut nav = format!(
        "<a href=\"{}\">{}</a>\n<a href=\"{}\">{}</a>\n<details{}><summary>{}</summary>\n",
        relative(&page.path, &home),
        words.home,
        relative(&page.path, &manual),
        words.manual,
        if page.path.starts_with("docs/") {
            " open"
        } else {
            ""
        },
        words.docs
    );
    for doc in docs {
        nav.push_str(&format!(
            "<a href=\"{}\"{}>{}</a>\n",
            relative(&page.path, &doc.path),
            if doc.path == page.path {
                " aria-current=\"page\""
            } else {
                ""
            },
            manual::escape(&doc.title)
        ));
    }
    nav.push_str("</details>\n");
    // A page with no twin offers the other language's home; an English document read from the German site says so.
    let other_lang = Lang::ALL
        .into_iter()
        .find(|lang| *lang != page.lang)
        .unwrap_or(Lang::En);
    let switch_to = page
        .other
        .clone()
        .unwrap_or_else(|| format!("{}index.html", other_lang.folder()));
    let switch = format!(
        "<a href=\"{}\" hreflang=\"{}\" lang=\"{}\">{}</a>",
        relative(&page.path, &switch_to),
        other_lang.code(),
        other_lang.code(),
        words.other_language
    );
    let source = page.source.as_ref().map_or_else(String::new, |source| {
        format!(
            "<a href=\"{}/blob/main/{source}\">{}</a>",
            links::GITHUB,
            words.source
        )
    });
    let link = |to: &str| relative(&page.path, to);
    let folder = page.lang.folder();
    let top = format!(
        "<header class=\"top\">\n<a class=\"brand\" href=\"{}\">Glut</a>\n<nav class=\"links\" aria-label=\"{}\">\
         <a href=\"{}\">{}</a><a href=\"{}\">{}</a><a class=\"wide\" href=\"{}\">GitHub</a>\
         <span class=\"lang\">{switch}</span></nav>\n</header>",
        link(&home),
        words.site_label,
        link("docs/getting-started.html"),
        words.docs,
        link(&manual),
        words.manual,
        links::GITHUB,
    );
    let foot = format!(
        "<footer class=\"foot\">\n<div><h2>{}</h2><a href=\"{}\">{}</a><a href=\"{}\">{}</a>\
         <a href=\"{}\">{}</a><a href=\"{}\">{}</a></div>\n<div><h2>{}</h2><a href=\"{}\">GitHub</a>\
         <a href=\"{}\">X</a><a href=\"{}\">{}</a><a href=\"{}\">{}</a><a href=\"{}\">{}</a></div>\n\
         <div><h2>{}</h2><a href=\"{}\">{}</a><a href=\"{}\">{}</a></div>\n<p class=\"fine\">{}</p>\n</footer>",
        words.docs,
        link("docs/getting-started.html"),
        words.home_getting_started,
        link("docs/install.html"),
        words.install,
        link(&manual),
        words.manual,
        link("docs/security.html"),
        words.security,
        words.project,
        links::GITHUB,
        links::X,
        link("docs/changelog.html"),
        words.changelog,
        link("docs/contributing.html"),
        words.contributing,
        link("docs/security-policy.html"),
        words.security_policy,
        words.legal,
        link(&format!("{folder}legal.html")),
        words.legal_notice,
        link(&format!("{folder}privacy.html")),
        words.privacy,
        words.fine,
    );
    // The content goes in last, so the words `{{root}}` or `{{nav}}` written in a document stay as written.
    layout
        .replace("{{lang}}", page.lang.code())
        .replace("{{title}}", &manual::escape(&page.title))
        .replace("{{nav}}", &nav)
        .replace("{{switch}}", &switch)
        .replace("{{source}}", &source)
        .replace("{{top}}", &top)
        .replace("{{foot}}", &foot)
        .replace("{{side_label}}", words.side_label)
        .replace("{{skip}}", words.skip)
        .replace("{{description}}", words.description)
        .replace("{{root}}", &root(&page.path))
        .replace("{{content}}", &page.body)
}

/// The manual's pages in the order the terminal UI lists them (`manual/en.rs`).
fn manual_order(repo: &Path) -> Result<Vec<String>, String> {
    let text = read(&repo.join("crates/glut-tui/src/manual/en.rs"))?;
    let names: Vec<String> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("(\""))
        .filter_map(|rest| rest.split_once('"').map(|(name, _)| name.to_owned()))
        .collect();
    if names.is_empty() {
        return Err("no manual pages named in crates/glut-tui/src/manual/en.rs".to_owned());
    }
    Ok(names)
}

/// Each command's topic, by the order of [`Words::topics`], as `commands.rs` files it — the topics of keys alone
/// (`Input`, `Editing`) have no commands.
fn topics(repo: &Path) -> Result<HashMap<String, usize>, String> {
    const ORDER: [&str; 8] = [
        "Session", "Model", "View", "Panes", "Changes", "Project", "Settings", "Help",
    ];
    let text = read(&repo.join("crates/glut-tui/src/commands.rs"))?;
    let start = text
        .find("pub const COMMANDS")
        .ok_or("no COMMANDS in crates/glut-tui/src/commands.rs")?;
    let mut found = HashMap::new();
    let mut rest = &text[start..];
    while let Some(at) = rest.find("Topic::") {
        rest = &rest[at + "Topic::".len()..];
        let topic: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect();
        let Some(quote) = rest.find('"') else { break };
        let name: String = rest[quote + 1..]
            .chars()
            .take_while(|c| *c != '"')
            .collect();
        if let Some(at) = ORDER.iter().position(|known| *known == topic) {
            found.entry(name).or_insert(at);
        }
    }
    Ok(found)
}

/// The layout a hand-written page names in its first line, `<!-- layout: name -->` (a plain word); `None` for a page
/// that names none.
///
/// # Errors
///
/// A first line that tries to name one and fails: the page would go out in the wrong frame unnoticed.
fn named_layout(body: &str) -> Result<Option<String>, String> {
    let first = body.trim_start().lines().next().unwrap_or_default().trim();
    if !first.starts_with("<!--") || !first.contains("layout") {
        return Ok(None);
    }
    let name = first
        .strip_prefix("<!--")
        .and_then(|rest| rest.strip_suffix("-->"))
        .and_then(|rest| rest.trim().strip_prefix("layout:"))
        .map(str::trim)
        .filter(|name| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        });
    match name {
        Some(name) => Ok(Some(name.to_owned())),
        None => Err(format!("a layout line that names no layout: {first}")),
    }
}

/// The text of a hand-written page's first `<h1>`, without its tags.
fn title_of(body: &str) -> Option<String> {
    let start = body.find("<h1")?;
    let open = body[start..].find('>')? + start + 1;
    let close = body[open..].find("</h1>")? + open;
    let mut text = String::new();
    let mut in_tag = false;
    // A line break between words parts them; any other tag (`<em>`, `<code>`) stands inside a word's run.
    let inner = body[open..close]
        .replace("<br>", " ")
        .replace("<br/>", " ")
        .replace("<br />", " ");
    for c in inner.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    // The title is escaped again when it is framed: what the page wrote as an entity is read back first.
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

/// The files of `dir` ending in `.{extension}`, sorted; none when the folder is missing.
fn files(dir: &Path, extension: &str) -> Result<Vec<String>, String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|err| format!("cannot list {}: {err}", dir.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(&format!(".{extension}")) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
}

/// The recordings the owner kept (`site/recordings/*.json`) — their scripts stay in the repository.
fn copy_recordings(from: &Path, to: &Path) -> Result<(), String> {
    for name in files(from, "json")? {
        std::fs::create_dir_all(to)
            .map_err(|err| format!("cannot make {}: {err}", to.display()))?;
        std::fs::copy(from.join(&name), to.join(&name))
            .map_err(|err| format!("cannot copy {name}: {err}"))?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(from) else {
        return Ok(());
    };
    std::fs::create_dir_all(to).map_err(|err| format!("cannot make {}: {err}", to.display()))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("cannot list {}: {err}", from.display()))?;
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if source.is_dir() {
            copy_tree(&source, &target)?;
        } else {
            std::fs::copy(&source, &target)
                .map_err(|err| format!("cannot copy {}: {err}", source.display()))?;
        }
    }
    Ok(())
}

/// The version a manifest's package carries: its first `version = "…"` line.
pub(crate) fn manifest_version(manifest: &str) -> Option<String> {
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|err| format!("cannot read {}: {err}", path.display()))
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|err| format!("cannot make {}: {err}", dir.display()))?;
    }
    std::fs::write(path, text).map_err(|err| format!("cannot write {}: {err}", path.display()))
}

/// What the pages are made from, for [`serve`] to see a change: the newest time any source was written, and how many
/// there are (a file removed or moved in leaves the newest time as it was).
fn sources_state(repo: &Path) -> (Option<std::time::SystemTime>, usize) {
    let mut newest = None;
    let mut count = 0;
    let mut stack: Vec<PathBuf> = vec![
        repo.join("site"),
        repo.join("docs"),
        repo.join(MANUAL_DIR),
        repo.join("crates/glut-tui/src/manual/en.rs"),
        repo.join("crates/glut-tui/src/commands.rs"),
    ];
    stack.extend(DOCUMENTS.iter().map(|(source, _)| repo.join(source)));
    while let Some(path) = stack.pop() {
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                stack.extend(entries.flatten().map(|entry| entry.path()));
            }
        } else {
            count += 1;
            newest = newest.max(meta.modified().ok());
        }
    }
    (newest, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifests_version_is_its_packages() {
        let manifest = "[package]\nname = \"glut-tui\"\nversion = \"0.1.2\"\n\n[dependencies]\n\
                        glut = { version = \"0.1.0\", path = \"../glut\" }\n";
        assert_eq!(manifest_version(manifest).as_deref(), Some("0.1.2"));
        assert_eq!(manifest_version("[package]\nname = \"x\"\n"), None);
    }

    fn repo() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn every_command_has_a_topic_and_a_page() {
        let repo = repo();
        let commands = manual_order(&repo).unwrap();
        let topics = topics(&repo).unwrap();
        assert!(commands.len() > 60, "{}", commands.len());
        for name in &commands {
            assert!(topics.contains_key(name), "/{name} has no topic");
        }
    }

    #[test]
    fn a_page_may_name_its_layout() {
        assert_eq!(
            named_layout("<!-- layout: home -->\n<h1>x</h1>"),
            Ok(Some("home".to_owned()))
        );
        assert_eq!(
            named_layout("\n<!--layout:home-->\n"),
            Ok(Some("home".to_owned()))
        );
        assert_eq!(named_layout("<h1>x</h1>"), Ok(None));
        assert_eq!(named_layout("<!-- a draft -->\n<h1>x</h1>"), Ok(None));
        assert!(named_layout("<!-- layout: ../x -->").is_err());
    }

    #[test]
    fn a_hand_written_pages_title_is_its_heading() {
        assert_eq!(
            title_of("<main><h1 class=\"x\">Glut <em>now</em></h1></main>").as_deref(),
            Some("Glut now")
        );
        assert_eq!(
            title_of("<h1>Foo <code>bar</code>, baz<br>qux</h1>").as_deref(),
            Some("Foo bar, baz qux")
        );
        assert_eq!(title_of("<p>none</p>"), None);
    }

    #[test]
    fn only_a_folder_the_tool_built_is_emptied() {
        let out = std::env::temp_dir().join(format!("glut-site-out-{}", std::process::id()));
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("precious.md"), "x").unwrap();
        assert!(empty_output(&out).is_err());
        assert!(out.join("precious.md").is_file(), "nothing was removed");
        std::fs::remove_file(out.join("precious.md")).unwrap();
        empty_output(&out).unwrap();
        std::fs::write(out.join("old.html"), "x").unwrap();
        empty_output(&out).unwrap();
        assert!(!out.join("old.html").exists() && out.join(MARKER).is_file());
        std::fs::remove_dir_all(&out).unwrap();
    }

    #[test]
    fn the_site_builds_and_every_link_leads_somewhere() {
        let out = std::env::temp_dir().join(format!("glut-site-{}", std::process::id()));
        let repo = repo();
        let count = build(&repo, &out).unwrap();
        assert!(count > 140, "{count} pages");
        let report = check_folder(&out).unwrap();
        assert!(report.broken.is_empty(), "{}", report.broken.join("\n"));
        std::fs::remove_dir_all(&out).unwrap();
    }
}
