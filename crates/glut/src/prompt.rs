//! The harness's own system prompt (H6, T73): who the model is, where it works, and how to work — the last spelled
//! out since a model left alone with four sentences read for forty steps, announced its next step and stopped (MiMo
//! on T70, 2026-10-27). The tools describe themselves through the API's `tools` field.

use std::path::Path;

/// The fixed part. Kept to a few lines: every token here is paid on every turn of every run.
const BASE: &str = "You are an agent working in a folder on the user's machine. \
Use the tools to read and change files; paths are relative to the working folder. \
Look before you change: read a file before you edit it. \
When the task is done, answer briefly with what you did or found, and do not call another tool.";

/// How to work (T73, grilled with the owner): a section of its own after [`BASE`], the same for every model — the
/// tokens are paid once per cached session, and quality comes before them.
const WORKING: &str = "## How to work
- Carry the task through to its end. Never write what you will do next and then stop: do it. Stop only when the \
task is done, when you need the owner's answer, or when something blocks you — then say what.
- Read as much as the task needs, then change. Long reading without a change is a sign to start, or to say what is \
unclear.
- Reads that do not depend on each other may go into one reply — a few at a time, never more than 16 calls in a \
reply. Change files with edit_file rather than writing them anew.
- When a command fails, read the error and fix its cause; do not repeat the same command unchanged. After two \
failures with one approach, take another or say what blocks you.
- Before you say the task is done, check it: run the project's tests or its gate where it has them, and look at \
their result.
- Ask the owner only where the answer changes the result; otherwise decide, and say what you decided and why.
- Do what was asked and stay in its scope; match the style of the code around your change.
- Nothing that cannot be undone without the owner's word: no commit, no push, no deleting the owner's files.
- What the owner tells you holds until the owner takes it back, not only for this turn.
- End with what you changed and where, what you checked, and what is still open — briefly.
- Answer in the language the owner writes in.
- Lines that begin with [glut] come from Glut, the harness, not from the owner: follow them.";

/// The task list's part of [`WORKING`], only where `todo_write` is offered.
const WORKING_TODOS: &str = "
- For work of three steps or more, write the task list with todo_write first. Keep it current: mark an item \
in_progress when you start it and done the moment it is done — the owner watches the list live. Make the items \
the steps you will really take one after another; what you do in one go, mark done together once it is.";

/// The system prompt for a run in `cwd` on `date` with `model`, with the caller's instructions, if any, after it;
/// `todos`: `todo_write` is offered, and the guide says how to keep its list.
/// It names the harness and the model, so a model asked who it is says where it runs rather than a name it learned
/// (DeepSeek in the Glut chat called itself "Claude", `docs/plans/glut-round.md`).
pub fn system_prompt(
    cwd: &Path,
    date: &str,
    model: &str,
    instructions: Option<&str>,
    todos: bool,
) -> String {
    let mut prompt = format!(
        "{BASE}\nYou run in Glut, Axiomata's own agent harness, on the model {model}.\n\
         Working folder: {}\nToday: {date}\n\n{WORKING}{}",
        cwd.display(),
        if todos { WORKING_TODOS } else { "" }
    );
    if let Some(extra) = instructions.map(str::trim).filter(|text| !text.is_empty()) {
        prompt.push_str("\n\n");
        prompt.push_str(extra);
    }
    prompt
}

/// The system prompt of a subagent (HS8, D5): its own, because isolation is the point of one — it works on a
/// single task handed to it and returns the result; the parent's instructions are never repeated here, the
/// parent's model passes along whatever context the subagent needs.
pub fn subagent_prompt(cwd: &Path, date: &str) -> String {
    format!(
        "You are a subagent: one worker inside a larger run on the user's machine.\n\
         Do only the task in the user message; use the tools, and read a file before you change it.\n\
         When the task is done, answer with its result alone — no preamble, no account of how you worked. \
         The agent that sent you continues from your answer.\n\
         Working folder: {}\nToday: {date}",
        cwd.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_names_folder_and_date_and_appends_instructions() {
        let prompt = system_prompt(
            Path::new("/w"),
            "2026-10-13",
            "m/x",
            Some("  Be a reviewer.  "),
            true,
        );
        assert!(prompt.contains("Working folder: /w\nToday: 2026-10-13"));
        assert!(
            prompt.contains("You run in Glut, Axiomata's own agent harness, on the model m/x.")
        );
        assert!(prompt.ends_with("\n\nBe a reviewer."));
        assert!(prompt.contains("## How to work") && prompt.contains("todo_write"));
        let bare = system_prompt(Path::new("/w"), "2026-10-13", "m/x", Some("   "), false);
        assert!(bare.ends_with(WORKING), "blank instructions add nothing");
        assert!(
            !bare.contains("todo_write"),
            "no list to keep where none is offered"
        );
    }

    #[test]
    fn the_prompt_names_the_runs_limit_of_calls_per_reply() {
        assert!(WORKING.contains(&format!(
            "never more than {} calls",
            crate::run::MAX_CALLS_PER_TURN
        )));
    }

    #[test]
    fn the_prompt_stays_within_its_bounds() {
        // Some 500 tokens since T73 (the owner's bound): a guard against it growing into a page.
        let prompt = system_prompt(
            Path::new("/w"),
            "d",
            "deepseek/deepseek-v4.1-flash",
            None,
            true,
        );
        assert!(prompt.len() < 3_000, "{}", prompt.len());
    }

    #[test]
    fn the_subagent_prompt_says_subagent_and_carries_folder_and_date() {
        let prompt = subagent_prompt(Path::new("/w"), "2026-10-14");
        assert!(prompt.contains("subagent"));
        assert!(prompt.contains("result alone"));
        assert!(prompt.contains("Working folder: /w\nToday: 2026-10-14"));
    }
}
