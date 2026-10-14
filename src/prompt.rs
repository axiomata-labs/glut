//! The harness's own system prompt: short on purpose (H6). The tools describe themselves through the API's `tools`
//! field, so the prompt says only who the model is, where it works, and how to answer.

use std::path::Path;

/// The fixed part. Kept to a few lines: every token here is paid on every turn of every run.
const BASE: &str = "You are an agent working in a folder on the user's machine. \
Use the tools to read and change files; paths are relative to the working folder. \
Look before you change: read a file before you edit it. \
When the task is done, answer briefly with what you did or found, and do not call another tool.";

/// The system prompt for a run in `cwd` on `date`, with the caller's instructions, if any, after it.
pub fn system_prompt(cwd: &Path, date: &str, instructions: Option<&str>) -> String {
    let mut prompt = format!("{BASE}\nWorking folder: {}\nToday: {date}", cwd.display());
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
        let prompt = system_prompt(Path::new("/w"), "2026-10-13", Some("  Be a reviewer.  "));
        assert!(prompt.contains("Working folder: /w\nToday: 2026-10-13"));
        assert!(prompt.ends_with("\n\nBe a reviewer."));
        let bare = system_prompt(Path::new("/w"), "2026-10-13", Some("   "));
        assert!(
            bare.ends_with("Today: 2026-10-13"),
            "blank instructions add nothing"
        );
    }

    #[test]
    fn the_base_prompt_stays_small() {
        // A guard against the prompt growing back into a page: it is paid on every turn.
        assert!(system_prompt(Path::new("/w"), "d", None).len() < 600);
    }

    #[test]
    fn the_subagent_prompt_says_subagent_and_carries_folder_and_date() {
        let prompt = subagent_prompt(Path::new("/w"), "2026-10-14");
        assert!(prompt.contains("subagent"));
        assert!(prompt.contains("result alone"));
        assert!(prompt.contains("Working folder: /w\nToday: 2026-10-14"));
    }
}
