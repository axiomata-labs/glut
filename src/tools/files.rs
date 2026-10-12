//! The file tools: read, write, edit, list, search by content and by name — all inside the run's folder.
//!
//! Every path goes through `axiomata-files`' [`Root`] with [`LinkPolicy::Contained`], the guard the editor and the
//! Studio already use: a path that climbs out (`..`), is absolute, or follows a link to a place outside the folder is
//! refused there, not here. The file system work is blocking, so each call runs on tokio's blocking pool.

use std::sync::atomic::AtomicBool;

use axiomata_files::{
    EntryKind, FilesError, LinkPolicy, MAX_READ_BYTES, MAX_WRITE_BYTES, Root, SearchQuery,
    index_files, list_dir, make_dir, read_text, search, write_text,
};
use globset::Glob;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{StopFlag, Tool, ToolFuture};
use crate::error::HarnessError;
use crate::model::ToolSpec;

/// Lines `read_file` gives out when the model does not say how many.
const DEFAULT_READ_LINES: usize = 2_000;
/// Characters of one line `read_file` keeps: a minified file's single line would otherwise fill the whole result.
const MAX_LINE_CHARS: usize = 2_000;
/// Entries `list_dir` names.
const MAX_LIST_ENTRIES: usize = 500;
/// Matching lines `grep` reports.
const MAX_GREP_LINES: usize = 200;
/// Paths `glob` reports.
const MAX_GLOB_PATHS: usize = 500;

/// Folders a run neither reads nor lists: a version control's own store. `.git/config` can hold credentials in a
/// remote's URL, and nothing in there is the task's.
const PRIVATE_DIRS: [&str; 3] = [".git", ".hg", ".jj"];

/// What a run never writes: the version control's store and the configuration a later program obeys. A line written
/// into `.git/config` (`core.fsmonitor`, an alias with `!`) runs as the owner at their next `git status`; a role, a
/// task file or an agent's settings would be obeyed by the next session. Security audit of HS1, 2026-10-13.
const PROTECTED_DIRS: [&str; 7] = [
    ".git",
    ".hg",
    ".jj",
    ".axiomata",
    ".claude",
    ".opencode",
    ".vscode",
];
const PROTECTED_FILES: [&str; 2] = [".mcp.json", "opencode.json"];

/// Refuses a path whose folders include one of `dirs`, or whose name is one of `files`.
fn guard(path: &str, dirs: &[&str], files: &[&str]) -> Result<(), String> {
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.iter().any(|part| dirs.contains(part))
        || parts.last().is_some_and(|name| files.contains(name))
    {
        return Err(format!("{path} is protected: the run may not touch it"));
    }
    Ok(())
}

/// The six file tools, confined to `cwd`.
///
/// # Errors
///
/// [`HarnessError::Workspace`] if `cwd` is not a folder that can be used as a root.
pub fn file_tools(cwd: &std::path::Path) -> Result<Vec<Box<dyn Tool>>, HarnessError> {
    let root = Root::dir(cwd, LinkPolicy::Contained).map_err(|err| HarnessError::Workspace {
        path: cwd.display().to_string(),
        reason: err.to_string(),
    })?;
    Ok(vec![
        Box::new(FileTool::new(&root, Kind::Read)),
        Box::new(FileTool::new(&root, Kind::Write)),
        Box::new(FileTool::new(&root, Kind::Edit)),
        Box::new(FileTool::new(&root, Kind::List)),
        Box::new(FileTool::new(&root, Kind::Grep)),
        Box::new(FileTool::new(&root, Kind::Glob)),
    ])
}

#[derive(Debug, Clone, Copy)]
enum Kind {
    Read,
    Write,
    Edit,
    List,
    Grep,
    Glob,
}

struct FileTool {
    root: Root,
    kind: Kind,
}

impl FileTool {
    fn new(root: &Root, kind: Kind) -> Self {
        FileTool {
            root: root.clone(),
            kind,
        }
    }
}

impl Tool for FileTool {
    fn spec(&self) -> ToolSpec {
        let (name, description, parameters) = match self.kind {
            Kind::Read => (
                "read_file",
                concat!(
                    "Reads a text file. Lines come numbered. ",
                    "Use offset (first line, 1-based) and limit for a part of a long file."
                ),
                json!({"type": "object", "properties": {
                    "path": {"type": "string", "description": "Relative to the working folder."},
                    "offset": {"type": "integer", "minimum": 1},
                    "limit": {"type": "integer", "minimum": 1}
                }, "required": ["path"]}),
            ),
            Kind::Write => (
                "write_file",
                concat!(
                    "Writes a whole file, creating it and its folders if needed. ",
                    "Overwrites an existing file: prefer edit_file for a change."
                ),
                json!({"type": "object", "properties": {
                    "path": {"type": "string"},
                    "content": {"type": "string"}
                }, "required": ["path", "content"]}),
            ),
            Kind::Edit => (
                "edit_file",
                concat!(
                    "Replaces old_string with new_string in a file. ",
                    "old_string must match exactly and be unique unless replace_all is true."
                ),
                json!({"type": "object", "properties": {
                    "path": {"type": "string"},
                    "old_string": {"type": "string"},
                    "new_string": {"type": "string"},
                    "replace_all": {"type": "boolean"}
                }, "required": ["path", "old_string", "new_string"]}),
            ),
            Kind::List => (
                "list_dir",
                "Lists a folder (the working folder when path is empty). Folders end with /.",
                json!({"type": "object", "properties": {"path": {"type": "string"}}}),
            ),
            Kind::Grep => (
                "grep",
                concat!(
                    "Searches file contents with a regular expression. Answers path:line: text. ",
                    "include is a glob (e.g. **/*.rs)."
                ),
                json!({"type": "object", "properties": {
                    "pattern": {"type": "string"},
                    "include": {"type": "string"},
                    "case_sensitive": {"type": "boolean"}
                }, "required": ["pattern"]}),
            ),
            Kind::Glob => (
                "glob",
                "Finds files by name with a glob pattern (e.g. src/**/*.ts). Ignored files (.gitignore) are left out.",
                json!({"type": "object", "properties": {"pattern": {"type": "string"}}, "required": ["pattern"]}),
            ),
        };
        ToolSpec {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    fn call(&self, arguments: Value, stop: StopFlag) -> ToolFuture<'_> {
        let root = self.root.clone();
        let kind = self.kind;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || match kind {
                Kind::Read => read(&root, parse(arguments)?),
                Kind::Write => write(&root, parse(arguments)?),
                Kind::Edit => edit(&root, parse(arguments)?),
                Kind::List => list(&root, parse(arguments)?),
                Kind::Grep => grep(&root, parse(arguments)?, &stop),
                Kind::Glob => glob(&root, parse(arguments)?),
            })
            .await
            .unwrap_or_else(|err| Err(format!("the tool failed: {err}")))
        })
    }
}

fn parse<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|err| format!("bad arguments: {err}"))
}

/// A path as the model writes it, as the root wants it: `./a` and `a` alike, `.` and `""` for the folder itself.
fn rel(path: &str) -> &str {
    let path = path.trim();
    let path = path.strip_prefix("./").unwrap_or(path);
    if path == "." { "" } else { path }
}

fn describe(err: FilesError) -> String {
    err.to_string()
}

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

fn read(root: &Root, args: ReadArgs) -> Result<String, String> {
    guard(rel(&args.path), &PRIVATE_DIRS, &[])?;
    let file = read_text(root, rel(&args.path), MAX_READ_BYTES).map_err(describe)?;
    let first = args.offset.unwrap_or(1).max(1);
    let limit = args.limit.unwrap_or(DEFAULT_READ_LINES).max(1);
    let lines: Vec<&str> = file.content.lines().collect();
    if lines.is_empty() {
        return Ok("(empty file)".into());
    }
    if first > lines.len() {
        return Err(format!(
            "offset {first} is past the end ({} lines)",
            lines.len()
        ));
    }
    let mut out = String::new();
    for (index, line) in lines.iter().enumerate().skip(first - 1).take(limit) {
        let shown: String = line.chars().take(MAX_LINE_CHARS).collect();
        out.push_str(&format!("{:>6}\t{shown}\n", index + 1));
    }
    let last = (first - 1 + limit).min(lines.len());
    if last < lines.len() {
        out.push_str(&format!(
            "[{} more lines; read on with offset={}]\n",
            lines.len() - last,
            last + 1
        ));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

fn write(root: &Root, args: WriteArgs) -> Result<String, String> {
    let path = rel(&args.path);
    guard(path, &PROTECTED_DIRS, &PROTECTED_FILES)?;
    make_parents(root, path)?;
    write_text(root, path, &args.content, None, MAX_WRITE_BYTES).map_err(describe)?;
    Ok(format!("wrote {} bytes to {path}", args.content.len()))
}

/// Creates the folders above `path` that do not exist yet, one level at a time, each through the root's guard.
fn make_parents(root: &Root, path: &str) -> Result<(), String> {
    let mut so_far = String::new();
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        if !so_far.is_empty() {
            so_far.push('/');
        }
        so_far.push_str(part);
        // `Root::resolve` refuses a folder (it resolves files); listing it is the guarded way to ask whether it is one.
        let exists = list_dir(root, &so_far).is_ok();
        if !exists {
            make_dir(root, &so_far).map_err(describe)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

fn edit(root: &Root, args: EditArgs) -> Result<String, String> {
    let path = rel(&args.path);
    guard(path, &PROTECTED_DIRS, &PROTECTED_FILES)?;
    if args.old_string.is_empty() {
        return Err("old_string is empty; use write_file to fill a file".into());
    }
    if args.old_string == args.new_string {
        return Err("old_string and new_string are the same".into());
    }
    let file = read_text(root, path, MAX_READ_BYTES).map_err(describe)?;
    let count = file.content.matches(&args.old_string).count();
    let changed = match count {
        0 => return Err(format!("old_string was not found in {path}")),
        1 => file.content.replacen(&args.old_string, &args.new_string, 1),
        _ if args.replace_all => file.content.replace(&args.old_string, &args.new_string),
        n => {
            return Err(format!(
                "old_string occurs {n} times in {path}; add context to make it unique, or set replace_all"
            ));
        }
    };
    // The version read above: a file changed in between (by the owner, another agent) is not overwritten.
    write_text(root, path, &changed, Some(&file.version), MAX_WRITE_BYTES).map_err(describe)?;
    Ok(format!("replaced {count} occurrence(s) in {path}"))
}

#[derive(Deserialize)]
struct ListArgs {
    #[serde(default)]
    path: String,
}

fn list(root: &Root, args: ListArgs) -> Result<String, String> {
    guard(rel(&args.path), &PRIVATE_DIRS, &[])?;
    let listing = list_dir(root, rel(&args.path)).map_err(describe)?;
    let mut entries = listing.entries;
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out: Vec<String> = entries
        .iter()
        .take(MAX_LIST_ENTRIES)
        .map(|entry| match entry.kind {
            EntryKind::Dir => format!("{}/", entry.name),
            _ => match entry.size {
                Some(size) => format!("{} ({size} bytes)", entry.name),
                None => entry.name.clone(),
            },
        })
        .collect();
    if entries.len() > MAX_LIST_ENTRIES || listing.truncated {
        out.push(format!("[more entries than the {MAX_LIST_ENTRIES} shown]"));
    }
    if out.is_empty() {
        return Ok("(empty folder)".into());
    }
    Ok(out.join("\n"))
}

#[derive(Deserialize)]
struct GrepArgs {
    pattern: String,
    include: Option<String>,
    #[serde(default = "yes")]
    case_sensitive: bool,
}

fn yes() -> bool {
    true
}

fn grep(root: &Root, args: GrepArgs, stop: &AtomicBool) -> Result<String, String> {
    let query = SearchQuery {
        pattern: args.pattern,
        regex: true,
        case_sensitive: args.case_sensitive,
        whole_word: false,
        include: args.include.into_iter().collect(),
        exclude: Vec::new(),
    };
    let mut lines = Vec::new();
    let mut more = false;
    let summary = search(root, &query, stop, |found| {
        for hit in found.matches {
            if lines.len() < MAX_GREP_LINES {
                // One-based, like `read_file`'s numbers, so the model can read on at that line.
                lines.push(format!(
                    "{}:{}: {}",
                    found.rel,
                    hit.line + 1,
                    hit.text.trim_end()
                ));
            } else {
                more = true;
            }
        }
    })
    .map_err(describe)?;
    if lines.is_empty() {
        return Ok("no matches".into());
    }
    if more || summary.truncated {
        lines.push(format!(
            "[more matches than the {MAX_GREP_LINES} shown; narrow the pattern or include]"
        ));
    }
    Ok(lines.join("\n"))
}

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
}

fn glob(root: &Root, args: GlobArgs) -> Result<String, String> {
    let matcher = Glob::new(rel(&args.pattern))
        .map_err(|err| format!("bad glob: {err}"))?
        .compile_matcher();
    let index = index_files(root).map_err(describe)?;
    let mut found: Vec<&String> = index
        .files
        .iter()
        .filter(|path| matcher.is_match(path))
        .collect();
    found.sort();
    if found.is_empty() {
        return Ok("no files match".into());
    }
    let mut out: Vec<String> = found
        .iter()
        .take(MAX_GLOB_PATHS)
        .map(|path| (*path).clone())
        .collect();
    if found.len() > MAX_GLOB_PATHS || index.truncated {
        out.push(format!("[more files than the {MAX_GLOB_PATHS} shown]"));
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Toolbox;

    /// A fresh folder with a toolbox of the file tools on it.
    fn setup() -> (std::path::PathBuf, Toolbox) {
        let dir = std::env::temp_dir().join(format!(
            "axh-files-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut toolbox = Toolbox::new();
        toolbox.extend(file_tools(&dir).unwrap());
        (dir, toolbox)
    }

    #[tokio::test]
    async fn read_numbers_lines_and_pages_a_long_file() {
        let (dir, tools) = setup();
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let (ok, text) = tools
            .call("read_file", r#"{"path":"./a.txt","offset":2,"limit":2}"#)
            .await;
        assert!(ok, "{text}");
        assert_eq!(
            text,
            "     2\ttwo\n     3\tthree\n[1 more lines; read on with offset=4]\n"
        );
        let (ok, text) = tools
            .call("read_file", r#"{"path":"a.txt","offset":9}"#)
            .await;
        assert!(!ok && text.contains("past the end"));
    }

    #[tokio::test]
    async fn write_creates_folders_and_edit_replaces_exactly_once() {
        let (dir, tools) = setup();
        let (ok, text) = tools
            .call(
                "write_file",
                r#"{"path":"src/deep/b.rs","content":"let a = 1;\nlet a = 1;\n"}"#,
            )
            .await;
        assert!(ok, "{text}");
        assert!(dir.join("src/deep/b.rs").is_file());

        let (ok, text) = tools
            .call(
                "edit_file",
                r#"{"path":"src/deep/b.rs","old_string":"let a = 1;","new_string":"let b = 2;"}"#,
            )
            .await;
        assert!(!ok && text.contains("occurs 2 times"), "{text}");
        let (ok, text) = tools
            .call(
                "edit_file",
                r#"{"path":"src/deep/b.rs","old_string":"nope","new_string":"x"}"#,
            )
            .await;
        assert!(!ok && text.contains("not found"), "{text}");
        let (ok, _) = tools.call(
                "edit_file",
                r#"{"path":"src/deep/b.rs","old_string":"let a = 1;","new_string":"let b = 2;","replace_all":true}"#,
            )
            .await;
        assert!(ok);
        assert_eq!(
            std::fs::read_to_string(dir.join("src/deep/b.rs")).unwrap(),
            "let b = 2;\nlet b = 2;\n"
        );
    }

    #[tokio::test]
    async fn paths_outside_the_folder_are_refused() {
        let (dir, tools) = setup();
        let outside = dir.parent().unwrap().join(format!(
            "{}-outside",
            dir.file_name().unwrap().to_string_lossy()
        ));
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("link")).unwrap();
        for path in ["../x.txt", "/etc/hosts", "link/secret.txt"] {
            let (ok, text) = tools
                .call("read_file", &json!({"path": path}).to_string())
                .await;
            assert!(!ok, "{path}: {text}");
            let (ok, _) = tools
                .call(
                    "write_file",
                    &json!({"path": path, "content": "x"}).to_string(),
                )
                .await;
            assert!(!ok, "{path}");
        }
        assert_eq!(
            std::fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "s"
        );
    }

    #[tokio::test]
    async fn list_grep_and_glob_find_what_is_there() {
        let (dir, tools) = setup();
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}\n// TODO later\n").unwrap();
        std::fs::write(dir.join("README.md"), "todo: nothing\n").unwrap();

        let (_, text) = tools.call("list_dir", "{}").await;
        assert_eq!(text, "README.md (14 bytes)\nsrc/");
        let (_, text) = tools.call("grep", r#"{"pattern":"TODO"}"#).await;
        assert_eq!(text, "src/main.rs:2: // TODO later");
        let (_, text) = tools
            .call(
                "grep",
                r#"{"pattern":"todo","case_sensitive":false,"include":"*.md"}"#,
            )
            .await;
        assert_eq!(text, "README.md:1: todo: nothing");
        let (_, text) = tools.call("glob", r#"{"pattern":"src/**/*.rs"}"#).await;
        assert_eq!(text, "src/main.rs");
        let (_, text) = tools.call("glob", r#"{"pattern":"*.py"}"#).await;
        assert_eq!(text, "no files match");
    }

    #[test]
    fn a_folder_that_does_not_exist_is_no_root() {
        assert!(matches!(
            file_tools(std::path::Path::new("/definitely/not/here")),
            Err(HarnessError::Workspace { .. })
        ));
    }

    #[tokio::test]
    async fn a_second_file_goes_into_an_existing_folder() {
        let (dir, tools) = setup();
        for name in ["src/a.rs", "src/b.rs"] {
            let (ok, text) = tools
                .call(
                    "write_file",
                    &json!({"path": name, "content": "x"}).to_string(),
                )
                .await;
            assert!(ok, "{name}: {text}");
        }
        assert!(dir.join("src/b.rs").is_file());
    }

    #[tokio::test]
    async fn version_control_and_configuration_are_off_limits() {
        let (dir, tools) = setup();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/config"), "[core]\n").unwrap();
        for path in [
            ".git/config",
            "sub/.git/hooks/pre-commit",
            ".claude/settings.json",
            ".axiomata/tasks.json",
            "a/.mcp.json",
            "opencode.json",
        ] {
            let (ok, text) = tools
                .call(
                    "write_file",
                    &json!({"path": path, "content": "x"}).to_string(),
                )
                .await;
            assert!(!ok && text.contains("protected"), "{path}: {text}");
        }
        let (ok, _) = tools
            .call("edit_file", r#"{"path":".git/config","old_string":"[core]","new_string":"[core]\nfsmonitor=x"}"#)
            .await;
        assert!(!ok);
        assert_eq!(
            std::fs::read_to_string(dir.join(".git/config")).unwrap(),
            "[core]\n"
        );
        let (ok, _) = tools.call("read_file", r#"{"path":".git/config"}"#).await;
        assert!(!ok);
        let (ok, _) = tools.call("list_dir", r#"{"path":".git"}"#).await;
        assert!(!ok);
        // A name that only contains a protected word is fine.
        let (ok, text) = tools
            .call(
                "write_file",
                r#"{"path":"docs/.github-notes.md","content":"x"}"#,
            )
            .await;
        assert!(ok, "{text}");
    }

    #[tokio::test]
    async fn a_stopped_run_starts_no_tool() {
        let (_dir, tools) = setup();
        let stop = StopFlag::default();
        stop.store(true, std::sync::atomic::Ordering::SeqCst);
        let (ok, text) = tools.call_until("list_dir", "{}", &stop).await;
        assert!(!ok && text.contains("stopped"), "{text}");
    }
}
