Let this pane play a role: a named agent's prompt and tools, never its model.

/role — the roles to pick from
/role <name> — plays that role from now on
/role none — an all-rounder again, with every tool

Glut brings reviewer, tester and docs; your own roles live in `~/.glut/agents`, a project's in its `.glut/agents` (read only in a trusted folder). A role's tools take effect at once, its prompt from the next turn. A role that narrows the tools needs the Gate.
