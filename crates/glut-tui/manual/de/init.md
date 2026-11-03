Eine kurze `AGENTS.md` für diesen Ordner schreiben, oder den vorhandenen Leitfaden prüfen und kürzen.

/init — ohne Leitfaden schreibt Glut `AGENTS.md`; mit einem prüft es ihn und hält ihn kurz
/init --migrate — fragt erneut, ob `CLAUDE.md` in `AGENTS.md` umzieht

Gibt es nur eine `CLAUDE.md`, fragt Glut einmal, ob sie in `AGENTS.md` umzieht (`CLAUDE.md` importiert sie dann, ein Leitfaden für jedes Harness); ein Nein bleibt gemerkt. Die Leitfäden fahren bei jedem Turn mit: /context zeigt, was sie kosten.
