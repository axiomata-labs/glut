Ob die Language Server dem Modell helfen.

/lsp on — nach dem Bearbeiten kommen die Fehler der Datei mit dem Ergebnis, und das Modell hat das Werkzeug lsp: wo ein Symbol definiert ist, jede Stelle, an der es verwendet wird, sein Typ und seine Doku, und ein Umbenennen über das ganze Projekt
/lsp off — kein Server läuft: ihre Prozesse enden sofort, keine Fehler nach dem Bearbeiten, kein Werkzeug lsp
/lsp — die Wahl jetzt, und jeder Server dieser Session: bereit, indexiert das Projekt, oder aus für die Session

Glut startet nur Server, die nie Code des Projekts ausführen (rust-analyzer ohne Build-Skripte und Proc-Macros, pyright und die Server für TOML, YAML, JSON, CSS, HTML und Markdown), nur in der Sandbox ohne Netz und nur, wo das Gate ihre Befehlszeile erlaubt. Ein Server startet beim ersten Bearbeiten oder der ersten Frage seiner Sprache und bleibt für die Session; rust-analyzer braucht für ein großes Projekt einige Sekunden zum Indexieren, solange sagt lsp das, und das Modell nimmt grep. Ein Umbenennen ist eine Änderung: einmal für alle Dateien gefragt, alle oder keine, vorher ein Snapshot. Gespeichert in `~/.glut/prefs.json`; `"lsp": false` in `~/.glut/settings.json` schaltet es für jeden Lauf aus, `glut run --no-lsp` für einen.
