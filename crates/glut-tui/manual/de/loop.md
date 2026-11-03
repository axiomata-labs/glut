Eine Aufgabe immer wieder in einem eigenen Hintergrund-Agenten ausführen, solange Glut geöffnet ist.

/loop <interval> [fresh] [as <role>] [notify] <task> — startet eine Schleife; die erste Runde sofort
/loop — listet die Schleifen: Intervall, Runden, die nächste Runde und die bisherigen Kosten
/loop stop [name] — beendet diese Schleife, oder die einzige
/loop rounds [n] — die höchste Rundenzahl einer Schleife; allein zeigt es die jetzige Grenze
/loop budget [usd] — das Meiste, was eine Schleife kosten darf; allein zeigt es die jetzige Grenze
/loop tokens [n] — die meisten Tokens, die eine Schleife auf einem Abo verbrauchen darf (5M, 500k); allein zeigt es die jetzige Grenze

Runden überlappen sich nie: Das Intervall wird ab dem Ende einer Runde gezählt, als 30s, 5m, 2h oder 1h30m (von 10s bis 24h). Eine Runde macht mit dem Gespräch der Runde davor weiter; mit fresh beginnt jede Runde ein neues. Die Aufgabe ist eine Nachricht, oder /skill <name> [wofür], /compact oder /init — kein anderer Befehl.

Der Agent heißt loop (oder nach seiner Rolle) und übernimmt den Modus dieses Panes, das „immer“, das du hier gegeben hast, seine Ordner daneben und seine Rolle; as <role> darf nur die Rolle nennen, die dieses Pane spielt. Seine Fragen warten auf dich. Eine Kachel erscheint, wenn eine Runde fragt oder die Schleife endet; mit notify nach jeder Runde.

Eine Schleife endet nach ihren Runden oder sobald ihre Runden ihr Budget gekostet haben (standardmäßig 50 Runden und 5 $, sonst `loop.rounds` und `loop.maxUsd` aus `~/.glut/settings.json`; eine Änderung gilt für Schleifen, die danach starten), wenn eine Runde fehlschlägt oder du sie anhältst (⎋ in ihrer Runde), oder wenn ihr Modell `end_loop` aufruft, weil das Ende erreicht ist, das deine Aufgabe nannte. Sind die Preise des Modells unbekannt, beendet nur die Rundenzahl sie. Ihr Agent bleibt danach im Reiter Agenten, zum Lesen oder zum /close. x auf seiner Karte im Reiter Agenten (⌃X A) beendet die Schleife.

/loop 10m führe die Tests aus und behebe, was fehlschlägt; höre auf, wenn alle bestehen
/loop 1h fresh as reviewer notify prüfe den letzten Commit
