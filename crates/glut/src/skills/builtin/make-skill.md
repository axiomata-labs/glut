---
name: make-skill
description: Makes a skill of Glut's, or changes one — skills/<name>/SKILL.md, the instructions Glut loads by name. Use when the owner wants a repeatable way of doing something saved ("make a skill for …", "change my release skill").
---
You make or change one of Glut's skills with the owner. A skill is a folder `skills/<name>/` with a `SKILL.md`: YAML
frontmatter, then the instructions a run follows. The owner starts it with `/skill <name>`; the model loads it by its
`description` when it fits.

1. Look first: `glut_file` with `action: list` (scope `owner`, and `project` when it belongs to this repository). To
   change a skill, `read` its `SKILL.md`.
2. Ask the owner only what is still open, in one short message:
   - its name: lower case letters, digits and dashes (`release-notes`);
   - when it is used: one or two sentences — the `description` the model picks it by;
   - what it does, step by step, and what it ends with;
   - `allowed-tools` when a run of it should be narrowed, in Glut's rules (Claude Code's spelling: `Read`, `Edit`,
     `Bash(cargo test:*)`, …) — they only narrow, never allow beyond the owner's rules;
   - a model of its own, rarely: call `glut_file` with `action: pick_model` and take the model from the answer —
     it must be a model of the provider the skill will run on (a skill names no provider); `effort` (none,
     minimal, low, medium, high, xhigh, max) is optional;
   - tell the owner plainly: `allowed-tools`, `model`, `effort` and `timeout_secs` hold when the skill runs on its
     own (`glut run --skill <name>`); in a session (`/skill`, or loaded by the model) its instructions are followed
     but nothing is narrowed — for a narrowed helper inside a session, an agent (make-agent) is the way;
   - where it goes: `owner` (every project) or `project` (this repository only, a folder the owner trusts).
3. Write the `SKILL.md`. Frontmatter: `name` (the folder's), `description`, then only what is set —
   `allowed-tools`, `model`, `effort`, `timeout_secs`. The body speaks to the model that runs it: concrete steps,
   the commands to use, what to check, what to answer with. Short beats complete.

   ```markdown
   ---
   name: release-notes
   description: Writes the release notes from the commits since the last tag. Use before a release.
   allowed-tools: Read, Bash(git log:*), Bash(git describe:*), Edit
   ---
   1. Find the last tag: `git describe --tags --abbrev=0`.
   2. …
   ```
4. Show the owner the whole file — for a change, what changes — and ask. Only after their yes call `glut_file`
   `action: write` with `kind: skill`; Glut asks them once more with the content, which is expected. Never put a
   key, a token or a password into a skill: name an environment variable and tell the owner.
5. Say that it counts from the next session, and that `/skill <name>` starts it.
