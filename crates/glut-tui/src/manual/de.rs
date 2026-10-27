//! The commands' manual pages in German (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 54] = [
    (
        "clear",
        "Eine neue, leere Session in diesem Ordner beginnen.\n\
         \n\
         /clear — das Gespräch, die Aufgabenliste des Plans, die Kosten und die Messung fangen bei null an\n\
         \n\
         Die Session, die du verlässt, bleibt erhalten: /resume öffnet sie wieder. Modell, Modus und Einstellungen \
         bleiben, wie sie sind.",
    ),
    (
        "resume",
        "Eine frühere Session dieses Ordners öffnen und dort weitermachen.\n\
         \n\
         /resume — die Liste der gespeicherten Sessions dieses Ordners, die neueste zuerst; eine Abzweigung steht \
         unter der Session, aus der sie kam\n\
         \n\
         In der Liste: Tippen filtert, ⏎ öffnet eine, ⌫ löscht eine (vorher wird gefragt; ihre Abzweigungen bleiben \
         als eigene Sessions), ⌃E schreibt eine als Markdown nach `~/.glut/exports`. Eine Session, die in einem \
         anderen Pane offen ist, erreichst du dort (⌃P).\n\
         \n\
         /search findet eine Session über das, was in ihr gesagt wurde.",
    ),
    (
        "search",
        "Eine gespeicherte Session über ihre Worte finden: die Liste von /resume, gefiltert nach dem Text in ihrem \
         Gespräch.\n\
         \n\
         /search <text> — die Sessions dieses Ordners, deren Gespräch den Text enthält\n\
         \n\
         In der Liste schaltet ⌃A zwischen diesem Ordner und allen Projekten um. ⏎ öffnet eine Session dieses Ordners; \
         bei der eines anderen Projekts steht dessen Ordner dabei — starte Glut dort, um mit ihr weiterzumachen.",
    ),
    (
        "btw",
        "Eine Nebenfrage stellen, ohne das Gespräch zu stören: Die Antwort kommt in einer Karte und wird nie Teil der \
         Session.\n\
         \n\
         /btw <question> — fragt zur Session dieses Panes, auch während sein Turn läuft\n\
         /btw <name> <question> — fragt zu einem anderen Pane oder einem Hintergrund-Agenten, über seine Nummer oder \
         seinen Namen\n\
         \n\
         Das Modell antwortet aus dem Gespräch, wie es gerade steht, und führt kein Werkzeug aus: Muss es etwas \
         nachschlagen, stell die Frage stattdessen als Nachricht. Die Kosten zählen zu den Kosten der Session. ⎋ \
         schließt die Karte, PgUp PgDn blättern darin.\n\
         \n\
         /btw reviewer wie weit bist du?",
    ),
    (
        "projects",
        "In einem anderen Projekt weitermachen: in einem der Ordner, in denen Glut gearbeitet hat, oder in irgendeinem \
         Ordner über seinen Pfad.\n\
         \n\
         /projects — die Liste: ★ angeheftet, ● dieses hier; ⏎ öffnet eines, p heftet an oder löst, x blendet eines \
         aus, bis darin wieder gearbeitet wird\n\
         /projects <path> — öffnet diesen Ordner (⇥ ergänzt den Namen eines Ordners)\n\
         \n\
         Arbeitet noch ein Turn oder ein Agent, fragt Glut, bevor es sie anhält und wechselt. Die Sessions und die \
         Panes dieses Fensters bleiben erhalten. In einem Fenster, das eine Host-App geöffnet hat, wählt die Host-App \
         den Ordner.",
    ),
    (
        "folder",
        "Dieser Session Ordner neben dem Arbeitsordner geben, damit Glut projektübergreifend arbeiten kann.\n\
         \n\
         /folder — die Ordner neben dem Arbeitsordner\n\
         /folder add <path> — fügt für diese Session einen hinzu\n\
         /folder remove <path> — nimmt einen weg\n\
         \n\
         Die Dateiwerkzeuge erreichen so einen Ordner über seinen vollen Pfad, Befehle dürfen dort innerhalb der \
         Sandbox schreiben, das Gate beurteilt ihn wie den Arbeitsordner, und Undo holt auch seine Dateien zurück. Nur \
         du fügst einen Ordner hinzu, nie ein Modell, ein Skill oder ein Projekt. Glut lehnt den Home-Ordner ab, einen \
         Ordner, der das Projekt enthält oder darin liegt, einen mit Secrets und zwei Ordner, die ineinander liegen.\n\
         \n\
         Die Ordner gelten nur für diese Session; `glut run --folder <path>` gibt einem Lauf ohne Oberfläche einen.",
    ),
    (
        "compact",
        "Das bisherige Gespräch zusammenfassen, damit es weniger vom Fenster des Modells belegt.\n\
         \n\
         /compact — das Modell schreibt eine Zusammenfassung, die an die Stelle der älteren Nachrichten tritt\n\
         \n\
         Der Systemprompt, deine letzte Nachricht und ganze Schritte bleiben; was die Zusammenfassung ersetzt, liegt \
         weiter im Ergebnisspeicher der Session und wird nie weggeworfen. Glut kompaktiert auch von selbst kurz bevor \
         das Fenster voll ist (`compact` in `~/.glut/settings.json`). /context zeigt, wie voll das Fenster ist.",
    ),
    (
        "handoff",
        "Die Arbeit an eine spätere Session dieses Ordners übergeben.\n\
         \n\
         /handoff write — das Modell schreibt, was getan wurde, wie es steht und was als Nächstes kommt\n\
         /handoff read — nimmt die Übergabe des Ordners in diese Session\n\
         \n\
         Eine Übergabe liegt in `~/.glut/handoffs`, eine pro Ordner. Nach dem Lesen wird sie beiseitegelegt, damit \
         keine spätere Session sie zweimal liest. Sie zu schreiben ist ein eigener, bezahlter Aufruf; das Gespräch \
         bleibt, wie es ist.",
    ),
    (
        "export",
        "Die Session zum Lesen oder Teilen ausgeben.\n\
         \n\
         /export — Markdown nach `~/.glut/exports`\n\
         /export html — eine HTML-Datei, die Styles darin, ohne Skript\n\
         /export jsonl — die Nachrichten als JSON-Zeilen\n\
         /export copy — das Markdown statt in eine Datei in die Zwischenablage\n\
         /export full — nimmt die ganzen Werkzeug-Ergebnisse und den Systemprompt dazu\n\
         /export <format> <path> — in eine eigene Datei (`~/` ist dein Home-Ordner, ein relativer Pfad liegt im \
         Arbeitsordner)\n\
         \n\
         Die Wörter dürfen in beliebiger Reihenfolge stehen, der Pfad zuletzt. Ohne full zeigt der Export, was das \
         Modell gesehen hat: lange Ergebnisse gekürzt. Eine vorhandene Datei wird nie überschrieben. Die Keys, die \
         Glut verborgen hat, bleiben verborgen, aber was du selbst getippt hast, kann doch einen enthalten: Lies einen \
         Export, bevor du ihn teilst. In /resume exportiert ⌃E eine Session der Liste.",
    ),
    (
        "quit",
        "Glut beenden, mit allen Panes und Hintergrund-Agenten.\n\
         \n\
         /quit — arbeitet noch ein anderes Pane oder ein Agent, sagt Glut das zuerst: Ein zweites /quit beendet auch \
         sie\n\
         \n\
         ⌃C ⌃C tut dasselbe, ebenso ⌃D in einer leeren Zeile, während Glut ruht. Die Sessions bleiben für /resume \
         erhalten.",
    ),
    ("exit", "Glut beenden, wie /quit."),
    (
        "model",
        "Das Modell wählen, mit dem diese Session spricht.\n\
         \n\
         /model — die Modelle des aktuellen Providers, aus seiner gespeicherten Liste\n\
         /model <id> — wechselt über seine ID zu diesem Modell\n\
         \n\
         Das Gespräch geht mit dem neuen Modell weiter, und der nächste Start beginnt damit. Ein Modell eines anderen \
         Providers wählst du nach /provider. `glut models refresh` erneuert die Listen.",
    ),
    (
        "provider",
        "Zu einem anderen Modell-Provider wechseln.\n\
         \n\
         /provider — die Provider, die du eingerichtet hast\n\
         /provider <id> — wechselt zu diesem, auf sein Standardmodell\n\
         \n\
         Die Provider liegen in `~/.glut/providers.json`, ihre Keys im Schlüsselbund (oder in \
         `~/.glut/credentials.json`); `glut providers add <id>` fügt einen hinzu. Das Gespräch geht weiter, und der \
         nächste Start beginnt mit dem Provider, den du gewählt hast.",
    ),
    (
        "login",
        "Mit einem Abo statt mit einem Schlüssel anmelden — heute mit deinem ChatGPT-Plus- oder -Pro-Abo.\n\
         \n\
         /login — die Abos, jedes mit seinem Stand; ⏎ meldet an oder ab\n\
         /login <provider> — meldet sich sofort bei diesem an, etwa /login chatgpt\n\
         /login <provider> <address> — über SSH: die Adresse, auf der der Browser endete, zurück eingefügt\n\
         /login openrouter — OpenRouter gibt Glut einen eigenen Key, gehalten wie ein getippter; begrenzen oder \
         löschen unter openrouter.ai/settings/keys\n\
         \n\
         Die Adresse wird zuerst gezeigt; ⏎ öffnet sie in deinem Browser. Glut bewahrt die Anmeldung in der Keychain \
         auf (oder in `~/.glut/credentials.json`) und erneuert sie selbst; Abmelden widerruft sie bei OpenAI.\n\
         \n\
         Mit dem Abo kostet ein Aufruf kein Geld, aber die Nutzungsgrenzen des Abos gelten: Ist eine erreicht, endet \
         der Turn und Glut verweist auf ChatGPT-Einstellungen → Nutzung, wo du auch begrenzt, was jede App nutzen \
         darf. In der Shell: glut providers login|logout <provider>.",
    ),
    (
        "effort",
        "Einstellen, wie gründlich das Modell nachdenkt, bevor es antwortet.\n\
         \n\
         /effort — die Stufen zur Auswahl\n\
         /effort <level> — none, minimal, low, medium, high, xhigh oder max\n\
         /effort default — der Provider entscheidet\n\
         \n\
         Mehr Aufwand bedeutet meist bessere Antworten bei schwieriger Arbeit, aber langsamer und zu höheren Kosten. \
         Nicht jedes Modell kennt jede Stufe. Die Messung (⌃N) zeigt die Stufe, die gerade gilt.",
    ),
    (
        "context",
        "Sehen, wie voll das Fenster des Modells ist, oder Glut sagen, wie groß es ist.\n\
         \n\
         /context — die benutzten Tokens, das Fenster und woher seine Größe stammt, und was die Leitfäden \
         (`AGENTS.md`, `CLAUDE.md`) bei jedem Turn kosten\n\
         /context <tokens> — setzt das Fenster dieses Modells: 65536, 64k oder 1M\n\
         \n\
         Ein Fenster, das du setzt, bleibt in `~/.glut/providers.json` für dieses Modell erhalten und gilt vor der \
         Liste des Providers. Glut kompaktiert von selbst kurz bevor das Fenster voll ist.",
    ),
    (
        "thinking",
        "Wählen, wie das Denken des Modells im Stream erscheint.\n\
         \n\
         /thinking off — verborgen\n\
         /thinking folded — eine Zeile pro Gedanke; ein Klick öffnet einen\n\
         /thinking open — jeder Gedanke ganz gezeigt\n\
         \n\
         /thinking allein klappt sie zu. ⌃T klappt jeden Gedanken auf einmal auf oder zu. Deine Wahl bleibt in \
         `~/.glut/prefs.json` erhalten.",
    ),
    (
        "panel",
        "Die Seitenleiste auf ihrem Reiter Plan zeigen, oder ausblenden.\n\
         \n\
         /panel — die Seitenleiste mit ihren Reitern: Plan, Gate, Messung, Agenten, To-dos, Läufe und Tasten\n\
         \n\
         ⌃B tut dasselbe, ⌃X ⇥ gibt die Tasten an die Seitenleiste (←→ die Reiter, ↑↓ darin, ⎋ zurück). Die \
         Seitenleiste braucht ein Fenster von mindestens 100 Spalten Breite. Ob sie sichtbar ist, bleibt für den \
         nächsten Start erhalten.",
    ),
    (
        "meter",
        "Sehen, was die Session verbraucht: den Kontext, die Tokens und die Kosten.\n\
         \n\
         /meter — der Reiter Messung der Seitenleiste\n\
         \n\
         Er zeigt, wie voll das Fenster ist und wann Glut kompaktiert, die Tokens der Session hinein, aus dem Cache \
         und hinaus mit ihrem Preis, jeden Turn und jedes Werkzeug, und was die schlanke Ausgabe von Glut gespart hat. \
         ⌃N öffnet ihn ebenfalls.",
    ),
    (
        "agents",
        "Die Subagenten sehen, die Glut in dieser Session gestartet hat.\n\
         \n\
         /agents — die Liste der Subagenten, jeder unter dem, der ihn gestartet hat; ⏎ öffnet den Stream eines \
         Subagenten\n\
         \n\
         Auch ein Klick auf die Kachel eines Subagenten öffnet ihn. ⌃X A öffnet den Reiter Agenten: jeder Subagent und \
         jeder Hintergrund-Agent, live — ⏎ öffnet einen, x hält ihn an.",
    ),
    (
        "plan",
        "Den Plan lesen, den Glut aus dem Plan-Modus geschickt hat, ganz, mit seiner Aufgabenliste.\n\
         \n\
         /plan — der letzte Plan dieser Session\n\
         \n\
         Im Plan-Modus (⇧⇥) liest und plant Glut, ohne etwas zu ändern, und schickt dann seinen Plan, damit du ihn \
         annimmst oder zurückgibst. /send plan gibt ihn einem anderen Pane zum Bauen.",
    ),
    (
        "scrollback",
        "Das Gespräch in den eigenen Scrollback des Terminals legen, um es mit den Werkzeugen des Terminals zu \
         blättern, zu durchsuchen und zu kopieren.\n\
         \n\
         /scrollback — gibt das Gespräch aus; ⏎ oder ⌃C geht zurück zu Glut\n\
         \n\
         F3 tut dasselbe.",
    ),
    (
        "split",
        "Eine neue Session in einem Pane neben dem Haupt-Pane öffnen.\n\
         \n\
         /split — ein neues Pane; in einem Git-Repository fragt Glut, wo es arbeitet\n\
         /split here — es arbeitet in diesem Ordner, neben dir\n\
         /split tree [branch] — es arbeitet in einem neuen Worktree auf einem eigenen Branch\n\
         /split resume — es öffnet eine frühere Session, aus der Liste gewählt\n\
         /split as <role> — es spielt eine Rolle (/role listet sie)\n\
         \n\
         Bis zu sechs Panes stehen neben dem Haupt-Pane, in zwei Reihen zu drei. ⌃P geht reihum durch die Panes, ⌃X 2 \
         teilt ebenfalls, ⌃X } und ⌃X { machen das Haupt-Pane breiter oder schmaler. Zwei Panes hier ändern dieselben \
         Dateien; ein Worktree hält sie auseinander, bis /close fragt, wie er endet.\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "swap",
        "Das Haupt-Pane und ein anderes die Plätze tauschen lassen.\n\
         \n\
         /swap — dieses Pane wird das Haupt-Pane\n\
         /swap <n> — Pane n wird das Haupt-Pane",
    ),
    (
        "close",
        "Ein Pane schließen oder einen Hintergrund-Agenten beenden.\n\
         \n\
         /close — schließt dieses Pane (nie das Haupt-Pane); arbeitet es noch, hält ein zweites /close es an und \
         schließt es\n\
         /close <name> — beendet diesen Hintergrund-Agenten, sein Turn wird angehalten\n\
         \n\
         Die Session bleibt für /resume erhalten. Ein Pane in einem Worktree fragt, wie der Worktree endet: behalten \
         samt Branch, übernehmen (in den Branch mergen, auf dem du bist) oder verwerfen. ⌃X 0 schließt dieses Pane \
         ebenfalls.",
    ),
    (
        "send",
        "Eine Nachricht an ein anderes Pane oder einen Hintergrund-Agenten schicken, dort genommen, als hättest du sie \
         getippt.\n\
         \n\
         /send <text> — an das einzige andere Pane\n\
         /send <n|name> <text> — an Pane n oder den Agenten dieses Namens (bei drei oder mehr nötig)\n\
         /send plan — gibt den Plan dieses Panes dem anderen Pane zum Bauen\n\
         /send <n|name> plan — gibt ihn diesem\n\
         \n\
         Die Nachricht beginnt sofort, wo die Session ruht, sonst nach ihrem Turn. Die Tasten bleiben in diesem Pane.",
    ),
    (
        "role",
        "Dieses Pane eine Rolle spielen lassen: Prompt und Werkzeuge eines benannten Agenten, nie sein Modell.\n\
         \n\
         /role — die Rollen zur Auswahl\n\
         /role <name> — spielt ab jetzt diese Rolle\n\
         /role none — wieder ein Allrounder, mit allen Werkzeugen\n\
         \n\
         Glut bringt reviewer, tester und docs mit; deine eigenen Rollen liegen in `~/.glut/agents`, die eines \
         Projekts in dessen `.glut/agents` (nur in einem vertrauten Ordner gelesen). Die Werkzeuge einer Rolle gelten \
         sofort, ihr Prompt ab dem nächsten Turn. Eine Rolle, die die Werkzeuge einschränkt, braucht das Gate.",
    ),
    (
        "bg",
        "Einen Agenten im Hintergrund starten: eine Session ohne Pane, erreichbar über ihren Namen.\n\
         \n\
         /bg — ein Agent namens agent (oder agent-2, …); in einem Git-Repository fragt Glut, wo er arbeitet\n\
         /bg <name> — ein Agent dieses Namens (Kleinbuchstaben, Ziffern und Bindestriche)\n\
         /bg as <role> — er spielt eine Rolle und ist nach ihr benannt\n\
         /bg here — er arbeitet in diesem Ordner\n\
         /bg tree [branch] — er arbeitet in einem neuen Worktree\n\
         \n\
         /send <name> gibt ihm Arbeit, /peek <name> liest seinen Stream, /show <name> holt ihn nach vorn, /close \
         <name> beendet ihn. Jeder Agent arbeitet und kostet für sich: Glut fragt einmal pro Fenster, bevor ein \
         siebter startet. ⌃X B startet einen mit zuerst gewählter Rolle, ⌃X A listet sie.\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "Einen Hintergrund-Agenten als Pane nach vorn holen.\n\
         \n\
         /show <name> — der Agent wird das letzte Pane des Rasters\n\
         \n\
         Hat das Fenster keinen Platz mehr, blende vorher ein Pane mit /hide aus oder schließe es mit /close. ⌃X A \
         listet die Agenten im Hintergrund.",
    ),
    (
        "hide",
        "Dieses Pane in den Hintergrund schicken, sein Gedächtnis bleibt.\n\
         \n\
         /hide — das Pane verlässt den Bildschirm und arbeitet als Hintergrund-Agent weiter\n\
         \n\
         Es behält einen Namen, unter dem es erreichbar ist (seine Rolle, oder pane-<n>): /show <name> holt es zurück, \
         /send <name> und /peek <name> erreichen es. Das Haupt-Pane und das letzte bleiben. ⌃X H tut dasselbe.",
    ),
    (
        "peek",
        "Den Stream eines Hintergrund-Agenten über diesem Pane lesen, ohne ihn nach vorn zu holen.\n\
         \n\
         /peek <name> — sein Stream, live, nur zum Lesen\n\
         /peek — schließt ihn wieder\n\
         \n\
         Auch ⎋ schließt ihn; /show <name> holt den Agenten nach vorn.",
    ),
    (
        "loop",
        "Eine Aufgabe immer wieder in einem eigenen Hintergrund-Agenten ausführen, solange Glut geöffnet ist.\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — startet eine Schleife; die erste Runde sofort\n\
         /loop — listet die Schleifen: Intervall, Runden, die nächste Runde und die bisherigen Kosten\n\
         /loop stop [name] — beendet diese Schleife, oder die einzige\n\
         /loop rounds [n] — die höchste Rundenzahl einer Schleife; allein zeigt es die jetzige Grenze\n\
         /loop budget [usd] — das Meiste, was eine Schleife kosten darf; allein zeigt es die jetzige Grenze\n\
         /loop tokens [n] — die meisten Tokens, die eine Schleife auf einem Abo verbrauchen darf (5M, 500k); allein \
         zeigt es die jetzige Grenze\n\
         \n\
         Runden überlappen sich nie: Das Intervall wird ab dem Ende einer Runde gezählt, als 30s, 5m, 2h oder 1h30m \
         (von 10s bis 24h). Eine Runde macht mit dem Gespräch der Runde davor weiter; mit fresh beginnt jede Runde ein \
         neues. Die Aufgabe ist eine Nachricht, oder /skill <name> [wofür], /compact oder /init — kein anderer \
         Befehl.\n\
         \n\
         Der Agent heißt loop (oder nach seiner Rolle) und übernimmt den Modus dieses Panes, das „immer“, das du hier \
         gegeben hast, seine Ordner daneben und seine Rolle; as <role> darf nur die Rolle nennen, die dieses Pane \
         spielt. Seine Fragen warten auf dich. Eine Kachel erscheint, wenn eine Runde fragt oder die Schleife endet; \
         mit notify nach jeder Runde.\n\
         \n\
         Eine Schleife endet nach ihren Runden oder sobald ihre Runden ihr Budget gekostet haben (standardmäßig 50 \
         Runden und 5 $, sonst `loop.rounds` und `loop.maxUsd` aus `~/.glut/settings.json`; eine Änderung gilt für \
         Schleifen, die danach starten), wenn eine Runde fehlschlägt oder du sie anhältst (⎋ in ihrer Runde), oder \
         wenn ihr Modell `end_loop` aufruft, weil das Ende erreicht ist, das deine Aufgabe nannte. Sind die Preise des \
         Modells unbekannt, beendet nur die Rundenzahl sie. Ihr Agent bleibt danach im Reiter Agenten, zum Lesen oder \
         zum /close. x auf seiner Karte im Reiter Agenten (⌃X A) beendet die Schleife.\n\
         \n\
         /loop 10m führe die Tests aus und behebe, was fehlschlägt; höre auf, wenn alle bestehen\n\
         /loop 1h fresh as reviewer notify prüfe den letzten Commit",
    ),
    (
        "schedule",
        "Pläne, die laufen, während Glut geschlossen ist: eine Aufgabe zu einer Uhrzeit oder alle so und so lange.\n\
         \n\
         /schedule — der Reiter Läufe (⌃X R): jeder Plan mit seiner nächsten Zeit und seinem letzten Ergebnis\n\
         /schedule list — die Pläne im Stream\n\
         /schedule add <name> [flags] <task> — macht einen Plan, oder ändert den dieses Namens\n\
         /schedule run <name> — führt ihn jetzt aus, neben Glut\n\
         /schedule pause <name> — hält ihn an, bis resume\n\
         /schedule resume <name> — er läuft wieder zu seinen Zeiten\n\
         /schedule remove <name> — der Plan und sein Protokoll gehen; seine Sessions bleiben\n\
         \n\
         Die Flags sind die von `glut schedule add`:\n\
         - `--at 07:00` eine Uhrzeit, mit `--days mo-fr` oder `sa,su` (ohne: jeden Tag)\n\
         - `--every 2h` ein Intervall von 10m bis 7d, statt --at\n\
         - `--mode ask|build|plan|dont_ask|auto` was er ohne Regel darf; ask, wenn nichts genannt ist\n\
         - `--allow <rule>`, `--deny <rule>` Regeln in der Form von Claude Code, etwa `Bash(cargo test:*)`; \
         wiederholbar\n\
         - `--folder <path>` ein Ordner neben seinem eigenen; wiederholbar\n\
         - `--max-usd 1`, `--max-minutes 30` das Meiste, was ein Lauf kosten und dauern darf: 1 $ und 30 min, wenn \
         nichts genannt ist\n\
         - `--max-tokens 1M` auf einem Abo die meisten Tokens, die ein Lauf verbrauchen darf: 1M, wenn nicht genannt\n\
         - `--replace` die genannten Regeln und Ordner ersetzen die des Plans, statt zu ihnen hinzuzukommen\n\
         - `--cwd <path>`, `--provider <id>`, `--model <id>` sein Ordner (sonst dieser), Provider und Modell, \
         festgelegt, wenn er gemacht wird\n\
         \n\
         Die Aufgabe ist eine Nachricht, oder /skill <name> [wofür]. launchd startet jeden Lauf, nur solange du \
         angemeldet bist; eine im Ruhezustand verpasste Zeit wird beim Aufwachen einmal nachgeholt. Niemand ist da, um \
         zu antworten: Jede Frage wird abgelehnt und vermerkt, also erlaube, was er braucht. Ein Lauf ist eine eigene \
         gespeicherte Session, gemeldet durch eine macOS-Benachrichtigung. Im Reiter Läufe: ⏎ öffnet einen Plan oder \
         die Session eines Laufs, r führt ihn jetzt aus, p hält an oder setzt fort, d löscht ihn.\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" führe die Tests aus und melde, was \
         fehlschlägt",
    ),
    (
        "diff",
        "Prüfen, was der letzte Turn geändert hat, Hunk für Hunk.\n\
         \n\
         /diff — die Änderungen des letzten Turns, der Dateien geändert hat\n\
         /diff session — jede Änderung dieser Session\n\
         \n\
         In der Prüfung: ↑↓ ein Hunk, ←→ eine Datei, a nimmt an, r verwirft (der Hunk geht zurück, und Glut liest mit \
         deiner nächsten Nachricht, warum), c kommentiert (mit deiner nächsten Nachricht gesendet), v markiert eine \
         Datei als geprüft, s schaltet zwischen Turn und Session um, ⎋ schließt. ⌃R öffnet sie ebenfalls; /review ist \
         derselbe Befehl.",
    ),
    (
        "steps",
        "Die Schritte sehen, die Dateien geändert haben, und Dateien zurückholen.\n\
         \n\
         /steps — jeder Schritt, der eine Datei geändert hat; ⏎ auf einem zeigt, was er geändert hat, oder holt die \
         Dateien auf den Stand davor zurück\n\
         \n\
         Beim Zurückholen gehen deine Änderungen seither mit; das Gespräch bleibt. Glut legt vor jedem ändernden \
         Schritt einen Snapshot an; auch der Stand der Dateien unmittelbar davor wird gesichert, sodass `glut undo` \
         ihn zurückbringt. ⎋⎋ spult stattdessen das Gespräch zurück.",
    ),
    (
        "todo",
        "Die To-dos des Projekts in seiner `TODO.md` führen, einer Checkliste, die du und Glut beide benutzen.\n\
         \n\
         /todo — der Reiter To-dos: Leertaste hakt einen Punkt ab, a fügt einen hinzu, o öffnet den Editor\n\
         /todo <text> — fügt am Ende der Liste einen Punkt hinzu, auch während Glut arbeitet\n\
         /todo open — die Liste in deinem Editor\n\
         /todo clean — entfernt die erledigten Punkte, vorher wird gefragt\n\
         \n\
         Der Rest der Datei wird nie angefasst. ⌃Y zeigt oder verbirgt den Reiter.",
    ),
    (
        "memory",
        "Sehen, was das Gedächtnis des Projekts behält: die Fakten, die Glut für spätere Sessions notiert hat.\n\
         \n\
         /memory — die Fakten mit ihren Beschreibungen; ⏎ zeigt einen ganz\n\
         /memory open — der Index (`MEMORY.md`) in deinem Editor\n\
         \n\
         Das Gedächtnis liegt außerhalb des Repositorys: in `~/.glut/memory/<project>/`, oder im Gedächtnis von Claude \
         Code, wo Claude Code im Repository arbeitet. Glut schreibt es mit seinem Werkzeug `memory`; es hier zu lesen \
         fragt kein Modell.",
    ),
    (
        "skill",
        "Einen Skill starten: Anweisungen für eine Art von Aufgabe, die das Modell lädt und befolgt.\n\
         \n\
         /skill — die Skills zur Auswahl\n\
         /skill <name> [wofür] — führt diesen Skill auf das aus, was nach seinem Namen folgt\n\
         \n\
         Glut bringt make-agent, make-skill und make-hook mit, um eigene zu schreiben. Deine liegen in \
         `~/.glut/skills`, die eines Projekts in dessen `.glut/skills` (nur in einem vertrauten Ordner gelesen); einer \
         desselben Namens verdeckt den von Glut.\n\
         \n\
         /skill make-hook blockiere git push auf main",
    ),
    (
        "init",
        "Eine kurze `AGENTS.md` für diesen Ordner schreiben, oder den vorhandenen Leitfaden prüfen und kürzen.\n\
         \n\
         /init — ohne Leitfaden schreibt Glut `AGENTS.md`; mit einem prüft es ihn und hält ihn kurz\n\
         /init --migrate — fragt erneut, ob `CLAUDE.md` in `AGENTS.md` umzieht\n\
         \n\
         Gibt es nur eine `CLAUDE.md`, fragt Glut einmal, ob sie in `AGENTS.md` umzieht (`CLAUDE.md` importiert sie \
         dann, ein Leitfaden für jedes Harness); ein Nein bleibt gemerkt. Die Leitfäden fahren bei jedem Turn mit: \
         /context zeigt, was sie kosten.",
    ),
    (
        "gate",
        "Die Regeln sehen, nach denen das Gate entscheidet, und ein „immer“ zurücknehmen.\n\
         \n\
         /gate — die geltenden Regeln: dein „immer“ dieser Session, dann die aus `~/.glut/settings.json` und dem \
         `.glut/` des Projekts; ⏎ auf einem „immer“ nimmt es zurück, und Glut fragt beim nächsten Mal wieder\n\
         \n\
         ⌃G öffnet den Reiter Gate: jedes Urteil live, die erreichten Hosts und die Verweigerungen. ⇧⇥ wechselt den \
         Modus.",
    ),
    (
        "mcp",
        "Die MCP-Server sehen und wie jeder steht, bei einem gehosteten anmelden, einen hinzufügen oder entfernen.\n\
         \n\
         /mcp — jeder Server: verbunden mit seinen Werkzeugen, wartend auf eine Anmeldung, oder fehlgeschlagen\n\
         /mcp login <name> — meldet bei einem gehosteten Server an; seine Adresse öffnet sich mit ⏎ in deinem \
         Browser\n\
         /mcp login <name> <address> — über SSH: die Adresse, auf der der Browser landete, hier eingefügt\n\
         /mcp logout <name> — meldet ab und vergisst sein Token\n\
         /mcp add <name> <url> — fügt einen gehosteten Server zu `~/.glut/mcp.json` hinzu und verbindet ihn\n\
         /mcp add <name> -- <command> [args] — fügt einen Server hinzu, den Glut selbst startet\n\
         /mcp remove <name> — nimmt ihn aus `~/.glut/mcp.json` und aus der Session\n\
         \n\
         Ein Server, der keine Anmeldung anbietet, die Glut nutzen kann, fragt nach einem Token, verdeckt getippt. Die \
         eigenen Server eines Projekts ändert das Projekt: /mcp remove nimmt nur deine. `glut mcp` tut dasselbe \
         außerhalb einer Session.",
    ),
    (
        "images",
        "Wählen, wie Bilder im Stream gezeichnet werden.\n\
         \n\
         /images inline — das echte Bild, wo das Terminal eines zeigen kann\n\
         /images blocks — Halbblöcke in Echtfarbe\n\
         /images off — nur die Zeile des Bildes\n\
         \n\
         /images allein zeigt die jetzige Wahl. Standardmäßig zeichnet Glut inline, wo das Terminal es kann, und sonst \
         in Blöcken. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "links",
        "Wählen, wie Links im Stream erscheinen.\n\
         \n\
         /links clickable — der Text des Links ist der Link, wo das Terminal OSC 8 spricht\n\
         /links plain — die URL steht hinter dem Text\n\
         \n\
         /links allein zeigt die jetzige Wahl. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "notify",
        "Benachrichtigt werden, wenn ein Turn endet oder Glut fragt, während das Fenster nicht vorne ist.\n\
         \n\
         /notify on — das Ende eines Turns und jede Frage\n\
         /notify asks — nur die Fragen\n\
         /notify off — nichts\n\
         /notify system — über die eigenen Benachrichtigungen von macOS (ein Klick darauf öffnet den Skript-Editor, \
         nicht das Terminal)\n\
         /notify after <s> — das Ende eines Turns wird erst gemeldet, wenn er so viele Sekunden lief (standardmäßig \
         10, bis zu 3600)\n\
         /notify tiles on|off — die Kacheln oben rechts im Fenster\n\
         \n\
         /notify allein zeigt, was gerade gilt. Glut nutzt die eigenen Benachrichtigungen des Terminals, wo es weiß, \
         dass es sie hat, und die Glocke. Eine Benachrichtigung nennt Glut, das Projekt, das Pane und was geschah, nie \
         die Worte des Modells. Bleibt in `~/.glut/prefs.json` erhalten, vor `notify` in `~/.glut/settings.json`.",
    ),
    (
        "secrets",
        "Die Keys sehen und festlegen, die in dieser Session vor dem Modell verborgen sind.\n\
         \n\
         /secrets — jeder bisher verborgene Wert, mit Nummer, Art, Länge und wo er gesehen wurde (nie der Wert)\n\
         /secrets allow <n> — lässt Secret n für diese Session durch\n\
         /secrets allow <n> always — lässt es immer durch (sein Fingerabdruck wird gespeichert, nie der Wert)\n\
         /secrets add <name> <regex> — verbirgt ein eigenes Format, ab dem nächsten Ergebnis\n\
         /secrets remove <name> — entfernt ein eigenes Format\n\
         /secrets on|off — Maskierung an oder aus\n\
         \n\
         Ein Key in der Antwort eines Werkzeugs erreicht das Modell als ‹secret n: …›; Glut setzt seinen Wert nur in \
         eine Datei zurück, die ihn schon enthält. Bei einer Nachricht von dir, die aussieht, als enthielte sie einen \
         Key, wird nachgefragt, bevor sie abgeschickt wird. Das Modell erfährt von einer Änderung mit deiner nächsten \
         Nachricht. Deine Formate und Entscheidungen bleiben in `~/.glut/secrets.json` erhalten.\n\
         \n\
         Die Maskierung schützt vor einem Key, der aus Versehen gezeigt wird; die Sandbox und das Netzwerk-Gate \
         bleiben die Schranken.",
    ),
    (
        "motion",
        "Wählen, wie viel sich auf dem Bildschirm bewegt.\n\
         \n\
         /motion full — Glimm atmet, Funken steigen auf, die Flamme flackert\n\
         /motion calm — alles bleibt ruhig; ein Bild wird nur gezeichnet, wenn sich etwas geändert hat\n\
         /motion off — wie calm\n\
         \n\
         /motion allein zeigt die jetzige Wahl. „Bewegung reduzieren“ von macOS und der Lesemodus halten den \
         Bildschirm ruhig, was du auch wählst. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "reader",
        "Glut für einen Bildschirmleser einrichten.\n\
         \n\
         /reader on — Tasten in Worten, eine schlichte Fußzeile, kein Glimm, schlichte Rahmen, nichts bewegt sich und \
         bei jedem Wechsel des Zustands eine Textzeile\n\
         /reader off — der übliche Bildschirm\n\
         \n\
         /reader allein zeigt die jetzige Wahl. Bleibt in `~/.glut/prefs.json` erhalten; `glut --reader` hält es für \
         einen Lauf an.",
    ),
    (
        "language",
        "Die Sprache wählen, in der Glut mit dir spricht.\n\
         \n\
         /language — die Sprachen zur Auswahl\n\
         /language <code> — en, de, fr, it, es, zh, ja oder pt\n\
         \n\
         Ohne Wahl spricht Glut die Sprache deines Systems. Das Modell antwortet in der Sprache, in der du schreibst, \
         was du auch hier wählst. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "theme",
        "Die Farben von Glut wählen.\n\
         \n\
         /theme — die Themes zur Auswahl\n\
         /theme <name> — glut, das eigene von Glut, oder eines der Themes von Axiomata (catppuccin-mocha, github-dark, \
         …)\n\
         \n\
         Glut färbt nur den Text und behält den Hintergrund des Terminals: Ein helles Theme braucht ein helles \
         Terminal. Bleibt in `~/.glut/prefs.json` erhalten; in einem Fenster, das eine Host-App geöffnet hat, gilt die \
         Wahl für diese Session.",
    ),
    (
        "editor",
        "Den Editor wählen, in dem Glut Dateien und die Eingabe öffnet.\n\
         \n\
         /editor — die Editoren, die auf deinem System gefunden wurden\n\
         /editor <command> — dein eigener Befehl, mit `{file}` und `{line}` an den passenden Stellen\n\
         \n\
         Fehlt `{file}`, wird die Datei am Ende angehängt. Ein Editor mit Fenster muss warten, bis die Datei \
         geschlossen wird (`code --wait`). ⌃X E bearbeitet die Eingabe darin; /todo open und /memory open nutzen ihn \
         ebenfalls. Bleibt in `~/.glut/prefs.json` erhalten.\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "Wählen, wie eine ruhende Session den Prompt-Cache des Providers hält, damit der nächste Turn warm startet.\n\
         \n\
         /cache ping — Glut liest den Cache alle paar Minuten erneut, solange die Session ruht, bis zu 30 Minuten\n\
         /cache 1h — Anthropic hält ihn eine Stunde (jedes Schreiben kostet das Doppelte); andere Provider werden \
         gelesen, um ihn zu halten\n\
         /cache off — nicht gehalten: eine Pause über die wenigen Minuten des Providers hinaus startet kalt\n\
         \n\
         /cache allein zeigt die jetzige Wahl; ping ist der Standard. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "footer",
        "Wählen, wie die Fußzeile ihre Teile verbindet.\n\
         \n\
         /footer powerline — die Pfeile von Powerline\n\
         /footer plain — schlichte Blöcke\n\
         \n\
         /footer allein zeigt die jetzige Wahl und warum. Ohne Wahl rät Glut am Terminal, ob es die Zeichen von \
         Powerline zeichnet. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "vim",
        "Die Eingabe wie in vi bearbeiten.\n\
         \n\
         /vim on — ⎋ wechselt in den Normal-Modus (Bewegungen, Operatoren, Zähler, u und ⌃R), i a I A o O zurück in \
         den Insert-Modus\n\
         /vim off — die üblichen Bearbeitungstasten\n\
         \n\
         /vim allein schaltet ihn ein. ⎋ im Normal-Modus hält Glut an. Bleibt in `~/.glut/prefs.json` erhalten.",
    ),
    (
        "help",
        "Jeder Befehl und jede Taste, in einer eigenen Box.\n\
         \n\
         /help — die Befehle mit je einer Zeile; Tippen filtert sie, ↑↓ wählen, ⏎ öffnet die ganze Seite eines \
         Befehls\n\
         /help <command> — die Seite dieses Befehls sofort, wie /help loop\n\
         \n\
         Auf einer Seite blättern ↑↓ und PgUp PgDn, ⎋ geht zurück zur Liste und schließt von dort die Box. Der letzte \
         Eintrag enthält jede Taste nach Thema, !cmd und den vi-Modus; ⌃K behält die Tasten in der Seitenleiste.",
    ),
];
