Damit Glut seine eigene Ablage in ~/.glut nicht endlos wachsen lässt.

/clean — was Glut ablegt, Posten für Posten, und was davon nach deinen Fristen weg kann
/clean now — räumt sofort auf, im Hintergrund
/clean keep — die Fristen
/clean keep <kind> <days>d — setzt eine: snapshots, journals, sessions, results oder lean

Was geht: die Snapshots eines Laufs, die älter als ihre Frist sind (7 Tage) — das ganze Lager eines Ordners, sobald keiner mehr übrig ist, ob der Ordner noch da ist oder nicht; das Journal eines unvollendeten Laufs (7 Tage); eine Session, die 90 Tage nicht geschrieben wurde, mit ihrem Ereignisprotokoll und ihren langen Ergebnissen; die langen Ergebnisse ohne Session nach 30 Tagen; die Bilder, die keine behaltene Session mehr nennt; die Zeilen von lean.jsonl, die älter als 90 Tage sind. Exporte werden nur gezählt: das sind deine Dateien.

Einmal am Tag räumt Glut beim Start von selbst auf — hier und in `glut run` — und sagt, was frei wurde. Build-Ausgaben und Caches (ein Ordner mit CACHEDIR.TAG, unter node_modules/ oder dem target/ eines Cargo-Projekts) bekommen gar keine Snapshots.

Die Fristen stehen in `~/.glut/clean.json`. Auf der Kommandozeile: `glut clean`, `glut clean --dry-run`, `glut clean keep sessions 30d`; `glut clean --dry-run --json` schreibt den Bericht für ein Programm.
