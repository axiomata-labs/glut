//! Deutsch.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "Sprache: {name}",
    language_unkept: "Sprache: {name} — nur für diesen Lauf: {reason}",
    language_unknown: "keine Sprache {name} — {known}",
    language_title: "Sprache von Glut",
    language_now: "jetzt",
    panes_count: Plural {
        one: "{n} Pane",
        other: "{n} Panes",
    },
    topics: &[
        "Session",
        "Modell",
        "Ansicht",
        "Panes",
        "Änderungen",
        "Projekt",
        "Einstellungen",
        "Eingabe",
        "Zeile bearbeiten",
        "Hilfe",
    ],
    commands: &[
        ("clear", "eine neue Session beginnen"),
        (
            "resume",
            "eine frühere Session dieses Ordners öffnen — die Abzweigungen unter ihrer",
        ),
        (
            "projects",
            "in einem anderen Projekt weitermachen: die Ordner, in denen Glut gearbeitet hat, oder einer über seinen Pfad",
        ),
        ("compact", "das bisherige Gespräch zusammenfassen"),
        (
            "handoff",
            "/handoff write: was getan ist und was als Nächstes kommt; /handoff read in einer neuen Session",
        ),
        ("quit", "die Session beenden"),
        ("exit", "die Session beenden, wie /quit"),
        (
            "model",
            "ein Modell des aktuellen Providers wählen, oder /model <id>",
        ),
        ("provider", "den Provider wechseln, oder /provider <id>"),
        (
            "effort",
            "wie gründlich das Modell nachdenkt, oder /effort <level>",
        ),
        ("context", "wie voll das Fenster ist; /context 64k setzt es"),
        (
            "thinking",
            "das Denken des Modells: /thinking off, folded oder open",
        ),
        (
            "panel",
            "die Seitenleiste mit dem Plan, oder ausblenden (⌃B; ⌃G das Gate)",
        ),
        (
            "meter",
            "Kontext, Tokens und Kosten pro Turn und Werkzeug, die Ersparnis von lean (⌃N)",
        ),
        (
            "agents",
            "die Subagenten der Session: den Stream eines öffnen (auch ein Klick auf seine Kachel; ⌃X A: der Reiter Agenten)",
        ),
        (
            "plan",
            "der Plan, den Glut aus dem Plan-Modus geschickt hat, ganz, mit seiner Aufgabenliste",
        ),
        (
            "scrollback",
            "das Gespräch in den eigenen Scrollback des Terminals (F3)",
        ),
        (
            "split",
            "eine neue Session neben dem Haupt-Pane (bis zu sechs, zwei Reihen zu drei); in einem Git-Repository hier oder in einem Worktree",
        ),
        (
            "swap",
            "das Haupt-Pane und Pane n (oder dieses) tauschen die Plätze",
        ),
        (
            "close",
            "dieses Pane schließen (nicht das Haupt-Pane) — oder einen Hintergrund-Agenten beenden; seine Session bleibt für /resume",
        ),
        (
            "send",
            "eine Nachricht an ein anderes Pane (bei drei oder mehr Panes zuerst seine Nummer), dort genommen, als wäre sie getippt; /send plan gibt ihm deinen Plan zum Bauen",
        ),
        (
            "role",
            "die Rolle dieses Panes: Prompt und Werkzeuge eines benannten Agenten, nicht sein Modell — reviewer, tester, docs oder deine",
        ),
        (
            "bg",
            "ein Agent im Hintergrund: eine Session ohne Pane, erreichbar über ihren Namen — /bg as reviewer",
        ),
        (
            "show",
            "einen Agenten aus dem Hintergrund als Pane nach vorn holen",
        ),
        (
            "hide",
            "dieses Pane in den Hintergrund schicken, sein Gedächtnis bleibt (⌃X H)",
        ),
        (
            "peek",
            "den Stream eines Hintergrund-Agenten über diesem Pane lesen; Esc schließt ihn",
        ),
        (
            "diff",
            "prüfen, was der letzte Turn geändert hat, Hunk für Hunk; /diff session (⌃R)",
        ),
        (
            "steps",
            "die Schritte, die Dateien geändert haben: was jeder geändert hat, oder die Dateien zurückholen",
        ),
        (
            "todo",
            "die TODO.md des Projekts (⌃Y): /todo <text> fügt einen Punkt hinzu, /todo open bearbeitet sie, /todo clean entfernt die erledigten",
        ),
        (
            "memory",
            "was das Gedächtnis des Projekts behält: ⏎ zeigt einen Fakt; /memory open bearbeitet seinen Index",
        ),
        (
            "skill",
            "einen Skill starten: /skill <name> [wofür] — /skill allein listet sie (make-agent, make-skill, make-hook, …)",
        ),
        (
            "init",
            "eine kurze AGENTS.md für diesen Ordner, oder seinen Leitfaden prüfen (/init --migrate fragt wieder nach CLAUDE.md)",
        ),
        (
            "gate",
            "die geltenden Regeln; ein „immer“ zurücknehmen (⌃G: das Gate, live)",
        ),
        (
            "images",
            "wie Bilder gezeichnet werden: das echte Bild, wo das Terminal es kann, Halbblöcke in Echtfarbe, oder nur ihre Zeile",
        ),
        (
            "links",
            "der Text eines Links ist der Link, wo das Terminal OSC 8 spricht, sonst steht die URL dahinter",
        ),
        (
            "motion",
            "wie viel sich auf dem Bildschirm bewegt; calm und off halten Glimm still (macOS „Bewegung reduzieren“ zählt als off)",
        ),
        (
            "reader",
            "für einen Bildschirmleser: Tasten in Worten, ein schlichter Bildschirm, nichts bewegt sich, eine Zeile bei jedem Wechsel des Zustands",
        ),
        (
            "language",
            "die Sprache, in der Glut mit dir spricht: en de fr it es zh ja pt — das Modell antwortet in der Sprache, in der du schreibst",
        ),
        (
            "theme",
            "die Farben von Glut, oder die eines Axiomata-Themes: /theme <name>",
        ),
        ("editor", "deinen Editor wählen, oder /editor <command>"),
        (
            "cache",
            "wie eine ruhende Session ihren Cache hält: erneut lesen (ping), Anthropics Stunde (1h), oder gar nicht",
        ),
        (
            "footer",
            "die Übergänge der Fußzeile: Powerline-Pfeile oder schlichte Blöcke (vom Terminal geraten)",
        ),
        ("vim", "die Eingabe wie in vi bearbeiten: /vim on oder off"),
        ("help", "Befehle und Tasten"),
    ],
    keys: &[
        ("⇧⇥", "der nächste Modus: plan → ask → build → auto"),
        ("⎋", "Glut anhalten"),
        (
            "⏎",
            "während Glut arbeitet: lenken, vor seinem nächsten Schritt aufgenommen",
        ),
        (
            "⇥",
            "während Glut arbeitet: die Nachricht für danach einreihen",
        ),
        ("⌥↑", "die letzte wartende Nachricht zurückholen"),
        ("⌃C ⌃C", "beenden"),
        ("⌃F", "den Stream durchsuchen"),
        ("⌃O", "jedes Werkzeug-Ergebnis auf- oder zuklappen"),
        ("⌃T", "jeden Gedanken auf- oder zuklappen"),
        ("⌃B", "die Seitenleiste mit dem Plan, oder ausblenden"),
        (
            "⌃G",
            "das Gate, live: jedes Urteil, die Hosts, die Verweigerungen",
        ),
        (
            "⌃N",
            "die Messung: Kontext, Tokens und Kosten pro Turn und Werkzeug",
        ),
        (
            "⌃X A",
            "der Reiter Agenten: jeder Subagent und die Agenten im Hintergrund, live",
        ),
        (
            "⌃X ↑",
            "den Stream Block für Block lesen: ⏎ klappt auf und zu, c kopiert, ⎋ zurück",
        ),
        (
            "⌃X ⇥",
            "die Seitenleiste bekommt die Tasten: ←→ die Reiter, ↑↓ darin, ⎋ zurück",
        ),
        ("PgUp PgDn", "den Stream blättern (⇧↑ ⇧↓ eine Zeile)"),
        ("F3", "das Gespräch in den Scrollback des Terminals"),
        ("F2", "die Maus an das Terminal geben und zurück"),
        (
            "⌃P",
            "das nächste Pane, reihum in Lesereihenfolge (auch ⌃]: ⌃6 auf deutscher Tastatur)",
        ),
        ("⌃X 2", "ein neues Pane neben dem Haupt-Pane (auch ⌃X 3)"),
        (
            "⌃X }",
            "das Haupt-Pane breiter (auch mit der Maus an seiner Grenze)",
        ),
        ("⌃X {", "das Haupt-Pane schmaler"),
        ("⌃X 0", "dieses Pane schließen"),
        (
            "⌃X B",
            "ein Agent im Hintergrund, seine Rolle zuerst gewählt",
        ),
        ("⌃X H", "dieses Pane in den Hintergrund"),
        (
            "⌃R",
            "prüfen, was der letzte Turn geändert hat, Hunk für Hunk",
        ),
        (
            "⎋⎋",
            "zurückspulen oder abzweigen, vor eine deiner Nachrichten",
        ),
        ("⌃Y", "der Reiter Aufgaben: Leertaste hakt ab, a fügt hinzu"),
        ("/", "die Befehle"),
        ("@", "eine Datei erwähnen"),
        (
            "⌃V",
            "das Bild aus der Zwischenablage in die Nachricht (auch eine hineingezogene Bilddatei)",
        ),
        ("↑", "die zuvor gesendeten Nachrichten"),
        ("⇧⏎", "eine neue Zeile (auch ⌥⏎)"),
        ("⌃X E", "die Eingabe in deinem Editor bearbeiten"),
        ("⌃A ⌃E", "Anfang der Zeile, ihr Ende (auch Pos1 Ende)"),
        (
            "⌥B ⌥F",
            "ein Wort nach links, ein Wort nach rechts (auch ⌥← ⌥→)",
        ),
        ("⌃W", "das Wort davor löschen (auch ⌥⌫)"),
        ("⌃U", "bis zum Anfang der Zeile löschen"),
        ("⌃J", "eine neue Zeile"),
        ("⌃D", "beenden, auf leerer Zeile, während Glut ruht"),
        ("⌃K", "dieses Blatt der Tasten"),
    ],
    help_commands: "Befehle",
    help_keys: "Tasten ({legend}; ⌃K behält sie in der Seitenleiste)",
    help_also: "Außerdem: !cmd führt einen Befehl aus und zeigt Glut seine Ausgabe · !!cmd führt ihn nur für dich \
                aus · ein großes Einfügen steht als [pasted N lines] · Ziehen kopiert · ein Klick auf die Kachel \
                eines Subagenten öffnet ihn",
    help_vim: "/vim on: ⎋ in den Normal-Modus (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y mit einer \
               Bewegung oder doppelt · Zahlen davor · p P · u ⌃R), i a I A o O zum Einfügen; ⎋ im Normal-Modus hält \
               Glut an",
    legend: "⌃ Control · ⇧ Umschalt · ⌥ Wahl · ⏎ Return · ⇥ Tab · ⎋ Esc",
};
