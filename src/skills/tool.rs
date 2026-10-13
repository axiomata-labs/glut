//! The `skill` tool (HS6, K1.2): in every run the model is shown which skills exist and can load one's instructions
//! into the conversation, the way Claude Code does it. Loading a skill's text is no action of its own
//! ([`Access::Own`]) — what the model then does with it is judged as usual.
//!
//! Only offered when the run knows at least one skill: an empty catalog would give the model a tool that can only
//! fail (`axh` and the CLI leave it out).

use std::sync::Arc;

use serde_json::{Value, json};

use super::{Catalog, Skill};
use crate::event::{Event, SharedSink, SkillHow};
use crate::model::ToolSpec;
use crate::permissions::Access;
use crate::tools::{CallContext, Tool, ToolFuture};

/// Characters of one skill's description the tool's own description shows; a longer one is cut with `…`.
const DESCRIPTION_CHARS: usize = 300;
/// Characters of the whole skill list the tool's own description holds. Past it the list stops and counts what is
/// left out — the model gets every name through `list=true`, however many skills there are.
const LIST_CHARS: usize = 8_000;

/// The `skill` tool over a run's catalog, reporting each load as an [`Event::Skill`] (K1.2).
pub struct SkillTool {
    catalog: Arc<Catalog>,
    sink: SharedSink,
}

impl SkillTool {
    pub fn new(catalog: Arc<Catalog>, sink: SharedSink) -> Self {
        SkillTool { catalog, sink }
    }

    /// The tool's description: what it does, then every skill by name (K1.2) — each description cut to
    /// [`DESCRIPTION_CHARS`], the whole list to [`LIST_CHARS`].
    fn described(&self) -> String {
        let mut out = String::from(
            "Loads a skill's instructions into this run: call it with a skill's name and follow what comes back. \
             Skills:\n",
        );
        let skills = self.catalog.list();
        let mut shown = 0;
        for skill in skills {
            let line = format!(
                "{}: {}\n",
                skill.name,
                cut(&skill.description, DESCRIPTION_CHARS)
            );
            if out.chars().count() + line.chars().count() > LIST_CHARS {
                break;
            }
            out.push_str(&line);
            shown += 1;
        }
        if shown == skills.len() {
            while out.ends_with('\n') {
                out.pop();
            }
        } else {
            out.push_str(&format!(
                "…and {} more, call skill with list=true",
                skills.len() - shown
            ));
        }
        out
    }
}

impl Tool for SkillTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "skill".into(),
            description: self.described(),
            parameters: json!({"type": "object", "properties": {
                "name": {"type": "string", "description": "The skill to load."},
                "list": {"type": "boolean", "description": "List every skill's name instead of loading one."}
            }}),
        }
    }

    /// Loading a skill's text touches nothing on disk or in the run's state: the rules judge what the model does
    /// with it (K1.2, like Claude Code's model-invoked skills).
    fn access(&self, _arguments: &Value) -> Access {
        Access::Own
    }

    fn call(&self, arguments: Value, _context: CallContext) -> ToolFuture<'_> {
        Box::pin(async move {
            if arguments
                .get("list")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok(self.catalog.names().join("\n"));
            }
            let name = arguments
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim();
            let Some(skill) = self.catalog.get(name) else {
                let known = self.catalog.known();
                return Err(if name.is_empty() {
                    format!("say which skill to load (name), or list=true; known: {known}")
                } else {
                    format!("no skill named {name:?}; known: {known}")
                });
            };
            self.sink.send(Event::Skill {
                name: skill.name.clone(),
                how: SkillHow::Loaded,
            });
            Ok(result(skill))
        })
    }
}

/// What a load brings back: the skill's Markdown body — the instructions the model follows — and the files beside
/// `SKILL.md` as the `skill:<name>/<rel>` paths `read_file` takes (K4).
fn result(skill: &Skill) -> String {
    let mut out = skill.body.clone();
    let files = skill.files();
    if files.is_empty() {
        out.push_str("\n\n(no other files in this skill)");
    } else {
        out.push_str("\n\n## Files beside SKILL.md, read with read_file:\n");
        let listed: Vec<String> = files
            .iter()
            .map(|file| format!("skill:{}/{file}", skill.name))
            .collect();
        out.push_str(&listed.join("\n"));
    }
    out
}

/// `text` at most `max` characters, with `…` when there was more.
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::event::EventSink;

    /// Events kept behind the shared sink, like the collector of `event`'s own tests.
    #[derive(Clone, Default)]
    struct Kept(Arc<Mutex<Vec<Event>>>);

    impl EventSink for Kept {
        fn emit(&mut self, event: Event) {
            self.0.lock().unwrap().push(event);
        }
    }

    /// A catalog of `skills` (name, description) in a scratch folder, with `files` written beside their
    /// `SKILL.md`s (`("demo/ref.md", "x")` is `demo`'s).
    fn catalog_of(skills: &[(&str, &str)], files: &[(&str, &str)]) -> Arc<Catalog> {
        let dir = std::env::temp_dir().join(format!(
            "axh-skilltool-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        for (name, description) in skills {
            let folder = dir.join(name);
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: {description}\n---\nFollow {name}.\n"),
            )
            .unwrap();
        }
        for (path, content) in files {
            let full = dir.join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        let (catalog, notes) = Catalog::load(
            &dir.join("home"),
            &dir.join("project"),
            false,
            std::slice::from_ref(&dir),
        );
        assert!(notes.is_empty(), "{notes:?}");
        Arc::new(catalog)
    }

    fn tool(catalog: Arc<Catalog>) -> (SkillTool, Kept) {
        let kept = Kept::default();
        let tool = SkillTool::new(catalog, SharedSink::new(Box::new(kept.clone())));
        (tool, kept)
    }

    #[tokio::test]
    async fn the_tool_lists_every_skill_and_loads_one_with_its_files() {
        let catalog = catalog_of(
            &[
                ("demo", "Does the demo."),
                ("reviewer", "Reviews a change."),
            ],
            &[("demo/ref.md", "reference"), ("demo/scripts/run.sh", "x")],
        );
        let (skill, kept) = tool(catalog);
        let spec = skill.spec();
        assert_eq!(spec.name, "skill");
        assert!(
            spec.description.contains("demo: Does the demo."),
            "{spec:?}"
        );
        assert!(
            spec.description.contains("reviewer: Reviews a change."),
            "{spec:?}"
        );

        let out = skill
            .call(json!({"name": "demo"}), CallContext::default())
            .await
            .unwrap();
        // The body is what the model follows; the files come as paths `read_file` takes (K4).
        assert!(out.starts_with("Follow demo."), "{out}");
        assert!(out.contains("skill:demo/ref.md"), "{out}");
        assert!(out.ends_with("skill:demo/scripts/run.sh"), "{out}");

        // A skill without other files says so rather than ending in the air.
        let reviewer = skill
            .call(json!({"name": "reviewer"}), CallContext::default())
            .await
            .unwrap();
        assert!(
            reviewer.ends_with("(no other files in this skill)"),
            "{reviewer}"
        );

        let events = kept.0.lock().unwrap();
        assert!(
            events.iter().any(
                |e| matches!(e, Event::Skill { name, how: SkillHow::Loaded } if name == "demo")
            ),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn an_unknown_or_missing_name_names_the_known_skills() {
        let (skill, _kept) = tool(catalog_of(&[("demo", "d")], &[]));
        let err = skill
            .call(json!({"name": "nope"}), CallContext::default())
            .await
            .unwrap_err();
        assert!(
            err.contains("no skill named \"nope\"") && err.contains("demo"),
            "{err}"
        );
        let err = skill
            .call(json!({}), CallContext::default())
            .await
            .unwrap_err();
        assert!(
            err.contains("say which skill") && err.contains("demo"),
            "{err}"
        );
    }

    #[tokio::test]
    async fn list_true_returns_every_name() {
        let (skill, _kept) = tool(catalog_of(&[("beta", "b"), ("alpha", "a")], &[]));
        let out = skill
            .call(json!({"list": true}), CallContext::default())
            .await
            .unwrap();
        // Sorted by name, like the catalog lists everything (K2).
        assert_eq!(out, "alpha\nbeta");
    }

    #[test]
    fn a_long_list_is_cut_and_counts_what_is_left_out() {
        // 60 skills with long descriptions: past [`LIST_CHARS`] the list stops with a count, and the tool's own
        // description stays a size a model can carry on every turn.
        let many: Vec<(String, String)> = (0..60)
            .map(|index| (format!("skill-{index:02}"), "d".repeat(200)))
            .collect();
        let refs: Vec<(&str, &str)> = many
            .iter()
            .map(|(name, description)| (name.as_str(), description.as_str()))
            .collect();
        let (skill, _kept) = tool(catalog_of(&refs, &[]));
        let description = skill.spec().description;
        assert!(
            description.contains("…and ")
                && description.contains("more, call skill with list=true"),
            "{description}"
        );
        assert!(
            description.chars().count() <= LIST_CHARS + 64,
            "the list grew past its cap"
        );
    }

    #[test]
    fn one_skills_description_is_cut_with_an_ellipsis() {
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut("longer text", 6), "longer…");
    }
}
