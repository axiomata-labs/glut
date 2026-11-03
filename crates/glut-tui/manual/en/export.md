Write the session out to read or share.

/export — Markdown into `~/.glut/exports`
/export html — one HTML file, its styles inside, no script
/export jsonl — the messages as JSON lines
/export copy — the Markdown onto the clipboard instead of a file
/export full — adds the whole tool results and the system prompt
/export <format> <path> — into a file of your own (`~/` is your home, a relative path lies in the working folder)

The words go in any order, the path last. Without full the export shows what the model saw: long results shortened. An existing file is never written over. The keys Glut hid stay hidden, but what you typed yourself may still hold one: read an export before you share it. In /resume, ⌃E exports a listed session.
