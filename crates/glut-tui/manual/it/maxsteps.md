Quanti passi fa al massimo un turno.

/maxsteps — il limite attuale
/maxsteps <n> — un turno si ferma dopo n passi, da 1 a 1000

Un passo è una chiamata al modello. Al limite il turno si ferma e lo dice; ⏎ sull'input vuoto invia «Go on where you stopped.» e il lavoro continua. I budget e il limite di tempo restano i veri freni. Tenuto in `~/.glut/prefs.json`; `maxTurns` in `~/.glut/settings.json` lo fissa per ogni esecuzione, `glut run --max-turns` per una.
