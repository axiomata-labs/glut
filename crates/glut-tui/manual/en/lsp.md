Whether the language servers help the model.

/lsp on — after an edit the file's errors come with the result, and the model has the lsp tool: where a symbol is defined, every place it is used, its type and docs, and a rename across the project
/lsp off — no server runs: their processes end at once, no errors after an edit, no lsp tool
/lsp — the choice now, and each server of this session: ready, indexing the project, or off for the session

Glut starts only servers that never run the project's code (rust-analyzer without build scripts and proc macros, pyright, and the servers of TOML, YAML, JSON, CSS, HTML and Markdown), only in the sandbox without network, and only where the gate allows its command line. A server starts at the first edit or question of its language and stays for the session; rust-analyzer needs some seconds to index a large project, meanwhile lsp says so and the model uses grep. A rename is one change: asked once for all its files, all or none, a snapshot before it. Kept in `~/.glut/prefs.json`; `"lsp": false` in `~/.glut/settings.json` turns it off for every run, `glut run --no-lsp` for one.
