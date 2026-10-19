//! The file tools: read, write, edit, list, search by content and by name — all inside the run's folder.
//!
//! Every path goes through `axiomata-files`' [`Root`] with [`LinkPolicy::Contained`], the guard the editor and the
//! Studio already use: a path that climbs out (`..`), is absolute, or follows a link to a place outside the folder is
//! refused there, not here. The file system work is blocking, so each call runs on tokio's blocking pool.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use axiomata_files::{
    EntryKind, FilesError, LinkPolicy, MAX_READ_BYTES, MAX_WRITE_BYTES, Root, SearchQuery,
    index_files, list_dir, make_dir, read_text, search, write_text,
};
use globset::Glob;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{CallContext, Tool, ToolFuture};
use crate::error::HarnessError;
use crate::model::ToolSpec;
use crate::permissions::{Access, Gate};

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
/// task file or an agent's settings — the harness's own rules in `.axh/` included — would be obeyed by the next
/// session. Security audit of HS1, 2026-10-13.
const PROTECTED_DIRS: [&str; 10] = [
    ".git",
    ".hg",
    ".jj",
    ".axiomata",
    ".claude",
    ".opencode",
    ".vscode",
    ".axh",
    // Run code at the owner's next build or commit: cargo's runner and flags, git hooks of husky.
    ".cargo",
    ".husky",
];
/// `.envrc` runs when the owner next enters the folder (direnv).
const PROTECTED_FILES: [&str; 3] = [".mcp.json", "opencode.json", ".envrc"];

/// Refuses a path whose folders include one of `dirs`, or whose name is one of `files` — whatever its case: macOS
/// volumes do not tell `.git` from `.GIT`.
fn guard(path: &str, dirs: &[&str], files: &[&str]) -> Result<(), String> {
    let is = |part: &str, names: &[&str]| names.iter().any(|name| part.eq_ignore_ascii_case(name));
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    if parts.iter().any(|part| is(part, dirs)) || parts.last().is_some_and(|name| is(name, files)) {
        return Err(format!("{path} is protected: the run may not touch it"));
    }
    Ok(())
}

/// `path` as the rules should see it: relative to the root, through any link inside it to where it really leads; as
/// written when it cannot be resolved (it does not exist yet, or leads outside — then the tool refuses it anyway).
fn real_path(root: &Root, path: &str) -> String {
    let path = rel(path);
    let full = root.path().join(path);
    let mut existing = full.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return path.to_owned(),
        }
    }
    match existing
        .canonicalize()
        .ok()
        .and_then(|real| real.strip_prefix(root.path()).ok().map(Path::to_path_buf))
    {
        Some(inside) => {
            let rest = full.strip_prefix(existing).unwrap_or(Path::new(""));
            let joined = inside.join(rest);
            let text = joined.to_string_lossy();
            if text.is_empty() {
                ".".to_owned()
            } else {
                text.into_owned()
            }
        }
        None => path.to_owned(),
    }
}

/// Refuses to change a file with more than one name: through a hard link, a change here is also a change to a file
/// elsewhere — perhaps in a protected folder — that no rule was asked about.
fn refuse_hard_links(root: &Root, path: &str) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    match std::fs::symlink_metadata(root.path().join(path)) {
        Ok(meta) if meta.is_file() && meta.nlink() > 1 => Err(format!(
            "{path} has {} names (hard links): change it outside the run",
            meta.nlink()
        )),
        _ => Ok(()),
    }
}

/// [`guard`] on the path as written and on where it really leads: a link inside the folder (`docs -> .axh`) must not
/// open a protected folder under another name.
fn guard_resolved(root: &Root, path: &str, dirs: &[&str], files: &[&str]) -> Result<(), String> {
    guard(path, dirs, files)?;
    let full = root.path().join(path);
    // The deepest part of the path that exists, resolved; what lies below it does not exist yet and cannot be a link.
    let mut existing = full.as_path();
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return Ok(()),
        }
    }
    if let Ok(real) = existing.canonicalize()
        && let Ok(inside) = real.strip_prefix(root.path())
    {
        let rest = full
            .strip_prefix(existing)
            .unwrap_or(std::path::Path::new(""));
        guard(&inside.join(rest).to_string_lossy(), dirs, files)?;
    }
    Ok(())
}

/// The six file tools, confined to `cwd`.
///
/// # Errors
///
/// [`HarnessError::Workspace`] if `cwd` is not a folder that can be used as a root.
pub fn file_tools(
    cwd: &std::path::Path,
    skills: SkillRoots,
) -> Result<Vec<Box<dyn Tool>>, HarnessError> {
    let root = Root::dir(cwd, LinkPolicy::Contained).map_err(|err| HarnessError::Workspace {
        path: cwd.display().to_string(),
        reason: err.to_string(),
    })?;
    Ok(vec![
        Box::new(FileTool::new(&root, skills.clone(), Kind::Read)),
        Box::new(FileTool::new(&root, skills.clone(), Kind::Write)),
        Box::new(FileTool::new(&root, skills.clone(), Kind::Edit)),
        Box::new(FileTool::new(&root, skills.clone(), Kind::List)),
        Box::new(FileTool::new(&root, skills.clone(), Kind::Grep)),
        Box::new(FileTool::new(&root, skills, Kind::Glob)),
    ])
}

/// How a run names a skill's folder: `skill:<name>/<rel>`, read-only (HS6, K4).
const SKILL_PREFIX: &str = "skill:";

/// The skill folders a run may read beside the working folder (HS6, K4): each skill's own folder is a root of its
/// own, reached as `skill:<name>/<rel>` — a skill's scripts and references are another tool's files, never the
/// run's to change. Built from the run's catalog; a folder that cannot be opened is left out (the skill's body is
/// in memory already).
#[derive(Clone, Default)]
pub struct SkillRoots(Arc<[(String, Root)]>);

impl SkillRoots {
    /// Every folder of `folders` (name, folder) as a root, through the same guard the working folder gets.
    pub fn new(folders: &[(String, std::path::PathBuf)]) -> Self {
        let roots: Vec<(String, Root)> = folders
            .iter()
            .filter_map(|(name, dir)| {
                Root::dir(dir, LinkPolicy::Contained)
                    .ok()
                    .map(|root| (name.clone(), root))
            })
            .collect();
        SkillRoots(roots.into())
    }

    /// The root of the skill named `name` and the path inside its folder.
    ///
    /// # Errors
    ///
    /// A `skill:` path naming no skill of the run: the message lists the ones there are.
    fn find<'a>(&'a self, name: &str, rel: &'a str) -> Result<(&'a Root, &'a str), String> {
        match self.0.iter().find(|(skill, _)| skill == name) {
            Some((_, root)) => Ok((root, rel)),
            None => Err(format!(
                "there is no skill named {name:?}; known: {}",
                self.known()
            )),
        }
    }

    /// The skill names, for that message.
    fn known(&self) -> String {
        let names: Vec<&str> = self.0.iter().map(|(name, _)| name.as_str()).collect();
        if names.is_empty() {
            "no skills are loaded".into()
        } else {
            names.join(", ")
        }
    }
}

/// `skill:<name>/<rel>` as the skill's name and the path inside its folder; `None` for an ordinary path. The model
/// learns these paths from the `skill` tool's result (K4).
fn skill_path(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix(SKILL_PREFIX)?;
    match rest.split_once('/') {
        Some((name, rel)) if !name.is_empty() => Some((name, rel)),
        Some(_) => None,
        None if !rest.is_empty() => Some((rest, "")),
        None => None,
    }
}

/// Where a path leads, as the rules see it: the working folder, or a skill's folder with its `skill:<name>/`
/// prefix kept — a link inside a skill folder resolved to where it really leads.
fn rule_path(working: &Root, skills: &SkillRoots, path: &str) -> String {
    let path = rel(path);
    let Some((name, inside)) = skill_path(path) else {
        return real_path(working, path);
    };
    let Ok((root, inside)) = skills.find(name, inside) else {
        return path.to_owned();
    };
    let real = real_path(root, inside);
    if real.is_empty() {
        format!("skill:{name}")
    } else {
        format!("skill:{name}/{real}")
    }
}

/// The folder a search over the glob `pattern` stays in, as the rules see it: its leading components without a glob
/// character (`News/2026/*.md` → `News/2026`), or `.` — the whole working folder — when there are none, or when one of
/// them steps up or the pattern is absolute.
fn searched_folder(working: &Root, skills: &SkillRoots, pattern: &str) -> String {
    let pattern = rel(pattern);
    if pattern.starts_with('/') {
        return ".".into();
    }
    let mut fixed = Vec::new();
    for component in pattern.split('/') {
        if component.contains(['*', '?', '[', '{']) {
            break;
        }
        if component == ".." {
            return ".".into();
        }
        if !component.is_empty() && component != "." {
            fixed.push(component);
        }
    }
    // The last fixed component of a pattern without any glob is a file name, not a folder.
    if !pattern.contains(['*', '?', '[', '{']) {
        fixed.pop();
    }
    if fixed.is_empty() {
        ".".into()
    } else {
        rule_path(working, skills, &fixed.join("/"))
    }
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
    skills: SkillRoots,
    kind: Kind,
}

impl FileTool {
    fn new(root: &Root, skills: SkillRoots, kind: Kind) -> Self {
        FileTool {
            root: root.clone(),
            skills,
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

    fn access(&self, arguments: &Value) -> Access {
        let path = |key: &str| {
            arguments
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        };
        match self.kind {
            // A `skill:` path is judged as `skill:<name>/<rel>` (K4); anything else the rules see where it really
            // leads: `link/x` through a link to `secrets/` is `secrets/x`.
            Kind::Read | Kind::List => Access::Read {
                path: rule_path(&self.root, &self.skills, &path("path")),
            },
            // A search reads a whole folder; what the read rules hold back is left out of its results (`shown`).
            // An `include`/`pattern` naming a skill searches that skill's folder, not the working folder (K4).
            // A glob's fixed folder (`News/**` → `News`) is all it can match below, so the search is judged as a read
            // of that folder: `Read(News/**)` allows a search of `News/` and nothing wider (glut-round W8).
            Kind::Grep => Access::Read {
                path: match skill_path(rel(&path("include"))) {
                    Some((name, _)) => format!("skill:{name}"),
                    None => searched_folder(&self.root, &self.skills, &path("include")),
                },
            },
            Kind::Glob => Access::Read {
                path: match skill_path(rel(&path("pattern"))) {
                    Some((name, _)) => format!("skill:{name}"),
                    None => searched_folder(&self.root, &self.skills, &path("pattern")),
                },
            },
            Kind::Write | Kind::Edit => Access::Edit {
                path: rule_path(&self.root, &self.skills, &path("path")),
            },
        }
    }

    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_> {
        let CallContext { stop, gate } = context;
        let root = self.root.clone();
        let skills = self.skills.clone();
        let kind = self.kind;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || match kind {
                Kind::Read => read(&root, &skills, parse(arguments)?),
                Kind::Write => write(&root, parse(arguments)?),
                Kind::Edit => edit(&root, parse(arguments)?),
                Kind::List => list(&root, &skills, parse(arguments)?),
                Kind::Grep => grep(&root, &skills, parse(arguments)?, &stop, gate.as_deref()),
                Kind::Glob => glob(&root, &skills, parse(arguments)?, gate.as_deref()),
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

fn read(working: &Root, skills: &SkillRoots, args: ReadArgs) -> Result<String, String> {
    // K4: a skill's folder is a read-only root beside the working folder.
    let (root, path) = match skill_path(rel(&args.path)) {
        Some((name, inside)) => {
            let (root, inside) = skills.find(name, inside)?;
            guard_resolved(root, inside, &PRIVATE_DIRS, &[])?;
            (root, inside)
        }
        None => {
            let path = rel(&args.path);
            guard_resolved(working, path, &PRIVATE_DIRS, &[])?;
            (working, path)
        }
    };
    let file = read_text(root, path, MAX_READ_BYTES).map_err(describe)?;
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
    // K4: a skill's folder is read-only for the run, whatever its rules say.
    refuse_skill_path(path)?;
    guard_resolved(root, path, &PROTECTED_DIRS, &PROTECTED_FILES)?;
    refuse_hard_links(root, path)?;
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
    // K4: a skill's folder is read-only for the run, whatever its rules say.
    refuse_skill_path(path)?;
    guard_resolved(root, path, &PROTECTED_DIRS, &PROTECTED_FILES)?;
    refuse_hard_links(root, path)?;
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

fn list(working: &Root, skills: &SkillRoots, args: ListArgs) -> Result<String, String> {
    let (root, path) = match skill_path(rel(&args.path)) {
        Some((name, inside)) => {
            let (root, inside) = skills.find(name, inside)?;
            guard_resolved(root, inside, &PRIVATE_DIRS, &[])?;
            (root, inside)
        }
        None => {
            let path = rel(&args.path);
            guard_resolved(working, path, &PRIVATE_DIRS, &[])?;
            (working, path)
        }
    };
    let listing = list_dir(root, path).map_err(describe)?;
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

/// Whether a search shows `rel`: not when a read rule holds it back (the file would be asked about or refused by
/// `read_file`; a search must not hand out its lines instead).
fn shown(gate: Option<&Gate>, rel: &str) -> bool {
    gate.is_none_or(|gate| !gate.read_held_back(rel) && guard(rel, &PRIVATE_DIRS, &[]).is_ok())
}

/// Refuses a write to a skill's folder (K4): its files belong to the skill, not to the run.
fn refuse_skill_path(path: &str) -> Result<(), String> {
    if skill_path(path).is_some() {
        return Err(
            "a skill's files are read-only for the run; change them outside the harness".into(),
        );
    }
    Ok(())
}

fn grep(
    working: &Root,
    skills: &SkillRoots,
    args: GrepArgs,
    stop: &AtomicBool,
    gate: Option<&Gate>,
) -> Result<String, String> {
    // K4: an `include` naming a skill searches that skill's folder, and its hits keep the `skill:<name>/` prefix
    // the rules and `read_file` speak.
    let (root, prefix, include) = match skill_path(args.include.as_deref().unwrap_or("")) {
        Some((name, inside)) => {
            let (root, _) = skills.find(name, "")?;
            (root, format!("skill:{name}/"), Some(inside.to_owned()))
        }
        None => (working, String::new(), args.include.clone()),
    };
    let query = SearchQuery {
        pattern: args.pattern,
        regex: true,
        case_sensitive: args.case_sensitive,
        whole_word: false,
        include: include.into_iter().collect(),
        exclude: Vec::new(),
    };
    let mut lines = Vec::new();
    let mut more = false;
    let mut held_back = 0usize;
    let summary = search(root, &query, stop, |found| {
        let path = format!("{prefix}{}", found.rel);
        if !shown(gate, &path) {
            held_back += 1;
            return;
        }
        for hit in found.matches {
            if lines.len() < MAX_GREP_LINES {
                // One-based, like `read_file`'s numbers, so the model can read on at that line.
                lines.push(format!("{path}:{}: {}", hit.line + 1, hit.text.trim_end()));
            } else {
                more = true;
            }
        }
    })
    .map_err(describe)?;
    if held_back > 0 {
        lines.push(format!(
            "[{held_back} matching file(s) left out: the read rules hold them back]"
        ));
    }
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

fn glob(
    working: &Root,
    skills: &SkillRoots,
    args: GlobArgs,
    gate: Option<&Gate>,
) -> Result<String, String> {
    // K4: a pattern naming a skill searches that skill's folder, and its paths keep the `skill:<name>/` prefix the
    // rules and `read_file` speak.
    let (root, prefix, pattern) = match skill_path(rel(&args.pattern)) {
        Some((name, inside)) => {
            let (root, _) = skills.find(name, "")?;
            (root, format!("skill:{name}/"), inside.to_owned())
        }
        None => (working, String::new(), rel(&args.pattern).to_owned()),
    };
    let matcher = Glob::new(&pattern)
        .map_err(|err| format!("bad glob: {err}"))?
        .compile_matcher();
    let index = index_files(root).map_err(describe)?;
    let mut found: Vec<String> = index
        .files
        .iter()
        .filter(|path| matcher.is_match(path))
        .map(|path| format!("{prefix}{path}"))
        .filter(|path| shown(gate, path))
        .collect();
    found.sort();
    if found.is_empty() {
        return Ok("no files match".into());
    }
    let more = found.len() > MAX_GLOB_PATHS;
    let mut out: Vec<String> = found.into_iter().take(MAX_GLOB_PATHS).collect();
    if more || index.truncated {
        out.push(format!("[more files than the {MAX_GLOB_PATHS} shown]"));
    }
    Ok(out.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::StopFlag;
    use crate::tools::Toolbox;

    /// A fresh folder with a toolbox of the file tools on it.
    fn setup() -> (std::path::PathBuf, Toolbox) {
        setup_with(&[])
    }

    /// [`setup`] with skill folders beside the working folder (HS6, K4): each entry is a skill's name and the files
    /// to write into its folder.
    fn setup_with(skills: &[(&str, &[(&str, &str)])]) -> (std::path::PathBuf, Toolbox) {
        let dir = std::env::temp_dir().join(format!(
            "axh-files-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut folders = Vec::new();
        for (name, files) in skills {
            let folder = dir.join("skills-folder").join(name);
            std::fs::create_dir_all(&folder).unwrap();
            for (path, content) in *files {
                let full = folder.join(path);
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }
                std::fs::write(full, content).unwrap();
            }
            folders.push((name.to_string(), folder));
        }
        let mut toolbox = Toolbox::new();
        toolbox.extend(file_tools(&dir, SkillRoots::new(&folders)).unwrap());
        (dir, toolbox)
    }

    /// glut-round W8: a search is judged as a read of its glob's fixed folder, so `Read(News/**)` lets a skill search
    /// `News/` — and only `News/`: a search of the whole folder, one that steps up and the rest of the tree are refused.
    #[tokio::test]
    async fn a_search_is_a_read_of_the_folder_its_glob_stays_in() {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let (dir, mut tools) = setup();
        std::fs::create_dir_all(dir.join("News")).unwrap();
        std::fs::write(
            dir.join("News/KI.md"),
            "[Original](https://example.org/a)\n",
        )
        .unwrap();
        std::fs::write(dir.join("secret.md"), "https://example.org/a\n").unwrap();
        let declared = vec![Rule::parse("Read(News/**)").unwrap()];
        let mut gate = Gate::new(
            RuleSet {
                allow: declared.clone(),
                ..RuleSet::default()
            },
            Mode::Default,
            std::sync::Arc::new(Nobody),
        );
        gate.narrow("the skill news", declared);
        tools.set_gate(std::sync::Arc::new(gate));

        let (ok, text) = tools
            .call(
                "grep",
                r#"{"pattern":"example\\.org/a","include":"News/**"}"#,
            )
            .await;
        assert!(
            ok && text.contains("News/KI.md") && !text.contains("secret"),
            "{text}"
        );
        let (ok, text) = tools.call("glob", r#"{"pattern":"News/*.md"}"#).await;
        assert!(ok && text.contains("News/KI.md"), "{text}");
        for refused in [
            r#"{"pattern":"example"}"#,
            r#"{"pattern":"example","include":"**/*.md"}"#,
            r#"{"pattern":"example","include":"News/../*.md"}"#,
            r#"{"pattern":"example","include":"/etc/*"}"#,
        ] {
            let (ok, text) = tools.call("grep", refused).await;
            assert!(!ok && text.contains("not permitted"), "{refused}: {text}");
        }
        assert_eq!(
            searched_folder(
                &Root::dir(&dir, LinkPolicy::Contained).unwrap(),
                &SkillRoots::new(&[]),
                "News/KI.md"
            ),
            "News/"
        );
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
            file_tools(
                std::path::Path::new("/definitely/not/here"),
                SkillRoots::default()
            ),
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

    #[tokio::test]
    async fn protected_folders_hold_in_any_case_and_behind_a_link() {
        let (dir, tools) = setup();
        std::fs::create_dir_all(dir.join(".axh")).unwrap();
        std::os::unix::fs::symlink(dir.join(".axh"), dir.join("docs")).unwrap();
        for path in [".AXH/settings.json", ".Git/config", "docs/settings.json"] {
            let (ok, text) = tools
                .call(
                    "write_file",
                    &json!({"path": path, "content": "{}"}).to_string(),
                )
                .await;
            assert!(!ok && text.contains("protected"), "{path}: {text}");
        }
        assert!(!dir.join(".axh/settings.json").exists());
    }

    #[tokio::test]
    async fn a_search_leaves_out_what_the_read_rules_hold_back() {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let (dir, mut tools) = setup();
        std::fs::create_dir_all(dir.join("secrets")).unwrap();
        std::fs::write(dir.join("secrets/key.txt"), "TOKEN=abc\n").unwrap();
        std::fs::write(dir.join("app.txt"), "TOKEN from env\n").unwrap();
        let rules = RuleSet {
            deny: vec![Rule::parse("Read(secrets/**)").unwrap()],
            ..RuleSet::default()
        };
        tools.set_gate(std::sync::Arc::new(Gate::new(
            rules,
            Mode::Default,
            std::sync::Arc::new(Nobody),
        )));

        let (ok, text) = tools.call("grep", r#"{"pattern":"TOKEN"}"#).await;
        assert!(ok, "{text}");
        assert!(text.contains("app.txt") && !text.contains("abc"), "{text}");
        assert!(text.contains("1 matching file(s) left out"), "{text}");
        let (_, text) = tools.call("glob", r#"{"pattern":"**/*.txt"}"#).await;
        assert_eq!(text, "app.txt");
        let (ok, _) = tools
            .call("read_file", r#"{"path":"secrets/key.txt"}"#)
            .await;
        assert!(!ok);
    }

    #[tokio::test]
    async fn rules_see_the_real_path_behind_a_link_and_hard_links_are_not_changed() {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let (dir, mut tools) = setup();
        std::fs::create_dir_all(dir.join("secrets")).unwrap();
        std::fs::write(dir.join("secrets/key.txt"), "k").unwrap();
        std::os::unix::fs::symlink(dir.join("secrets"), dir.join("link")).unwrap();
        let rules = RuleSet {
            deny: vec![Rule::parse("Read(secrets/**)").unwrap()],
            ..RuleSet::default()
        };
        tools.set_gate(std::sync::Arc::new(Gate::new(
            rules,
            Mode::Default,
            std::sync::Arc::new(Nobody),
        )));
        let (ok, text) = tools.call("read_file", r#"{"path":"link/key.txt"}"#).await;
        assert!(!ok && text.contains("Read(secrets/**)"), "{text}");

        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::hard_link(dir.join("a.txt"), dir.join("b.txt")).unwrap();
        // Without a gate, so the write reaches the tool and its own refusal is what answers.
        let mut plain = crate::tools::Toolbox::new();
        plain.extend(file_tools(&dir, SkillRoots::default()).unwrap());
        let (ok, text) = plain
            .call("write_file", r#"{"path":"b.txt","content":"x"}"#)
            .await;
        assert!(!ok && text.contains("hard links"), "{text}");
        assert_eq!(std::fs::read_to_string(dir.join("a.txt")).unwrap(), "a");
    }

    #[tokio::test]
    async fn a_skills_files_are_read_only_roots_of_their_own() {
        let (dir, tools) = setup_with(&[(
            "demo",
            &[
                ("ref.md", "the reference\n"),
                ("scripts/run.sh", "echo hi\n"),
            ],
        )]);
        // Read, list and search through the skill's own root, its paths kept as `skill:<name>/<rel>` (K4).
        let (ok, text) = tools
            .call("read_file", r#"{"path":"skill:demo/ref.md"}"#)
            .await;
        assert!(ok, "{text}");
        assert!(text.contains("the reference"), "{text}");
        let (_, text) = tools.call("list_dir", r#"{"path":"skill:demo"}"#).await;
        assert_eq!(text, "ref.md (14 bytes)\nscripts/");
        let (_, text) = tools
            .call("glob", r#"{"pattern":"skill:demo/**/*.md"}"#)
            .await;
        assert_eq!(text, "skill:demo/ref.md");
        let (_, text) = tools
            .call(
                "grep",
                r#"{"pattern":"reference","include":"skill:demo/**"}"#,
            )
            .await;
        assert_eq!(text, "skill:demo/ref.md:1: the reference");

        // A skill's files are never written, whatever the run's rules allow.
        for (tool, args) in [
            ("write_file", r#"{"path":"skill:demo/x.md","content":"x"}"#),
            (
                "edit_file",
                r#"{"path":"skill:demo/ref.md","old_string":"the","new_string":"a"}"#,
            ),
        ] {
            let (ok, text) = tools.call(tool, args).await;
            assert!(!ok && text.contains("read-only"), "{tool}: {text}");
        }
        assert_eq!(
            std::fs::read_to_string(dir.join("skills-folder/demo/ref.md")).unwrap(),
            "the reference\n"
        );

        // Climbing out of the skill's folder, a link out of it and an unknown name are all refused.
        let skill_dir = dir.join("skills-folder").join("demo");
        std::os::unix::fs::symlink(std::env::temp_dir(), skill_dir.join("out")).unwrap();
        for path in [
            "skill:demo/../../etc/passwd",
            "skill:demo/out/anything",
            "skill:unknown/ref.md",
        ] {
            let (ok, text) = tools
                .call("read_file", &json!({"path": path}).to_string())
                .await;
            assert!(!ok, "{path}: {text}");
        }
    }

    #[tokio::test]
    async fn a_search_of_a_skill_folder_leaves_out_what_the_read_rules_hold_back() {
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let (_dir, mut tools) = setup_with(&[(
            "demo",
            &[("open.md", "TOKEN here\n"), ("secret.md", "TOKEN=abc\n")],
        )]);
        let rules = RuleSet {
            deny: vec![Rule::parse("Read(skill:demo/secret.md)").unwrap()],
            ..RuleSet::default()
        };
        tools.set_gate(std::sync::Arc::new(Gate::new(
            rules,
            Mode::Default,
            std::sync::Arc::new(Nobody),
        )));
        let (_, text) = tools
            .call("glob", r#"{"pattern":"skill:demo/**/*.md"}"#)
            .await;
        assert_eq!(text, "skill:demo/open.md");
        let (ok, text) = tools
            .call("read_file", r#"{"path":"skill:demo/secret.md"}"#)
            .await;
        assert!(!ok && text.contains("skill:demo/secret.md"), "{text}");
    }
}
