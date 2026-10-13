//! Slash commands (`docs/plans/harness.md`, HS4, C1–C6): the Markdown files below `.axh/commands/`, the built-ins
//! answered without a model, and turning a `/name rest of line` into the prompt a run starts with.
//!
//! The files are read through `axiomata_files::Root` rooted at the `commands` folder itself, with
//! [`LinkPolicy::Contained`] (C1): a link inside it cannot lead anywhere else, and a path that climbs out is refused
//! there rather than here. A project's files count only where the folder is trusted — the same trust its allow rules
//! need (`permissions::settings`, P5).

pub mod builtin;

use std::path::{Path, PathBuf};

use axiomata_files::{EntryKind, LinkPolicy, Root, list_dir, read_text};

use crate::error::HarnessError;
use crate::permissions::settings::{self, SandboxSettings};
use crate::permissions::{Gate, Rule};

/// The most command files one run reads (C1): a folder with more of them is no command folder, and walking it would
/// hold up every start.
const MAX_FILES: usize = 200;
/// The most bytes one command file may hold; a bigger one is refused rather than read in part.
const MAX_FILE_BYTES: u64 = 64 * 1024;
/// The most folders below `commands/` a command sits in (C1): `a/b/c/x.md` counts, `a/b/c/d/x.md` does not.
const MAX_DEPTH: usize = 3;
/// The extension a file must have to be a command.
const EXTENSION: &str = ".md";
/// The folder below `.axh/` the command files live in.
const COMMANDS_DIR: &str = "commands";

/// Where a command comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Written in Rust in [`builtin`]: always there, and always before a file's command of the same name (C5).
    Builtin,
    /// `~/.axh/commands/**/*.md`.
    User,
    /// `<project>/.axh/commands/**/*.md`, and only in a folder the owner trusted once (P5).
    Project,
}

impl Source {
    /// The word `/help` shows for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Builtin => "built-in",
            Source::User => "user",
            Source::Project => "project",
        }
    }
}

/// One command: a built-in's, or one read from a Markdown file.
#[derive(Debug, Clone)]
pub struct Command {
    /// The name as it is typed after the `/`: the file's stem, with a folder as a prefix (`git/commit.md` is
    /// `git:commit`). Each segment is lower case and `[a-z0-9_-]`.
    pub name: String,
    pub source: Source,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    /// The `allowed-tools` frontmatter, parsed (C3): a rule that does not parse made the file an error.
    pub allowed_tools: Vec<Rule>,
    pub model: Option<String>,
    /// The text after the frontmatter: what a run started with this command is prompted.
    pub body: String,
}

/// What a `/name …` line became.
#[derive(Debug)]
pub enum Expanded {
    /// The command's body with its arguments put in: the prompt to run, the tools its `allowed-tools` rules name —
    /// which only narrow the run's own rules (C3) — and the model it asks for, if it names one.
    Prompt {
        name: String,
        prompt: String,
        allowed_tools: Vec<Rule>,
        model: Option<String>,
    },
    /// A built-in, answered without a model (C5): its text is for the front end, not for the model.
    Builtin { name: String, text: String },
}

/// What `/permissions` shows (C5): the run's mode and rules as written, and what the settings say about the sandbox.
#[derive(Debug, Clone, Default)]
pub struct PermissionsView {
    /// The run's mode, spelled the way the flags and settings spell it.
    pub mode: String,
    /// The rules as written, in the order allow / ask / deny.
    pub allow: Vec<String>,
    pub ask: Vec<String>,
    pub deny: Vec<String>,
    /// What the settings say about the sandbox.
    pub sandbox: SandboxSettings,
}

impl PermissionsView {
    /// The view of `gate`: its mode and its rules, with the sandbox the settings built it alongside (HS3b).
    pub fn of(gate: &Gate, sandbox: &SandboxSettings) -> Self {
        let rules = gate.rules();
        let texts = |list: &[Rule]| -> Vec<String> {
            list.iter().map(|rule| rule.text().to_owned()).collect()
        };
        PermissionsView {
            mode: gate.mode_name().to_owned(),
            allow: texts(&rules.allow),
            ask: texts(&rules.ask),
            deny: texts(&rules.deny),
            sandbox: sandbox.clone(),
        }
    }
}

/// Every command a run knows: built-ins first, then the files.
#[derive(Debug)]
pub struct Registry {
    commands: Vec<Command>,
    /// What `/permissions` shows; the caller puts the gate's view in right after loading.
    permissions: PermissionsView,
}

impl Registry {
    /// The built-ins, then the command files below `home` (source [`Source::User`]) and — only where `trusted`, the
    /// same trust the project's own allow rules need — below `project` (source [`Source::Project`]).
    ///
    /// A built-in always wins; between files a project command of a name the owner already has wins, as in Claude
    /// Code (C1).
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] for a folder or file that cannot be used: more files than [`MAX_FILES`], a file
    /// larger than [`MAX_FILE_BYTES`], a folder deeper than [`MAX_DEPTH`], a name that is no command name,
    /// frontmatter or a rule that does not parse, or a link that leads out of the `commands` folder. A broken
    /// command stops the run rather than being skipped: a command file is the harness's own configuration (P4).
    pub fn load(home: &Path, project: &Path, trusted: bool) -> Result<Registry, HarnessError> {
        let mut registry = Registry {
            commands: builtin::builtins(),
            permissions: PermissionsView::default(),
        };
        let mut files = 0usize;
        registry.read(
            home.join(settings::USER_DIR).join(COMMANDS_DIR),
            Source::User,
            &mut files,
        )?;
        if trusted {
            registry.read(
                project.join(settings::PROJECT_DIR).join(COMMANDS_DIR),
                Source::Project,
                &mut files,
            )?;
        }
        Ok(registry)
    }

    /// The commands of the run, built-ins first — what `/help` lists.
    pub fn commands(&self) -> &[Command] {
        &self.commands
    }

    /// What `/permissions` shows; set by the caller as soon as the gate is built.
    pub fn permissions_view(&self) -> &PermissionsView {
        &self.permissions
    }

    /// Puts what `/permissions` shows into the registry.
    pub fn set_permissions(&mut self, view: PermissionsView) {
        self.permissions = view;
    }

    /// Where the command `name` comes from — for the `command` event (C6).
    pub fn source(&self, name: &str) -> Option<Source> {
        self.commands
            .iter()
            .find(|command| command.name == name)
            .map(|command| command.source)
    }

    /// A `/name …` line as the run should see it: [`None`] for an ordinary prompt (C6), an error for a line the
    /// harness cannot make a prompt of — an unknown name, or a `!` command line it does not run yet (C4).
    ///
    /// An `@path` in the body is left as it stands: HS4 does not read the file (C4); the model can call `read_file`.
    /// Nothing but `$ARGUMENTS` and `$1`, `$2`, … is substituted.
    pub fn expand(&self, input: &str) -> Option<Result<Expanded, String>> {
        if !input.starts_with('/') {
            return None;
        }
        let line = &input[1..];
        let (name, arguments) = match line.find(char::is_whitespace) {
            Some(at) => (&line[..at], line[at..].trim()),
            None => (line, ""),
        };
        let Some(command) = self.commands.iter().find(|command| command.name == name) else {
            return Some(Err(format!("unknown command /{name}; /help lists them")));
        };
        if command.source == Source::Builtin {
            let Some(text) = builtin::text(&command.name, self) else {
                return Some(Err(format!(
                    "the built-in /{} answers nothing",
                    command.name
                )));
            };
            return Some(Ok(Expanded::Builtin {
                name: command.name.clone(),
                text,
            }));
        }
        // C4: `!` lines would need the gate and the sandbox, so they come with a later step — saying so beats
        // silently sending their text to the model as if it were the task.
        if command.body.lines().any(|line| line.starts_with('!')) {
            return Some(Err(format!(
                "/{name} uses !command lines, which the harness does not run yet"
            )));
        }
        Some(Ok(Expanded::Prompt {
            name: command.name.clone(),
            prompt: substitute(&command.body, arguments),
            allowed_tools: command.allowed_tools.clone(),
            model: command.model.clone(),
        }))
    }

    /// Reads every command file below `dir` into the registry; `files` counts them across both sources.
    fn read(
        &mut self,
        dir: PathBuf,
        source: Source,
        files: &mut usize,
    ) -> Result<(), HarnessError> {
        match std::fs::symlink_metadata(&dir) {
            // No command folder is the ordinary case: the built-ins are there all the same.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(refuse(&dir, err.to_string())),
            Ok(meta) if meta.is_dir() => {}
            // A `commands` that is a link would make the harness read anywhere on disk (C1).
            Ok(_) => {
                return Err(refuse(
                    &dir,
                    "not a real folder: a link is refused here".into(),
                ));
            }
        }
        let root =
            Root::dir(&dir, LinkPolicy::Contained).map_err(|err| refuse(&dir, err.to_string()))?;
        self.walk(&root, "", &[], source, files)
    }

    /// Every command file in `folder` (`""` is the root of the walk) and below it; `segments` are the names it adds.
    fn walk(
        &mut self,
        root: &Root,
        folder: &str,
        segments: &[String],
        source: Source,
        files: &mut usize,
    ) -> Result<(), HarnessError> {
        let listing = list_dir(root, folder)
            .map_err(|err| refuse(&root.path().join(folder), err.to_string()))?;
        for entry in listing.entries {
            let path = if folder.is_empty() {
                entry.name.clone()
            } else {
                format!("{folder}/{}", entry.name)
            };
            let full = root.path().join(&path);
            if entry.kind == EntryKind::Dir {
                if segments.len() + 1 > MAX_DEPTH {
                    return Err(refuse(
                        &full,
                        format!("deeper than {MAX_DEPTH} folders below the command folder"),
                    ));
                }
                let mut deeper = segments.to_vec();
                deeper.push(segment(&entry.name, &full)?);
                self.walk(root, &path, &deeper, source, files)?;
                continue;
            }
            if !entry.name.ends_with(EXTENSION) {
                continue;
            }
            *files += 1;
            if *files > MAX_FILES {
                return Err(refuse(
                    &full,
                    format!("more than {MAX_FILES} command files"),
                ));
            }
            // A symlink and a FIFO reach `read_text` too: one that leads out of the folder (or is no file at all)
            // is refused there, and that refusal stops the run rather than dropping a command silently.
            let text = read_text(root, &path, MAX_FILE_BYTES)
                .map_err(|err| refuse(&full, err.to_string()))?;
            let stem = entry.name.strip_suffix(EXTENSION).unwrap_or(&entry.name);
            let mut parts = segments.to_vec();
            parts.push(segment(stem, &full)?);
            let command = Command::read(parts.join(":"), source, &text.content, &full)?;
            self.put(command);
        }
        Ok(())
    }

    /// Adds a file's command (C1): a name a built-in holds is skipped — shadowed, no error — and the project's
    /// command of a name the owner already has takes its place, as in Claude Code. Two files of one source: the
    /// later one read wins.
    fn put(&mut self, command: Command) {
        if builtin::holds(&command.name) {
            return;
        }
        match self
            .commands
            .iter_mut()
            .find(|known| known.name == command.name)
        {
            Some(known) if known.source == Source::Project && command.source == Source::User => {}
            Some(known) => *known = command,
            None => self.commands.push(command),
        }
    }
}

impl Command {
    /// One command file: its name and source from where it lies, its four frontmatter fields and its body from its
    /// text (C2).
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] for a `---` block that never closes, a line that is no `key: value`, an unknown
    /// key (the same spirit as the settings files' `deny_unknown_fields`), or a rule that does not parse.
    fn read(name: String, source: Source, text: &str, path: &Path) -> Result<Self, HarnessError> {
        let (block, body) = match head_and_body(text).map_err(|reason| refuse(path, reason))? {
            None => ("", text),
            Some((block, body)) => (block, body),
        };
        let head = Head::parse(block, path)?;
        Ok(Command {
            name,
            source,
            description: head.description,
            argument_hint: head.argument_hint,
            allowed_tools: head.allowed_tools,
            model: head.model,
            body: body.to_owned(),
        })
    }
}

/// What a file's frontmatter says.
#[derive(Debug, Default)]
struct Head {
    description: Option<String>,
    argument_hint: Option<String>,
    allowed_tools: Vec<Rule>,
    model: Option<String>,
}

impl Head {
    /// The `key: value` lines of a frontmatter block (C2). Unknown keys are refused, the same way the settings
    /// files refuse an unknown field: a typo must not silently drop a rule list.
    fn parse(block: &str, path: &Path) -> Result<Head, HarnessError> {
        let mut head = Head::default();
        for raw in block.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                return Err(refuse(path, format!("{line:?} is no `key: value` line")));
            };
            let value = unquote(value.trim());
            match key.trim() {
                "description" => head.description = some(value),
                "argument-hint" => head.argument_hint = some(value),
                "model" => head.model = some(value),
                "allowed-tools" => head.allowed_tools = parse_rules(value, path)?,
                other => return Err(refuse(path, format!("unknown frontmatter key {other:?}"))),
            }
        }
        Ok(head)
    }
}

/// The frontmatter block of `text` and the body after its closing `---` line. [`None`] when the file does not open
/// with a `---` line (then the whole file is the body), an error when an opened block never closes.
fn head_and_body(text: &str) -> Result<Option<(&str, &str)>, String> {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return Ok(None);
    };
    if line_of(first) != "---" {
        return Ok(None);
    }
    // The offset of the line being looked at; the block ends where that line starts.
    let mut at = first.len();
    for line in lines {
        if line_of(line) == "---" {
            let after = &text[at..];
            let rest = after.strip_prefix("---").unwrap_or(after);
            let body = rest
                .strip_prefix("\r\n")
                .or_else(|| rest.strip_prefix('\n'))
                .unwrap_or(rest);
            return Ok(Some((&text[first.len()..at], body)));
        }
        at += line.len();
    }
    Err("a `---` frontmatter that never closes".into())
}

/// A line without its `\n` and without a trailing `\r`: a file written on Windows reads the same.
fn line_of(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// A value wrapped in one pair of `"`: the quotes are for the file, not for the value (C2).
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(value)
}

/// An empty value is no value: a `model:` line without a model names none.
fn some(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    }
}

/// The `allowed-tools` value: comma-separated rules, each parsed with [`Rule::parse`] (C3). A rule that does not
/// parse makes the whole file an error — a narrowed run must know exactly what it may do.
fn parse_rules(value: &str, path: &Path) -> Result<Vec<Rule>, HarnessError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    value
        .split(',')
        .map(|text| {
            Rule::parse(text).map_err(|reason| refuse(path, format!("allowed-tools: {reason}")))
        })
        .collect()
}

/// A file or folder name as one command-name segment (C1): lower case, `[a-z0-9_-]` — a name that does not survive
/// this makes the file an error, never a silently different command.
fn segment(name: &str, path: &Path) -> Result<String, HarnessError> {
    let lower = name.to_lowercase();
    let known = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-';
    if lower.is_empty() || !lower.chars().all(known) {
        return Err(refuse(
            path,
            format!("{name:?} is no command name: a name holds a-z, 0-9, _ and -, in lower case"),
        ));
    }
    Ok(lower)
}

/// The error a command file that cannot be used gives.
fn refuse(path: &Path, reason: String) -> HarnessError {
    HarnessError::Settings {
        path: path.display().to_string(),
        reason,
    }
}

/// `$ARGUMENTS` (the whole rest, trimmed) and `$1`, `$2`, … (its whitespace-separated words) put into the body; a
/// missing one becomes empty, and nothing else is substituted (C2) — `$HOME` stays `$HOME`.
fn substitute(body: &str, arguments: &str) -> String {
    let words: Vec<&str> = arguments.split_whitespace().collect();
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find('$') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        if let Some(after) = tail.strip_prefix("ARGUMENTS") {
            out.push_str(arguments);
            rest = after;
            continue;
        }
        let digits = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        match tail[..digits].parse::<usize>() {
            // `$0` is no position of the argument list: it is left as it stands.
            Ok(n) if n >= 1 => {
                out.push_str(words.get(n - 1).copied().unwrap_or_default());
                rest = &tail[digits..];
            }
            _ => {
                out.push('$');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-commands-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// An owner home and a project folder, with the owner's command files below `~/.axh/commands/`.
    fn setup(name: &str, files: &[(&str, &str)]) -> (PathBuf, PathBuf) {
        let (home, project) = (
            temp(&format!("{name}-home")),
            temp(&format!("{name}-project")),
        );
        for (rel, text) in files {
            write(&home.join(".axh/commands").join(rel), text);
        }
        (home, project)
    }

    /// The file commands of a registry, after the built-ins, by name: the order they were read in is the file
    /// system's.
    fn file_names(registry: &Registry) -> Vec<String> {
        let mut names: Vec<String> = registry
            .commands()
            .iter()
            .skip(builtin::builtins().len())
            .map(|command| command.name.clone())
            .collect();
        names.sort();
        names
    }

    /// The prompt of a file command, or what went wrong with it.
    fn prompt(registry: &Registry, input: &str) -> Result<String, String> {
        match registry.expand(input) {
            Some(Ok(Expanded::Prompt { prompt, .. })) => Ok(prompt),
            Some(Ok(Expanded::Builtin { .. })) => Err("a built-in, not a prompt".into()),
            Some(Err(reason)) => Err(reason),
            None => Err("no command at all".into()),
        }
    }

    /// What a command line was refused with; panics when it was no command at all.
    fn problem(registry: &Registry, input: &str) -> String {
        match registry.expand(input) {
            Some(Err(reason)) => reason,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    #[test]
    fn a_file_without_frontmatter_is_a_whole_command() {
        let (home, project) = setup("plain", &[("review.md", "Review what $ARGUMENTS names.\n")]);
        let registry = Registry::load(&home, &project, false).unwrap();
        let command = &registry.commands()[builtin::builtins().len()];
        assert_eq!(command.name, "review");
        assert_eq!(command.source, Source::User);
        assert_eq!(command.description, None);
        assert_eq!(command.argument_hint, None);
        assert_eq!(command.model, None);
        assert!(command.allowed_tools.is_empty());
        assert_eq!(command.body, "Review what $ARGUMENTS names.\n");
    }

    #[test]
    fn all_four_frontmatter_keys_are_read_and_quoted_values_lose_their_quotes() {
        let (home, project) = setup(
            "frontmatter",
            &[(
                "git/commit.md",
                "---\n\
                 description: \"Reviews a commit\"\n\
                 argument-hint: \"<sha>\"\n\
                 allowed-tools: Bash(git status:*), Read\n\
                 model: claude-haiku\n\
                 ---\n\
                 Body\n",
            )],
        );
        let registry = Registry::load(&home, &project, false).unwrap();
        let command = &registry.commands()[builtin::builtins().len()];
        assert_eq!(command.name, "git:commit");
        assert_eq!(command.description.as_deref(), Some("Reviews a commit"));
        assert_eq!(command.argument_hint.as_deref(), Some("<sha>"));
        assert_eq!(command.model.as_deref(), Some("claude-haiku"));
        assert_eq!(
            command
                .allowed_tools
                .iter()
                .map(|rule| rule.text())
                .collect::<Vec<_>>(),
            ["Bash(git status:*)", "Read"]
        );
        assert_eq!(command.body, "Body\n");
    }

    #[test]
    fn an_unknown_key_or_a_rule_that_does_not_parse_stops_the_load() {
        for (name, text) in [
            ("unknown.md", "---\ntoolz: Read\n---\nbody"),
            ("rules.md", "---\nallowed-tools: Bash(git status\n---\nbody"),
            ("open.md", "---\ndescription: never closed\nbody"),
            ("nocolon.md", "---\njust a line\n---\nbody"),
        ] {
            let (home, project) = setup("broken", &[(name, text)]);
            let error = Registry::load(&home, &project, false).unwrap_err();
            let HarnessError::Settings { path, .. } = error else {
                panic!("{name} should stop the load");
            };
            assert!(path.ends_with(name), "{path}");
        }
    }

    #[test]
    fn names_come_from_folders_and_stems_and_a_name_that_is_no_name_stops_the_load() {
        let (home, project) = setup(
            "names",
            &[
                ("review.md", "a"),
                ("git/commit.md", "b"),
                ("a/b/c/deep.md", "c"),
                ("no-command.txt", "not a command"),
            ],
        );
        let registry = Registry::load(&home, &project, false).unwrap();
        assert_eq!(
            file_names(&registry),
            ["a:b:c:deep", "git:commit", "review"]
        );

        let (home, project) = setup("badname", &[("Bad Name.md", "a")]);
        assert!(Registry::load(&home, &project, false).is_err());

        // Three folders below the command folder still count; a fourth one does not (C1).
        let (home, project) = setup("toodeep", &[("a/b/c/d/x.md", "a")]);
        let error = Registry::load(&home, &project, false).unwrap_err();
        assert!(
            matches!(&error, HarnessError::Settings { reason, .. } if reason.contains("deeper than 3")),
            "{error}"
        );
    }

    #[test]
    fn a_built_in_beats_the_project_and_the_project_beats_the_owner() {
        let (home, project) = setup(
            "precedence",
            &[
                ("review.md", "owner"),
                ("only-user.md", "owner"),
                ("help.md", "a file that shall never answer"),
            ],
        );
        write(&project.join(".axh/commands/review.md"), "project");
        write(&project.join(".axh/commands/only-project.md"), "project");

        let trusted = Registry::load(&home, &project, true).unwrap();
        assert_eq!(prompt(&trusted, "/review").unwrap(), "project");
        assert_eq!(prompt(&trusted, "/only-user").unwrap(), "owner");
        assert_eq!(prompt(&trusted, "/only-project").unwrap(), "project");
        assert_eq!(trusted.source("review"), Some(Source::Project));
        assert_eq!(trusted.source("only-user"), Some(Source::User));
        assert!(
            matches!(trusted.expand("/help"), Some(Ok(Expanded::Builtin { .. }))),
            "a built-in always wins over a file"
        );

        let untrusted = Registry::load(&home, &project, false).unwrap();
        assert_eq!(prompt(&untrusted, "/review").unwrap(), "owner");
        assert_eq!(untrusted.source("review"), Some(Source::User));
        assert!(
            matches!(untrusted.expand("/only-project"), Some(Err(_))),
            "an untrusted project's files are not loaded at all"
        );
    }

    #[test]
    fn arguments_go_into_the_body_and_nothing_else_is_substituted() {
        let (home, project) = setup(
            "arguments",
            &[(
                "x.md",
                "$ARGUMENTS | $1 by $2 | third: [$3] | $HOME stays\n",
            )],
        );
        let registry = Registry::load(&home, &project, false).unwrap();
        assert_eq!(
            prompt(&registry, "/x alpha beta").unwrap(),
            "alpha beta | alpha by beta | third: [] | $HOME stays\n"
        );
        assert_eq!(
            prompt(&registry, "/x").unwrap(),
            " |  by  | third: [] | $HOME stays\n",
            "an empty rest is an empty argument"
        );
    }

    #[test]
    fn an_unknown_command_and_a_bang_line_are_refused_and_plain_text_is_no_command() {
        let (home, project) = setup(
            "expand",
            &[
                ("bang.md", "!ls\nthen do it\n"),
                ("at.md", "read @notes.md first\n"),
            ],
        );
        let registry = Registry::load(&home, &project, false).unwrap();
        assert!(
            registry.expand("hello").is_none() && registry.expand("").is_none(),
            "text without a slash is no command at all"
        );
        let unknown = problem(&registry, "/nope everything");
        assert!(unknown.contains("unknown command /nope") && unknown.contains("/help"));
        let bang = problem(&registry, "/bang");
        assert!(bang.contains("!command lines"), "{bang}");
        assert_eq!(
            prompt(&registry, "/at").unwrap(),
            "read @notes.md first\n",
            "@file is left in the text: HS4 does not read it (C4)"
        );
    }

    #[test]
    fn too_many_files_too_large_a_file_and_a_link_out_are_refused() {
        let names: Vec<String> = (0..MAX_FILES + 1).map(|i| format!("f{i}.md")).collect();
        let files: Vec<(&str, &str)> = names.iter().map(|name| (name.as_str(), "x")).collect();
        let (home, project) = setup("many", &files);
        let error = Registry::load(&home, &project, false).unwrap_err();
        assert!(
            matches!(&error, HarnessError::Settings { reason, .. } if reason.contains("more than 200")),
            "{error}"
        );

        let (home, project) = setup(
            "large",
            &[("big.md", &"x".repeat((MAX_FILE_BYTES + 1) as usize))],
        );
        let error = Registry::load(&home, &project, false).unwrap_err();
        assert!(
            matches!(&error, HarnessError::Settings { path, .. } if path.ends_with("big.md")),
            "{error}"
        );

        let (home, project) = setup("link", &[("keep.md", "x")]);
        let outside = temp("link-target");
        std::os::unix::fs::symlink(
            outside.join("secret.md"),
            home.join(".axh/commands/escape.md"),
        )
        .unwrap();
        let error = Registry::load(&home, &project, false).unwrap_err();
        assert!(
            matches!(&error, HarnessError::Settings { path, .. } if path.ends_with("escape.md")),
            "{error}"
        );
    }

    #[test]
    fn builtins_answer_without_a_model() {
        let (home, project) = setup(
            "builtins",
            &[(
                "review.md",
                "---\ndescription: Reviews files\nargument-hint: src/\n---\nbody\n",
            )],
        );
        let mut registry = Registry::load(&home, &project, false).unwrap();
        registry.set_permissions(PermissionsView {
            mode: "plan".into(),
            deny: vec!["Bash(rm:*)".into()],
            ..PermissionsView::default()
        });

        let help = registry.expand("/help").unwrap().unwrap();
        let Expanded::Builtin { name, text } = help else {
            panic!("/help is a built-in");
        };
        assert_eq!(name, "help");
        assert!(text.contains("/help (built-in)"), "{text}");
        assert!(
            text.contains("/review (user) [src/] — Reviews files"),
            "{text}"
        );
        assert!(text.contains("/permissions (built-in)"), "{text}");

        let permissions = registry.expand("/permissions").unwrap().unwrap();
        let Expanded::Builtin { text, .. } = permissions else {
            panic!("/permissions is a built-in");
        };
        assert!(text.contains("mode: plan"), "{text}");
        assert!(text.contains("deny:"), "{text}");
        assert!(text.contains("Bash(rm:*)"), "{text}");
        assert!(text.contains("sandbox.enabled:"), "{text}");

        let clear = registry.expand("/clear").unwrap().unwrap();
        let Expanded::Builtin { text, .. } = clear else {
            panic!("/clear is a built-in");
        };
        assert_eq!(text, "/clear is not available in a single run yet");
    }
}
