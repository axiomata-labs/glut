Whether Glut looks once a day for a newer version and shows it in the footer.

/update on — once a day, in the background, Glut reads the newest version's number from `https://glut-ai.com/latest.txt`; when it is newer than this one, the footer shows it quietly (`↑ 0.1.2`)
/update off — Glut asks nothing, and the footer shows nothing

/update alone says whether a newer Glut is out and the command that updates this one, by how it was installed (`brew upgrade glut`, `cargo install glut-tui` or the install script). Glut never updates itself, and never asks in a plan's run, a loop's round, `glut run`, `glut acp` or the arena. Kept in `~/.glut/prefs.json`; `"updateCheck": false` in `~/.glut/settings.json` turns it off wherever /update did not say otherwise.
