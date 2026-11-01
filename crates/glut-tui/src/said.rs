//! What a command says (T64b): the commands whose answer is text, shared by the TUI and an editor's session — each
//! does what it does and hands back the line for the stream, `Err` for a fault.

use std::path::Path;

use crate::prefs::{self, Prefs};
use crate::session::{self, Engine, Parts};
use crate::store::Session;

/// A command's line for the stream, or why it failed.
pub type Said = Result<String, String>;

/// The most rules `/rule` keeps for a session, and the most characters of one: a few lines the model reads at
/// every turn, not a second guide.
pub const MOST_RULES: usize = 20;
pub const MOST_RULE_CHARS: usize = 500;

/// `/classifier`: which model judges a command only the mode would ask about (T52), kept in
/// `~/.glut/classifier.json` for every session from its next turn.
pub fn classifier(home: &Path, rest: &str) -> Said {
    use glut::classifier::Choice;
    let shown = |choice: &Choice| match choice {
        Choice::Session => tr!(classifier_session).to_owned(),
        other => other.text(),
    };
    if rest.trim().is_empty() {
        return match Choice::load(home) {
            Ok(Choice::Off) => Ok(tr!(classifier_off_now).to_string()),
            Ok(choice) => Ok(tr!(classifier_now, choice = shown(&choice)).to_string()),
            Err(err) => Err(err.to_string()),
        };
    }
    let choice = Choice::parse(rest).map_err(|_| tr!(classifier_usage).to_string())?;
    choice.save(home).map_err(|err| err.to_string())?;
    Ok(match choice {
        Choice::Off => tr!(classifier_off_now).to_string(),
        choice => tr!(classifier_set, choice = shown(&choice)).to_string(),
    })
}

/// `/sandbox`: how the commands Glut runs are held — the sandbox here, and whether git folders are read-only to
/// them (T59, the owner's switch, kept in `~/.glut/sandbox.json` for every session from its next command).
pub fn sandbox(parts: &Parts, rest: &str) -> Said {
    use glut::sandbox::choices::Choices;
    let words: Vec<&str> = rest.split_whitespace().collect();
    let set = match words.as_slice() {
        [] => None,
        ["git", "read-only"] => Some(true),
        ["git", "writable"] => Some(false),
        _ => return Err(tr!(sandbox_usage).to_string()),
    };
    if let Some(read_only) = set {
        Choices {
            git_read_only: read_only,
        }
        .save(&parts.home)
        .map_err(|err| err.to_string())?;
        return Ok(if read_only {
            tr!(sandbox_git_now_read_only)
        } else {
            tr!(sandbox_git_now_writable)
        }
        .to_string());
    }
    let enabled = parts
        .config
        .sandbox
        .as_ref()
        .is_some_and(|sandbox| sandbox.enabled);
    let held = match glut::sandbox::unavailable() {
        _ if !enabled => tr!(sandbox_cmd_off).to_string(),
        Some(why) => tr!(sandbox_cmd_missing, why = why).to_string(),
        None => tr!(sandbox_cmd_on, kind = glut::sandbox::mechanism()).to_string(),
    };
    let git = match Choices::load(&parts.home) {
        Ok(choices) if choices.git_read_only => tr!(sandbox_git_read_only).to_string(),
        Ok(_) => tr!(sandbox_git_writable).to_string(),
        Err(err) => tr!(sandbox_unreadable, reason = err.to_string()).to_string(),
    };
    Ok(format!("{held}\n{git}"))
}

/// `/folder`: the folders beside the working one (T60c), the owner's word for this session alone — listed alone,
/// one added (checked: never the home folder, one holding the project or a secret) or taken away. What is added
/// counts from the next call on: the file tools, the commands' sandbox, the gate and the snapshots share it.
pub fn folder(parts: &Parts, rest: &str) -> Said {
    let folders = parts.tools.folders().clone();
    let rest = rest.trim();
    let (word, path) = rest.split_once(' ').unwrap_or((rest, ""));
    let path = path.trim();
    match word {
        "" => {
            let listed = folders.list();
            if listed.is_empty() {
                return Ok(tr!(folders_none).to_string());
            }
            let mut text = tr!(folders_heading).to_string();
            for folder in listed {
                text.push_str("\n  ");
                text.push_str(folder.shown());
            }
            Ok(text)
        }
        "add" if !path.is_empty() => add_folder(parts, path),
        "remove" if !path.is_empty() => {
            match folders.remove(path, &parts.config.cwd, &parts.home) {
                Some(folder) => Ok(tr!(folder_removed, folder = folder.shown()).to_string()),
                None => Err(tr!(folder_unknown, folder = path).to_string()),
            }
        }
        _ => Err(tr!(folder_usage).to_string()),
    }
}

/// One folder beside the working one, after the same check `/folder add` makes; one known already is no fault.
pub fn add_folder(parts: &Parts, path: &str) -> Said {
    let deny = parts
        .config
        .sandbox
        .as_ref()
        .map(|sandbox| sandbox.deny_read.clone())
        .unwrap_or_default();
    let folder = glut::folders::check(path, &parts.config.cwd, &parts.home, &deny)
        .map_err(|reason| tr!(folder_refused, reason = reason).to_string())?;
    let shown = folder.shown().to_owned();
    match parts.tools.folders().add(folder) {
        Ok(()) => Ok(tr!(folder_added, folder = shown).to_string()),
        Err(glut::folders::NotAdded::Known) => Ok(tr!(folder_known, folder = shown).to_string()),
        Err(glut::folders::NotAdded::Overlaps(other)) => {
            Err(tr!(folder_overlaps, folder = shown, other = other).to_string())
        }
    }
}

/// `/rule` (T73b): `<text>` adds a rule for this session, alone it lists them, `remove <n>` takes one back.
pub fn rule(rules: &mut Vec<String>, rest: &str) -> Said {
    if rest.is_empty() {
        if rules.is_empty() {
            return Ok(tr!(rule_none).to_string());
        }
        let listed: Vec<String> = rules
            .iter()
            .enumerate()
            .map(|(at, rule)| format!("{}. {}", at + 1, crate::transcript::clean(rule)))
            .collect();
        return Ok(format!("{}\n{}", tr!(rule_list), listed.join("\n")));
    }
    if rest == "remove" {
        return Err(tr!(rule_bad).to_string());
    }
    if let Some(number) = rest.strip_prefix("remove ") {
        return match number
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| (1..=rules.len()).contains(n))
        {
            Some(n) => {
                rules.remove(n - 1);
                Ok(tr!(rule_removed, n = n).to_string())
            }
            None => Err(tr!(rule_bad).to_string()),
        };
    }
    if rules.len() >= MOST_RULES {
        return Err(tr!(rule_full, most = MOST_RULES).to_string());
    }
    // One line a rule: a pasted heading of its own would stand as a section of the instructions.
    let rule: String = rest
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MOST_RULE_CHARS)
        .collect();
    rules.push(rule);
    Ok(tr!(rule_added, n = rules.len()).to_string())
}

/// The most steps a turn takes (T73b): what `/maxsteps` kept, else the owner's `maxTurns`, else 100.
pub fn steps_now(parts: &Parts) -> u32 {
    Prefs::load(&parts.home)
        .steps
        .filter(|steps| (1..=glut::config::MOST_TURNS).contains(steps))
        .unwrap_or(parts.config.max_turns)
}

/// `/maxsteps`: alone how many steps a turn takes, with a number that many from the next turn on, kept.
pub fn maxsteps(parts: &Parts, rest: &str) -> Said {
    if rest.is_empty() {
        return Ok(tr!(steps_now, n = steps_now(parts)).to_string());
    }
    let steps = rest
        .parse::<u32>()
        .ok()
        .filter(|steps| (1..=glut::config::MOST_TURNS).contains(steps))
        .ok_or_else(|| tr!(steps_bad, most = glut::config::MOST_TURNS).to_string())?;
    prefs::set_steps(&parts.home, steps)?;
    Ok(tr!(steps_set, n = steps).to_string())
}

/// Whether Glut asks the model once to go on (T73): what `/nudge` kept, else the owner's settings.
pub fn nudges(parts: &Parts) -> bool {
    Prefs::load(&parts.home).nudge.unwrap_or(parts.config.nudge)
}

/// `/nudge`: alone whether Glut asks the model once to go on, `on|off` switches it, kept.
pub fn nudge(parts: &Parts, rest: &str) -> Said {
    match rest {
        "on" | "off" => {
            prefs::set_nudge(&parts.home, rest == "on")?;
            Ok(if rest == "on" {
                tr!(nudge_on)
            } else {
                tr!(nudge_off)
            }
            .to_string())
        }
        _ => Ok(tr!(nudge_now, nudge = if nudges(parts) { "on" } else { "off" }).to_string()),
    }
}

/// `/context`: how full the window is, with what the guides cost; `/context <tokens>` sets it for this model and
/// keeps it.
pub fn context(parts: &Parts, engine: &mut Engine, used: u64, rest: &str) -> Said {
    if rest.is_empty() {
        let mut text = match engine.window {
            Some((window, from)) => tr!(
                context_known,
                used = used,
                window = window,
                share = used * 100 / u64::from(window.max(1)),
                from = match from {
                    session::WINDOW_SET_HERE => tr!(window_set_here),
                    session::WINDOW_FROM_LIST => tr!(window_from_list),
                    other => other,
                }
            )
            .to_string(),
            None => tr!(context_unknown, used = used, model = engine.model_name()).to_string(),
        };
        // T35: the guides ride along on every turn — what they cost, so a growing one is seen.
        if parts.guide_chars > 0 {
            text.push('\n');
            text.push_str(&tr!(guides_cost, tokens = parts.guide_chars / 4));
        }
        return Ok(text);
    }
    let tokens = crate::app::parse_tokens(rest)
        .ok_or_else(|| tr!(context_unknown_word, word = rest).to_string())?;
    session::keep_window(&parts.home, engine, tokens)?;
    engine.window = Some((tokens, session::WINDOW_SET_HERE));
    Ok(tr!(context_set, model = engine.model_name(), tokens = tokens).to_string())
}

/// `/export` into a file (T54): the session as it was saved after its last turn; `masking` names the hidden
/// secrets' note.
pub fn export_to_file(
    session: &Session,
    parts: &Parts,
    asked: &crate::export::Asked,
    masking: bool,
) -> Said {
    let home = &parts.home;
    let text = crate::export::render(
        &crate::export::of_session(session, asked.full, home),
        asked.format,
    );
    let path = match &asked.path {
        Some(typed) => crate::export::named_path(typed, home, &parts.config.cwd),
        None => crate::export::default_path(home, &session.cwd, asked.format, chrono::Local::now()),
    };
    crate::export::write(&path, &text, home)?;
    Ok(exported(&path, home, masking))
}

/// What an export became, said where it stays.
pub fn exported(path: &Path, home: &Path, masking: bool) -> String {
    let shown = crate::projects::shown_path(path, home);
    if masking {
        tr!(export_written_masked, path = shown).to_string()
    } else {
        tr!(export_written, path = shown).to_string()
    }
}

/// The gate's rules as `/gate` lists them: what an "always" answer granted in this session (each with its rules),
/// then the settings' own as verdict and rule — an "always" rule shown once, among the granted.
pub fn gate_rules(
    gate: &glut::permissions::Gate,
) -> (Vec<Vec<String>>, Vec<(&'static str, String)>) {
    let granted: Vec<Vec<String>> = gate
        .granted()
        .iter()
        .map(|given| given.rules.clone())
        .collect();
    let mut given: Vec<&str> = granted.iter().flatten().map(String::as_str).collect();
    let rules = gate.rules();
    let mut settings = Vec::new();
    for rule in rules.allow.iter().rev() {
        if let Some(at) = given.iter().position(|text| *text == rule.text()) {
            given.remove(at);
        } else {
            settings.push(("allow", rule.text().to_owned()));
        }
    }
    settings.reverse();
    settings.extend(rules.ask.iter().map(|rule| ("ask", rule.text().to_owned())));
    settings.extend(
        rules
            .deny
            .iter()
            .map(|rule| ("deny", rule.text().to_owned())),
    );
    (granted, settings)
}

/// One step of the session that changed files, as `/steps` lists them, the newest first.
pub struct Step {
    pub run: String,
    pub turn: u32,
    /// The owner's message whose turn it was.
    pub prompt: String,
    pub commit: String,
}

/// The steps of the session's turns whose snapshots are still kept, the newest first.
pub fn steps(parts: &Parts, marks: &[crate::store::Mark]) -> Option<Vec<Step>> {
    let root = parts.config.snapshots.as_ref()?;
    let runs = glut::snapshot::runs(root, &parts.config.cwd);
    let mut steps = Vec::new();
    for mark in marks.iter().rev() {
        let Some(run) = &mark.run else { continue };
        let Some(record) = runs.iter().find(|record| &record.run == run) else {
            continue;
        };
        steps.extend(record.steps.iter().rev().map(|step| Step {
            run: run.clone(),
            turn: step.turn,
            prompt: mark.prompt.clone(),
            commit: step.commit[..step.commit.len().min(8)].to_owned(),
        }));
    }
    Some(steps)
}

/// The run the review of `scope` starts from: the latest turn that changed files, or the session's first — and its
/// prompt; `None` when no kept turn changed one.
pub fn review_run(
    parts: &Parts,
    marks: &[crate::store::Mark],
    scope: crate::review::Scope,
) -> Option<(String, String)> {
    let root = parts.config.snapshots.as_ref()?;
    let kept = glut::snapshot::runs(root, &parts.config.cwd);
    let changed = |mark: &&crate::store::Mark| {
        mark.run
            .as_ref()
            .is_some_and(|run| kept.iter().any(|record| &record.run == run))
    };
    let mark = match scope {
        crate::review::Scope::Turn => marks.iter().rev().find(changed),
        crate::review::Scope::Session => marks.iter().find(changed),
    }?;
    Some((mark.run.clone()?, mark.prompt.clone()))
}

/// `/init` in a folder without a guide (T35).
pub const INIT_NEW: &str = "/init: write a short AGENTS.md for this folder, the guide every coding agent reads here. \
    Look first — the build and test commands, how the code is laid out, what is easy to get wrong — reading only \
    what you need. Keep it under 4 KB: commands, a few lines of layout, the traps; nothing a reader finds in a \
    minute, nothing Glut's own conventions already say. Show the draft and ask before you write the file.";

/// `/init` moving the guide `claude` (a `CLAUDE.md`) into an `AGENTS.md` beside it (T35).
pub fn init_migrate(claude: &str) -> String {
    format!(
        "/init: move {claude} into an AGENTS.md beside it, so every harness reads one guide. Write AGENTS.md with its \
         text — shortened where it repeats itself or says what the code shows, under 4 KB if it can be —, then leave \
         {claude} as the single line `@AGENTS.md`, which Claude Code follows. Show both before you write them."
    )
}

/// `/init` checking the guide `name` there is (T35).
pub fn init_check(name: &str) -> String {
    format!(
        "/init: check the guide {name}, which every turn carries. Is it still true (commands, layout, \
         traps)? Is anything in it that a reader finds in a minute, or that belongs in docs/ with a pointer left \
         behind? Propose a shorter version — under 4 KB if it can be, nothing important lost — and say what you would \
         drop or move. Show it and ask before you change the file."
    )
}

/// What `/init` comes to: the owner asked first whether a `CLAUDE.md` moves into `AGENTS.md`, or the turn's prompt
/// with the guide it is about.
pub enum Init {
    /// The `CLAUDE.md` (as the folder names it) the question is about.
    AskClaude(String),
    Turn(String, std::path::PathBuf),
}

/// `/init` (T35): a short guide for the folder. With only a `CLAUDE.md` the owner is asked once whether it moves into
/// `AGENTS.md` — a no is kept, and the guide checked is then `CLAUDE.md` itself; `--migrate` asks again.
pub fn init(parts: &Parts, rest: &str) -> Result<Init, String> {
    let migrate = rest.trim() == "--migrate";
    if !rest.trim().is_empty() && !migrate {
        return Err(tr!(init_usage).to_string());
    }
    // The nearest guide the session read decides: an `AGENTS.md` is checked, a `CLAUDE.md` (read only where its
    // folder has no `AGENTS.md`) is the question — none, and a new one is written in the working folder.
    let nearest = parts.guide_paths.last().cloned();
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|file| file == name);
    let claude = nearest.as_ref().filter(|path| named(path, "CLAUDE.md"));
    let folder = claude
        .and_then(|path| path.parent())
        .unwrap_or(&parts.config.cwd)
        .display()
        .to_string();
    if migrate {
        prefs::set_keep_claude(&parts.home, &folder, false)?;
    }
    let kept = Prefs::load(&parts.home).keep_claude.contains(&folder);
    if let Some(claude) = claude
        && !kept
    {
        return Ok(Init::AskClaude(shown_in(parts, claude)));
    }
    Ok(match nearest {
        Some(path) => Init::Turn(init_check(&shown_in(parts, &path)), path),
        None => Init::Turn(INIT_NEW.to_owned(), parts.config.cwd.join("AGENTS.md")),
    })
}

/// The owner's answer to `/init`'s question about `claude`: moved into `AGENTS.md`, or kept (and remembered).
pub fn init_with(
    parts: &Parts,
    claude: &str,
    migrate: bool,
) -> Result<(String, std::path::PathBuf), String> {
    let claude_path = parts.config.cwd.join(claude);
    if migrate {
        return Ok((
            init_migrate(claude),
            claude_path.with_file_name("AGENTS.md"),
        ));
    }
    let folder = claude_path
        .parent()
        .map(|folder| folder.display().to_string())
        .unwrap_or_default();
    prefs::set_keep_claude(&parts.home, &folder, true)?;
    Ok((init_check(claude), claude_path))
}

/// A path as the working folder names it.
fn shown_in(parts: &Parts, path: &Path) -> String {
    path.strip_prefix(&parts.config.cwd).map_or_else(
        |_| path.display().to_string(),
        |rel| rel.display().to_string(),
    )
}

/// `/route` (T90): which hosts serve the model on OpenRouter — alone the order Glut sends and why, with the owner's
/// rule and the model's pins; `prefer|avoid <hosts>`, `quant <levels>`, `strict on|off`, `off` pin the model;
/// `rule on|off`, `rule margin|uptime|speed|wait <n>` set the rule. Kept in `providers.json`; the caller sets the
/// model's routing anew after a change (`Ok((said, true))`).
pub fn route(
    home: &Path,
    provider: &str,
    endpoint: &glut::ModelEndpoint,
    rest: &str,
) -> Result<(String, bool), String> {
    use glut::providers::routing::{self, RouteError};
    let words: Vec<&str> = rest.split_whitespace().collect();
    let model = endpoint.model.as_str();
    let said = |error: RouteError| match error {
        RouteError::Usage => tr!(route_usage).to_string(),
        RouteError::Quant => format!(
            "{} ({})",
            tr!(route_usage),
            routing::QUANTIZATIONS.join(", ")
        ),
        RouteError::NotHere => tr!(route_not_here, provider = provider).to_string(),
        RouteError::NoProvider => tr!(no_provider_entry, provider = provider).to_string(),
        RouteError::File(error) => error,
    };
    match routing::parse(&words).map_err(said)? {
        Some(change) => {
            routing::apply(home, provider, &endpoint.base_url, model, change).map_err(said)?;
            Ok((tr!(route_set, model = model).to_string(), true))
        }
        None => {
            let shown = routing::show(home, provider, &endpoint.base_url, model).map_err(said)?;
            Ok((shown_route(&shown), false))
        }
    }
}

/// Why a host stands where it does, with its quantization: `fp8, the tier the rule chose`.
fn standing(host: &glut::providers::routing::Ranked) -> String {
    use glut::providers::routing::Standing;
    let level = crate::transcript::clean(host.quantization.as_deref().unwrap_or("?"));
    match host.standing {
        Standing::Chosen => format!("{level}, {}", tr!(route_chosen)),
        Standing::Slow => format!("{level}, {}", tr!(route_slow)),
        Standing::Other => level,
    }
}

/// `/route`'s answer without words: the rule, the model's pins and the order Glut sends.
fn shown_route(shown: &glut::providers::routing::Shown) -> String {
    let (rule, pins) = (&shown.rule, &shown.pins);
    let mut lines = vec![
        tr!(
            route_rule_now,
            on = if rule.on { "on" } else { "off" },
            margin = (rule.margin * 100.0).round(),
            uptime = rule.uptime,
            speed = rule.tokens_per_second,
            wait = rule.first_token_seconds
        )
        .to_string(),
    ];
    if !pins.is_empty() {
        let mut said = Vec::new();
        if !pins.prefer.is_empty() {
            said.push(format!("prefer {}", pins.prefer.join(" ")));
        }
        if !pins.avoid.is_empty() {
            said.push(format!("avoid {}", pins.avoid.join(" ")));
        }
        if !pins.quant.is_empty() {
            said.push(format!("quant {}", pins.quant.join(" ")));
        }
        if pins.strict {
            said.push("strict".to_owned());
        }
        lines.push(tr!(route_pins, pins = said.join(" · ")).to_string());
    }
    let model = crate::transcript::clean(&shown.model);
    match &shown.hosts {
        Some(hosts) if !hosts.is_empty() => {
            lines.push(tr!(route_head, model = model).to_string());
            for (at, host) in hosts.iter().enumerate() {
                let price = host
                    .price
                    .map(|price| format!(" · ${price:.2}/M"))
                    .unwrap_or_default();
                let speed = host
                    .tokens_per_second
                    .map(|speed| format!(" · {speed:.0} tok/s"))
                    .unwrap_or_default();
                lines.push(format!(
                    "  {}. {} — {}{price}{speed}",
                    at + 1,
                    crate::transcript::clean(&host.tag),
                    standing(host)
                ));
            }
        }
        Some(_) => {}
        None => lines.push(tr!(route_none_read, model = model).to_string()),
    }
    lines.join("\n")
}
