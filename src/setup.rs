//! What every caller puts together the same way before a run — `axh`, `axiomata-cli harness run` and the core's
//! `axh` backend: the toolbox in its one order, and the prompt a slash command (HS4) or a skill (HS6) makes of the
//! caller's. Kept here so the callers cannot drift apart in what a run is offered or how it is narrowed.

use std::path::Path;
use std::sync::Arc;

use crate::commands::{Expanded, Registry, Source};
use crate::config::{ModelEndpoint, RunConfig};
use crate::error::HarnessError;
use crate::event::{Event, EventSink, SharedSink, SkillHow};
use crate::mcp;
use crate::mcp::config::ServerEntry;
use crate::permissions::Gate;
use crate::permissions::settings::SandboxSettings;
use crate::skills::{self, Catalog, SkillTool};
use crate::tools::files::{SkillRoots, file_tools};
use crate::tools::shell::{ShellOptions, shell_tools};
use crate::tools::{Tool, Toolbox};

/// A run's toolbox under `gate`: the file tools (with the catalog's skill folders as read-only roots, K4), the
/// `skill` tool while there is a skill to load (K1.2), the tools of the MCP `servers` the caller has already
/// approved (HS5, M2) and the shell, whose commands run in `sandbox` (HS3b).
///
/// # Errors
///
/// [`HarnessError`] when `cwd` cannot be opened as the run's folder or the shell cannot be set up. A server that
/// fails to start is an event, never an error (M3).
pub async fn toolbox(
    cwd: &Path,
    catalog: &Arc<Catalog>,
    gate: Gate,
    servers: Vec<ServerEntry>,
    sandbox: SandboxSettings,
    sink: &mut SharedSink,
) -> Result<Toolbox, HarnessError> {
    let mut tools = Toolbox::new();
    tools.extend(file_tools(cwd, SkillRoots::new(&catalog.folders()))?);
    // The web is part of the core (docs/plans/glut-round.md): no MCP server to set up for a page.
    tools.extend([Box::new(crate::tools::web::WebFetch) as Box<dyn Tool>]);
    if !catalog.is_empty() {
        tools
            .extend([Box::new(SkillTool::new(Arc::clone(catalog), sink.clone())) as Box<dyn Tool>]);
    }
    tools.set_gate(Arc::new(gate));
    tools.extend(mcp::start(servers, sink).await);
    tools.extend(shell_tools(
        cwd,
        ShellOptions {
            sandbox: Some(sandbox),
            ..ShellOptions::from_environment()
        },
    )?);
    Ok(tools)
}

/// A run as it stands before a slash command and a skill have had their say about it.
pub struct Draft {
    /// The run's settings; a skill adds its instructions and may shorten the time limit.
    pub config: RunConfig,
    /// The model to use; a skill's or a command's `model` replaces it.
    pub endpoint: ModelEndpoint,
    /// The run's gate; a skill's and a command's tool lists each narrow it.
    pub gate: Gate,
}

/// Why [`prepare`] could not make a run of the caller's input — each one for the person who started it.
#[derive(Debug, thiserror::Error)]
pub enum PrepareError {
    /// An unknown command, or a command line the harness cannot make a prompt of (`Registry::expand`).
    #[error("{0}")]
    Command(String),
    /// `--skill` names no skill of the catalog; `known` lists the ones there are.
    #[error("no skill named {name:?}; known: {known}")]
    UnknownSkill { name: String, known: String },
    /// The skill exists but its instructions cannot be built (`Skill::start_run`).
    #[error(transparent)]
    Skill(#[from] HarnessError),
}

/// What [`prepare`] made of the caller's input.
#[derive(Debug, PartialEq, Eq)]
pub enum Prepared {
    /// A built-in command answered it without a model (C5); its text has been sent as the answer.
    Answered,
    /// The prompt to run.
    Prompt(String),
}

/// Applies a slash command named in `prompt` and the skill `skill` to `draft`, and says what to run.
///
/// The skill comes first (K1.1, K5): its body becomes the run's instructions, its model and time limit replace
/// the draft's, its `allowed_tools` narrow the gate. The command after it (C2, C3): its `allowed-tools` narrow
/// the gate on top — neither narrowing undoes the other — and its model wins where both name one. A skill run
/// without a prompt of its own gets [`skills::DEFAULT_PROMPT`]. The `command` and `skill` events are sent here.
///
/// # Errors
///
/// [`PrepareError`]: an unknown command or skill, a command line the harness cannot make a prompt of, a skill
/// whose instructions cannot be built.
pub fn prepare(
    draft: &mut Draft,
    registry: &Registry,
    catalog: &Catalog,
    prompt: Option<String>,
    skill: Option<&str>,
    sink: &mut dyn EventSink,
) -> Result<Prepared, PrepareError> {
    let mut prompt = prompt.unwrap_or_default();
    let command = match registry.expand(&prompt) {
        // An ordinary prompt: no command was named (C6).
        None => None,
        Some(Err(message)) => return Err(PrepareError::Command(message)),
        Some(Ok(Expanded::Builtin { name, text })) => {
            sink.emit(Event::Command {
                name,
                source: Source::Builtin.as_str().to_owned(),
                prompt_chars: 0,
            });
            sink.emit(Event::AssistantText { turn: 0, text });
            return Ok(Prepared::Answered);
        }
        Some(Ok(Expanded::Prompt {
            name,
            prompt: expanded,
            allowed_tools,
            model,
        })) => {
            let source = registry.source(&name).unwrap_or(Source::User);
            sink.emit(Event::Command {
                source: source.as_str().to_owned(),
                name: name.clone(),
                prompt_chars: expanded.chars().count(),
            });
            Some((name, expanded, allowed_tools, model))
        }
    };
    if let Some(name) = skill {
        let Some(chosen) = catalog.get(name) else {
            return Err(PrepareError::UnknownSkill {
                name: name.to_owned(),
                known: catalog.known(),
            });
        };
        let cwd = draft.config.cwd.clone();
        chosen.start_run(
            &cwd,
            &mut draft.config,
            &mut draft.endpoint,
            &mut draft.gate,
        )?;
        sink.emit(Event::Skill {
            name: name.to_owned(),
            how: SkillHow::Run,
        });
    }
    if let Some((name, expanded, allowed_tools, model)) = command {
        draft
            .gate
            .narrow(format!("the command /{name}"), allowed_tools);
        prompt = expanded;
        if let Some(model) = model {
            draft.endpoint.model = model;
        }
    }
    // Decided on the prompt as it finally stands: a command whose body expands to nothing leaves it empty too.
    if skill.is_some() && prompt.trim().is_empty() {
        prompt = skills::DEFAULT_PROMPT.to_owned();
    }
    Ok(Prepared::Prompt(prompt))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::permissions::{Mode, Nobody, RuleSet};

    /// An owner home with three commands and one skill, and a project folder beside it; removed when dropped.
    struct Fixture {
        base: PathBuf,
        home: PathBuf,
        project: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "axh-setup-{tag}-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
            ));
            let (home, project) = (base.join("home"), base.join("project"));
            std::fs::create_dir_all(home.join(".axh/commands")).unwrap();
            std::fs::create_dir_all(home.join(".axh/skills/reviewer")).unwrap();
            std::fs::create_dir_all(&project).unwrap();
            let write = |rel: &str, text: &str| std::fs::write(home.join(rel), text).unwrap();
            write(
                ".axh/commands/fix.md",
                "---\nallowed-tools: Bash(git:*)\nmodel: command/model\n---\nFix $ARGUMENTS\n",
            );
            write(".axh/commands/plain.md", "Look at $ARGUMENTS\n");
            write(
                ".axh/commands/empty.md",
                "---\ndescription: nothing to say\n---\n",
            );
            write(
                ".axh/skills/reviewer/SKILL.md",
                "---\nname: reviewer\ndescription: d\nallowed_tools: Read Bash(git:*)\nmodel: skill/model\n\
                 ---\nReview.\n",
            );
            Fixture {
                base,
                home,
                project,
            }
        }

        fn registry(&self) -> Registry {
            Registry::load(&self.home, &self.project, false).unwrap()
        }

        fn catalog(&self) -> Catalog {
            Catalog::load(&self.home, &self.project, false, &[]).0
        }

        fn draft(&self) -> Draft {
            Draft {
                config: RunConfig::new(&self.project),
                endpoint: ModelEndpoint {
                    base_url: "http://localhost".into(),
                    api_key: None,
                    model: "caller/model".into(),
                },
                gate: Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody)),
            }
        }

        fn prepare(
            &self,
            draft: &mut Draft,
            prompt: Option<&str>,
            skill: Option<&str>,
            events: &mut Vec<Event>,
        ) -> Result<Prepared, PrepareError> {
            prepare(
                draft,
                &self.registry(),
                &self.catalog(),
                prompt.map(str::to_owned),
                skill,
                events,
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn a_skill_and_a_command_both_narrow_the_run_and_the_commands_model_wins() {
        let fx = Fixture::new("both");
        let mut draft = fx.draft();
        let mut events = Vec::new();
        let prepared = fx
            .prepare(
                &mut draft,
                Some("/fix the build"),
                Some("reviewer"),
                &mut events,
            )
            .unwrap();
        assert_eq!(prepared, Prepared::Prompt("Fix the build\n".into()));
        assert_eq!(
            draft.gate.narrowed_by(),
            ["the skill reviewer", "the command /fix"]
        );
        assert_eq!(draft.endpoint.model, "command/model");
        assert!(
            draft
                .config
                .instructions
                .as_deref()
                .is_some_and(|text| text.contains("Review.")),
            "the skill's body is the run's instructions"
        );
        assert!(matches!(events[0], Event::Command { .. }));
        assert!(matches!(events[1], Event::Skill { .. }));
    }

    #[test]
    fn a_command_alone_narrows_and_names_its_model_and_one_without_either_changes_nothing() {
        let fx = Fixture::new("alone");
        let mut draft = fx.draft();
        let prepared = fx
            .prepare(&mut draft, Some("/fix it"), None, &mut Vec::new())
            .unwrap();
        assert_eq!(prepared, Prepared::Prompt("Fix it\n".into()));
        assert_eq!(draft.gate.narrowed_by(), ["the command /fix"]);
        assert_eq!(draft.endpoint.model, "command/model");
        assert!(
            draft.config.instructions.is_none(),
            "no skill, no instructions"
        );

        let mut draft = fx.draft();
        fx.prepare(&mut draft, Some("/plain x"), None, &mut Vec::new())
            .unwrap();
        assert!(draft.gate.narrowed_by().is_empty());
        assert_eq!(draft.endpoint.model, "caller/model");

        // An ordinary prompt passes as it is.
        let mut draft = fx.draft();
        assert_eq!(
            fx.prepare(&mut draft, Some("hello"), None, &mut Vec::new())
                .unwrap(),
            Prepared::Prompt("hello".into())
        );
    }

    #[test]
    fn a_skill_run_whose_prompt_ends_up_empty_runs_the_default_prompt() {
        let fx = Fixture::new("default");
        // No prompt at all …
        assert_eq!(
            fx.prepare(&mut fx.draft(), None, Some("reviewer"), &mut Vec::new())
                .unwrap(),
            Prepared::Prompt(skills::DEFAULT_PROMPT.into())
        );
        // … and a command whose body is empty: decided after the command, not before.
        assert_eq!(
            fx.prepare(
                &mut fx.draft(),
                Some("/empty"),
                Some("reviewer"),
                &mut Vec::new()
            )
            .unwrap(),
            Prepared::Prompt(skills::DEFAULT_PROMPT.into())
        );
    }

    #[test]
    fn a_builtin_answers_and_unknown_names_are_their_own_errors() {
        let fx = Fixture::new("builtin");
        let mut events = Vec::new();
        let answered = fx.prepare(&mut fx.draft(), Some("/help"), None, &mut events);
        assert!(matches!(answered, Ok(Prepared::Answered)));
        assert!(matches!(events.last(), Some(Event::AssistantText { .. })));

        let unknown = fx.prepare(&mut fx.draft(), None, Some("ghost"), &mut Vec::new());
        assert!(
            matches!(&unknown, Err(PrepareError::UnknownSkill { known, .. }) if known == "reviewer"),
            "{unknown:?}"
        );
        let unknown = fx.prepare(&mut fx.draft(), Some("/ghost"), None, &mut Vec::new());
        assert!(
            matches!(unknown, Err(PrepareError::Command(_))),
            "{unknown:?}"
        );
    }

    #[test]
    fn a_skill_whose_instructions_cannot_be_built_is_a_skill_error() {
        let fx = Fixture::new("broken");
        std::fs::create_dir_all(fx.home.join(".axh/skills/broken")).unwrap();
        std::fs::write(
            fx.home.join(".axh/skills/broken/SKILL.md"),
            "---\nname: broken\ndescription: d\nprepend_files: [\"../outside.md\"]\n---\nBody.\n",
        )
        .unwrap();
        let result = fx.prepare(&mut fx.draft(), None, Some("broken"), &mut Vec::new());
        assert!(
            matches!(result, Err(PrepareError::Skill(HarnessError::Skill { .. }))),
            "{result:?}"
        );
    }

    #[tokio::test]
    async fn the_toolbox_is_gated_and_offers_skill_only_with_a_skill_to_load() {
        let fx = Fixture::new("toolbox");
        let mut sink = SharedSink::new(Box::new(Vec::<Event>::new()));
        let gate = || Gate::new(RuleSet::default(), Mode::Default, Arc::new(Nobody));

        let with = Arc::new(fx.catalog());
        let tools = toolbox(
            &fx.project,
            &with,
            gate(),
            Vec::new(),
            SandboxSettings::default(),
            &mut sink,
        )
        .await
        .unwrap();
        assert!(tools.gate().is_some(), "every call is judged");
        assert!(tools.names().contains(&"skill".to_owned()));
        assert!(tools.names().contains(&"bash".to_owned()));

        let without = Arc::new(Catalog::default());
        let tools = toolbox(
            &fx.project,
            &without,
            gate(),
            Vec::new(),
            SandboxSettings::default(),
            &mut sink,
        )
        .await
        .unwrap();
        assert!(!tools.names().contains(&"skill".to_owned()));
    }
}
