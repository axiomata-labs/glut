Se Glut chiede al modello di continuare alla fine di un turno, di stare alla sua lista di compiti e di scriverne una quando lavora senza.

/nudge on — quando un turno finisce con punti della lista di compiti aperti, o con parole che annunciano un passo («ora passo…», due punti alla fine), Glut manda al modello una riga sua: continuare o dire perché si ferma; il flusso lo mostra in silenzio. Se la lista non cambia mentre lavora, Glut gliela ricorda, e se non cambia nulla, una volta con un messaggio a parte; se in un'esecuzione non ne ha scritta nessuna, gliene chiede una, una volta per turno
/nudge off — un turno finisce dove il modello lo finisce: nessun richiamo alla lista, nessuna richiesta

La domanda alla fine viene una volta per turno, mai in modalità plan, dopo una domanda a te, in un giro di un ciclo o nell'esecuzione di un piano; il richiamo e la richiesta seguono la stessa scelta, ma nell'esecuzione di un piano Glut ricorda sempre. /nudge da solo mostra la scelta attuale. Tenuto in `~/.glut/prefs.json`; `"nudge": false` in `~/.glut/settings.json` lo spegne per ogni esecuzione.
