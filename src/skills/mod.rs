//! Skills (HS6, K1–K6): a folder with a `SKILL.md` — YAML frontmatter the harness reads (K3) and Markdown
//! instructions the run follows — and, in [`tool`], the `skill` tool that lets the model load one while a run goes
//! on (K1.2). A skill the run starts with becomes that run's instructions (K1.1, [`Skill::start_run`]).
//!
//! Skills live in folders (K2): the owner's `~/.axh/skills`, the folders the caller names (`--skill-dir`, in
//! `axiomata-cli` the app's `~/.axiomata/skills`) and, in a trusted folder, the project's `.axh/skills`. Each is a
//! folder of its own with its `SKILL.md` at the top. A skill that cannot be used is left out with a note for
//! stderr — skill files come from other tools too — and never fails the run.

pub mod tool;

pub use tool::SkillTool;

use std::path::{Path, PathBuf};
use std::time::Duration;

use axiomata_files::{EntryKind, FilesError, LinkPolicy, Root, list_dir, read_text};
use gray_matter::Matter;
use gray_matter::engine::YAML;
use serde::Deserialize;
use serde::de::{self, SeqAccess, Visitor};

use crate::config::{ModelEndpoint, RunConfig};
use crate::error::HarnessError;
use crate::permissions::settings;
use crate::permissions::{Gate, Rule};

/// The folder of skills inside `~/.axh/` and a project's `.axh/` (K2).
const SKILLS_DIR: &str = "skills";
/// What a skill run is prompted with when the caller adds no text of its own (K1.1).
pub const DEFAULT_PROMPT: &str = "Run the skill.";
/// The file a skill folder is named by (K1); everything else in it is a reference or a script (K4).
const SKILL_FILE: &str = "SKILL.md";
/// Skills one run knows (K1): past this a folder is no collection of skills, and walking it would hold up every
/// start. One note says so, whatever how many folders push on.
const MAX_SKILLS: usize = 200;
/// Bytes of one `SKILL.md` (K1): a real skill needs a few KiB; a bigger one is a mistake or somebody's data dump.
const MAX_SKILL_BYTES: u64 = 256 * 1024;
/// Bytes of one `prepend_files` file (K5).
const MAX_PREPEND_BYTES: u64 = 256 * 1024;
/// Files a skill folder lists beside its `SKILL.md` (K4).
const MAX_SKILL_FILES: usize = 200;
/// Folders below the skill folder the walk of K4 enters.
const MAX_SKILL_DEPTH: usize = 3;

/// One skill: its frontmatter as the harness reads it (K3) and its Markdown body.
#[derive(Debug, Clone)]
pub struct Skill {
    /// The skill's name — its folder's, and the one the model calls it by (K1.2).
    pub name: String,
    pub description: String,
    /// The skill's folder on disk; the `skill` tool lists the files beside `SKILL.md` and the file tools read
    /// them as `skill:<name>/<rel>` (K4).
    pub dir: PathBuf,
    /// The Markdown below the frontmatter — the run's instructions (K1).
    pub body: String,
    /// The model a skill run uses instead of the caller's (K5).
    pub model: Option<String>,
    /// What a skill run is narrowed to (K5), like a command's `allowed-tools` (HS4, C3).
    pub allowed_tools: Vec<Rule>,
    /// `timeout_secs`: a skill run's time limit, capped by the one it already has (K5).
    pub timeout: Option<Duration>,
    /// Files inlined before the body in a skill run (K5), each relative to the working folder.
    pub prepend_files: Vec<String>,
}

impl Skill {
    /// One `SKILL.md`'s text as the harness reads it (K3): the frontmatter's fields, the body below, every key it
    /// does not know ignored — skill files come from other tools, whose `backend`, `output` and `metadata` the
    /// harness does not act on.
    ///
    /// # Errors
    ///
    /// The reason the skill must be skipped: a missing field, a name that is not the folder's, frontmatter the
    /// YAML parser must not be given. The caller turns it into a note; a skill never fails a run.
    fn parse(folder: &str, dir: PathBuf, raw: &str) -> Result<Skill, String> {
        if !is_skill_name(folder) {
            return Err(format!(
                "its folder name {folder:?} is no skill name (lower case letters, digits and dashes, up to 64)"
            ));
        }
        check_frontmatter_shape(raw)?;
        let parsed = Matter::<YAML>::new()
            .parse::<Frontmatter>(raw)
            .map_err(|err| err.to_string())?;
        let Some(front) = parsed.data else {
            return Err("it has no YAML frontmatter block".into());
        };
        let Some(name) = front.name.filter(|name| !name.is_empty()) else {
            return Err("its frontmatter has no name".into());
        };
        if name != folder {
            return Err(format!(
                "its frontmatter says name {name:?}, not its folder {folder:?}"
            ));
        }
        let Some(description) = front
            .description
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        else {
            return Err("its frontmatter has no description".into());
        };
        Ok(Skill {
            name,
            description: description.to_owned(),
            dir,
            body: parsed.content.trim().to_owned(),
            model: front.model.filter(|text| !text.trim().is_empty()),
            allowed_tools: front.allowed_tools.unwrap_or_default().0,
            timeout: front.timeout_secs.map(Duration::from_secs),
            prepend_files: front
                .prepend_files
                .into_iter()
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
                .collect(),
        })
    }

    /// What a skill run is told (K1.1, K5): every `prepend_files` file in order, each as a `## Context file:`
    /// block relative to the working folder, then the body. Read through a contained root, so a link inside the
    /// working folder cannot lead the run's instructions elsewhere.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Skill`] for a `prepend_files` path that is absolute or climbs out, or a file that cannot
    /// be read; a file that simply is not there is skipped (K5).
    pub fn instructions(&self, cwd: &Path) -> Result<String, HarnessError> {
        let mut out = String::new();
        if !self.prepend_files.is_empty() {
            let root =
                Root::dir(cwd, LinkPolicy::Contained).map_err(|err| HarnessError::Skill {
                    name: self.name.clone(),
                    message: format!(
                        "cannot read its prepended files in {}: {err}",
                        cwd.display()
                    ),
                })?;
            for file in &self.prepend_files {
                if Path::new(file).is_absolute() || file.split('/').any(|part| part == "..") {
                    return Err(HarnessError::Skill {
                        name: self.name.clone(),
                        message: format!(
                            "{file:?} in prepend_files is not a path inside the working folder"
                        ),
                    });
                }
                let rel = file.strip_prefix("./").unwrap_or(file);
                match read_text(&root, rel, MAX_PREPEND_BYTES) {
                    Ok(text) => {
                        out.push_str(&format!(
                            "## Context file: {rel}\n{}\n\n",
                            text.content.trim_end()
                        ));
                    }
                    Err(FilesError::NotFound { .. }) => continue,
                    // A missing parent folder is a missing file too: the guard canonicalises the folder the file
                    // would be in before it can say the file is not there (K5).
                    Err(FilesError::Io { source, .. })
                        if source.kind() == std::io::ErrorKind::NotFound =>
                    {
                        continue;
                    }
                    Err(err) => {
                        return Err(HarnessError::Skill {
                            name: self.name.clone(),
                            message: format!("cannot read its prepended file {rel:?}: {err}"),
                        });
                    }
                }
            }
        }
        out.push_str(&self.body);
        Ok(out)
    }

    /// Everything a skill run takes from the skill (K1.1 + K5): its instructions in the run's config (after the
    /// caller's own, where there are any), its model over the endpoint's, its `allowed_tools` as the gate's
    /// narrowing and its `timeout_secs` as the run's time limit — capped by the one the run already has. The
    /// binaries of a skill run both call this, so the two cannot drift apart.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Skill`] when the instructions cannot be built ([`Skill::instructions`]).
    pub fn start_run(
        &self,
        cwd: &Path,
        config: &mut RunConfig,
        endpoint: &mut ModelEndpoint,
        gate: &mut Gate,
    ) -> Result<(), HarnessError> {
        let instructions = self.instructions(cwd)?;
        config.instructions = Some(match config.instructions.take() {
            Some(earlier) => format!("{earlier}\n\n{instructions}"),
            None => instructions,
        });
        if let Some(model) = &self.model {
            endpoint.model = model.clone();
        }
        // K5: the skill's tools narrow the run the way a command's do (HS4, C3) — here, in a skill run only. A
        // skill loaded through the `skill` tool leaves the rules as they are (Claude Code behaves the same).
        if !self.allowed_tools.is_empty() {
            gate.restrict_name = Some(self.name.clone());
            gate.restrict = Some(self.allowed_tools.clone());
        }
        if let Some(timeout) = self.timeout {
            config.timeout = config.timeout.min(timeout);
        }
        Ok(())
    }

    /// The skill folder's files beside `SKILL.md` (K4), as paths relative to it — the body comes with the skill
    /// itself, so `SKILL.md` is left out. Best effort within the caps: a folder that cannot be walked gives no
    /// files rather than an error, the body is what a skill is.
    pub fn files(&self) -> Vec<String> {
        let Ok(root) = Root::dir(&self.dir, LinkPolicy::Contained) else {
            return Vec::new();
        };
        let mut files = Vec::new();
        walk_files(&root, "", 0, &mut files);
        files.sort();
        files
    }
}

/// Every file below `folder` (empty: the skill folder itself) that the walk's caps allow; no error to report, the
/// caller shows what there is.
fn walk_files(root: &Root, folder: &str, depth: usize, files: &mut Vec<String>) {
    let Ok(listing) = list_dir(root, folder) else {
        return;
    };
    for entry in listing.entries {
        if files.len() >= MAX_SKILL_FILES {
            return;
        }
        let path = if folder.is_empty() {
            entry.name.clone()
        } else {
            format!("{folder}/{}", entry.name)
        };
        match entry.kind {
            // A hidden entry is configuration, not a reference: the file tools refuse a version control's store
            // all the same (`tools::files::PRIVATE_DIRS`), and listing it would only name files that cannot be
            // read. `.DS_Store` and its like are not worth a line either.
            _ if entry.name.starts_with('.') => continue,
            EntryKind::Dir if depth < MAX_SKILL_DEPTH => {
                walk_files(root, &path, depth + 1, files);
            }
            EntryKind::Dir => continue,
            _ if path == SKILL_FILE => continue,
            _ => files.push(path),
        }
    }
}

/// Every skill a run knows (K1.2), read once at start (K2).
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    skills: Vec<Skill>,
}

impl Catalog {
    /// Every skill below `extra_dirs`, `home/.axh/skills` and — where `trusted`, the same trust the project's own
    /// allow rules need — `project/.axh/skills` (K2). On equal names the more specific folder wins: project over
    /// caller folders (later ones over earlier) over the owner's.
    ///
    /// What could not be read comes back as notes for stderr; a broken skill is one of them, never an error (K3).
    pub fn load(
        home: &Path,
        project: &Path,
        trusted: bool,
        extra_dirs: &[PathBuf],
    ) -> (Self, Vec<String>) {
        let mut catalog = Catalog::default();
        let mut notes = Vec::new();
        let mut capped = false;
        catalog.read(
            &home.join(settings::USER_DIR).join(SKILLS_DIR),
            &mut notes,
            &mut capped,
        );
        for dir in extra_dirs {
            catalog.read(dir, &mut notes, &mut capped);
        }
        if trusted {
            catalog.read(
                &project.join(settings::PROJECT_DIR).join(SKILLS_DIR),
                &mut notes,
                &mut capped,
            );
        }
        catalog.skills.sort_by(|a, b| a.name.cmp(&b.name));
        (catalog, notes)
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    /// The skills, by name (K1.2): what the `skill` tool lists.
    pub fn list(&self) -> &[Skill] {
        &self.skills
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    /// Every skill's name.
    pub fn names(&self) -> Vec<String> {
        self.skills.iter().map(|skill| skill.name.clone()).collect()
    }

    /// The skill folders, by name: the read-only `skill:<name>/` roots the file tools get beside the working
    /// folder (K4).
    pub fn folders(&self) -> Vec<(String, PathBuf)> {
        self.skills
            .iter()
            .map(|skill| (skill.name.clone(), skill.dir.clone()))
            .collect()
    }

    /// What an error about an unknown skill says: every skill by name, or that there are none.
    pub fn known(&self) -> String {
        if self.skills.is_empty() {
            "no skills were found".into()
        } else {
            self.names().join(", ")
        }
    }

    /// The skill folders directly below `dir` (K2), each read on its own; `capped` keeps the note about the
    /// skills limit to one (K1).
    fn read(&mut self, dir: &Path, notes: &mut Vec<String>, capped: &mut bool) {
        match std::fs::symlink_metadata(dir) {
            // No skills folder is the ordinary case.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => {
                notes.push(format!("{}: {err}", dir.display()));
                return;
            }
            Ok(meta) if meta.is_dir() => {}
            // A `skills` that is a link would let one folder stand for all of them, wherever they lead (C1's
            // reason for the same refusal in a command folder).
            Ok(_) => {
                notes.push(format!(
                    "{}: not a real folder: a link is refused here",
                    dir.display()
                ));
                return;
            }
        }
        let root = match Root::dir(dir, LinkPolicy::Contained) {
            Ok(root) => root,
            Err(err) => {
                notes.push(format!("{}: {err}", dir.display()));
                return;
            }
        };
        let listing = match list_dir(&root, "") {
            Ok(listing) => listing,
            Err(err) => {
                notes.push(format!("{}: {err}", dir.display()));
                return;
            }
        };
        for entry in listing.entries {
            // Only a folder can hold a skill — a link too, which `read_text` refuses where it leads out (K3), so
            // a stray file in the skills folder is nobody's skill and says nothing.
            if !matches!(entry.kind, EntryKind::Dir | EntryKind::Link) {
                continue;
            }
            let name = entry.name.clone();
            let full = dir.join(&name);
            let text_path = format!("{name}/{SKILL_FILE}");
            let text = match read_text(&root, &text_path, MAX_SKILL_BYTES) {
                Ok(file) => file.content,
                Err(FilesError::NotFound { .. }) => {
                    notes.push(format!("{}: no {SKILL_FILE}", full.display()));
                    continue;
                }
                Err(err) => {
                    notes.push(format!("{}: {err}", full.display()));
                    continue;
                }
            };
            match Skill::parse(&name, full.clone(), &text) {
                Ok(skill) => {
                    if self.get(&skill.name).is_none()
                        && self.skills.len() >= MAX_SKILLS
                        && !*capped
                    {
                        notes.push(format!(
                            "more than {MAX_SKILLS} skills: the rest are left out"
                        ));
                        *capped = true;
                        continue;
                    }
                    self.put(skill);
                }
                Err(note) => notes.push(format!("{}: {note}", full.display())),
            }
        }
    }

    /// A skill into the catalog: two folders of one name are one skill, and the later read wins (K2).
    fn put(&mut self, skill: Skill) {
        match self
            .skills
            .iter_mut()
            .find(|known| known.name == skill.name)
        {
            Some(known) => *known = skill,
            None => self.skills.push(skill),
        }
    }
}

/// Whether `name` is a skill name (K3): the name of its folder, lower case letters, digits and dashes.
fn is_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

/// A skill's frontmatter (K3): what the harness reads and nothing else — skill files come from other tools, whose
/// keys (`backend`, `output`, `license`, `metadata`, …) the harness does not act on.
#[derive(Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    model: Option<String>,
    /// `allowed-tools` is the other spelling skill files use.
    #[serde(alias = "allowed-tools")]
    allowed_tools: Option<RuleList>,
    timeout_secs: Option<u64>,
    #[serde(default)]
    prepend_files: Vec<String>,
}

/// `allowed_tools` as skill files write it (K3): one string of rules or a YAML list of them. `Rule` is not
/// `Deserialize` — the harness's own lexer decides what a rule is (HS3a), whatever the YAML says.
#[derive(Default)]
struct RuleList(Vec<Rule>);

impl<'de> Deserialize<'de> for RuleList {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Rules;
        impl<'de> Visitor<'de> for Rules {
            type Value = RuleList;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("rules as a list or one space- or comma-separated string")
            }

            fn visit_str<E>(self, text: &str) -> Result<RuleList, E>
            where
                E: de::Error,
            {
                rules_of(text).map(RuleList).map_err(E::custom)
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<RuleList, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut rules = Vec::new();
                while let Some(text) = seq.next_element::<String>()? {
                    rules.extend(rules_of(&text).map_err(<A::Error as de::Error>::custom)?);
                }
                Ok(RuleList(rules))
            }
        }
        deserializer.deserialize_any(Rules)
    }
}

/// One string of rules as the harness reads them: split on spaces and commas outside parentheses — `Bash(git
/// status:*)` holds both — and each part parsed as one rule. Public for a caller that narrows a run by a skill's
/// `allowed_tools` frontmatter without loading it as a harness skill (HS6b, B3); the harness's own skill
/// frontmatter parser uses the same function, so both sides split identically.
///
/// # Errors
///
/// The reason one part does not parse as a rule, as [`Rule::parse`] reports it.
pub fn rules_of(text: &str) -> Result<Vec<Rule>, String> {
    split_rules(text).into_iter().map(Rule::parse).collect()
}

fn split_rules(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    for (index, ch) in text.char_indices() {
        match ch {
            '(' => {
                depth += 1;
                start.get_or_insert(index);
            }
            ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 && (ch == ',' || ch.is_whitespace()) => {
                if let Some(begin) = start.take() {
                    parts.push(&text[begin..index]);
                }
            }
            _ => {
                start.get_or_insert(index);
            }
        }
    }
    if let Some(begin) = start {
        parts.push(&text[begin..]);
    }
    parts
}

/// Longest frontmatter the YAML parser reads, in bytes. A real skill needs about 1 KiB.
const MAX_FRONTMATTER_BYTES: usize = 8 * 1024;
/// Deepest `[`/`{` nesting accepted in the frontmatter.
const MAX_FLOW_DEPTH: usize = 8;

/// Refuses frontmatter the YAML parser could be made to blow up on, **before** it runs (K3).
///
/// The parser (yaml-rust2) resolves an alias by cloning the anchored node and has no expansion limit, so a few
/// kilobytes of nested `&a`/`*a` exhaust memory — and a skill run starts the parser at once. A skill has no use
/// for anchors, aliases or tags, so they are refused; the size and flow-depth caps bound what is left (deeply
/// nested flow collections recurse).
///
/// `&`, `*` and `!` are refused where a YAML token can start — outside quoted strings and comments, like the
/// roster's check for `AGENT.md`, which this follows. One difference, and it is against the parser itself
/// (checked 2026-10-13): a `:` counts as opening the next token only where it really ends the one before it.
/// yaml-rust2 reads `key:*x` and `Bash(git:*)` as plain scalars, so refusing the star after every colon would
/// turn the idiomatic `allowed-tools: Bash(git:*)` into a skipped skill; but after a quoted scalar or a `}` the
/// same star **is** an alias (`{"k":*x}` expands), so there the colon counts. Still conservative everywhere else:
/// text that merely looks like YAML — a line of a multi-line value starting with `*` — is refused too; write it
/// as one line or quote it, and the note says why.
///
/// # Errors
///
/// The reason the frontmatter must not be parsed; the skill is skipped with it as a note.
fn check_frontmatter_shape(raw: &str) -> Result<(), String> {
    if !raw.starts_with("---") {
        // No frontmatter block: the parser never sees YAML.
        return Ok(());
    }
    let end = raw[3..].find("\n---").map_or(raw.len(), |at| at + 3);
    let front = &raw[..end];
    if front.len() > MAX_FRONTMATTER_BYTES {
        return Err(format!("longer than {MAX_FRONTMATTER_BYTES} bytes"));
    }
    let refuse = |what: &str| -> Result<(), String> {
        Err(format!(
            "{what} are not allowed in a skill's frontmatter; write plain values (quote text that starts with one)"
        ))
    };
    let mut depth = 0usize;
    let mut prev: Option<char> = None;
    // A plain scalar is running; a `:` outside one opens the next token, inside one it is plain text (see above).
    let mut plain = false;
    let mut boundary = false;
    let mut chars = front.chars().peekable();
    while let Some(ch) = chars.next() {
        let at_token_start =
            boundary || prev.is_none_or(|p| p.is_whitespace() || matches!(p, '[' | '{' | ','));
        boundary = false;
        match ch {
            '"' if at_token_start => {
                // A double-quoted string runs to the next unescaped quote.
                let mut escaped = false;
                for inner in chars.by_ref() {
                    if escaped {
                        escaped = false;
                    } else if inner == '\\' {
                        escaped = true;
                    } else if inner == '"' {
                        break;
                    }
                }
                prev = Some('"');
                plain = false;
                continue;
            }
            '\'' if at_token_start => {
                // A single-quoted string runs to the next quote that is not doubled.
                while let Some(inner) = chars.next() {
                    if inner == '\'' {
                        let mut ahead = chars.clone();
                        if ahead.next() == Some('\'') {
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                prev = Some('\'');
                plain = false;
                continue;
            }
            '#' if at_token_start => {
                // A comment runs to the end of the line.
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        break;
                    }
                }
                prev = Some('\n');
                plain = false;
                continue;
            }
            '&' | '*' if at_token_start => return refuse("anchors and aliases"),
            '!' if at_token_start => return refuse("tags"),
            '[' | '{' => {
                depth += 1;
                if depth > MAX_FLOW_DEPTH {
                    return Err("nested too deeply".into());
                }
                plain = false;
            }
            ']' | '}' => {
                depth = depth.saturating_sub(1);
                plain = false;
            }
            ',' if depth > 0 => plain = false,
            ':' => {
                boundary = !plain;
                if plain && chars.peek().is_none_or(|next| next.is_whitespace()) {
                    plain = false;
                }
            }
            '\n' => plain = false,
            _ => plain = true,
        }
        prev = Some(ch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::permissions::{Access, Mode, Nobody, RuleSet};
    use crate::prompt::system_prompt;

    /// The header of the app's own `mail-digest` skill, copied here — a test never reads the real file.
    const MAIL_DIGEST: &str = "---\n\
        name: mail-digest\n\
        description: Reads recent mail via whichever mail tool is available (the apple-mail MCP server today), \
        picks out messages the agent judges important plus ones matching the owner's configured topics, \
        summarises each in a bit more depth (including any links the email contains), and reports them as one \
        JSON object for the Mail dashboard module to read back from this skill's last run.\n\
        backend: opencode\n\
        prepend_files: [\"Mail/.topics.md\"]\n\
        allowed_tools: mcp__apple-mail__search_emails mcp__apple-mail__get_needs_response \
        mcp__apple-mail__get_email_source\n\
        timeout_secs: 600\n\
        output: json\n\
        ---\n\n# Mail Digest\n\nReport the mail as one JSON object.\n";

    /// A skill frontmatter the way Claude Code writes it: folded description (its lines indented, as YAML wants),
    /// `allowed-tools` as a list with a quoted command rule holding both a space and a star.
    const CLAUDE_STYLE: &str = "---\n\
        name: reviewer\n\
        description: >\n  \
        Reviews a change against the project's\n  \
        rules before it is taken over.\n\
        allowed-tools: [Read, \"Bash(git status:*)\"]\n\
        model: anthropic/claude-haiku-5-5\n\
        ---\n\nReview the change.\n";

    /// A fresh folder for one test — outside the repo, like the other tests' folders.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axh-skills-{name}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A skill folder with its `SKILL.md` below `root` (`demo` is `root/demo/SKILL.md`).
    fn skill_file(root: &Path, folder: &str, text: &str) -> PathBuf {
        let dir = root.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(SKILL_FILE), text).unwrap();
        dir
    }

    /// One `SKILL.md`'s text as a skill, without a folder on disk — the parser reads the text alone.
    fn skill_of(folder: &str, raw: &str) -> Skill {
        Skill::parse(folder, PathBuf::from("/s").join(folder), raw).unwrap()
    }

    fn endpoint(model: &str) -> ModelEndpoint {
        ModelEndpoint {
            base_url: "https://example.test/api".into(),
            api_key: None,
            model: model.into(),
        }
    }

    #[test]
    fn axiomatas_mail_digest_header_parses() {
        let skill = skill_of("mail-digest", MAIL_DIGEST);
        assert!(skill.description.starts_with("Reads recent mail"));
        assert_eq!(skill.prepend_files, ["Mail/.topics.md"]);
        assert_eq!(skill.allowed_tools.len(), 3);
        assert_eq!(skill.timeout, Some(Duration::from_secs(600)));
        // Keys the harness does not know are ignored, not refused (K3).
        assert_eq!(skill.model, None);
        assert!(skill.body.contains("one JSON object"));
    }

    #[test]
    fn claude_style_frontmatter_parses() {
        let skill = skill_of("reviewer", CLAUDE_STYLE);
        // A folded multi-line description, trimmed to one string.
        assert!(skill.description.starts_with("Reviews a change"));
        assert!(!skill.description.contains('\n'));
        assert_eq!(skill.allowed_tools.len(), 2);
        assert_eq!(skill.model.as_deref(), Some("anthropic/claude-haiku-5-5"));
    }

    #[test]
    fn rules_of_splits_one_string_the_way_the_frontmatter_reads_it() {
        // Public since HS6b: a caller that narrows a run by a skill's `allowed_tools` splits exactly like the
        // harness's own frontmatter parser.
        let rules = rules_of("Bash(git status:*) Read, Edit").unwrap();
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].text(), "Bash(git status:*)");
        assert_eq!(rules[1].text(), "Read");
        assert_eq!(rules[2].text(), "Edit");
        assert!(rules_of("Bash(").is_err());
    }

    #[test]
    fn owner_caller_and_trusted_project_merge_with_the_more_specific_winning() {
        let root = temp("merge");
        let home = root.join("home");
        let project = root.join("project");
        let caller = root.join("caller");
        let skill_text = |body: &str| format!("---\nname: demo\ndescription: d\n---\n{body}\n");
        skill_file(
            &home.join(".axh").join(SKILLS_DIR),
            "demo",
            &skill_text("from the owner"),
        );
        skill_file(&caller, "demo", &skill_text("from the caller"));
        skill_file(
            &project.join(".axh").join(SKILLS_DIR),
            "demo",
            &skill_text("from the project"),
        );

        let (catalog, notes) = Catalog::load(&home, &project, true, std::slice::from_ref(&caller));
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(catalog.list().len(), 1);
        assert_eq!(catalog.get("demo").unwrap().body, "from the project");
        // Without trust the project's folder is not read at all — the same trust its allow rules need (K2).
        let (catalog, notes) = Catalog::load(&home, &project, false, &[caller]);
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(catalog.get("demo").unwrap().body, "from the caller");
    }

    #[test]
    fn an_untrusted_projects_own_skills_are_left_out() {
        let root = temp("untrusted");
        let home = root.join("home");
        let project = root.join("project");
        let caller = root.join("caller");
        skill_file(
            &project.join(".axh").join(SKILLS_DIR),
            "project-only",
            "---\nname: project-only\ndescription: d\n---\nP.\n",
        );
        skill_file(
            &caller,
            "caller-only",
            "---\nname: caller-only\ndescription: d\n---\nC.\n",
        );
        let (catalog, notes) = Catalog::load(&home, &project, false, &[caller]);
        assert!(notes.is_empty(), "{notes:?}");
        assert!(catalog.get("project-only").is_none());
        assert!(catalog.get("caller-only").is_some());
    }

    #[test]
    fn a_broken_skill_is_a_note_and_the_catalog_goes_on() {
        let root = temp("broken");
        let home = root.join("home");
        let skills = home.join(".axh").join(SKILLS_DIR);
        skill_file(
            &skills,
            "good",
            "---\nname: good\ndescription: d\n---\nG.\n",
        );
        skill_file(
            &skills,
            "no-description",
            "---\nname: no-description\n---\nX.\n",
        );
        skill_file(
            &skills,
            "wrong-name",
            "---\nname: other\ndescription: d\n---\nX.\n",
        );
        skill_file(&skills, "no-frontmatter", "# just Markdown\n");
        std::fs::create_dir_all(skills.join("empty-folder")).unwrap();
        // One byte over the cap of K1.
        skill_file(
            &skills,
            "too-big",
            &format!(
                "---\nname: too-big\ndescription: d\n---\n{}",
                "x".repeat(256 * 1024 + 1)
            ),
        );

        let (catalog, notes) = Catalog::load(&home, &root, false, &[]);
        assert_eq!(catalog.names(), ["good"]);
        let notes = notes.join("\n");
        assert!(
            notes.contains("no-description") && notes.contains("no description"),
            "{notes}"
        );
        assert!(
            notes.contains("wrong-name") && notes.contains("not its folder"),
            "{notes}"
        );
        assert!(
            notes.contains("no-frontmatter") && notes.contains("frontmatter"),
            "{notes}"
        );
        assert!(
            notes.contains("empty-folder") && notes.contains("no SKILL.md"),
            "{notes}"
        );
        assert!(notes.contains("too-big"), "{notes}");
    }

    #[test]
    fn an_alias_bomb_is_refused_before_the_parser_sees_it() {
        let mut bomb = String::from("---\nname: demo\ndescription: d\na0: &a0 [z, z, z, z]\n");
        for level in 1..8 {
            let refs = vec![format!("*a{}", level - 1); 4].join(", ");
            bomb.push_str(&format!("a{level}: &a{level} [{refs}]\n"));
        }
        bomb.push_str("---\nbody\n");
        let err = Skill::parse("demo", PathBuf::from("/s/demo"), &bomb).unwrap_err();
        assert!(err.contains("anchors"), "{err}");
    }

    #[test]
    fn an_unquoted_rule_with_a_star_is_plain_text_not_an_alias() {
        // The shape check must not refuse the idiomatic `allowed-tools: Bash(git status:*)` — the `*` there is
        // plain text to the parser (checked against yaml-rust2, see `check_frontmatter_shape`).
        let skill = skill_of(
            "builder",
            "---\nname: builder\ndescription: d\nallowed-tools: Read, Bash(git status:*)\n---\nBuild.\n",
        );
        assert_eq!(skill.allowed_tools.len(), 2);
    }

    #[test]
    fn the_body_becomes_the_instructions_of_the_system_prompt() {
        let cwd = temp("body-prompt");
        let skill = skill_of(
            "demo",
            "---\nname: demo\ndescription: d\n---\nDo the thing.\n",
        );
        let mut config = RunConfig::new(&cwd);
        let mut model = endpoint("before");
        let mut gate = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        skill
            .start_run(&cwd, &mut config, &mut model, &mut gate)
            .unwrap();
        let prompt = system_prompt(&cwd, "2026-10-13", config.instructions.as_deref());
        assert!(prompt.ends_with("Do the thing."), "{prompt}");
    }

    #[test]
    fn prepend_files_are_inlined_in_order_and_a_missing_one_is_skipped() {
        let cwd = temp("prepends");
        std::fs::write(cwd.join("a.md"), "first\n").unwrap();
        std::fs::write(cwd.join("b.md"), "second").unwrap();
        // `Mail/.topics.md` the way `mail-digest` names it: no `Mail/` folder here at all, which the file guard
        // reports as a failed folder rather than a missing file — a missing file either way (K5).
        let skill = skill_of(
            "demo",
            "---\nname: demo\ndescription: d\nprepend_files: [\"a.md\", \"missing.md\", \"Mail/.topics.md\", \"./b.md\"]\n---\nBody.\n",
        );
        assert_eq!(
            skill.instructions(&cwd).unwrap(),
            "## Context file: a.md\nfirst\n\n## Context file: b.md\nsecond\n\nBody."
        );
    }

    #[test]
    fn a_prepended_file_outside_the_working_folder_fails_the_run() {
        let cwd = temp("prepend-outside");
        for path in ["../x.md", "/etc/hosts"] {
            let skill = skill_of(
                "demo",
                &format!(
                    "---\nname: demo\ndescription: d\nprepend_files: [\"{path}\"]\n---\nBody.\n"
                ),
            );
            let err = skill.instructions(&cwd).unwrap_err();
            assert!(matches!(err, HarnessError::Skill { .. }), "{path}: {err}");
            assert!(err.to_string().contains("working folder"), "{err}");
        }
    }

    #[tokio::test]
    async fn allowed_tools_narrow_the_run_and_model_and_time_limit_follow_the_frontmatter() {
        let cwd = temp("skill-run");
        let skill = skill_of(
            "reviewer",
            "---\nname: reviewer\ndescription: d\nallowed_tools: Read\nmodel: some/model\ntimeout_secs: 5\n---\nReview.\n",
        );
        let mut config = RunConfig::new(&cwd);
        let mut model = endpoint("before");
        let mut gate = Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));
        skill
            .start_run(&cwd, &mut config, &mut model, &mut gate)
            .unwrap();
        assert_eq!(model.model, "some/model");
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert!(gate.restrict.is_some());
        assert_eq!(gate.restrict_name.as_deref(), Some("reviewer"));
        // K5, as a command's narrowing (HS4, C3): a shell call is refused, a read is judged as usual.
        let refused = gate
            .judge(
                "bash",
                &Access::Bash {
                    command: "git status".into(),
                    folder: String::new(),
                    unsandboxed: false,
                },
            )
            .await;
        assert!(
            matches!(&refused, Err(refused) if refused.contains("allows only")),
            "{refused:?}"
        );
        assert!(
            gate.judge(
                "read_file",
                &Access::Read {
                    path: "a.md".into()
                }
            )
            .await
            .is_ok()
        );
        // The skill's limit is capped by the one the run already has (K5), never raised.
        config.timeout = Duration::from_secs(2);
        skill
            .start_run(&cwd, &mut config, &mut model, &mut gate)
            .unwrap();
        assert_eq!(config.timeout, Duration::from_secs(2));
    }

    #[test]
    fn the_files_beside_a_skill_are_listed_relative_to_its_folder() {
        let root = temp("files");
        let dir = skill_file(
            &root,
            "demo",
            "---\nname: demo\ndescription: d\n---\nBody.\n",
        );
        std::fs::write(dir.join("ref.md"), "x").unwrap();
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("scripts").join("run.sh"), "x").unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git").join("config"), "x").unwrap();
        std::fs::create_dir_all(dir.join("too-deep").join("a").join("b").join("c")).unwrap();
        std::fs::write(
            dir.join("too-deep")
                .join("a")
                .join("b")
                .join("c")
                .join("x.md"),
            "x",
        )
        .unwrap();
        let skill = skill_of("demo", "---\nname: demo\ndescription: d\n---\nBody.\n");
        let skill = Skill { dir, ..skill };
        assert_eq!(skill.files(), ["ref.md", "scripts/run.sh"]);
    }
}
