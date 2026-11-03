Keep Glut's own data in ~/.glut from growing without end.

/clean — what Glut keeps, kind by kind, and what of it can go by your terms
/clean now — cleans at once, in the background
/clean keep — the terms
/clean keep <kind> <days>d — sets one: snapshots, journals, sessions, results or lean

What goes: the snapshots of a run older than its term (7 days) — a folder's whole store once none is left, the folder there or not; an unfinished run's journal (7 days); a session not written for 90 days, with its event log and its long results; the long results of no session after 30 days; the images no kept session holds; the lines of lean.jsonl older than 90 days. Exports are only counted: they are your files.

Once a day Glut cleans on its own as it starts — here and in `glut run` — and says what it freed. Build output and caches (a folder with a CACHEDIR.TAG, below node_modules/ or a Cargo project's target/) get no snapshots at all.

The terms are kept in `~/.glut/clean.json`. On the command line: `glut clean`, `glut clean --dry-run`, `glut clean keep sessions 30d`; `glut clean --dry-run --json` writes the report for a program to read.
