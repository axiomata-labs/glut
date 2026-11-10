Ob Glut einmal am Tag nach einer neueren Version schaut und sie im Fuß zeigt.

/update on — einmal am Tag liest Glut im Hintergrund die Nummer der neuesten Version aus `https://glut-ai.com/latest.txt`; ist sie neuer als diese, zeigt der Fuß sie leise (`↑ 0.1.2`)
/update off — Glut fragt nichts, und der Fuß zeigt nichts

/update allein sagt, ob ein neueres Glut da ist, und nennt den Befehl, der dieses aktualisiert, je nachdem, wie es installiert wurde (`brew upgrade glut`, `cargo install glut-tui` oder das Install-Skript). Glut aktualisiert sich nie selbst und fragt nie im Lauf eines Zeitplans, in einer Runde einer Schleife, in `glut run`, `glut acp` oder in der Arena. Gehalten in `~/.glut/prefs.json`; `"updateCheck": false` in `~/.glut/settings.json` schaltet es ab, wo /update nichts anderes gesagt hat.
