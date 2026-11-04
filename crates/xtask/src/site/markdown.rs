//! A Markdown file of the repository as a page: CommonMark with GitHub's tables, its heading anchors, and every
//! relative link turned into the page it names on the site, or into the file on GitHub.

use std::collections::HashMap;

use pulldown_cmark::{CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd, html};

use super::links::Linker;

/// A page rendered: its title (the first heading) and its body.
pub struct Rendered {
    /// The text of the first `#` heading, else the file's path.
    pub title: String,
    /// The page's HTML, without the layout.
    pub body: String,
}

/// The CommonMark extensions GitHub shows, so a file reads on the site as it does there.
fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS
}

/// `text` (the file `source`, a path from the repository's top) as the page `page`.
///
/// # Errors
///
/// A relative link that leaves the repository.
pub fn render(text: &str, source: &str, page: &str, linker: &Linker) -> Result<Rendered, String> {
    let slugs = heading_slugs(text);
    let mut at = 0;
    let mut title = None;
    let mut in_first = false;
    let mut title_text = String::new();
    let mut events = Vec::new();
    for event in Parser::new_ext(text, options()) {
        let event = match event {
            Event::Start(Tag::Heading {
                level,
                id: _,
                classes,
                attrs,
            }) => {
                // A heading of no letters or digits has no anchor, as on GitHub.
                let id = slugs.get(at).filter(|id| !id.is_empty()).cloned();
                at += 1;
                in_first = title.is_none() && level == HeadingLevel::H1;
                Event::Start(Tag::Heading {
                    level,
                    id: id.map(CowStr::from),
                    classes,
                    attrs,
                })
            }
            Event::End(TagEnd::Heading(level)) => {
                if in_first {
                    title = Some(std::mem::take(&mut title_text));
                    in_first = false;
                }
                Event::End(TagEnd::Heading(level))
            }
            Event::Text(ref piece) | Event::Code(ref piece) if in_first => {
                title_text.push_str(piece);
                event
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Link {
                link_type,
                dest_url: CowStr::from(linker.href(source, page, &dest_url)?),
                title,
                id,
            }),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => Event::Start(Tag::Image {
                link_type,
                dest_url: CowStr::from(linker.image(source, page, &dest_url)?),
                title,
                id,
            }),
            other => other,
        };
        events.push(event);
    }
    let mut body = String::new();
    html::push_html(&mut body, events.into_iter());
    Ok(Rendered {
        title: title.unwrap_or_else(|| source.to_owned()),
        body,
    })
}

/// Every heading's anchor in order, as GitHub makes them: a second heading of the same words gets `-1`, a third `-2`.
pub fn heading_slugs(text: &str) -> Vec<String> {
    let mut slugs = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut words: Option<String> = None;
    for event in Parser::new_ext(text, options()) {
        match event {
            Event::Start(Tag::Heading { .. }) => words = Some(String::new()),
            Event::Text(piece) | Event::Code(piece) => {
                if let Some(words) = words.as_mut() {
                    words.push_str(&piece);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                let base = slug(&words.take().unwrap_or_default());
                // The next free `base`, `base-1`, … — also free of an anchor made that way before.
                let mut count = seen.get(&base).copied().unwrap_or(0);
                let mut id = base.clone();
                while count > 0 || slugs.contains(&id) {
                    id = if count == 0 {
                        base.clone()
                    } else {
                        format!("{base}-{count}")
                    };
                    if !slugs.contains(&id) {
                        break;
                    }
                    count += 1;
                }
                seen.insert(base, count + 1);
                slugs.push(id);
            }
            _ => {}
        }
    }
    slugs
}

/// GitHub's anchor of a heading: lower case, letters, digits, `-` and `_` kept, spaces as `-`, the rest dropped.
pub fn slug(words: &str) -> String {
    words
        .trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            '-' | '_' => Some(c),
            c if c.is_alphanumeric() => Some(c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_are_githubs() {
        assert_eq!(
            slug("`mcp.json`, `.mcp.json` and `mcp-approved.json`"),
            "mcpjson-mcpjson-and-mcp-approvedjson"
        );
        assert_eq!(
            slug("Known limits and remaining risks"),
            "known-limits-and-remaining-risks"
        );
        assert_eq!(slug("Données — été"), "données--été");
        let text = "# A\n## Rules\n## Rules\n## `trusted.json`\n";
        assert_eq!(
            heading_slugs(text),
            ["a", "rules", "rules-1", "trustedjson"]
        );
    }
}
