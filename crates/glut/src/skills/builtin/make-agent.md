---
name: make-agent
description: Makes a named subagent of Glut's, or changes one — agents/<name>.md with its purpose, its tools and maybe a model or provider of its own. Use when the owner asks for a subagent ("make me an agent that …", "let the reviewer use another model").
---
You make or change one of Glut's named subagents with the owner. A subagent is a file `agents/<name>.md`: YAML
frontmatter, then its system prompt. The main agent hands work to it through the `task` tool (`agent: <name>`).

1. Look first: `glut_file` with `action: list` (scope `owner`, and `project` when the work belongs to this repository).
   To change an agent, `read` its file.
2. Ask the owner only what is still open, in one short message:
   - its name: lower case letters, digits and dashes (`reviewer`, `test-writer`);
   - when to use it: one or two sentences — the main agent picks it by this `description`;
   - what it does and what it answers with — its system prompt;
   - its tools, in Glut's rules (Claude Code's spelling): `Read`, `Edit`, `Write`, `Bash(cargo test:*)`,
     `Network(docs.rs)`, `WebFetch`, a tool by its name (`grep`, `mcp__mail`) … They only narrow: no `tools` line
     gives it every tool of the main agent but `task`;
     `tools: []` gives it none. A subagent whose tools name no `Network` rule has no network in its commands;
   - whether it runs on a model of its own: if so, call `glut_file` with `action: pick_model` and take the provider
     and the model from the answer — never guess an id. `effort` (none, minimal, low, medium, high, xhigh, max) is optional;
   - where it goes: `owner` (every project) or `project` (this repository only, a folder the owner trusts).
3. Write the file. Frontmatter: `name`, `description`, then only what is set — `tools`, `model`, `provider`,
   `effort`. The system prompt speaks to the subagent ("You …"): its job, how to work, what to answer with — the
   agent that sent it continues from that answer. Keep it short and concrete.

   ```markdown
   ---
   name: reviewer
   description: Reviews a diff for defects before a commit. Use when a change is ready to be committed.
   tools: Read, grep, Bash(git diff:*)
   model: anthropic/claude-opus
   provider: openrouter
   ---
   You review the uncommitted diff of this repository. Report only real defects, each with file:line …
   ```
   Never put a key, a token or a password into the file: name an environment variable and tell the owner.
4. Show the owner the whole file — for a change, what changes — and ask. Only after their yes call `glut_file` with
   `action: write`, `scope`, `kind: agent`, `name` and the whole `content`; Glut asks them once more with the
   content, which is expected.
5. Say that it counts from the next session, and how to use it ("let reviewer check this").
