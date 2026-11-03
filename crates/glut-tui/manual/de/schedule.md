Pläne, die laufen, während Glut geschlossen ist: eine Aufgabe zu einer Uhrzeit oder alle so und so lange.

/schedule — der Reiter Läufe (⌃X R): jeder Plan mit seiner nächsten Zeit und seinem letzten Ergebnis
/schedule list — die Pläne im Stream
/schedule add <name> [flags] <task> — macht einen Plan, oder ändert den dieses Namens
/schedule run <name> — führt ihn jetzt aus, neben Glut
/schedule pause <name> — hält ihn an, bis resume
/schedule resume <name> — er läuft wieder zu seinen Zeiten
/schedule remove <name> — der Plan und sein Protokoll gehen; seine Sessions bleiben

Die Flags sind die von `glut schedule add`:
- `--at 07:00` eine Uhrzeit, mit `--days mo-fr` oder `sa,su` (ohne: jeden Tag)
- `--every 2h` ein Intervall von 10m bis 7d, statt --at
- `--mode ask|build|plan|dont_ask|auto` was er ohne Regel darf; ask, wenn nichts genannt ist
- `--allow <rule>`, `--deny <rule>` Regeln in der Form von Claude Code, etwa `Bash(cargo test:*)`; wiederholbar
- `--folder <path>` ein Ordner neben seinem eigenen; wiederholbar
- `--max-usd 1`, `--max-minutes 30` das Meiste, was ein Lauf kosten und dauern darf: 1 $ und 30 min, wenn nichts genannt ist
- `--max-tokens 1M` auf einem Abo die meisten Tokens, die ein Lauf verbrauchen darf: 1M, wenn nicht genannt
- `--replace` die genannten Regeln und Ordner ersetzen die des Plans, statt zu ihnen hinzuzukommen
- `--cwd <path>`, `--provider <id>`, `--model <id>` sein Ordner (sonst dieser), Provider und Modell, festgelegt, wenn er gemacht wird

Die Aufgabe ist eine Nachricht, oder /skill <name> [wofür]. launchd startet jeden Lauf, nur solange du angemeldet bist; eine im Ruhezustand verpasste Zeit wird beim Aufwachen einmal nachgeholt. Niemand ist da, um zu antworten: Jede Frage wird abgelehnt und vermerkt, also erlaube, was er braucht. Ein Lauf ist eine eigene gespeicherte Session, gemeldet durch eine macOS-Benachrichtigung. Im Reiter Läufe: ⏎ öffnet einen Plan oder die Session eines Laufs, r führt ihn jetzt aus, p hält an oder setzt fort, d löscht ihn.

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" führe die Tests aus und melde, was fehlschlägt
