//! Named subagents (`docs/plans/glut-tui.md`, T37a), as Claude Code has them: a Markdown file `<name>.md` whose YAML
//! frontmatter says what the agent is for and what it runs with, and whose text is its system prompt. The `task` tool
//! lists them, and a `task` call that names one runs its subagent as the file says.
//!
//! They live in the owner's `~/.glut/agents` and, in a trusted folder, the project's `.glut/agents` — the project's
//! wins a name. What an agent may do only narrows the parent's rights (`tools`, read like a skill's
//! `allowed-tools`); whether a `task` call runs at all stays the gate's word. An agent that names its own `model`,
//! `provider` or `effort` runs on a model the caller builds ([`ModelSource`]); one that cannot be built fails its
//! call, never runs on another.

use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;

use axiomata_files::{EntryKind, FilesError, LinkPolicy, Root, list_dir, read_text};
use gray_matter::Matter;
use gray_matter::engine::YAML;
use serde::Deserialize;

use crate::config::{Effort, Prices};
use crate::model::Model;
use crate::permissions::{Rule, settings};
use crate::skills::{RuleList, check_frontmatter_shape, is_skill_name};

/// The folder of agents inside `~/.glut/` and a project's `.glut/`.
pub const AGENTS_DIR: &str = "agents";
/// Agents one run knows: past this a folder is no collection of agents, and the `task` tool's description would
/// carry them all on every turn.
const MAX_AGENTS: usize = 50;
/// Bytes of one agent file: a system prompt of a few KiB; more is a mistake.
const MAX_AGENT_BYTES: u64 = 64 * 1024;
/// Characters of an agent's description the `task` tool's description carries.
const MAX_DESCRIPTION_CHARS: usize = 300;

/// One agent: its frontmatter as Glut reads it and its system prompt.
#[derive(Debug, Clone)]
pub struct Agent {
    /// Its name — its file's, and the one a `task` call names it by.
    pub name: String,
    /// When it is the one to use.
    pub description: String,
    /// What it may call: narrows the parent's rights, never widens them. `None` (no `tools` key): all the parent's
    /// tools but `task`; an empty list (`tools: []`) is no tool at all — never read as "no limit".
    pub tools: Option<Vec<Rule>>,
    /// A model of its own (the provider's model id); `None`: the session's.
    pub model: Option<String>,
    /// A provider of its own (an id of `providers.json`); `None`: the session's.
    pub provider: Option<String>,
    pub effort: Option<Effort>,
    /// Its system prompt: the text below the frontmatter.
    pub prompt: String,
}

impl Agent {
    /// One agent file's text: its name must be the file's (`file_name` without `.md`).
    ///
    /// # Errors
    ///
    /// Why the agent is left out: no frontmatter, a field missing, a name that is not the file's, an effort Glut does
    /// not know. The caller turns it into a note; an agent never fails a run.
    pub fn parse(file_name: &str, raw: &str) -> Result<Agent, String> {
        if !is_skill_name(file_name) {
            return Err(format!(
                "its file name {file_name:?} is no agent name (lower case letters, digits and dashes, up to 64)"
            ));
        }
        check_frontmatter_shape(raw)?;
        let parsed = Matter::<YAML>::new()
            .parse::<Frontmatter>(raw)
            .map_err(|err| err.to_string())?;
        let Some(front) = parsed.data else {
            return Err("it has no YAML frontmatter block".into());
        };
        let name = front.name.unwrap_or_default();
        if name != file_name {
            return Err(format!(
                "its frontmatter says name {name:?}, not its file's {file_name:?}"
            ));
        }
        let description = front.description.unwrap_or_default().trim().to_owned();
        if description.is_empty() {
            return Err("its frontmatter has no description".into());
        }
        let prompt = parsed.content.trim().to_owned();
        if prompt.is_empty() {
            return Err("it has no system prompt below the frontmatter".into());
        }
        let text = |value: Option<String>| {
            value
                .map(|text| text.trim().to_owned())
                .filter(|text| !text.is_empty())
        };
        let effort = text(front.effort)
            .as_deref()
            .map(Effort::parse_or_explain)
            .transpose()?;
        Ok(Agent {
            name,
            description,
            tools: front.tools.map(|list| list.0),
            model: text(front.model),
            provider: text(front.provider),
            effort,
            prompt,
        })
    }

    /// How an ask names it: the agent, and the provider, model and effort its file picks.
    pub fn ask_line(&self) -> String {
        let mut line = format!("agent {}", self.name);
        let picks: Vec<String> = [
            self.provider
                .as_ref()
                .map(|provider| format!("provider {provider}")),
            self.model.as_ref().map(|model| format!("model {model}")),
            self.effort
                .map(|effort| format!("effort {}", effort.as_str())),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !picks.is_empty() {
            line.push_str(" · ");
            line.push_str(&picks.join(" · "));
        }
        line
    }

    /// Whether it runs on a model of its own rather than its parent's.
    pub fn own_model(&self) -> bool {
        self.model.is_some() || self.provider.is_some() || self.effort.is_some()
    }
}

/// An agent file's frontmatter: what Glut reads, every other key ignored.
#[derive(Deserialize)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
    /// Claude Code's spelling; `allowed-tools` as a skill writes it is read too.
    #[serde(alias = "allowed-tools", alias = "allowed_tools")]
    tools: Option<RuleList>,
    model: Option<String>,
    provider: Option<String>,
    effort: Option<String>,
}

/// Glut's own agents, by name: written to be roles of a pane and subagents alike.
const GLUT_OWN: [(&str, &str); 3] = [
    ("docs", include_str!("roles/docs.md")),
    ("reviewer", include_str!("roles/reviewer.md")),
    ("tester", include_str!("roles/tester.md")),
];

/// The agents a run knows, by name.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    agents: Vec<Agent>,
}

impl Catalog {
    /// The owner's agents, then — in a trusted folder — the project's, which win a name; with a note for each one
    /// left out.
    pub fn load(home: &Path, project: &Path, trusted: bool) -> (Self, Vec<String>) {
        let mut catalog = Catalog::default();
        let mut notes = Vec::new();
        catalog.read(&home.join(settings::USER_DIR).join(AGENTS_DIR), &mut notes);
        if trusted {
            catalog.read(
                &project.join(settings::PROJECT_DIR).join(AGENTS_DIR),
                &mut notes,
            );
        }
        catalog.agents.sort_by(|a, b| a.name.cmp(&b.name));
        (catalog, notes)
    }

    /// A catalog of `agents` as they are (a host's own, tests).
    pub fn of(agents: Vec<Agent>) -> Self {
        let mut catalog = Catalog::default();
        for agent in agents {
            catalog.put(agent);
        }
        catalog.agents.sort_by(|a, b| a.name.cmp(&b.name));
        catalog
    }

    /// Glut's own agents beside these (TUI T36c: the roles a pane can play — reviewer, tester, docs): an agent of the
    /// owner's or the project's by the same name wins.
    pub fn with_glut_own(mut self) -> Self {
        for (name, raw) in GLUT_OWN {
            if self.get(name).is_none()
                && let Ok(agent) = Agent::parse(name, raw)
            {
                self.agents.push(agent);
            }
        }
        self.agents.sort_by(|a, b| a.name.cmp(&b.name));
        self
    }

    pub fn get(&self, name: &str) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.name == name)
    }

    pub fn list(&self) -> &[Agent] {
        &self.agents
    }

    pub fn is_empty(&self) -> bool {
        self.agents.is_empty()
    }

    /// What the `task` tool's description adds: each agent by name with what it is for.
    pub fn described(&self) -> String {
        self.agents
            .iter()
            .map(|agent| {
                // Shown to the model as its tool list: no control, bidi or zero-width character a file could hide.
                let about: String = crate::permissions::printable(&agent.description)
                    .chars()
                    .take(MAX_DESCRIPTION_CHARS)
                    .collect();
                format!("- {}: {about}", agent.name)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// What an error about an unknown agent says.
    pub fn known(&self) -> String {
        if self.agents.is_empty() {
            "there are no agents".into()
        } else {
            format!(
                "the agents are {}",
                self.agents
                    .iter()
                    .map(|agent| agent.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
    }

    fn put(&mut self, agent: Agent) {
        match self
            .agents
            .iter_mut()
            .find(|known| known.name == agent.name)
        {
            Some(known) => *known = agent,
            None => self.agents.push(agent),
        }
    }

    /// The agents of one folder: each `<name>.md`, read through a contained root (a link inside may lead elsewhere
    /// inside, never out).
    fn read(&mut self, dir: &Path, notes: &mut Vec<String>) {
        match std::fs::symlink_metadata(dir) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return,
            Err(err) => return notes.push(format!("{}: {err}", dir.display())),
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return notes.push(format!(
                    "{}: not a real folder: a link is refused here",
                    dir.display()
                ));
            }
        }
        let listing = match Root::dir(dir, LinkPolicy::Contained).and_then(|root| {
            let listing = list_dir(&root, "")?;
            Ok((root, listing))
        }) {
            Ok(listing) => listing,
            Err(err) => return notes.push(format!("{}: {err}", dir.display())),
        };
        let (root, listing) = listing;
        for entry in listing.entries {
            if !matches!(entry.kind, EntryKind::File | EntryKind::Link) {
                continue;
            }
            // A `README.md` or any other name that is no agent's is a note for people, not an agent left out.
            let Some(name) = entry
                .name
                .strip_suffix(".md")
                .filter(|name| is_skill_name(name))
            else {
                continue;
            };
            let full = dir.join(&entry.name);
            let text = match read_text(&root, &entry.name, MAX_AGENT_BYTES) {
                Ok(file) => file.content,
                Err(FilesError::NotFound { .. }) => continue,
                Err(err) => {
                    notes.push(format!("agent left out: {}: {err}", full.display()));
                    continue;
                }
            };
            match Agent::parse(name, &text) {
                Ok(agent) => {
                    if self.get(&agent.name).is_none() && self.agents.len() >= MAX_AGENTS {
                        notes.push(format!(
                            "more than {MAX_AGENTS} agents: {} left out",
                            full.display()
                        ));
                        continue;
                    }
                    self.put(agent);
                }
                Err(note) => notes.push(format!("agent left out: {}: {note}", full.display())),
            }
        }
    }
}

/// A model an agent runs on, built by the caller, with what it costs.
#[derive(Clone)]
pub struct Picked {
    pub model: Arc<Model>,
    pub prices: Option<Prices>,
    /// Its context window in tokens where known; `None`: the run asks the provider's list.
    pub window: Option<u32>,
}

/// What builds an agent's own model: the caller knows the providers, their keys and the session's model. `None` for
/// `provider` or `model` is the session's; an error is the `task` call's answer.
pub trait ModelSource: Send + Sync {
    fn model<'a>(
        &'a self,
        provider: Option<&'a str>,
        model: Option<&'a str>,
        effort: Option<Effort>,
    ) -> Pin<Box<dyn Future<Output = Result<Picked, String>> + Send + 'a>>;
}

/// A [`ModelSource`] as a run's config holds it.
#[derive(Clone)]
pub struct Models(pub Arc<dyn ModelSource>);

impl std::fmt::Debug for Models {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Models")
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn glut_brings_its_roles_and_an_agent_of_the_owners_name_wins() {
        let own = Catalog::default().with_glut_own();
        let names: Vec<&str> = own.list().iter().map(|agent| agent.name.as_str()).collect();
        assert_eq!(names, ["docs", "reviewer", "tester"]);
        let reviewer = own.get("reviewer").unwrap();
        assert!(
            reviewer.model.is_none()
                && reviewer
                    .tools
                    .as_ref()
                    .is_some_and(|tools| !tools.is_empty())
        );
        let mine = Agent::parse(
            "reviewer",
            "---\nname: reviewer\ndescription: mine\n---\nMy way.",
        )
        .unwrap();
        let catalog = Catalog::of(vec![mine]).with_glut_own();
        assert_eq!(catalog.get("reviewer").unwrap().description, "mine");
    }

    const REVIEWER: &str = "---\nname: reviewer\ndescription: Reviews a diff for defects.\ntools: read_file, \
                            grep\nmodel: anthropic/claude-opus\nprovider: openrouter\neffort: high\n---\n\nYou \
                            review diffs. Report defects only.\n";

    #[test]
    fn an_agent_file_is_read_as_claude_writes_it() {
        let agent = Agent::parse("reviewer", REVIEWER).unwrap();
        assert_eq!(agent.description, "Reviews a diff for defects.");
        assert_eq!(agent.tools.as_ref().map(Vec::len), Some(2));
        let none = Agent::parse("x", "---\nname: x\ndescription: d\ntools: []\n---\nGo.").unwrap();
        assert_eq!(
            none.tools.map(|tools| tools.len()),
            Some(0),
            "an empty list is no tool, not every tool"
        );
        assert_eq!(agent.model.as_deref(), Some("anthropic/claude-opus"));
        assert_eq!(agent.provider.as_deref(), Some("openrouter"));
        assert_eq!(agent.effort, Some(Effort::High));
        assert_eq!(agent.prompt, "You review diffs. Report defects only.");
        assert!(agent.own_model());
        assert_eq!(
            agent.ask_line(),
            "agent reviewer · provider openrouter · model anthropic/claude-opus · effort high"
        );
    }

    #[test]
    fn an_agent_without_name_description_or_prompt_is_left_out() {
        assert!(
            Agent::parse("other", REVIEWER)
                .unwrap_err()
                .contains("not its file's")
        );
        assert!(
            Agent::parse("x", "---\nname: x\n---\nprompt")
                .unwrap_err()
                .contains("no description")
        );
        assert!(
            Agent::parse("x", "---\nname: x\ndescription: d\n---\n")
                .unwrap_err()
                .contains("no system prompt")
        );
        assert!(
            Agent::parse("Bad", "---\nname: Bad\n---\n")
                .unwrap_err()
                .contains("no agent name")
        );
        let plain = Agent::parse("x", "---\nname: x\ndescription: d\n---\nGo.").unwrap();
        assert!(!plain.own_model() && plain.tools.is_none());
    }

    #[test]
    fn the_projects_agent_wins_its_name_and_only_in_a_trusted_folder() {
        let base = std::env::temp_dir().join(format!("glut-agents-{}", std::process::id()));
        let (home, project) = (base.join("home"), base.join("project"));
        let owners = home.join(".glut/agents");
        let projects = project.join(".glut/agents");
        std::fs::create_dir_all(&owners).unwrap();
        std::fs::create_dir_all(&projects).unwrap();
        std::fs::write(owners.join("reviewer.md"), REVIEWER).unwrap();
        std::fs::write(owners.join("broken.md"), "no frontmatter").unwrap();
        std::fs::write(owners.join("notes.txt"), "not an agent").unwrap();
        std::fs::write(
            projects.join("reviewer.md"),
            "---\nname: reviewer\ndescription: The project's.\n---\nProject prompt.",
        )
        .unwrap();
        let (catalog, notes) = Catalog::load(&home, &project, false);
        assert_eq!(
            catalog.get("reviewer").unwrap().description,
            "Reviews a diff for defects."
        );
        assert_eq!(notes.len(), 1, "{notes:?}");
        let (catalog, _) = Catalog::load(&home, &project, true);
        assert_eq!(catalog.list().len(), 1);
        assert_eq!(
            catalog.get("reviewer").unwrap().description,
            "The project's."
        );
        assert_eq!(catalog.described(), "- reviewer: The project's.");
        let _ = std::fs::remove_dir_all(&base);
    }
}
