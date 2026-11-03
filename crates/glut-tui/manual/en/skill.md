Start a Skill: instructions for a kind of task, which the model loads and follows.

/skill — the Skills to pick from
/skill <name> [what for] — runs that Skill on what follows its name

Glut brings make-agent, make-skill and make-hook to write your own. Yours live in `~/.glut/skills`, a project's in its `.glut/skills` (read only in a trusted folder); one of the same name hides Glut's.

/skill make-hook block git push on main
