//! The `glut_file` tool (`docs/plans/glut-tui.md`, T37b): Glut's own files a skill, an agent or a hook lives in — the
//! owner's (`~/.glut/`) or the project's (`.glut/`) — for the skills that make them (`make-skill`, `make-hook`,
//! `make-agent`). The file tools never reach these folders, so this tool is the one way in, and only to three kinds
//! of file, each named, never a path: `skills/<name>/SKILL.md`, `agents/<name>.md` and the owner's `hooks.json`.
//!
//! It is offered only while one of those skills runs (the loop adds it, `run.rs`), a write is checked the way Glut
//! will read it — a file that would be left out is refused before it is asked about — and asked always, in `auto`
//! too, never done in plan mode (`Access::GlutFile`: a change that outlives the run, no "always").

use std::path::{Path, PathBuf};

use axiomata_files::{
    EntryKind, FilesError, LinkPolicy, Root, list_dir, make_dir, read_text, write_text,
};
use serde_json::{Value, json};

use crate::model::ToolSpec;
use crate::permissions::Access;
use crate::tools::{CallContext, Tool, ToolFuture};

/// The tool's name.
pub const NAME: &str = "glut_file";

/// The skills that offer the tool while they run.
pub const MAKERS: [&str; 3] = ["make-skill", "make-hook", "make-agent"];

/// The most bytes one file may hold: a skill, an agent or the hooks are a few KiB.
const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Whose file: the owner's for every project, or this project's alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Owner,
    Project,
}

/// Which kind of file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Skill,
    Agent,
    Hooks,
}

/// What a call asks for.
enum Action {
    List(Scope),
    Read(Scope, Kind, String),
    Write(Scope, Kind, String, String),
}

impl Action {
    fn of(arguments: &Value) -> Result<Self, String> {
        let text = |key: &str| arguments.get(key).and_then(Value::as_str).map(str::trim);
        let scope = match text("scope") {
            Some("owner") => Scope::Owner,
            Some("project") => Scope::Project,
            _ => return Err("`scope` is owner (every project) or project (this one)".into()),
        };
        if text("action") == Some("list") {
            return Ok(Action::List(scope));
        }
        let kind = match text("kind") {
            Some("skill") => Kind::Skill,
            Some("agent") => Kind::Agent,
            Some("hooks") => Kind::Hooks,
            _ => return Err("`kind` is skill, agent or hooks".into()),
        };
        if kind == Kind::Hooks && scope == Scope::Project {
            return Err("hooks are the owner's only: a project's hooks are never run".into());
        }
        let name = match kind {
            Kind::Hooks => String::new(),
            _ => {
                let name = text("name").unwrap_or_default();
                if !crate::skills::is_skill_name(name) {
                    return Err(
                        "a name is lower case letters, digits and dashes, at most 64 — never a path".into(),
                    );
                }
                name.to_owned()
            }
        };
        match text("action") {
            Some("read") => Ok(Action::Read(scope, kind, name)),
            Some("write") => {
                let content = arguments
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or("`content` is required to write: the whole file")?;
                Ok(Action::Write(scope, kind, name, content.to_owned()))
            }
            _ => Err("`action` is one of list, read, write".into()),
        }
    }
}

/// The owner's and the project's `.glut` folders.
pub struct GlutFiles {
    owner: PathBuf,
    project: PathBuf,
    /// Whether the owner trusts the project: its skills and agents are read only then.
    trusted: bool,
}

impl GlutFiles {
    /// The folders below `home` and `cwd`.
    pub fn new(home: &Path, cwd: &Path, trusted: bool) -> Self {
        GlutFiles {
            owner: home.join(crate::permissions::settings::USER_DIR),
            project: cwd.join(crate::permissions::settings::PROJECT_DIR),
            trusted,
        }
    }

    /// What the owner is asked about a write: where, whether it replaces a file or hides the owner's own of that name,
    /// and the whole content — a hook is a command that runs from then on.
    fn ask_text(&self, scope: Scope, kind: Kind, name: &str, content: &str) -> String {
        let path = self.base(scope).join(relative(kind, name));
        let mut head = format!("write {}", path.display());
        head.push_str(if path.symlink_metadata().is_ok() {
            " — replaces the file there"
        } else {
            " — a new file"
        });
        if scope == Scope::Project
            && kind != Kind::Hooks
            && self.owner.join(relative(kind, name)).exists()
        {
            head.push_str(&format!(" — hides your own {name} in this project"));
        }
        let shown: Vec<String> = content.lines().map(crate::permissions::printable).collect();
        format!("{head}:\n{}", shown.join("\n"))
    }

    fn base(&self, scope: Scope) -> &Path {
        match scope {
            Scope::Owner => &self.owner,
            Scope::Project => &self.project,
        }
    }
}

/// The file of `kind` named `name`, relative to its `.glut` folder.
fn relative(kind: Kind, name: &str) -> String {
    match kind {
        Kind::Skill => format!("skills/{name}/SKILL.md"),
        Kind::Agent => format!("agents/{name}.md"),
        Kind::Hooks => crate::permissions::settings::HOOKS_FILE.to_owned(),
    }
}

impl Tool for GlutFiles {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: NAME.to_owned(),
            description: "Glut's own files of a skill, an agent or the hooks — the owner's (scope owner, every \
                          project) or this project's (scope project) — which the file tools cannot reach. list \
                          names what there is, read gives one file, write keeps one whole: a skill \
                          (skills/<name>/SKILL.md), an agent (agents/<name>.md) or the owner's hooks.json. A write \
                          is checked the way Glut reads it and asked of the owner every time; show the owner the \
                          file, or what changes in it, before you write."
                .to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": { "type": "string", "enum": ["list", "read", "write"] },
                    "scope": { "type": "string", "enum": ["owner", "project"] },
                    "kind": { "type": "string", "enum": ["skill", "agent", "hooks"] },
                    "name": { "type": "string", "description": "The skill's or agent's name; none for hooks." },
                    "content": { "type": "string", "description": "The whole file, to write." }
                },
                "required": ["action", "scope"]
            }),
        }
    }

    fn access(&self, arguments: &Value) -> Access {
        match Action::of(arguments) {
            // The owner's hook commands may hold a token: what goes to the model provider is the owner's word.
            Ok(Action::Read(Scope::Owner, Kind::Hooks, _)) => Access::GlutFile {
                change: format!(
                    "read {} — its commands go to the model",
                    self.owner.join(relative(Kind::Hooks, "")).display()
                ),
            },
            Ok(Action::Write(scope, kind, name, content)) => Access::GlutFile {
                change: self.ask_text(scope, kind, &name, &content),
            },
            // A call it cannot read fails before it touches anything: nothing to ask.
            Ok(Action::List(_) | Action::Read(..)) | Err(_) => Access::Own,
        }
    }

    fn offered_as(&self) -> Access {
        Access::Other
    }

    fn replay_safe(&self, arguments: &Value) -> bool {
        matches!(
            Action::of(arguments),
            Ok(Action::List(_) | Action::Read(..))
        )
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        let (owner, project, trusted) = (self.owner.clone(), self.project.clone(), self.trusted);
        Box::pin(async move {
            let action = Action::of(&arguments)?;
            let project_scope = matches!(
                &action,
                Action::List(Scope::Project)
                    | Action::Read(Scope::Project, ..)
                    | Action::Write(Scope::Project, ..)
            );
            // An untrusted folder's `.glut` is read by nobody: nothing to show from it, nothing to plant in it.
            if project_scope && !trusted {
                return Err("this folder is not trusted: its .glut is neither read nor written — use scope owner, \
                            or the owner trusts the folder first"
                    .into());
            }
            tokio::task::spawn_blocking(move || {
                let base = |scope| match scope {
                    Scope::Owner => owner.as_path(),
                    Scope::Project => project.as_path(),
                };
                match action {
                    Action::List(scope) => list(base(scope)),
                    Action::Read(scope, kind, name) => read(base(scope), kind, &name),
                    Action::Write(scope, kind, name, content) => {
                        write(base(scope), kind, &name, &content)?;
                        let mut done = format!(
                            "wrote {}",
                            base(scope).join(relative(kind, &name)).display()
                        );
                        done.push_str("\nIt counts from the next session on.");
                        Ok(done)
                    }
                }
            })
            .await
            .map_err(|err| format!("the files could not be reached: {err}"))?
        })
    }
}

/// The skills, the agents and whether there are hooks, of one `.glut` folder.
fn list(base: &Path) -> Result<String, String> {
    let Some(root) = existing_root(base, LinkPolicy::Contained)? else {
        return Ok("nothing there yet".into());
    };
    let names = |folder: &str, want: EntryKind, suffix: &str| -> Vec<String> {
        list_dir(&root, folder)
            .map(|listing| {
                listing
                    .entries
                    .into_iter()
                    .filter(|entry| entry.kind == want || entry.kind == EntryKind::Link)
                    .filter_map(|entry| entry.name.strip_suffix(suffix).map(str::to_owned))
                    .filter(|name| crate::skills::is_skill_name(name))
                    .collect()
            })
            .unwrap_or_default()
    };
    let skills = names("skills", EntryKind::Dir, "");
    let agents = names("agents", EntryKind::File, ".md");
    let hooks = read_text(
        &root,
        crate::permissions::settings::HOOKS_FILE,
        MAX_FILE_BYTES,
    )
    .is_ok();
    let shown = |list: &[String]| {
        if list.is_empty() {
            "none".to_owned()
        } else {
            list.join(", ")
        }
    };
    Ok(format!(
        "skills: {}\nagents: {}\nhooks.json: {}",
        shown(&skills),
        shown(&agents),
        if hooks { "there" } else { "none" }
    ))
}

/// One file's text.
fn read(base: &Path, kind: Kind, name: &str) -> Result<String, String> {
    let rel = relative(kind, name);
    let Some(root) = existing_root(base, LinkPolicy::Contained)? else {
        return Err(format!("no {rel}: nothing there yet"));
    };
    match read_text(&root, &rel, MAX_FILE_BYTES) {
        Ok(file) => Ok(file.content),
        Err(FilesError::NotFound { .. }) => Err(format!("no {rel}")),
        Err(err) => Err(err.to_string()),
    }
}

/// Writes one file whole, once it reads the way Glut will read it; its folders made as needed, none through a link.
fn write(base: &Path, kind: Kind, name: &str, content: &str) -> Result<(), String> {
    check(kind, name, content)?;
    if content.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("at most {MAX_FILE_BYTES} bytes: keep it short"));
    }
    if existing_root(base, LinkPolicy::Strict)?.is_none() {
        // Glut's folder is the owner's alone, as `Trust` makes it.
        use std::os::unix::fs::DirBuilderExt as _;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(base)
            .map_err(|err| format!("{} could not be made: {err}", base.display()))?;
    }
    // Strict: a written file is never one a link stands for, not even inside `.glut`.
    let root = existing_root(base, LinkPolicy::Strict)?
        .ok_or_else(|| format!("{} could not be made", base.display()))?;
    let rel = relative(kind, name);
    let mut folder = String::new();
    for part in rel
        .split('/')
        .collect::<Vec<_>>()
        .split_last()
        .map(|(_, dirs)| dirs)
        .unwrap_or_default()
    {
        if !folder.is_empty() {
            folder.push('/');
        }
        folder.push_str(part);
        match std::fs::symlink_metadata(base.join(&folder)) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(format!(
                    "{} is no plain folder",
                    base.join(&folder).display()
                ));
            }
            Err(_) => make_dir(&root, &folder).map_err(|err| err.to_string())?,
        }
    }
    // The hooks a write replaces are kept beside it: one wrong answer must not lose the owner's own.
    if kind == Kind::Hooks
        && let Ok(old) = read_text(&root, &rel, MAX_FILE_BYTES)
    {
        write_text(
            &root,
            &format!("{rel}.bak"),
            &old.content,
            None,
            MAX_FILE_BYTES,
        )
        .map_err(|err| err.to_string())?;
    }
    write_text(&root, &rel, content, None, MAX_FILE_BYTES).map_err(|err| err.to_string())?;
    Ok(())
}

/// Whether `content` is a file Glut would read: a skill and an agent with their frontmatter, hooks that compile.
fn check(kind: Kind, name: &str, content: &str) -> Result<(), String> {
    let refused = |why: String| format!("not written — Glut would leave it out: {why}");
    match kind {
        Kind::Skill => crate::skills::Skill::check(name, content).map_err(refused),
        Kind::Agent => crate::agents::Agent::parse(name, content)
            .map(|_| ())
            .map_err(refused),
        Kind::Hooks => crate::permissions::settings::check_hooks(content).map_err(refused),
    }
}

/// The `.glut` folder as a root, when it is there and a folder of its own; a link is refused.
fn existing_root(base: &Path, policy: LinkPolicy) -> Result<Option<Root>, String> {
    match std::fs::symlink_metadata(base) {
        Err(_) => Ok(None),
        Ok(meta) if meta.is_dir() => Root::dir(base, policy)
            .map(Some)
            .map_err(|err| err.to_string()),
        Ok(_) => Err(format!("{} is no plain folder", base.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("glut-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (home, cwd) = (base.join("home"), base.join("cwd"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cwd).unwrap();
        (home, cwd)
    }

    fn call(tool: &GlutFiles, arguments: Value) -> Result<String, String> {
        let action = Action::of(&arguments)?;
        match action {
            Action::List(scope) => list(tool.base(scope)),
            Action::Read(scope, kind, name) => read(tool.base(scope), kind, &name),
            Action::Write(scope, kind, name, content) => {
                write(tool.base(scope), kind, &name, &content).map(|()| "ok".into())
            }
        }
    }

    const AGENT: &str = "---\nname: reviewer\ndescription: Reviews.\n---\nReview it.";

    #[test]
    fn it_writes_an_agent_a_skill_and_hooks_and_reads_them_back() {
        let (home, cwd) = scratch("write");
        let tool = GlutFiles::new(&home, &cwd, true);
        let write = |scope: &str, kind: &str, name: &str, content: &str| {
            call(
                &tool,
                json!({"action": "write", "scope": scope, "kind": kind, "name": name, "content": content}),
            )
        };
        write("project", "agent", "reviewer", AGENT).unwrap();
        assert_eq!(
            std::fs::read_to_string(cwd.join(".glut/agents/reviewer.md")).unwrap(),
            AGENT
        );
        write(
            "owner",
            "skill",
            "greet",
            "---\nname: greet\ndescription: Says hello.\n---\nSay hello.",
        )
        .unwrap();
        assert!(home.join(".glut/skills/greet/SKILL.md").is_file());
        write(
            "owner",
            "hooks",
            "",
            r#"{"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "true"}]}]}}"#,
        )
        .unwrap();
        let listed = call(&tool, json!({"action": "list", "scope": "owner"})).unwrap();
        assert_eq!(listed, "skills: greet\nagents: none\nhooks.json: there");
        let read = call(
            &tool,
            json!({"action": "read", "scope": "project", "kind": "agent", "name": "reviewer"}),
        );
        assert_eq!(read.unwrap(), AGENT);
    }

    #[test]
    fn what_glut_would_leave_out_paths_and_a_projects_hooks_are_refused() {
        let (home, cwd) = scratch("refuse");
        let tool = GlutFiles::new(&home, &cwd, true);
        let write = |scope: &str, kind: &str, name: &str, content: &str| {
            call(
                &tool,
                json!({"action": "write", "scope": scope, "kind": kind, "name": name, "content": content}),
            )
        };
        assert!(
            write("owner", "agent", "other", AGENT)
                .unwrap_err()
                .contains("would leave it out")
        );
        assert!(
            write("owner", "agent", "../x", AGENT)
                .unwrap_err()
                .contains("never a path")
        );
        assert!(
            write("project", "hooks", "", "{}")
                .unwrap_err()
                .contains("owner's only")
        );
        assert!(
            write(
                "owner",
                "hooks",
                "",
                r#"{"hooks": {"Stop": [{"hooks": [{"type": "x", "command": "y"}]}]}}"#
            )
            .unwrap_err()
            .contains("would leave it out")
        );
        // A linked folder is no way out of `.glut`.
        std::fs::create_dir_all(cwd.join(".glut")).unwrap();
        std::os::unix::fs::symlink(&home, cwd.join(".glut/agents")).unwrap();
        assert!(
            write("project", "agent", "reviewer", AGENT)
                .unwrap_err()
                .contains("no plain folder")
        );
        assert!(!home.join("reviewer.md").exists());
    }

    #[test]
    fn a_write_is_asked_with_its_content_and_reading_the_owners_hooks_is_asked_too() {
        let (home, cwd) = scratch("ask");
        std::fs::create_dir_all(home.join(".glut/agents")).unwrap();
        std::fs::write(home.join(".glut/agents/a.md"), AGENT).unwrap();
        let tool = GlutFiles::new(&home, &cwd, true);
        let write = tool.access(
            &json!({"action": "write", "scope": "project", "kind": "agent", "name": "a",
            "content": "---\nname: a\n---\nDo \u{1b}[31mit"}),
        );
        let Access::GlutFile { change } = write else {
            panic!("{write:?}")
        };
        assert!(
            change.contains("a new file — hides your own a in this project"),
            "{change}"
        );
        assert!(
            change.ends_with("\n---\nname: a\n---\nDo  [31mit"),
            "the whole content, made printable: {change}"
        );
        assert!(matches!(
            tool.access(&json!({"action": "read", "scope": "owner", "kind": "hooks"})),
            Access::GlutFile { .. }
        ));
        assert_eq!(
            tool.access(&json!({"action": "list", "scope": "owner"})),
            Access::Own
        );
        assert_eq!(
            tool.access(&json!({"action": "nonsense"})),
            Access::Own,
            "a call it cannot read asks nothing"
        );
    }

    #[test]
    fn a_hooks_write_keeps_the_hooks_it_replaces() {
        let (home, cwd) = scratch("bak");
        let tool = GlutFiles::new(&home, &cwd, true);
        let hooks = |command: &str| {
            format!(
                r#"{{"hooks": {{"Stop": [{{"hooks": [{{"type": "command", "command": "{command}"}}]}}]}}}}"#
            )
        };
        let write = |content: String| {
            call(
                &tool,
                json!({"action": "write", "scope": "owner", "kind": "hooks", "content": content}),
            )
        };
        write(hooks("first")).unwrap();
        write(hooks("second")).unwrap();
        assert_eq!(
            std::fs::read_to_string(home.join(".glut/hooks.json.bak")).unwrap(),
            hooks("first")
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".glut/hooks.json")).unwrap(),
            hooks("second")
        );
    }

    #[tokio::test]
    async fn an_untrusted_folders_glut_is_neither_read_nor_written() {
        let (home, cwd) = scratch("untrusted");
        let tool = GlutFiles::new(&home, &cwd, false);
        let refused = tool
            .call(
                json!({"action": "write", "scope": "project", "kind": "agent", "name": "reviewer", "content": AGENT}),
                CallContext::default(),
            )
            .await
            .unwrap_err();
        assert!(refused.contains("not trusted"), "{refused}");
        assert!(!cwd.join(".glut").exists());
    }
}
