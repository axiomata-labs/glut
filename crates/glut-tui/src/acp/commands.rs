//! The TUI's commands in an editor's chat (T64b): those whose answer is text, said as Glut's message and never the
//! model's, and the skills. Left out is what the editor does itself (a new, an earlier or a found session), what is
//! the terminal's (panes, views), keys and sign-ins, and what makes no sense beside an editor.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use glut::Cancel;
use serde_json::{Value, json};
use tokio::sync::mpsc;

use super::{Live, Peer, options};
use crate::session::{self, Report};

/// The commands an editor offers, in the TUI's order.
pub const OFFERED: [&str; 18] = [
    "compact",
    "handoff",
    "export",
    "folder",
    "model",
    "effort",
    "context",
    "steps",
    "diff",
    "todo",
    "memory",
    "init",
    "gate",
    "classifier",
    "rule",
    "maxsteps",
    "nudge",
    "help",
];

/// What a command came to: lines said in the chat, or a turn of the model's on `prompt` (a skill, `/init`) — with
/// the guide an `/init` turn checks.
pub enum Ran {
    Said(Vec<String>),
    Turn {
        prompt: String,
        guide: Option<std::path::PathBuf>,
    },
}

/// The `/` list: Glut's offered commands, then the skills, each described in the owner's language.
pub fn listed(live: &Live) -> Vec<Value> {
    let mut listed: Vec<Value> = OFFERED
        .iter()
        .filter_map(|name| {
            crate::commands::COMMANDS
                .iter()
                .find(|command| command.name == *name)
        })
        .map(|command| {
            let mut entry = json!({"name": command.name, "description": command.short()});
            if !command.args.is_empty() {
                entry["input"] = json!({"hint": command.args});
            }
            entry
        })
        .collect();
    listed.extend(live.parts.skills.iter().map(|(name, about)| {
        json!({"name": name, "description": crate::transcript::clean(about), "input": {"hint": "what for (optional)"}})
    }));
    listed
}

/// `text` as a command — `/name rest` with a name of [`OFFERED`] or a skill — or `None` for a message to the model.
pub fn parse<'a>(live: &Live, text: &'a str) -> Option<(&'a str, &'a str)> {
    let rest = text.trim().strip_prefix('/')?;
    let (name, rest) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    let known = OFFERED.contains(&name) || live.parts.skills.iter().any(|(skill, _)| skill == name);
    known.then_some((name, rest.trim()))
}

/// Runs command `name` on `rest`; a fault is said like any line. `cancel` is the editor's stop: a summary or a
/// handoff under way ends with it.
pub async fn run(
    peer: &Arc<Peer>,
    live: &Arc<Live>,
    name: &str,
    rest: &str,
    cancel: &Cancel,
) -> Ran {
    let said = |said: Result<String, String>| {
        Ran::Said(vec![said.unwrap_or_else(|reason| format!("⚠ {reason}"))])
    };
    let parts = &live.parts;
    match name {
        "help" => Ran::Said(vec![help(live)]),
        "classifier" => said(crate::said::classifier(&parts.home, rest)),
        "folder" => said(crate::said::folder(parts, rest)),
        "maxsteps" => said(crate::said::maxsteps(parts, rest)),
        "nudge" => said(crate::said::nudge(parts, rest)),
        "rule" => said(crate::said::rule(
            &mut live
                .rules
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
            rest,
        )),
        "context" => {
            let used = live.context_used.load(Ordering::SeqCst);
            let mut engine = live
                .engine
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            said(crate::said::context(parts, &mut engine, used, rest))
        }
        "gate" => Ran::Said(vec![gate(live)]),
        "model" => model(peer, live, rest).await,
        "effort" => effort(peer, live, rest).await,
        "compact" => compact(peer, live, cancel).await,
        "handoff" => handoff(live, rest, cancel).await,
        "export" => export(live, rest).await,
        "steps" => steps(peer, live, rest).await,
        "diff" => diff(live, rest).await,
        "todo" => todo(peer, live, rest).await,
        "memory" => memory(live, rest),
        "init" => init(peer, live, rest).await,
        skill => Ran::Turn {
            prompt: crate::app::skill_prompt(skill, rest),
            guide: None,
        },
    }
}

fn help(live: &Live) -> String {
    let mut lines: Vec<String> = OFFERED
        .iter()
        .filter_map(|name| {
            crate::commands::COMMANDS
                .iter()
                .find(|command| command.name == *name)
        })
        .map(|command| {
            let args = if command.args.is_empty() {
                String::new()
            } else {
                format!(" {}", command.args)
            };
            format!("/{}{args} — {}", command.name, command.what())
        })
        .collect();
    lines.extend(
        live.parts
            .skills
            .iter()
            .map(|(name, about)| format!("/{name} — {}", crate::transcript::clean(about))),
    );
    lines.join("\n")
}

fn gate(live: &Live) -> String {
    let Some(gate) = live.parts.tools.gate() else {
        return tr!(no_gate).to_string();
    };
    let (granted, settings) = crate::said::gate_rules(gate);
    let mut lines = vec![tr!(
        gate_title,
        mode = session::mode_label(gate.mode()),
        always = if granted.is_empty() {
            tr!(gate_no_always).to_owned()
        } else {
            tr!(gate_always_given, n = granted.len())
        }
    )];
    lines.extend(granted.iter().map(|rules| {
        format!(
            "allow {} — {}",
            crate::transcript::clean(&rules.join(", ")),
            tr!(gate_always_detail)
        )
    }));
    lines.extend(settings.into_iter().map(|(verdict, text)| {
        format!(
            "{verdict} {} — {}",
            crate::transcript::clean(&text),
            tr!(gate_from_settings)
        )
    }));
    lines.join("\n")
}

/// `/model` alone the one in use; `/model <model>` another of the provider in use, `/model <provider> <model>` one of
/// another provider — the editor's picker follows.
async fn model(peer: &Peer, live: &Live, rest: &str) -> Ran {
    let (provider, effort) = {
        let engine = live
            .engine
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        (engine.provider.clone(), engine.endpoint.effort)
    };
    if rest.is_empty() {
        let model = live
            .engine
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .model_name()
            .to_owned();
        return Ran::Said(vec![
            tr!(model_now, provider = provider, model = model).to_string(),
        ]);
    }
    let known = |id: &str| {
        glut::providers::Providers::load(&live.parts.home)
            .is_ok_and(|list| list.providers.contains_key(id))
    };
    let (provider, model) = match rest.split_once(char::is_whitespace) {
        Some((named, model)) if known(named) => (named.to_owned(), model.trim()),
        _ => (provider, rest),
    };
    switched(
        peer,
        live,
        options::switch(live, &provider, model, Some(effort)).await,
    )
    .await
}

/// `/effort` alone the level in use; `/effort <level>` or `default` another.
async fn effort(peer: &Peer, live: &Live, rest: &str) -> Ran {
    let (provider, model, now) = {
        let engine = live
            .engine
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        (
            engine.provider.clone(),
            engine.model_name().to_owned(),
            engine.endpoint.effort,
        )
    };
    let effort = match rest {
        "" => {
            let level = now.map_or_else(
                || tr!(effort_default).to_owned(),
                |effort| effort.to_string(),
            );
            return Ran::Said(vec![
                tr!(
                    model_now,
                    provider = provider,
                    model = format!("{model} · {level}")
                )
                .to_string(),
            ]);
        }
        "default" => None,
        level => match glut::Effort::parse_or_explain(level) {
            Ok(effort) => Some(effort),
            Err(reason) => return Ran::Said(vec![format!("⚠ {reason}")]),
        },
    };
    switched(
        peer,
        live,
        options::switch(live, &provider, &model, Some(effort)).await,
    )
    .await
}

/// A switch's notes, and the editor's pickers told the new values.
async fn switched(peer: &Peer, live: &Live, notes: Result<Vec<String>, String>) -> Ran {
    match notes {
        Ok(notes) => {
            super::options_changed(peer, live).await;
            Ran::Said(notes)
        }
        Err(reason) => Ran::Said(vec![format!("⚠ {reason}")]),
    }
}

/// `/compact`: the conversation summarised now, as the TUI does it.
async fn compact(peer: &Peer, live: &Live, cancel: &Cancel) -> Ran {
    use super::Compaction;
    let (engine, conversation) = live.engine_and_conversation();
    super::compaction(peer, live, Compaction::Begun).await;
    let (reports, mut came) = mpsc::unbounded_channel();
    session::compact(engine.clone(), conversation, reports);
    let came = tokio::select! {
        came = came.recv() => came,
        () = cancel.cancelled() => {
            super::compaction(peer, live, Compaction::Failed(tr!(not_compacted, reason = "⎋").to_string())).await;
            return Ran::Said(Vec::new());
        }
    };
    let Some(Report::Compacted(result, _)) = came else {
        super::compaction(
            peer,
            live,
            Compaction::Failed(tr!(not_compacted, reason = "-").to_string()),
        )
        .await;
        return Ran::Said(Vec::new());
    };
    match result {
        Ok(compacted) => {
            live.spent(&engine, compacted.usage);
            live.context_used.store(0, Ordering::SeqCst);
            // One made ahead is of the conversation this one replaced.
            live.ahead
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .take();
            {
                let mut kept = live
                    .kept
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner());
                kept.conversation = compacted.conversation;
                for mark in &mut kept.marks {
                    mark.at = None;
                }
            }
            live.save().await;
            let said = tr!(compacted, n = compacted.replaced).to_string();
            super::compaction(peer, live, Compaction::Done(Some(said))).await;
            Ran::Said(Vec::new())
        }
        Err(reason) => {
            let said = tr!(not_compacted, reason = reason).to_string();
            super::compaction(peer, live, Compaction::Failed(said)).await;
            Ran::Said(Vec::new())
        }
    }
}

/// `/handoff write|read` (T33).
async fn handoff(live: &Live, rest: &str, cancel: &Cancel) -> Ran {
    match rest {
        "write" => {
            let (engine, conversation) = live.engine_and_conversation();
            let (reports, mut came) = mpsc::unbounded_channel();
            session::write_handoff(
                Arc::clone(&live.parts),
                engine.clone(),
                conversation,
                reports,
            );
            let came = tokio::select! {
                came = came.recv() => came,
                () = cancel.cancelled() => return Ran::Said(Vec::new()),
            };
            match came {
                Some(Report::HandedOff(Ok((path, usage)), _)) => {
                    live.spent(&engine, usage);
                    Ran::Said(vec![
                        tr!(handoff_written, path = session::shown_folder(&path)).to_string(),
                    ])
                }
                Some(Report::HandedOff(Err(reason), _)) => {
                    Ran::Said(vec![format!("⚠ {}", tr!(no_handoff, reason = reason))])
                }
                _ => Ran::Said(vec![format!("⚠ {}", tr!(no_handoff, reason = "-"))]),
            }
        }
        "read" => match session::read_handoff(&live.parts) {
            Ok((message, text, moved)) => {
                live.kept
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .conversation
                    .push(message);
                live.save().await;
                Ran::Said(vec![format!(
                    "{}\n{}",
                    tr!(handoff_read, path = session::shown_folder(&moved)),
                    text.trim()
                )])
            }
            Err(reason) => Ran::Said(vec![format!("⚠ {reason}")]),
        },
        "" => Ran::Said(vec![tr!(handoff_usage).to_string()]),
        other => Ran::Said(vec![format!("⚠ {}", tr!(handoff_unknown, word = other))]),
    }
}

/// `/export` (T54): into a file, or onto macOS's clipboard with `copy` — never the terminal's, whose escape would land
/// in the protocol on stdout.
async fn export(live: &Live, rest: &str) -> Ran {
    let asked = crate::export::parse(rest);
    let session = live
        .kept
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    if session.conversation.is_empty() {
        return Ran::Said(vec![tr!(export_empty).to_string()]);
    }
    let masking = live
        .parts
        .config
        .masks
        .as_ref()
        .is_some_and(glut::masks::Masks::is_on);
    let parts = Arc::clone(&live.parts);
    let said = tokio::task::spawn_blocking(move || {
        if asked.copy {
            let text = crate::export::render(
                &crate::export::of_session(&session, asked.full, &parts.home),
                crate::export::Format::Markdown,
            );
            return match crate::select::pbcopy(&text) {
                Ok(()) if masking => tr!(export_copied_masked).to_string(),
                Ok(()) => tr!(export_copied).to_string(),
                Err(reason) => format!("⚠ {reason}"),
            };
        }
        crate::said::export_to_file(&session, &parts, &asked, masking)
            .unwrap_or_else(|reason| format!("⚠ {reason}"))
    })
    .await
    .unwrap_or_else(|err| format!("⚠ {err}"));
    Ran::Said(vec![said])
}

/// `/steps` the steps that changed files, numbered; `/steps show <n>` what one changed, `/steps undo <n>` the files
/// put back to before it.
async fn steps(peer: &Peer, live: &Live, rest: &str) -> Ran {
    let marks = live
        .kept
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .marks
        .clone();
    let Some(steps) = crate::said::steps(&live.parts, &marks) else {
        return Ran::Said(vec![tr!(steps_no_snapshots).to_string()]);
    };
    let (word, number) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if word.is_empty() {
        if steps.is_empty() {
            return Ran::Said(vec![tr!(steps_none).to_string()]);
        }
        let mut lines = vec![format!(
            "{} — /steps show <n> · /steps undo <n>",
            tr!(steps_title)
        )];
        lines.extend(steps.iter().enumerate().map(|(at, step)| {
            format!(
                "{}. {} ({})",
                at + 1,
                tr!(
                    step_of,
                    turn = step.turn,
                    prompt = crate::transcript::clean(&step.prompt)
                ),
                step.commit
            )
        }));
        return Ran::Said(vec![lines.join("\n")]);
    }
    let Some(step) = number
        .trim()
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|at| steps.get(at))
    else {
        return Ran::Said(vec![
            "⚠ /steps · /steps show <n> · /steps undo <n>".to_owned(),
        ]);
    };
    let Some(root) = live.parts.config.snapshots.clone() else {
        return Ran::Said(vec![tr!(steps_no_snapshots).to_string()]);
    };
    let (cwd, run, turn) = (live.parts.config.cwd.clone(), step.run.clone(), step.turn);
    // A number names the list as it was read: the step is shown by name before its files go back.
    let title = format!(
        "{} · {} ({})",
        tr!(step_title, turn = turn),
        crate::transcript::clean(&step.prompt),
        step.commit
    );
    match word {
        "show" => {
            let change =
                tokio::task::spawn_blocking(move || glut::snapshot::diff(&root, &cwd, &run, turn))
                    .await
                    .unwrap_or_else(|err| Err(err.to_string()));
            Ran::Said(vec![match change {
                Ok(change) if change.trim().is_empty() => {
                    tr!(step_changed_nothing, turn = turn).to_string()
                }
                Ok(change) => format!("{}\n{}", tr!(step_changes, turn = turn), fenced(&change)),
                Err(reason) => format!("⚠ {}", tr!(no_change_to_show, reason = reason)),
            }])
        }
        "undo" => {
            let undo =
                super::confirm(peer, live, &title, tr!(step_restore), tr!(todos_drop_no)).await;
            if undo != Some(true) {
                return Ran::Said(Vec::new());
            }
            let restored = tokio::task::spawn_blocking(move || {
                // T60c: the folders beside the working one the run recorded too go back with it.
                let beside = glut::snapshot::beside_of(&root, &cwd, &run);
                glut::snapshot::keep_now(&root, &cwd, &beside).and_then(|kept| {
                    glut::snapshot::restore_all(&root, &cwd, &run, Some(turn))
                        .map(|(commit, missed)| (commit, kept, missed))
                })
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()));
            Ran::Said(match restored {
                Ok((commit, kept, missed)) => {
                    let mut lines = vec![
                        tr!(
                            files_restored,
                            snapshot = &commit[..commit.len().min(8)],
                            kept = kept
                        )
                        .to_string(),
                    ];
                    lines.extend(
                        missed.iter().map(|what| {
                            format!("⚠ {}", tr!(files_beside_not_restored, what = what))
                        }),
                    );
                    lines
                }
                Err(reason) => vec![format!("⚠ {}", tr!(files_not_restored, reason = reason))],
            })
        }
        _ => Ran::Said(vec![
            "⚠ /steps · /steps show <n> · /steps undo <n>".to_owned(),
        ]),
    }
}

/// `/diff` what the latest turn that changed files changed until now, `/diff session` the whole session's.
async fn diff(live: &Live, rest: &str) -> Ran {
    let scope = match rest {
        "session" => crate::review::Scope::Session,
        _ => crate::review::Scope::Turn,
    };
    let Some(root) = live.parts.config.snapshots.clone() else {
        return Ran::Said(vec![tr!(review_no_snapshots).to_string()]);
    };
    let marks = live
        .kept
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .marks
        .clone();
    let Some((run, prompt)) = crate::said::review_run(&live.parts, &marks, scope) else {
        return Ran::Said(vec![tr!(review_no_change).to_string()]);
    };
    let cwd = live.parts.config.cwd.clone();
    let change =
        tokio::task::spawn_blocking(move || glut::snapshot::diff_to_now(&root, &cwd, &run, None))
            .await
            .unwrap_or_else(|err| Err(err.to_string()));
    Ran::Said(vec![match change {
        Ok(change) if change.trim().is_empty() => tr!(review_no_change).to_string(),
        Ok(change) => format!("{}\n{}", crate::transcript::clean(&prompt), fenced(&change)),
        Err(reason) => format!(
            "⚠ {}",
            tr!(
                no_change_to_review,
                reason = crate::transcript::clean(&reason)
            )
        ),
    }])
}

/// A diff as the editor's Markdown shows one: in a fence its own lines cannot close.
fn fenced(change: &str) -> String {
    // Cleaned first: a hidden character between backticks goes, and the run it leaves must count.
    let change = super::shown(&crate::transcript::clean(change));
    // Any run counts: a diff's context line starts with a space, and an indented fence closes too.
    let longest = change.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("{fence}diff\n{change}\n{fence}")
}

/// `/todo` the folder's list; `/todo <item>` one more; `/todo clean` the done ones dropped, asked first.
async fn todo(peer: &Peer, live: &Live, rest: &str) -> Ran {
    let cwd = live.parts.config.cwd.clone();
    let mut todos = crate::todos::Todos::default();
    todos.refresh(&cwd);
    let name = todos.name();
    match rest {
        // `open` hands the list to the TUI's editor: here the editor is the one the chat is in, so it is shown.
        "" | "open" => Ran::Said(vec![match todos.path() {
            None => tr!(todos_no_file, name = name).to_string(),
            Some(_) if todos.text().trim().is_empty() => {
                tr!(todos_nothing, name = name).to_string()
            }
            Some(_) => format!("{name}:\n{}", super::shown(todos.text())),
        }]),
        "clean" => {
            let done = todos.done();
            if done == 0 {
                return Ran::Said(vec![tr!(todos_nothing_done, name = name).to_string()]);
            }
            let title = tr!(todos_drop_title, done = done, name = name).to_string();
            let drop =
                super::confirm(peer, live, &title, tr!(todos_drop_yes), tr!(todos_drop_no)).await;
            if drop != Some(true) {
                return Ran::Said(Vec::new());
            }
            Ran::Said(vec![match todos.clean(&cwd) {
                Ok(dropped) => tr!(todos_dropped, n = dropped, name = name).to_string(),
                Err(failure) => format!("⚠ {}", failure.message(&name)),
            }])
        }
        item => Ran::Said(vec![match todos.add(&cwd, item) {
            Ok(()) => tr!(todos_added, name = name, item = item).to_string(),
            Err(failure) => format!("⚠ {}", failure.message(&name)),
        }]),
    }
}

/// `/memory` the facts the project's memory keeps, `/memory <name>` one of them whole.
fn memory(live: &Live, rest: &str) -> Ran {
    let Some(dir) = live.parts.memory.clone() else {
        return Ran::Said(vec![tr!(no_memory).to_string()]);
    };
    if !rest.is_empty() {
        let name: String = rest
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
            .collect();
        return Ran::Said(vec![
            match crate::memory::read(&dir.join(format!("{name}.md"))) {
                Some(text) if name == rest => {
                    format!("{name}:\n{}", crate::transcript::clean(&text))
                }
                _ => format!("⚠ {}", tr!(cannot_read, name = rest)),
            },
        ]);
    }
    let facts = crate::memory::facts(&dir);
    if facts.is_empty() {
        return Ran::Said(vec![tr!(memory_empty, folder = dir.display()).to_string()]);
    }
    let mut lines = vec![tr!(memory_title, folder = dir.display()).to_string()];
    lines.extend(
        facts
            .into_iter()
            .map(|(name, about)| format!("{name} — {}", crate::transcript::clean(&about))),
    );
    Ran::Said(vec![lines.join("\n")])
}

/// `/init` (T35): a turn on the folder's guide; the `CLAUDE.md` question asked through the editor's dialog.
async fn init(peer: &Peer, live: &Live, rest: &str) -> Ran {
    let turn = match crate::said::init(&live.parts, rest) {
        Ok(crate::said::Init::Turn(prompt, guide)) => Ok((prompt, guide)),
        Ok(crate::said::Init::AskClaude(claude)) => {
            let migrate = super::confirm(
                peer,
                live,
                tr!(init_claude_title),
                tr!(init_claude_yes),
                tr!(init_claude_no),
            )
            .await;
            // Dismissed: no answer of the owner's, nothing kept, no turn — as Esc in the TUI.
            let Some(migrate) = migrate else {
                return Ran::Said(Vec::new());
            };
            crate::said::init_with(&live.parts, &claude, migrate)
        }
        Err(reason) => Err(reason),
    };
    match turn {
        Ok((prompt, guide)) => Ran::Turn {
            prompt,
            guide: Some(guide),
        },
        Err(reason) => Ran::Said(vec![format!("⚠ {reason}")]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_offered_command_is_one_of_the_tuis() {
        for name in OFFERED {
            assert!(
                crate::commands::COMMANDS
                    .iter()
                    .any(|command| command.name == name),
                "{name}"
            );
        }
    }

    #[test]
    fn a_diff_cannot_close_its_fence() {
        let fenced = fenced("+```\n-x");
        assert!(fenced.starts_with("````diff\n"), "{fenced}");
        assert!(fenced.ends_with("\n````"), "{fenced}");
        // A hidden character between backticks goes in the cleaning: the run it leaves still counts.
        let hidden = super::fenced(" ``\u{200B}``\n");
        assert!(hidden.starts_with("`````diff\n"), "{hidden}");
    }
}
