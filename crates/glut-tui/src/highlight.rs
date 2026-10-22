//! Code blocks in colour (T7): tree-sitter's highlighter with the grammars of the owner's languages, built into the
//! binary — nothing is downloaded. A language Glut has no grammar for, or a block too large to be worth it, is shown
//! plain.

use std::sync::OnceLock;

use ratatui::style::{Modifier, Style};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::theme;

/// A block larger than this is shown plain: colour is not worth a frame's time on a dump, and a streaming answer is
/// laid out again on every frame it grows (about 60 ms for 32 KB of Rust).
const MAX_BYTES: usize = 32 * 1024;

/// The capture names Glut colours: a query's capture takes the one of these that matches most of its dotted parts
/// (`function.macro.rust` → `function.macro`).
const NAMES: [&str; 24] = [
    "attribute",
    "boolean",
    "comment",
    "constant.builtin",
    "constant",
    "constructor",
    "embedded",
    "escape",
    "function.builtin",
    "function.macro",
    "function.method",
    "function",
    "keyword",
    "label",
    "module",
    "number",
    "operator",
    "property",
    "punctuation",
    "string.special",
    "string",
    "tag",
    "type",
    "variable.builtin",
];

/// A run of text and its style.
pub type Run = (String, Style);

/// `code` as lines of coloured runs, for the language a fence names (`rust`, `ts`, `sh` …); `None` when Glut has no
/// grammar for it, the block is too large, or the grammar fails on it.
pub fn lines(code: &str, language: &str) -> Option<Vec<Vec<Run>>> {
    if code.len() > MAX_BYTES {
        return None;
    }
    let config = config(language)?;
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(config, code.as_bytes(), None, None, |_| None)
        .ok()?;
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut styles: Vec<Style> = Vec::new();
    for event in events {
        match event.ok()? {
            HighlightEvent::HighlightStart(highlight) => styles.push(style_of(NAMES[highlight.0])),
            HighlightEvent::HighlightEnd => {
                styles.pop();
            }
            HighlightEvent::Source { start, end } => {
                let style = styles.last().copied().unwrap_or_default();
                let text = code.get(start..end)?;
                for (at, piece) in text.split('\n').enumerate() {
                    if at > 0 {
                        lines.push(Vec::new());
                    }
                    if !piece.is_empty()
                        && let Some(line) = lines.last_mut()
                    {
                        line.push((piece.to_owned(), style));
                    }
                }
            }
        }
    }
    // A block ends with its last line's break: no empty line after it.
    if lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    Some(lines)
}

/// The colour of a capture in the theme's code colours, comments slanted.
fn style_of(name: &str) -> Style {
    let syntax = theme::c().syntax;
    let fg = |color| Style::default().fg(color);
    match name {
        "comment" => fg(syntax.comment).add_modifier(Modifier::ITALIC),
        "keyword" => fg(syntax.keyword),
        "string" | "string.special" | "escape" => fg(syntax.string),
        "function" | "function.method" | "function.builtin" | "function.macro" | "constructor" => {
            fg(syntax.function)
        }
        "type" | "module" => fg(syntax.kind),
        "attribute" => fg(syntax.attribute),
        "number" | "boolean" => fg(syntax.number),
        "constant" | "constant.builtin" => fg(syntax.constant),
        "property" | "label" => fg(syntax.property),
        "tag" => fg(syntax.tag),
        "operator" | "embedded" => fg(syntax.operator),
        "punctuation" | "variable.builtin" => fg(syntax.punctuation),
        _ => Style::default(),
    }
}

/// The highlighter's setup for a language a fence names, made once per language and kept for the process.
fn config(language: &str) -> Option<&'static HighlightConfiguration> {
    macro_rules! once {
        ($name:literal, $language:expr, $highlights:expr) => {{
            static MADE: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
            MADE.get_or_init(|| make($language.into(), $name, $highlights))
                .as_ref()
        }};
    }
    match language.trim().to_ascii_lowercase().as_str() {
        "rust" | "rs" => once!(
            "rust",
            tree_sitter_rust::LANGUAGE,
            tree_sitter_rust::HIGHLIGHTS_QUERY
        ),
        // TypeScript's query adds to JavaScript's, as its grammar does to JavaScript's.
        "typescript" | "ts" => once!(
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
            &format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY
            )
        ),
        "tsx" => once!(
            "tsx",
            tree_sitter_typescript::LANGUAGE_TSX,
            &format!(
                "{}\n{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
                tree_sitter_typescript::HIGHLIGHTS_QUERY
            )
        ),
        "javascript" | "js" | "jsx" | "mjs" | "cjs" => once!(
            "javascript",
            tree_sitter_javascript::LANGUAGE,
            &format!(
                "{}\n{}",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
                tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
            )
        ),
        "python" | "py" => once!(
            "python",
            tree_sitter_python::LANGUAGE,
            tree_sitter_python::HIGHLIGHTS_QUERY
        ),
        "swift" => once!(
            "swift",
            tree_sitter_swift::LANGUAGE,
            tree_sitter_swift::HIGHLIGHTS_QUERY
        ),
        "toml" => once!(
            "toml",
            tree_sitter_toml_ng::LANGUAGE,
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY
        ),
        "json" | "jsonc" | "json5" => once!(
            "json",
            tree_sitter_json::LANGUAGE,
            tree_sitter_json::HIGHLIGHTS_QUERY
        ),
        "sh" | "bash" | "zsh" | "shell" | "console" => {
            once!(
                "bash",
                tree_sitter_bash::LANGUAGE,
                tree_sitter_bash::HIGHLIGHT_QUERY
            )
        }
        "css" => once!(
            "css",
            tree_sitter_css::LANGUAGE,
            tree_sitter_css::HIGHLIGHTS_QUERY
        ),
        "html" | "htm" | "xml" => once!(
            "html",
            tree_sitter_html::LANGUAGE,
            tree_sitter_html::HIGHLIGHTS_QUERY
        ),
        "yaml" | "yml" => once!(
            "yaml",
            tree_sitter_yaml::LANGUAGE,
            tree_sitter_yaml::HIGHLIGHTS_QUERY
        ),
        // Svelte's query inherits HTML's: both, HTML's first.
        "svelte" => once!(
            "svelte",
            tree_sitter_svelte_ng::LANGUAGE,
            &format!(
                "{}\n{}",
                tree_sitter_html::HIGHLIGHTS_QUERY,
                tree_sitter_svelte_ng::HIGHLIGHTS_QUERY
            )
        ),
        _ => None,
    }
}

/// A language's setup; `None` when its query does not fit the grammar (a grammar update that broke it): plain text
/// then, never a failed answer.
fn make(
    language: tree_sitter::Language,
    name: &str,
    highlights: &str,
) -> Option<HighlightConfiguration> {
    let mut config = HighlightConfiguration::new(language, name, highlights, "", "").ok()?;
    config.configure(&NAMES);
    Some(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_grammar_loads_and_colours_its_code() {
        let samples = [
            ("rust", "fn main() { let x = 1; }"),
            ("ts", "const x: number = 1;"),
            ("tsx", "const a = <div>{x}</div>;"),
            ("js", "function f() { return 'a'; }"),
            ("python", "def f():\n    return 1"),
            ("swift", "let x = 1"),
            ("toml", "[a]\nb = 1"),
            ("json", "{\"a\": 1}"),
            ("bash", "echo \"hi\" | wc -l"),
            ("css", "a { color: red; }"),
            ("html", "<p class=\"a\">x</p>"),
            ("yaml", "a: 1"),
            ("svelte", "<script>let a = 1;</script>"),
        ];
        for (language, code) in samples {
            let lines =
                lines(code, language).unwrap_or_else(|| panic!("{language} has no grammar"));
            let text: String = lines
                .iter()
                .map(|line| {
                    line.iter()
                        .map(|(text, _)| text.as_str())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(text, code, "{language}: the text comes back whole");
            assert!(
                lines
                    .iter()
                    .flatten()
                    .any(|(_, style)| *style != Style::default()),
                "{language}: something is coloured"
            );
        }
        assert!(lines("x", "cobol").is_none());
        assert!(lines(&"a".repeat(MAX_BYTES + 1), "rust").is_none());
    }
}
