//! The `lsp` tool (T62): where a symbol is defined, where it is used, what it is, and renaming it — asked of the
//! language servers that report an edit's errors ([`crate::diagnostics`]), the same safe servers in the run's sandbox.
//! Named as Claude Code and Opencode name theirs, which models know: as `code`, beside `codemode`, it was never called.
//!
//! * **A symbol by line and name**: the model names the file, the line as `read_file` numbers it and the symbol as it
//!   stands there; Glut finds the column — models miscount columns, never the lines they read.
//! * **What it shows**: a place inside the working folder with its line (a definition with the lines after it), one
//!   outside (the standard library, a registry) by its path alone, and a file the rules hold back by its path alone —
//!   the server reads past the gate, the answer does not; a hover only where its definition may be read. Every path is
//!   judged where it really leads, as `read_file` judges it: a link inside the folder is no way around a rule.
//! * **A rename is a change of every file it touches**: judged as one (`Gate::judge_all`), all or nothing, each file
//!   written through the guard `edit_file` gets; a rename that would create, move or delete a file is not done.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};

use crate::model::ToolSpec;

/// The most files a rename shows as the editor's diffs (T91); past them only Glut's own card asks.
pub const RENAME_DIFFS: usize = 10;

pub(crate) const NAME: &str = "lsp";

/// Lines shown after a definition's own.
pub(crate) const DEFINITION_LINES_AFTER: usize = 3;
/// Characters of one shown line kept, as `read_file` keeps them: a minified file's single line would otherwise fill
/// the answer.
const MAX_LINE_CHARS: usize = 2_000;
/// Characters of the model's names an ask card shows.
const MAX_SHOWN_NAME: usize = 80;

pub(crate) fn spec() -> ToolSpec {
    ToolSpec {
        name: NAME.into(),
        description: "Ask the language server about a symbol: where it is defined, every place it is used, its type \
                      and docs, or rename it across the project. Name it by file, the line as read_file numbers it, \
                      and the symbol as it stands on that line. For definitions and uses prefer it to grep: it finds \
                      the symbol itself, not every word that looks like it. Where no server knows the file, it says \
                      so — then use grep."
            .into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "op": { "type": "string", "enum": ["definition", "references", "hover", "rename"] },
                "path": { "type": "string", "description": "The file, relative to the working folder." },
                "line": { "type": "integer", "description": "The line the symbol stands on, from 1." },
                "symbol": { "type": "string", "description": "The symbol's name as written on that line." },
                "occurrence": {
                    "type": "integer",
                    "description": "Which of several on the line, from 1; default 1."
                },
                "new_name": { "type": "string", "description": "rename only: the new name." }
            },
            "required": ["op", "path", "line", "symbol"]
        }),
    }
}

/// What a `code` call asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Op {
    Definition,
    References,
    Hover,
    Rename,
}

impl Op {
    /// The LSP request that answers it.
    pub(crate) fn method(self) -> &'static str {
        match self {
            Op::Definition => "textDocument/definition",
            Op::References => "textDocument/references",
            Op::Hover => "textDocument/hover",
            Op::Rename => "textDocument/rename",
        }
    }
}

/// A `code` call's arguments.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Args {
    pub op: Op,
    pub path: String,
    pub line: u32,
    pub symbol: String,
    #[serde(default)]
    pub occurrence: Option<u32>,
    #[serde(default)]
    pub new_name: Option<String>,
}

/// The arguments of a call, checked.
///
/// # Errors
///
/// Arguments that are no such object, a line or occurrence of 0, an empty symbol, a rename without a new name.
pub(crate) fn parse(arguments: &str) -> Result<Args, String> {
    let args: Args =
        serde_json::from_str(arguments).map_err(|err| format!("invalid arguments: {err}"))?;
    if args.line == 0 || args.occurrence == Some(0) {
        return Err("line and occurrence count from 1".into());
    }
    if args.symbol.trim().is_empty() {
        return Err("symbol is empty: name the symbol as it stands on the line".into());
    }
    if args.op == Op::Rename {
        match args.new_name.as_deref().map(str::trim) {
            None | Some("") => return Err("a rename needs new_name".into()),
            Some(name) if !fits_name(name) => {
                return Err(
                    "new_name must be a name: no spaces, quotes, brackets or separators".into(),
                );
            }
            Some(_) => {}
        }
    }
    Ok(args)
}

/// Whether `name` can be a symbol's new name: a server that does not check it (Markdown, CSS) would write anything.
fn fits_name(name: &str) -> bool {
    name.chars().count() <= 200
        && !name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "\"'`()[]{}<>;,/\\".contains(c))
}

/// A name of the model's as an ask card shows it: cut short.
pub(crate) fn short(name: &str) -> String {
    match name.char_indices().nth(MAX_SHOWN_NAME) {
        Some((at, _)) => format!("{}…", &name[..at]),
        None => name.to_owned(),
    }
}

/// `path` relative to `root` where it really is — links followed, `..` resolved —, `None` where that is outside
/// `root` or nowhere.
pub(crate) fn inside(root: &Path, path: &Path) -> Option<String> {
    let root = root.canonicalize().ok()?;
    let real = path.canonicalize().ok()?;
    real.strip_prefix(&root)
        .ok()
        .map(|rel| rel.to_string_lossy().into_owned())
}

/// The one name a `grep` pattern looks for — `Letter`, `\.send\(`, `fn send|async fn send` —, for the hint that
/// `lsp` finds it as a symbol (T62); `None` for a pattern of several names, of text, or of short words.
pub(crate) fn searched_name(pattern: &str) -> Option<String> {
    /// Words of a declaration around the name, not the name.
    const KEYWORDS: [&str; 14] = [
        "fn", "pub", "async", "struct", "enum", "trait", "impl", "type", "class", "def", "let",
        "const", "mut", "s",
    ];
    if pattern.contains(' ')
        && !KEYWORDS
            .iter()
            .any(|word| pattern.contains(&format!("{word} ")))
    {
        return None;
    }
    let mut names: Vec<&str> = pattern
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|word| !word.is_empty() && !KEYWORDS.contains(word))
        .collect();
    names.sort_unstable();
    names.dedup();
    match names.as_slice() {
        [name] if name.chars().count() >= 3 && !name.starts_with(|c: char| c.is_ascii_digit()) => {
            Some((*name).to_owned())
        }
        _ => None,
    }
}

/// The hint to a `grep` for `name` (T62): once a run, a line of Glut's own.
pub(crate) fn grep_hint(name: &str) -> String {
    format!(
        "[glut] For where `{name}` is defined and every place it is used, the lsp tool finds the symbol itself — \
         use it before grepping for a name."
    )
}

/// Whether a call's arguments ask for a rename — a change, so a snapshot comes first.
pub(crate) fn renames(arguments: &str) -> bool {
    parse(arguments).is_ok_and(|args| args.op == Op::Rename)
}

/// Where a question asks: the LSP position, and the 1-based line Glut took the symbol from where it was not on the
/// line named.
#[derive(Debug)]
pub(crate) struct Spot {
    pub position: Value,
    pub moved_to: Option<usize>,
}

/// The LSP position of the `occurrence`-th `symbol` on 1-based `line` of `text`: 0-based line, UTF-16 column. A
/// whole word is preferred to a part of one (`id` in `id + valid_id`). Not on that line, the whole word nearest to it
/// in the file is taken where `nearest` (a weak model guesses a line before it reads one) — never for a rename, which
/// must change the very symbol the model saw.
///
/// # Errors
///
/// A symbol not on the line (nor, with `nearest`, in the file), or not that often on it.
pub(crate) fn position(
    text: &str,
    line: u32,
    symbol: &str,
    occurrence: u32,
    nearest: bool,
) -> Result<Spot, String> {
    let symbol = symbol.trim();
    let index = usize::try_from(line - 1).unwrap_or(usize::MAX);
    let row = text.lines().nth(index);
    // A comment's word is no symbol: a line that has it only there counts as not having it.
    let on_line = row
        .is_some_and(|row| !whole_words(row, symbol).is_empty() && !(nearest && is_comment(row)));
    if !on_line && nearest {
        // Its declaration first, then the nearest place in code: a comment's word is no symbol to ask about.
        let places: Vec<(usize, &str, usize)> = text
            .lines()
            .enumerate()
            .filter(|(_, row)| !is_comment(row))
            .filter_map(|(at, row)| {
                whole_words(row, symbol)
                    .first()
                    .map(|column| (at, row, *column))
            })
            .collect();
        let found = places
            .iter()
            .find(|(_, row, column)| declares(&row[..*column]))
            .or_else(|| places.iter().min_by_key(|(at, _, _)| at.abs_diff(index)))
            .copied();
        if let Some((at, row, column)) = found {
            let column: usize = row[..column].chars().map(char::len_utf16).sum();
            return Ok(Spot {
                position: json!({ "line": at, "character": column }),
                moved_to: Some(at + 1),
            });
        }
        return Err(format!("`{symbol}` is nowhere in the file as a whole word"));
    }
    let Some(row) = row else {
        return Err(format!("the file has no line {line}"));
    };
    let all: Vec<usize> = row.match_indices(symbol).map(|(at, _)| at).collect();
    let word = |at: &usize| {
        let before = row[..*at].chars().next_back();
        let after = row[at + symbol.len()..].chars().next();
        !before.is_some_and(is_word) && !after.is_some_and(is_word)
    };
    let words: Vec<usize> = all.iter().copied().filter(word).collect();
    let pick = if words.is_empty() { &all } else { &words };
    let Some(&at) = pick.get(occurrence as usize - 1) else {
        return Err(match pick.len() {
            0 => format!("`{symbol}` is not on line {line}: {}", shown_line(row)),
            n => format!("line {line} has `{symbol}` {n} time(s), not {occurrence}"),
        });
    };
    let column: usize = row[..at].chars().map(char::len_utf16).sum();
    Ok(Spot {
        position: json!({ "line": index, "character": column }),
        moved_to: None,
    })
}

/// Whether `row` is a comment line, in the languages the servers know.
fn is_comment(row: &str) -> bool {
    let row = row.trim_start();
    ["//", "#", "/*", "*", "--", "<!--"]
        .iter()
        .any(|mark| row.starts_with(mark))
}

/// Whether what stands before a name declares it: `fn `, `pub struct `, `def `, `class `.
fn declares(before: &str) -> bool {
    /// The words that declare a name, in the languages the servers know.
    const DECLARING: [&str; 12] = [
        "fn",
        "struct",
        "enum",
        "trait",
        "type",
        "mod",
        "const",
        "static",
        "class",
        "def",
        "interface",
        "function",
    ];
    before
        .split_whitespace()
        .next_back()
        .is_some_and(|word| DECLARING.contains(&word))
}

/// Where `symbol` stands in `row` as a whole word, by byte.
fn whole_words(row: &str, symbol: &str) -> Vec<usize> {
    row.match_indices(symbol)
        .map(|(at, _)| at)
        .filter(|at| {
            let before = row[..*at].chars().next_back();
            let after = row[at + symbol.len()..].chars().next();
            !before.is_some_and(is_word) && !after.is_some_and(is_word)
        })
        .collect()
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The request's parameters.
pub(crate) fn params(op: Op, uri: &str, position: Value, new_name: Option<&str>) -> Value {
    let mut params = json!({ "textDocument": { "uri": uri }, "position": position });
    match op {
        Op::References => params["context"] = json!({ "includeDeclaration": true }),
        Op::Rename => params["newName"] = json!(new_name.unwrap_or_default().trim()),
        Op::Definition | Op::Hover => {}
    }
    params
}

/// A place an answer names: the file and its 0-based line.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Place {
    pub path: PathBuf,
    pub line: u32,
}

/// The places of a definition or references answer: a `Location`, a list of them, or `LocationLink`s.
pub(crate) fn places(answer: &Value) -> Vec<Place> {
    let one = |item: &Value| {
        let (uri, range) = match item.get("targetUri") {
            Some(uri) => (
                uri,
                item.get("targetSelectionRange")
                    .or_else(|| item.get("targetRange"))?,
            ),
            None => (item.get("uri")?, item.get("range")?),
        };
        Some(Place {
            path: crate::diagnostics::path_of(uri.as_str()?)?,
            line: u32::try_from(range["start"]["line"].as_u64()?).ok()?,
        })
    };
    match answer {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        Value::Null => Vec::new(),
        item => one(item).into_iter().collect(),
    }
}

/// The places as the model reads them, by file in the order the server named them first: inside `root` — where the
/// file really is — with the line (and `after` lines more) where `shown` lets it be read, else by its path and line
/// alone.
pub(crate) fn show_places(
    places: &[Place],
    root: &Path,
    after: usize,
    shown: &dyn Fn(&str) -> bool,
) -> String {
    let mut files: Vec<(&Path, Vec<u32>)> = Vec::new();
    for place in places {
        match files.iter_mut().find(|(path, _)| *path == place.path) {
            Some((_, lines)) => lines.push(place.line),
            None => files.push((&place.path, vec![place.line])),
        }
    }
    let mut out = Vec::new();
    for (path, mut lines) in files {
        lines.sort_unstable();
        lines.dedup();
        let inside = inside(root, path);
        let readable = inside.as_deref().is_some_and(shown);
        let text = if readable {
            crate::diagnostics::read_capped(path).ok()
        } else {
            None
        };
        let Some(text) = text else {
            let name = inside.unwrap_or_else(|| home_short(path));
            let numbers: Vec<String> = lines
                .iter()
                .map(|line| (u64::from(*line) + 1).to_string())
                .collect();
            out.push(format!("{name}:{}", numbers.join(",")));
            continue;
        };
        out.push(inside.unwrap_or_default());
        let rows: Vec<&str> = text.lines().collect();
        for line in lines {
            let first = line as usize;
            for at in first..=(first + after).min(rows.len().saturating_sub(1)) {
                if let Some(row) = rows.get(at) {
                    out.push(format!("  {}: {}", at + 1, shown_line(row)));
                }
            }
        }
    }
    out.join("\n")
}

/// A path outside the working folder as the owner writes it: the home folder as `~`.
fn home_short(path: &Path) -> String {
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if path.starts_with(&home) => {
            format!("~/{}", path.strip_prefix(&home).unwrap_or(path).display())
        }
        _ => path.display().to_string(),
    }
}

/// One line of a file as shown: its indentation dropped, cut to [`MAX_LINE_CHARS`].
fn shown_line(row: &str) -> String {
    let row = row.trim();
    match row.char_indices().nth(MAX_LINE_CHARS) {
        Some((at, _)) => format!("{}…", &row[..at]),
        None => row.to_owned(),
    }
}

/// A hover answer as plain text: its markup's fences dropped, its parts one after another.
pub(crate) fn hover_text(answer: &Value) -> String {
    fn part(value: &Value) -> String {
        match value {
            Value::String(text) => text.clone(),
            Value::Array(items) => items.iter().map(part).collect::<Vec<_>>().join("\n\n"),
            Value::Object(map) => map.get("value").map(part).unwrap_or_default(),
            _ => String::new(),
        }
    }
    let text = part(&answer["contents"]);
    text.lines()
        .filter(|line| !line.trim_start().starts_with("```"))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// One change of a file's text: a range in LSP's terms (0-based lines, UTF-16 columns) and what replaces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TextEdit {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub text: String,
}

/// The files a rename answer changes, with their edits, in the order of their paths.
///
/// # Errors
///
/// An answer that would create, move or delete a file, or that is no workspace edit at all.
pub(crate) fn workspace_edits(answer: &Value) -> Result<BTreeMap<PathBuf, Vec<TextEdit>>, String> {
    let mut files: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    let mut add = |uri: &Value, edits: &Value| -> Result<(), String> {
        let path = uri
            .as_str()
            .and_then(crate::diagnostics::path_of)
            .ok_or("the server named a file that is no local path")?;
        let list = edits.as_array().ok_or("the server's edits are no list")?;
        for edit in list {
            files
                .entry(path.clone())
                .or_default()
                .push(text_edit(edit)?);
        }
        Ok(())
    };
    if let Some(changes) = answer.get("documentChanges").and_then(Value::as_array) {
        for change in changes {
            if change.get("kind").is_some() {
                return Err(
                    "the rename would create, move or delete a file: not done; rename it by hand"
                        .into(),
                );
            }
            add(&change["textDocument"]["uri"], &change["edits"])?;
        }
    } else if let Some(changes) = answer.get("changes").and_then(Value::as_object) {
        for (uri, edits) in changes {
            add(&Value::String(uri.clone()), edits)?;
        }
    } else if !answer.is_null() {
        return Err("the server's answer is no workspace edit".into());
    }
    Ok(files)
}

fn text_edit(edit: &Value) -> Result<TextEdit, String> {
    let at = |point: &Value| -> Option<(u32, u32)> {
        Some((
            u32::try_from(point["line"].as_u64()?).ok()?,
            u32::try_from(point["character"].as_u64()?).ok()?,
        ))
    };
    let range = &edit["range"];
    Ok(TextEdit {
        start: at(&range["start"]).ok_or("an edit without its range")?,
        end: at(&range["end"]).ok_or("an edit without its range")?,
        text: edit["newText"]
            .as_str()
            .ok_or("an edit without its text")?
            .to_owned(),
    })
}

/// `text` with `edits` made, the last first so the earlier ranges still hold; with `old`, every piece replaced must be
/// that name — a server reckoning on a text changed since it read it would replace something else.
///
/// # Errors
///
/// A range outside the text, two that overlap, or a piece that is not `old`.
pub(crate) fn apply(text: &str, edits: &[TextEdit], old: Option<&str>) -> Result<String, String> {
    let mut spans: Vec<(usize, usize, &str)> = edits
        .iter()
        .map(|edit| {
            let start = offset(text, edit.start).ok_or("an edit outside the file")?;
            let end = offset(text, edit.end).ok_or("an edit outside the file")?;
            if end < start {
                return Err("an edit whose range runs backwards".to_owned());
            }
            if let Some(old) = old
                && end > start
                && text[start..end] != *old
            {
                return Err(format!(
                    "the server would replace `{}`, not `{old}`: the file changed since it read it — ask again",
                    short(&text[start..end])
                ));
            }
            Ok((start, end, edit.text.as_str()))
        })
        .collect::<Result<_, String>>()?;
    spans.sort_by_key(|span| std::cmp::Reverse(span.0));
    if spans.windows(2).any(|pair| pair[1].1 > pair[0].0) {
        return Err("the server's edits overlap".into());
    }
    let mut out = text.to_owned();
    for (start, end, replacement) in spans {
        out.replace_range(start..end, replacement);
    }
    Ok(out)
}

/// The byte offset of an LSP position in `text`; `None` past its end.
fn offset(text: &str, (line, column): (u32, u32)) -> Option<usize> {
    let mut start = 0;
    for _ in 0..line {
        start += text[start..].find('\n')? + 1;
    }
    let row_end = text[start..].find('\n').map_or(text.len(), |at| start + at);
    let mut units = 0;
    for (at, c) in text[start..row_end].char_indices() {
        if units >= column as usize {
            return Some(start + at);
        }
        units += c.len_utf16();
    }
    (units >= column as usize).then_some(row_end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symbol_is_found_by_its_line_and_name_as_a_whole_word_first() {
        let text = "fn main() {\n    let valid_id = id + id;\n}\n";
        assert_eq!(
            position(text, 2, "id", 1, false).unwrap().position,
            json!({ "line": 1, "character": 19 })
        );
        assert_eq!(
            position(text, 2, "id", 2, false).unwrap().position,
            json!({ "line": 1, "character": 24 })
        );
        assert!(
            position(text, 2, "id", 3, false)
                .unwrap_err()
                .contains("2 time(s)")
        );
        assert!(
            position(text, 2, "nothing", 1, false)
                .unwrap_err()
                .contains("not on line 2")
        );
        assert!(
            position(text, 9, "id", 1, false)
                .unwrap_err()
                .contains("no line 9")
        );
        // UTF-16 columns: an emoji before the symbol counts two.
        assert_eq!(
            position("// 🔥 x\n", 1, "x", 1, false).unwrap().position,
            json!({ "line": 0, "character": 6 })
        );
    }

    #[test]
    fn a_symbol_not_on_its_line_is_taken_from_the_nearest_line_except_for_a_rename() {
        let text = "//! Windows send letters.\nimpl Link {\n    let x = send;\n    fn send(&self) {}\n}\nfn other() { send() }\n";
        let spot = position(text, 1, "send", 1, true).unwrap();
        assert_eq!(
            spot.position,
            json!({ "line": 3, "character": 7 }),
            "the declaration, not the comment"
        );
        assert_eq!(spot.moved_to, Some(4));
        let plain = "a\nlet x = send;\nb\nc\nsend();\n";
        assert_eq!(position(plain, 5, "send", 1, true).unwrap().moved_to, None);
        assert_eq!(
            position(plain, 99, "send", 1, true).unwrap().moved_to,
            Some(5),
            "else the nearest"
        );
        assert!(
            position(text, 2, "send", 1, false).is_err(),
            "a rename takes no other line"
        );
        assert!(position(text, 2, "nowhere", 1, true).is_err());
        assert_eq!(position(text, 4, "send", 1, true).unwrap().moved_to, None);
    }

    #[test]
    fn arguments_are_checked() {
        assert!(parse(r#"{"op":"rename","path":"a.rs","line":1,"symbol":"x"}"#).is_err());
        assert!(parse(r#"{"op":"definition","path":"a.rs","line":0,"symbol":"x"}"#).is_err());
        assert!(parse(r#"{"op":"jump","path":"a.rs","line":1,"symbol":"x"}"#).is_err());
        assert!(renames(
            r#"{"op":"rename","path":"a.rs","line":1,"symbol":"x","new_name":"y"}"#
        ));
        assert!(!renames(
            r#"{"op":"hover","path":"a.rs","line":1,"symbol":"x"}"#
        ));
        let rename = params(
            Op::Rename,
            "file:///a.rs",
            json!({"line":0,"character":0}),
            Some(" y "),
        );
        assert_eq!(rename["newName"], "y");
        let references = params(
            Op::References,
            "file:///a.rs",
            json!({"line":0,"character":0}),
            None,
        );
        assert_eq!(references["context"]["includeDeclaration"], true);
    }

    #[test]
    fn places_show_inside_with_their_lines_and_outside_or_held_back_by_path_alone() {
        let root = std::env::temp_dir().join(format!("glut-code-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("src/a.rs"),
            "struct Letter;\n\nfn make() -> Letter {\n    Letter\n}\n",
        )
        .unwrap();
        std::fs::write(root.join("secret.rs"), "Letter\n").unwrap();
        let uri = |path: &Path| crate::diagnostics::uri_of(path);
        let answer = json!([
            { "uri": uri(&root.join("src/a.rs")), "range": { "start": { "line": 3, "character": 4 } } },
            { "uri": uri(&root.join("src/a.rs")), "range": { "start": { "line": 0, "character": 7 } } },
            { "uri": uri(&root.join("secret.rs")), "range": { "start": { "line": 0, "character": 0 } } },
            { "uri": "file:///elsewhere/lib.rs", "range": { "start": { "line": 9, "character": 0 } } }
        ]);
        let found = places(&answer);
        assert_eq!(found.len(), 4);
        let shown = show_places(&found, &root, 0, &|rel| rel != "secret.rs");
        assert_eq!(
            shown,
            "src/a.rs\n  1: struct Letter;\n  4: Letter\nsecret.rs:1\n/elsewhere/lib.rs:10"
        );
        // A definition shows the lines after it, and a link's target counts.
        let link = json!([{ "targetUri": uri(&root.join("src/a.rs")),
            "targetSelectionRange": { "start": { "line": 2, "character": 3 } } }]);
        let shown = show_places(&places(&link), &root, DEFINITION_LINES_AFTER, &|_| true);
        assert_eq!(
            shown,
            "src/a.rs\n  3: fn make() -> Letter {\n  4: Letter\n  5: }"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_name_is_a_name_and_an_ask_shows_names_short() {
        let rename = |name: &str| {
            parse(&format!(
                r#"{{"op":"rename","path":"a.rs","line":1,"symbol":"x","new_name":{name:?}}}"#
            ))
        };
        assert!(rename("a b").is_err());
        assert!(rename("x\"); y").is_err());
        assert!(rename("y_2").is_ok());
        assert_eq!(short(&"n".repeat(100)).chars().count(), 81);
    }

    #[test]
    fn a_path_is_inside_where_it_really_is() {
        let root = std::env::temp_dir().join(format!("glut-code-inside-{}", std::process::id()));
        let outside =
            std::env::temp_dir().join(format!("glut-code-outside-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("src/a.rs"), "").unwrap();
        std::fs::write(outside.join("b.rs"), "").unwrap();
        std::os::unix::fs::symlink(root.join("src/a.rs"), root.join("link.rs")).unwrap();
        std::os::unix::fs::symlink(outside.join("b.rs"), root.join("away.rs")).unwrap();
        assert_eq!(
            inside(&root, &root.join("src/a.rs")).as_deref(),
            Some("src/a.rs")
        );
        assert_eq!(
            inside(&root, &root.join("link.rs")).as_deref(),
            Some("src/a.rs"),
            "judged where it leads"
        );
        assert_eq!(
            inside(&root, &root.join("away.rs")),
            None,
            "a link out is outside"
        );
        assert_eq!(inside(&root, &root.join("src/../../x")), None);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn a_grep_for_one_name_is_told_of_lsp() {
        assert_eq!(searched_name("Letter").as_deref(), Some("Letter"));
        assert_eq!(searched_name("\\.send\\(").as_deref(), Some("send"));
        assert_eq!(
            searched_name("fn send|async fn send").as_deref(),
            Some("send")
        );
        assert_eq!(searched_name("Letter\\s*\\{").as_deref(), Some("Letter"));
        assert_eq!(
            searched_name("tell_window|send_window"),
            None,
            "several names"
        );
        assert_eq!(searched_name("TODO later"), None, "text");
        assert_eq!(searched_name("id"), None, "too short");
    }

    #[test]
    fn a_hover_is_plain_text() {
        let answer = json!({ "contents": { "kind": "markdown",
            "value": "```rust\npub struct Letter\n```\n\nA message between windows." } });
        assert_eq!(
            hover_text(&answer),
            "pub struct Letter\n\nA message between windows."
        );
        assert_eq!(
            hover_text(&json!({ "contents": ["a", { "language": "rust", "value": "b" }] })),
            "a\n\nb"
        );
    }

    #[test]
    fn a_rename_edits_documents_only_and_applies_from_the_end() {
        let answer = json!({ "documentChanges": [
            { "textDocument": { "uri": "file:///p/a.rs", "version": 1 }, "edits": [
                { "range": { "start": { "line": 0, "character": 7 }, "end": { "line": 0, "character": 13 } }, "newText": "Note" },
                { "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 10 } }, "newText": "Note" }
            ] }
        ] });
        let files = workspace_edits(&answer).unwrap();
        let edits = &files[Path::new("/p/a.rs")];
        assert_eq!(
            apply("struct Letter;\n    Letter\n", edits, Some("Letter")).unwrap(),
            "struct Note;\n    Note\n"
        );
        // A file changed since the server read it: the piece is no longer the old name, nothing is made.
        let stale = apply("struct Lettuce;\n    Letter\n", edits, Some("Letter"));
        assert!(stale.unwrap_err().contains("changed"));
        let moving = json!({ "documentChanges": [{ "kind": "rename", "oldUri": "file:///a", "newUri": "file:///b" }] });
        assert!(workspace_edits(&moving).unwrap_err().contains("not done"));
        let changes = json!({ "changes": { "file:///p/b.rs": [
            { "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } }, "newText": "y" }
        ] } });
        assert_eq!(workspace_edits(&changes).unwrap().len(), 1);
        // UTF-16 columns, overlapping and outside ranges.
        let after_emoji = TextEdit {
            start: (0, 3),
            end: (0, 4),
            text: "y".into(),
        };
        assert_eq!(apply("🔥 x", &[after_emoji], Some("x")).unwrap(), "🔥 y");
        let a = TextEdit {
            start: (0, 0),
            end: (0, 3),
            text: String::new(),
        };
        let b = TextEdit {
            start: (0, 2),
            end: (0, 4),
            text: String::new(),
        };
        assert!(apply("abcdef", &[a, b], None).is_err());
        let outside = TextEdit {
            start: (5, 0),
            end: (5, 1),
            text: String::new(),
        };
        assert!(apply("abc", &[outside], None).is_err());
    }
}
