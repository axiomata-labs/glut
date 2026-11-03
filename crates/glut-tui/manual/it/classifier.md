Lascia che un secondo modello giudichi i comandi per cui solo la modalità ti chiederebbe.

/classifier — quale modello giudica, o che nessuno lo fa
/classifier session — giudica il modello della sessione (finché non ne imposti uno)
/classifier <provider> <model> — un modello a parte, ne basta uno veloce (openrouter deepseek/deepseek-v4.1-flash)
/classifier off — nessuno giudica: ti viene chiesto ogni volta

Glut conosce molti comandi che leggono soltanto (ls, grep, git status, …) e li esegue in ogni modalità. Un ciclo o una pipeline di essi (for f in *.md; do grep -c x "$f"; done) non può giudicarlo dal solo testo, quindi chiede. Il secondo modello vede quel comando e la sua cartella — mai la conversazione, un file o un output — e dice READS o CHANGES. Solo con READS il comando viene eseguito come un comando di lettura noto, nella sandbox; tutto il resto, o nessuna risposta chiara in pochi secondi, ti chiede come prima, con il suo parere sotto il comando. Una tua regola, un comando pesante, il portachiavi e la sandbox restano come sono; lo stesso comando non viene giudicato due volte in una sessione.

Un comando che ha lasciato passare lo dice accanto. La scelta è salvata in `~/.glut/classifier.json` e vale dal prossimo turno; `glut run --classifier` la imposta per un'esecuzione.
