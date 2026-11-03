Write a short `AGENTS.md` for this folder, or check and shorten the guide it has.

/init — without a guide Glut writes `AGENTS.md`; with one it checks it and keeps it short
/init --migrate — asks again whether `CLAUDE.md` moves into `AGENTS.md`

With only a `CLAUDE.md`, Glut asks once whether it moves into `AGENTS.md` (`CLAUDE.md` then imports it, one guide for every harness); a no is kept. The guides ride along on every turn: /context shows what they cost.
