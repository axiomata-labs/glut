//! `read_file` with `outline: true` (Glut lean, N5): a file's declarations with their line numbers, so the model reads
//! the ranges it needs instead of a whole long file. One declaration reader per language the Studio knows — line based,
//! no parser, no language server started for it — that keeps a declaration's own line and leaves bodies out.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

/// A declaration line is shown up to this many characters.
const MAX_SHOWN_CHARS: usize = 160;

/// The languages the reader knows, by what a declaration line looks like.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Language {
    Rust,
    Script,
    Svelte,
    Python,
    Swift,
    C,
    Lua,
    Go,
}

fn language_of(path: &Path) -> Option<Language> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "rs" => Language::Rust,
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Language::Script,
        "svelte" => Language::Svelte,
        "py" | "pyi" => Language::Python,
        "swift" => Language::Swift,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" | "m" | "mm" => Language::C,
        "lua" => Language::Lua,
        "go" => Language::Go,
        _ => return None,
    })
}

static RUST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^\s*(pub(\([^)]*\))?\s+)?((async|const|unsafe|default|extern\s+\S+)\s+)*",
        r"(fn|struct|enum|trait|impl|mod|type|union|macro_rules!)[\s<{(]",
        r"|^(pub(\([^)]*\))?\s+)?(const|static)\s+[A-Z_][A-Z0-9_]*\s*:",
    ))
    .expect("valid regex")
});
static SCRIPT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^\s*(export\s+)?(default\s+)?(declare\s+)?(abstract\s+)?(async\s+)?",
        r"(function\*?|class|interface|type|enum|namespace|module)\s+[\w$]",
        r"|^(export\s+)?(const|let|var)\s+[\w${\[]",
        r"|^\s+((public|private|protected|static|readonly|async|override|get|set)\s+)*[\w$#]+\s*(<[^>]*>)?\(",
        r"[^)]*\)?\s*(:\s*[^={]+)?\{\s*$",
    ))
    .expect("valid regex")
});
static PYTHON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(async\s+)?def\s+\w+|^\s*class\s+\w+|^[A-Z_][A-Z0-9_]*\s*(:[^=]+)?=")
        .expect("valid regex")
});
static SWIFT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^\s*(@[\w()]+\s+)*((public|private|internal|fileprivate|open|final|static|class|override|mutating|",
        r"nonisolated|convenience|required|indirect|lazy|weak)\s+)*",
        r"(func|class|struct|enum|protocol|extension|actor|init|deinit|subscript|typealias|case)[\s<({?]",
        r"|^( {0,4})((public|private|internal|fileprivate|open|static|lazy)\s+)*(var|let)\s+\w+",
    ))
    .expect("valid regex")
});
static C: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(typedef\s+)?(struct|union|enum|class|namespace)\s+\w+[^;]*$|^#define\s+\w+",
        r"|^(template\s*<[^>]*>\s*)?(static\s+|inline\s+|extern\s+|virtual\s+|const\s+)*[A-Za-z_][\w:<>,\s\*&]*",
        r"[\s\*&]+\**[\w:~]+\s*\([^;]*$",
    ))
    .expect("valid regex")
});
static LUA: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(local\s+)?function\s+[\w.:]+|^\s*(local\s+)?[\w.]+\s*=\s*function\b")
        .expect("valid regex")
});
static GO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^func\s|^type\s+\w+|^(var|const)\s").expect("valid regex"));

/// A line that only opens a block or the like, which no reader takes for a declaration.
fn is_control(trimmed: &str) -> bool {
    const WORDS: [&str; 10] = [
        "if", "for", "while", "switch", "catch", "return", "else", "do", "try", "with",
    ];
    WORDS.iter().any(|word| {
        trimmed
            .strip_prefix(word)
            .is_some_and(|rest| rest.starts_with([' ', '(', '{']))
    })
}

fn declares(language: Language, line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//")
        || trimmed.starts_with('#') && language != Language::C
        || is_control(trimmed)
    {
        return false;
    }
    match language {
        Language::Rust => RUST.is_match(line),
        Language::Script | Language::Svelte => SCRIPT.is_match(line),
        Language::Python => PYTHON.is_match(line),
        Language::Swift => SWIFT.is_match(line),
        Language::C => C.is_match(line) && !trimmed.starts_with("return"),
        Language::Lua => LUA.is_match(line),
        Language::Go => GO.is_match(line),
    }
}

/// The outline of `text`, the file at `path`: its length and each declaration's line, numbered as `read_file` numbers.
///
/// # Errors
///
/// A text for the model when the file's language is none the reader knows.
pub(crate) fn of(path: &Path, text: &str) -> Result<String, String> {
    let language = language_of(path).ok_or_else(|| {
        format!(
            "no outline for {}: its language is none the outline knows; read it with offset and limit",
            path.display()
        )
    })?;
    let lines: Vec<&str> = text.lines().collect();
    let mut in_script = language != Language::Svelte;
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if language == Language::Svelte {
            let trimmed = line.trim_start();
            // A component's markup is no declaration: its script blocks are, and where script and style begin.
            if trimmed.starts_with("<script") || trimmed.starts_with("<style") {
                found.push((index + 1, line.trim_end().to_owned()));
                in_script = trimmed.starts_with("<script");
                continue;
            }
            if trimmed.starts_with("</script") {
                in_script = false;
                continue;
            }
        }
        if in_script && declares(language, line) {
            let shown = line.trim_end().trim_end_matches('{').trim_end();
            let shown: String = shown.chars().take(MAX_SHOWN_CHARS).collect();
            found.push((index + 1, shown));
        }
    }
    let mut out = format!(
        "{} — {} lines, {} declarations (read a range with offset and limit):\n",
        path.display(),
        lines.len(),
        found.len()
    );
    if found.is_empty() {
        out.push_str("(no declarations found; read it with offset and limit)\n");
    }
    for (number, line) in found {
        out.push_str(&format!("{number:>6}\t{line}\n"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outline(path: &str, text: &str) -> Vec<String> {
        of(Path::new(path), text)
            .unwrap()
            .lines()
            .skip(1)
            .map(|l| l.split('\t').nth(1).unwrap_or(l).to_owned())
            .collect()
    }

    #[test]
    fn rust_declarations_with_their_line_numbers_and_without_bodies() {
        let text = include_str!("outline.rs");
        let out = of(Path::new("src/tools/outline.rs"), text).unwrap();
        assert!(
            out.contains("pub(crate) fn of(path: &Path, text: &str) -> Result<String, String>"),
            "{out}"
        );
        assert!(out.contains("enum Language"));
        assert!(out.contains("const MAX_SHOWN_CHARS: usize = 160;"));
        assert!(!out.contains("let lines"), "bodies stay out: {out}");
        let line = text
            .lines()
            .position(|l| l.starts_with("fn language_of"))
            .unwrap()
            + 1;
        assert!(out.contains(&format!(
            "{line:>6}\tfn language_of(path: &Path) -> Option<Language>"
        )));
    }

    #[test]
    fn the_studios_other_languages() {
        let ts = "import x from 'y';\nexport class Store {\n  private items = [];\n  async load(id: string): Promise<void> {\n    if (id) {\n      return;\n    }\n  }\n}\nexport const LIMIT = 3;\nexport function add(a: number) {\n  const inner = 1;\n}\ninterface Shape {}\n";
        assert_eq!(
            outline("a.ts", ts),
            [
                "export class Store",
                "  async load(id: string): Promise<void>",
                "export const LIMIT = 3;",
                "export function add(a: number)",
                "interface Shape {}"
            ]
        );
        let py = "import os\nMAX = 3\nclass Store:\n    def load(self, id):\n        if id:\n            return\nasync def main():\n    pass\n";
        assert_eq!(
            outline("a.py", py),
            [
                "MAX = 3",
                "class Store:",
                "    def load(self, id):",
                "async def main():"
            ]
        );
        let swift = "import Foundation\npublic struct Calc {\n    let base: Int\n    public func add(_ a: Int) -> Int {\n        let x = 1\n        return x\n    }\n}\nextension Calc: Equatable {}\n";
        assert_eq!(
            outline("Calc.swift", swift),
            [
                "public struct Calc",
                "    let base: Int",
                "    public func add(_ a: Int) -> Int",
                "extension Calc: Equatable {}"
            ]
        );
        let c = "#include <stdio.h>\n#define MAX 3\nstruct point {\n  int x;\n};\nstatic int add(int a, int b)\n{\n  return a + b;\n}\nint main(void) {\n  if (x) {\n  }\n}\n";
        assert_eq!(
            outline("a.c", c),
            [
                "#define MAX 3",
                "struct point",
                "static int add(int a, int b)",
                "int main(void)"
            ]
        );
        let svelte = "<script lang=\"ts\">\n  let count = 0;\n  function inc() {\n    count += 1;\n  }\n</script>\n\n<button on:click={inc}>{count}</button>\n<style>\n  button { color: red; }\n</style>\n";
        assert_eq!(
            outline("A.svelte", svelte),
            ["<script lang=\"ts\">", "  function inc()", "<style>"]
        );
        let lua =
            "local M = {}\nfunction M.setup(opts)\nend\nlocal function helper()\nend\nreturn M\n";
        assert_eq!(
            outline("init.lua", lua),
            ["function M.setup(opts)", "local function helper()"]
        );
        let go = "package main\n\nimport \"fmt\"\n\ntype Point struct {\n\tX int\n}\n\nfunc main() {\n\tfmt.Println(1)\n}\n";
        assert_eq!(outline("main.go", go), ["type Point struct", "func main()"]);
    }

    #[test]
    fn a_long_files_outline_is_a_small_part_of_it() {
        let text = include_str!("../run.rs");
        let out = of(Path::new("src/run.rs"), text).unwrap();
        let (lines, outline) = (text.lines().count(), out.lines().count());
        assert!(outline * 10 < lines, "{outline} of {lines} lines");
        assert!(out.contains("pub async fn run"), "{out}");
        assert!(out.chars().count() * 10 < text.chars().count());
    }

    #[test]
    fn an_unknown_language_gets_a_clear_answer() {
        assert!(
            of(Path::new("notes.md"), "# x")
                .unwrap_err()
                .contains("offset and limit")
        );
        assert!(
            of(Path::new("a.rs"), "// only a comment\n")
                .unwrap()
                .contains("no declarations found")
        );
    }
}
