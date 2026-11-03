See how full the model's window is, or tell Glut how large it is.

/context — the tokens used, the window and where its size comes from, and what the guides (`AGENTS.md`, `CLAUDE.md`) cost on every turn
/context <tokens> — sets this model's window: 65536, 64k or 1M

A window you set is kept in `~/.glut/providers.json` for this model and counts before the provider's list. Glut compacts by itself shortly before the window is full.
