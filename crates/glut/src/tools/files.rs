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
use crate::folders::Folders;
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
/// task file or an agent's settings — the harness's own rules in `.glut/` included — would be obeyed by the next
/// session. Security audit of HS1, 2026-10-13.
pub(crate) const PROTECTED_DIRS: [&str; 10] = [
    ".git",
    ".hg",
    ".jj",
    ".axiomata",
    ".claude",
    ".opencode",
    ".vscode",
    ".glut",
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

/// [`guard`] on the path as written and on where it really leads: a link inside the folder (`docs -> .glut`) must not
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

/// Whether the owner's own write at `path` may go there — the review putting a hunk back (TUI T17), whose text came
/// from a model's earlier turn: the protected folders and files the run's write tools keep out of, also through a
/// link; no file with more than one name; and no link on the way at all, since one inside the folder (`docs -> .cargo`)
/// could lead the old text somewhere else than the review showed. What stands at `path` must be a plain file.
///
/// # Errors
///
/// Why the write may not go there, for the owner to read.
pub fn guard_owner_write(root: &Root, path: &str) -> Result<(), String> {
    guard_resolved(root, path, &PROTECTED_DIRS, &PROTECTED_FILES)?;
    refuse_hard_links(root, path)?;
    let mut at = root.path().to_path_buf();
    let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    for (index, part) in parts.iter().enumerate() {
        at.push(part);
        let Ok(meta) = std::fs::symlink_metadata(&at) else {
            break;
        };
        let last = index + 1 == parts.len();
        if meta.file_type().is_symlink() {
            return Err(format!("{path} goes through a link: put it back by hand"));
        }
        if last && !meta.is_file() || !last && !meta.is_dir() {
            return Err(format!("{path} is no plain file: put it back by hand"));
        }
    }
    Ok(())
}

/// The six file tools, confined to `cwd` — and to the folders `beside` it the owner gave the run (T60c), reached by
/// their full path.
///
/// # Errors
///
/// [`HarnessError::Workspace`] if `cwd` is not a folder that can be used as a root.
pub fn file_tools(
    cwd: &std::path::Path,
    skills: SkillRoots,
    beside: Folders,
) -> Result<Vec<Box<dyn Tool>>, HarnessError> {
    let working = Root::dir(cwd, LinkPolicy::Contained).map_err(|err| HarnessError::Workspace {
        path: cwd.display().to_string(),
        reason: err.to_string(),
    })?;
    let places = Places {
        working,
        skills,
        beside,
    };
    Ok([
        Kind::Read,
        Kind::Write,
        Kind::Edit,
        Kind::List,
        Kind::Grep,
        Kind::Glob,
    ]
    .into_iter()
    .map(|kind| {
        Box::new(FileTool {
            places: places.clone(),
            kind,
        }) as Box<dyn Tool>
    })
    .collect())
}

/// Every folder a run's file tools reach: its own, the skills' (read-only, K4) and those beside it (T60c).
#[derive(Clone)]
struct Places {
    working: Root,
    skills: SkillRoots,
    beside: Folders,
}

/// Where a path the model wrote leads: the root it goes through, the path inside that root, how the root's paths are
/// named for the model and the rules (`skill:<name>/`, `~/Vault/`; nothing for the working folder), and whether the
/// run may change what is there.
struct Located {
    root: Root,
    inside: String,
    prefix: String,
    writable: bool,
}

impl Places {
    /// Where `path` leads.
    ///
    /// # Errors
    ///
    /// A `skill:` path naming no skill of the run, a `~/…` path in no folder beside the run, or such a folder that
    /// cannot be opened.
    fn locate(&self, path: &str) -> Result<Located, String> {
        let path = rel(path);
        if let Some((name, inside)) = skill_path(path) {
            let (root, inside) = self.skills.find(name, inside)?;
            return Ok(Located {
                root: root.clone(),
                inside: inside.to_owned(),
                prefix: format!("{SKILL_PREFIX}{name}/"),
                writable: false,
            });
        }
        if let Some((folder, inside)) = self.beside.place(path) {
            let root = Root::dir(&folder.real, LinkPolicy::Contained)
                .map_err(|err| format!("{}: {err}", folder.shown))?;
            // The folder itself turned into a link since it was given (a command in a folder above it): the root
            // would lead wherever the link does.
            if root.path() != folder.real {
                return Err(format!(
                    "{} is no longer the folder it was when it was given",
                    folder.shown
                ));
            }
            return Ok(Located {
                root,
                inside,
                prefix: format!("{}/", folder.shown),
                writable: true,
            });
        }
        // `~` is the home folder to the model, never a folder of that name here (T60c).
        if path == "~" || path.starts_with("~/") {
            return Err(self.outside(path));
        }
        Ok(Located {
            root: self.working.clone(),
            inside: path.to_owned(),
            prefix: String::new(),
            writable: true,
        })
    }

    /// Why a path out of every folder of the run is refused.
    fn outside(&self, path: &str) -> String {
        let beside: Vec<String> = self
            .beside
            .list()
            .into_iter()
            .map(|folder| folder.shown)
            .collect();
        if beside.is_empty() {
            format!("{path} is outside the working folder")
        } else {
            format!(
                "{path} is outside the working folder and the folders beside it ({})",
                beside.join(", ")
            )
        }
    }
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

/// Where a path leads, as the rules see it: the working folder, a skill's folder with its `skill:<name>/` prefix
/// kept, or a folder beside the run as it is shown (`~/Vault/…`) — a link inside any of them resolved to where it
/// really leads.
fn rule_path(places: &Places, path: &str) -> String {
    let Ok(at) = places.locate(path) else {
        return rel(path).to_owned();
    };
    let real = real_path(&at.root, &at.inside);
    if at.prefix.is_empty() {
        return real;
    }
    if real.is_empty() || real == "." {
        at.prefix.trim_end_matches('/').to_owned()
    } else {
        format!("{}{real}", at.prefix)
    }
}

/// The folder a search over the glob `pattern` stays in, as the rules see it: its leading components without a glob
/// character (`News/2026/*.md` → `News/2026`), or `.` — the whole working folder — when there are none, or when one of
/// them steps up or the pattern is absolute. In a folder beside the run (T60c) the same below it: `~/Vault/**` is
/// `~/Vault`.
fn searched_folder(places: &Places, pattern: &str) -> String {
    let pattern = rel(pattern);
    if let Some((folder, inside)) = places.beside.place(pattern) {
        return match fixed_part(&inside) {
            Some(fixed) => rule_path(places, &format!("{}/{fixed}", folder.shown)),
            None => folder.shown,
        };
    }
    if pattern.starts_with(['/', '~']) {
        return ".".into();
    }
    match fixed_part(pattern) {
        Some(fixed) => rule_path(places, &fixed),
        None => ".".into(),
    }
}

/// The leading components of a relative glob without a glob character, the last one only where a glob follows (a
/// pattern without any names a file); `None` for none, or when one of them steps up.
fn fixed_part(pattern: &str) -> Option<String> {
    let mut fixed = Vec::new();
    for component in pattern.split('/') {
        if component.contains(['*', '?', '[', '{']) {
            break;
        }
        if component == ".." {
            return None;
        }
        if !component.is_empty() && component != "." {
            fixed.push(component);
        }
    }
    if !pattern.contains(['*', '?', '[', '{']) {
        fixed.pop();
    }
    (!fixed.is_empty()).then(|| fixed.join("/"))
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
    places: Places,
    kind: Kind,
}

impl Tool for FileTool {
    fn spec(&self) -> ToolSpec {
        let (name, description, parameters) = match self.kind {
            Kind::Read => (
                "read_file",
                concat!(
                    "Reads a text file. Lines come numbered. ",
                    "Use offset (first line, 1-based) and limit for a part of a long file. ",
                    "For a source file of more than a few hundred lines, first ask for its outline (outline: true): ",
                    "its declarations with their line numbers, then read the ranges you need."
                ),
                json!({"type": "object", "properties": {
                    "path": {"type": "string", "description": "Relative to the working folder."},
                    "offset": {"type": "integer", "minimum": 1},
                    "limit": {"type": "integer", "minimum": 1},
                    "outline": {"type": "boolean", "description": "Only the file's declarations, with line numbers."}
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
                    "include is a glob (e.g. **/*.rs). Hidden files (a name starting with a dot) and ignored ones ",
                    "(.gitignore) are not searched; read_file reads one by name."
                ),
                json!({"type": "object", "properties": {
                    "pattern": {"type": "string"},
                    "include": {"type": "string"},
                    "case_sensitive": {"type": "boolean"}
                }, "required": ["pattern"]}),
            ),
            Kind::Glob => (
                "glob",
                concat!(
                    "Finds files by name with a glob pattern (e.g. src/**/*.ts). Hidden files (a name starting ",
                    "with a dot) and ignored ones (.gitignore) are left out; list_dir shows them, read_file reads ",
                    "one by name."
                ),
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
                path: rule_path(&self.places, &path("path")),
            },
            // A search reads a whole folder; what the read rules hold back is left out of its results (`shown`).
            // An `include`/`pattern` naming a skill searches that skill's folder, not the working folder (K4).
            // A glob's fixed folder (`News/**` → `News`) is all it can match below, so the search is judged as a read
            // of that folder: `Read(News/**)` allows a search of `News/` and nothing wider (glut-round W8).
            Kind::Grep => Access::Read {
                path: match skill_path(rel(&path("include"))) {
                    Some((name, _)) => format!("skill:{name}"),
                    None => searched_folder(&self.places, &path("include")),
                },
            },
            Kind::Glob => Access::Read {
                path: match skill_path(rel(&path("pattern"))) {
                    Some((name, _)) => format!("skill:{name}"),
                    None => searched_folder(&self.places, &path("pattern")),
                },
            },
            Kind::Write | Kind::Edit => Access::Edit {
                path: rule_path(&self.places, &path("path")),
            },
        }
    }

    fn call(&self, arguments: Value, context: CallContext) -> ToolFuture<'_> {
        let CallContext {
            stop, gate, report, ..
        } = context;
        let places = self.places.clone();
        let kind = self.kind;
        Box::pin(async move {
            tokio::task::spawn_blocking(move || match kind {
                Kind::Read => read(&places, parse(arguments)?, &report),
                Kind::Write => write(&places, parse(arguments)?),
                Kind::Edit => edit(&places, parse(arguments)?),
                Kind::List => list(&places, parse(arguments)?),
                Kind::Grep => grep(&places, parse(arguments)?, &stop, gate.as_deref()),
                Kind::Glob => glob(&places, parse(arguments)?, gate.as_deref()),
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
    /// Glut lean, N5: the declarations instead of the lines.
    #[serde(default)]
    outline: bool,
}

fn read(places: &Places, args: ReadArgs, report: &super::Report) -> Result<String, String> {
    // K4: a skill's folder is a read-only root beside the working folder; T60c: the owner's folders beside it too.
    let at = places.locate(&args.path)?;
    guard_resolved(&at.root, &at.inside, &PRIVATE_DIRS, &[])?;
    let (root, path) = (&at.root, at.inside.as_str());
    // T45: an image is shown to the model — its bytes beside the answer, the answer only what it is.
    if crate::images::is_image_name(path) {
        let read = axiomata_files::read_image(root, path, crate::images::MOST_FILE_BYTES)
            .map_err(describe)?;
        let bytes =
            crate::images::from_base64(&read.base64).ok_or("the image could not be read")?;
        if !crate::images::is_image(&bytes) {
            return Err(format!(
                "{path} is no image Glut reads (PNG, JPEG, GIF, WebP)"
            ));
        }
        let (width, height) = crate::images::dimensions(&bytes).unwrap_or((0, 0));
        report.image(bytes);
        return Ok(format!("an image, {width}×{height}: {path}"));
    }
    let file = read_text(root, path, MAX_READ_BYTES).map_err(describe)?;
    if args.outline {
        return super::outline::of(std::path::Path::new(&args.path), &file.content);
    }
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

fn write(places: &Places, args: WriteArgs) -> Result<String, String> {
    let (root, path) = &writable(places, &args.path)?;
    guard_resolved(root, path, &PROTECTED_DIRS, &PROTECTED_FILES)?;
    refuse_hard_links(root, path)?;
    make_parents(root, path)?;
    write_text(root, path, &args.content, None, MAX_WRITE_BYTES).map_err(describe)?;
    Ok(format!(
        "wrote {} bytes to {}",
        args.content.len(),
        rel(&args.path)
    ))
}

/// The root and the path inside it a write goes to.
///
/// # Errors
///
/// A path out of every folder of the run, or into a skill's folder (K4: read-only for the run, whatever its rules
/// say).
fn writable(places: &Places, path: &str) -> Result<(Root, String), String> {
    let at = places.locate(path)?;
    if !at.writable {
        return Err(
            "a skill's files are read-only for the run; change them outside the harness".into(),
        );
    }
    Ok((at.root, at.inside))
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

fn edit(places: &Places, args: EditArgs) -> Result<String, String> {
    let (root, inside) = &writable(places, &args.path)?;
    let path = inside.as_str();
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

fn list(places: &Places, args: ListArgs) -> Result<String, String> {
    let at = places.locate(&args.path)?;
    guard_resolved(&at.root, &at.inside, &PRIVATE_DIRS, &[])?;
    let (root, path) = (&at.root, at.inside.as_str());
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

/// Whether a search's glob names another folder than the working one: a skill's (`skill:<name>/…`, K4) or one beside
/// the run (`~/Vault/**`, T60c) — a `~/…` in none of them is refused there, not searched here for nothing.
fn searches_elsewhere(glob: &str) -> bool {
    let glob = rel(glob);
    skill_path(glob).is_some() || glob == "~" || glob.starts_with("~/") || glob.starts_with('/')
}

/// What an empty search adds when its glob names a hidden file or folder (`.env*`, `.github/**`): the walk never
/// looks at those, so "nothing found" must not read as "not there" — a model took a missing `.env` for a forbidden
/// one and refused to read it (T58 Mac test).
fn hidden_note(glob: &str) -> &'static str {
    let names_hidden = rel(glob)
        .split('/')
        .any(|part| part.starts_with('.') && part != "." && part != "..");
    if names_hidden {
        "\n[hidden files and folders are not searched; list_dir shows them, read_file reads one by name]"
    } else {
        ""
    }
}

fn grep(
    places: &Places,
    args: GrepArgs,
    stop: &AtomicBool,
    gate: Option<&Gate>,
) -> Result<String, String> {
    // K4, T60c: an `include` naming a skill or a folder beside the run searches that folder, and its hits keep the
    // prefix (`skill:<name>/`, `~/Vault/`) the rules and `read_file` speak.
    let (root, prefix, include) = match args.include.as_deref() {
        Some(include) if searches_elsewhere(include) => {
            let at = places.locate(include)?;
            (at.root, at.prefix, Some(at.inside))
        }
        _ => (places.working.clone(), String::new(), args.include.clone()),
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
    let summary = search(&root, &query, stop, |found| {
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
        return Ok(format!(
            "no matches{}",
            hidden_note(args.include.as_deref().unwrap_or(""))
        ));
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

fn glob(places: &Places, args: GlobArgs, gate: Option<&Gate>) -> Result<String, String> {
    // K4, T60c: a pattern naming a skill or a folder beside the run searches that folder, and its paths keep the
    // prefix (`skill:<name>/`, `~/Vault/`) the rules and `read_file` speak.
    let (root, prefix, pattern) = if searches_elsewhere(&args.pattern) {
        let at = places.locate(&args.pattern)?;
        (at.root, at.prefix, at.inside)
    } else {
        (
            places.working.clone(),
            String::new(),
            rel(&args.pattern).to_owned(),
        )
    };
    let matcher = Glob::new(&pattern)
        .map_err(|err| format!("bad glob: {err}"))?
        .compile_matcher();
    let index = index_files(&root).map_err(describe)?;
    let mut found: Vec<String> = index
        .files
        .iter()
        .filter(|path| matcher.is_match(path))
        .map(|path| format!("{prefix}{path}"))
        .filter(|path| shown(gate, path))
        .collect();
    found.sort();
    if found.is_empty() {
        return Ok(format!("no files match{}", hidden_note(&args.pattern)));
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
        // The clock alone gave two tests running at once the same folder (macOS counts microseconds).
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "glut-files-{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
        toolbox.extend(file_tools(&dir, SkillRoots::new(&folders), Default::default()).unwrap());
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
                &Places {
                    working: Root::dir(&dir, LinkPolicy::Contained).unwrap(),
                    skills: SkillRoots::new(&[]),
                    beside: Folders::default(),
                },
                "News/KI.md"
            ),
            "News/"
        );
    }

    #[tokio::test]
    async fn read_shows_an_image_beside_its_answer_never_in_its_text() {
        let (dir, tools) = setup();
        // A PNG of 2 × 1 pixels.
        let png: [u8; 70] = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 1,
            8, 2, 0, 0, 0, 123, 64, 232, 221, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            0, 4, 255, 1, 7, 0, 1, 255, 226, 35, 158, 89, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96,
            130,
        ];
        std::fs::write(dir.join("shot.png"), png).unwrap();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called = tools
            .call_reporting("read_file", r#"{"path":"shot.png"}"#, &stop)
            .await;
        assert!(called.ok, "{}", called.text);
        assert_eq!(called.text, "an image, 2×1: shot.png");
        assert_eq!(called.images, vec![png.to_vec()]);
        // Text that only calls itself an image stays text.
        std::fs::write(dir.join("fake.png"), "not an image").unwrap();
        let called = tools
            .call_reporting("read_file", r#"{"path":"fake.png"}"#, &stop)
            .await;
        assert!(!called.ok && called.images.is_empty());
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

    #[tokio::test]
    async fn a_search_for_a_hidden_name_says_it_does_not_look_there() {
        let (dir, tools) = setup();
        std::fs::write(dir.join(".env"), "DB_PASSWORD=x\n").unwrap();

        let (_, text) = tools.call("glob", r#"{"pattern":"**/.env*"}"#).await;
        assert!(text.starts_with("no files match\n[hidden files"), "{text}");
        let (_, text) = tools
            .call("grep", r#"{"pattern":"DB_","include":".env"}"#)
            .await;
        assert!(text.starts_with("no matches\n[hidden files"), "{text}");
        // A plain miss stays plain: `./` and `../` are no hidden names.
        let (_, text) = tools.call("glob", r#"{"pattern":"./*.py"}"#).await;
        assert_eq!(text, "no files match");
        let (_, text) = tools.call("read_file", r#"{"path":".env"}"#).await;
        assert!(text.contains("DB_PASSWORD"), "{text}");
    }

    #[test]
    fn a_folder_that_does_not_exist_is_no_root() {
        assert!(matches!(
            file_tools(
                std::path::Path::new("/definitely/not/here"),
                SkillRoots::default(),
                Folders::default()
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

    #[test]
    fn an_owners_write_keeps_out_of_protected_places_links_and_shared_files() {
        let dir = std::env::temp_dir().join(format!("glut-owner-write-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        std::fs::write(dir.join("docs/a.md"), "a").unwrap();
        std::os::unix::fs::symlink(dir.join(".cargo"), dir.join("cfg")).unwrap();
        std::fs::write(dir.join("one"), "x").unwrap();
        std::fs::hard_link(dir.join("one"), dir.join("two")).unwrap();
        let root = Root::dir(&dir, LinkPolicy::Contained).unwrap();
        assert!(guard_owner_write(&root, "docs/a.md").is_ok());
        assert!(
            guard_owner_write(&root, "docs/new.md").is_ok(),
            "a file that is not there yet"
        );
        assert!(guard_owner_write(&root, ".cargo/config.toml").is_err());
        assert!(
            guard_owner_write(&root, "cfg/config.toml").is_err(),
            "through a link"
        );
        assert!(guard_owner_write(&root, "one").is_err(), "two names");
        assert!(guard_owner_write(&root, "docs").is_err(), "no plain file");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn protected_folders_hold_in_any_case_and_behind_a_link() {
        let (dir, tools) = setup();
        std::fs::create_dir_all(dir.join(".glut")).unwrap();
        std::os::unix::fs::symlink(dir.join(".glut"), dir.join("docs")).unwrap();
        for path in [".GLUT/settings.json", ".Git/config", "docs/settings.json"] {
            let (ok, text) = tools
                .call(
                    "write_file",
                    &json!({"path": path, "content": "{}"}).to_string(),
                )
                .await;
            assert!(!ok && text.contains("protected"), "{path}: {text}");
        }
        assert!(!dir.join(".glut/settings.json").exists());
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
        plain.extend(file_tools(&dir, SkillRoots::default(), Default::default()).unwrap());
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

    /// T60c: a folder beside the working one is reached by its full path — shown or real — with everything that holds
    /// in the working folder: its protected names, links out refused, the rules on its paths as shown.
    #[tokio::test]
    async fn a_folder_beside_the_working_one_is_reached_by_its_full_path() {
        use crate::folders::Folder;
        use crate::permissions::{Gate, Mode, Nobody, Rule, RuleSet};
        let (dir, _) = setup();
        let dir = dir.canonicalize().unwrap();
        let (work, vault) = (dir.join("work"), dir.join("vault"));
        std::fs::create_dir_all(&work).unwrap();
        std::fs::create_dir_all(&vault).unwrap();
        std::fs::write(vault.join("note.md"), "first line\n").unwrap();
        let mut tools = Toolbox::new();
        let beside = tools.folders().clone();
        tools.extend(file_tools(&work, SkillRoots::default(), beside.clone()).unwrap());
        let call = |tool: &'static str, args: Value| {
            let tools = &tools;
            async move { tools.call(tool, &args.to_string()).await }
        };
        let (ok, text) = call("read_file", json!({"path": "~/Vault/note.md"})).await;
        assert!(!ok && text.contains("outside the working folder"), "{text}");
        assert!(!work.join("~").exists());

        beside
            .add(Folder {
                real: vault.clone(),
                shown: "~/Vault".into(),
                git_dirs: Vec::new(),
            })
            .unwrap();
        let (ok, text) = call("read_file", json!({"path": "~/Vault/note.md"})).await;
        assert!(ok && text.contains("first line"), "{text}");
        let full = vault.join("note.md").display().to_string();
        let (ok, text) = call("read_file", json!({"path": full})).await;
        assert!(ok && text.contains("first line"), "{text}");
        let (ok, text) = call(
            "write_file",
            json!({"path": "~/Vault/new/b.md", "content": "second\n"}),
        )
        .await;
        assert!(ok, "{text}");
        let (ok, text) = call(
            "edit_file",
            json!({"path": "~/Vault/note.md", "old_string": "first", "new_string": "1st"}),
        )
        .await;
        assert!(ok, "{text}");
        assert_eq!(
            std::fs::read_to_string(vault.join("note.md")).unwrap(),
            "1st line\n"
        );
        let (_, text) = call("list_dir", json!({"path": "~/Vault"})).await;
        assert_eq!(text, "new/\nnote.md (9 bytes)");
        let (_, text) = call("glob", json!({"pattern": "~/Vault/**/*.md"})).await;
        assert_eq!(text, "~/Vault/new/b.md\n~/Vault/note.md");
        let (_, text) = call(
            "grep",
            json!({"pattern": "second", "include": "~/Vault/**"}),
        )
        .await;
        assert_eq!(text, "~/Vault/new/b.md:1: second");

        // What the working folder keeps out, the folder beside it keeps out too; nothing leads out of it.
        std::os::unix::fs::symlink(&work, vault.join("out")).unwrap();
        for (tool, args) in [
            (
                "write_file",
                json!({"path": "~/Vault/.git/config", "content": "x"}),
            ),
            (
                "write_file",
                json!({"path": "~/Vault/.envrc", "content": "x"}),
            ),
            ("read_file", json!({"path": "~/Vault/out/anything"})),
            ("read_file", json!({"path": "~/Vault/../vault/note.md"})),
            ("read_file", json!({"path": "~/elsewhere.md"})),
        ] {
            let (ok, text) = call(tool, args.clone()).await;
            assert!(!ok, "{args}: {text}");
        }

        // The rules see its paths as shown.
        tools.set_gate(std::sync::Arc::new(Gate::new(
            RuleSet {
                deny: vec![Rule::parse("Edit(~/Vault/new/**)").unwrap()],
                ..RuleSet::default()
            },
            Mode::AcceptEdits,
            std::sync::Arc::new(Nobody),
        )));
        let (ok, text) = tools
            .call(
                "write_file",
                &json!({"path": format!("{}/new/c.md", vault.display()), "content": "x"})
                    .to_string(),
            )
            .await;
        assert!(!ok && text.contains("not permitted"), "{text}");
        let (ok, text) = tools
            .call(
                "write_file",
                &json!({"path": "~/Vault/d.md", "content": "x"}).to_string(),
            )
            .await;
        assert!(ok, "{text}");

        // The folder swapped for a link to another one: refused, never followed.
        std::fs::rename(&vault, dir.join("vault-was")).unwrap();
        std::os::unix::fs::symlink(&work, &vault).unwrap();
        let (ok, text) = tools
            .call("list_dir", &json!({"path": "~/Vault"}).to_string())
            .await;
        assert!(!ok && text.contains("no longer the folder"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
