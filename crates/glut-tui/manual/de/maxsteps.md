Wie viele Schritte ein Turn höchstens macht.

/maxsteps — die jetzige Grenze
/maxsteps <n> — ein Turn hält nach n Schritten an, von 1 bis 1000

Ein Schritt ist ein Aufruf des Modells. An der Grenze hält der Turn an und sagt es; ⏎ in der leeren Eingabe schickt „Go on where you stopped.“ und die Arbeit geht weiter. Budgets und das Zeitlimit bleiben die eigentlichen Bremsen. Gehalten in `~/.glut/prefs.json`; `maxTurns` in `~/.glut/settings.json` setzt es für jeden Lauf, `glut run --max-turns` für einen.
