Die Session zum Lesen oder Teilen ausgeben.

/export — Markdown nach `~/.glut/exports`
/export html — eine HTML-Datei, die Styles darin, ohne Skript
/export jsonl — die Nachrichten als JSON-Zeilen
/export copy — das Markdown statt in eine Datei in die Zwischenablage
/export full — nimmt die ganzen Werkzeug-Ergebnisse und den Systemprompt dazu
/export <format> <path> — in eine eigene Datei (`~/` ist dein Home-Ordner, ein relativer Pfad liegt im Arbeitsordner)

Die Wörter dürfen in beliebiger Reihenfolge stehen, der Pfad zuletzt. Ohne full zeigt der Export, was das Modell gesehen hat: lange Ergebnisse gekürzt. Eine vorhandene Datei wird nie überschrieben. Die Keys, die Glut verborgen hat, bleiben verborgen, aber was du selbst getippt hast, kann doch einen enthalten: Lies einen Export, bevor du ihn teilst. In /resume exportiert ⌃E eine Session der Liste.
