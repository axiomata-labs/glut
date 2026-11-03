Dieselbe Aufgabe auf zwei bis vier Modellen zugleich laufen lassen und vergleichen (die Arena).

/arena — die Modelle aller Anbieter; die Leertaste markiert zwei bis vier, ⏎ setzt sie in die Eingabe, dann folgt die Aufgabe
/arena <model>, <model>: <task> — dasselbe in einer Zeile (Anbieter/Modell, oder ein Name, der auf ein Modell passt)
/arena say <text> — eine Nachfrage an jedes Modell, das geantwortet hat, wenn alle fertig sind (nicht auf dem Prüfstand); Zeit, Kosten und Tokens zählen über die Runden weiter
/arena take <n> — die Änderungen von Modell n in deinen Ordner, nicht committet
/arena card — die Vergleichskarte wieder: die Zahlen und die Prüfung jedes Modells, darunter sein Diff (←/→ ein anderes Modell, s zwei nebeneinander, c prüfen, t übernehmen)
/arena check <command> — die Prüfung für dieses Repository (cargo test, npm test …), im Worktree jedes Modells, in seiner Sandbox; gemerkt, läuft sie nach der nächsten Arena von selbst
/arena tasks — die Aufgabensätze des Prüfstands in ~/.glut/arena/tasks; /arena tasks <set> startet einen auf den Modellen der letzten Arena (oder /arena tasks <set>: <model>, <model>; mit x3 hinter dem Satz jede Aufgabe dreimal hintereinander (bis x5), die Tabelle zeigt dann 2/3; mit --background laufen die Modelle als Agenten im Hintergrund, ohne Pane, ihr Fortschritt im Agents-Reiter), Aufgabe für Aufgabe, jede von ihrer eigenen Prüfung bewertet
/arena results — die Ergebnisse jedes Satzes (bestandene Aufgaben, Ø Zeit und Kosten) und die Siege der freien Arena
/arena blind on|off — die freie Arena blind (Standard): ihre Modelle heißen Buchstaben in gezogener Reihenfolge, die Namen kommen, sobald du eins übernimmst oder die Arena beendest
/arena judge — nach deiner Wahl kommentiert das Modell des Panes, aus dem die Arena gestartet wurde, die Lösungen in zwei, drei Sätzen (ohne Werkzeuge; nie vorher, damit es deine Wahl nicht färbt)
/arena end — die Panes der Arena schließen, ihre Worktrees entfernen (fragt vorher)

Jedes Modell arbeitet in einem eigenen Pane und Worktree, der von deinem Ordner ausgeht, wie er ist, nicht committete und neue Dateien eingeschlossen (keine .env-Datei, kein Link), in auto: nur heftige Befehle fragen. Ein Modell hält nach 30 Minuten oder 2 $ an. Cloud-Modelle laufen zugleich, lokale (Ollama, LM Studio) nacheinander, wenn die anderen fertig sind. Sind alle fertig, stehen ihre Zahlen im Pane, aus dem die Arena gestartet wurde, und /arena card öffnet die Vergleichskarte: Zeit, Kosten, Tokens, Aufrufe und was jedes geändert hat. /arena take prüft, ob die Änderungen noch zu deinem Ordner passen; passen sie nicht, wird nichts geschrieben.

/arena sonnet, gpt-5: make the failing test pass
