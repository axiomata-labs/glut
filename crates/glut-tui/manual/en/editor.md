Choose the editor Glut opens files and the input in.

/editor — the editors found on your system
/editor <command> — your own command, with `{file}` and `{line}` where they go

Without `{file}` the file is added at the end. A windowed editor must wait until the file is closed (`code --wait`). ⌃X E edits the input in it; /todo open and /memory open use it too. Kept in `~/.glut/prefs.json`.

/editor nvim +{line} {file}
