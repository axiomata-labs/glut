Sehen, wie voll das Fenster des Modells ist, oder Glut sagen, wie groß es ist.

/context — die benutzten Tokens, das Fenster und woher seine Größe stammt, und was die Leitfäden (`AGENTS.md`, `CLAUDE.md`) bei jedem Turn kosten
/context <tokens> — setzt das Fenster dieses Modells: 65536, 64k oder 1M

Ein Fenster, das du setzt, bleibt in `~/.glut/providers.json` für dieses Modell erhalten und gilt vor der Liste des Providers. Glut kompaktiert von selbst kurz bevor das Fenster voll ist.
