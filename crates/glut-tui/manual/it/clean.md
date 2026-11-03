Evitare che i dati di Glut in ~/.glut crescano senza fine.

/clean — cosa tiene Glut, voce per voce, e cosa può andare secondo i tuoi termini
/clean now — pulisce subito, in background
/clean keep — i termini
/clean keep <kind> <days>d — ne imposta uno: snapshots, journals, sessions, results o lean

Cosa va: gli snapshot di un'esecuzione più vecchi del loro termine (7 giorni) — l'intero archivio di una cartella quando non ne resta nessuno, che la cartella ci sia o no; il journal di un'esecuzione incompiuta (7 giorni); una sessione non scritta da 90 giorni, con il suo registro eventi e i suoi risultati lunghi; i risultati lunghi senza sessione dopo 30 giorni; le immagini che nessuna sessione tenuta nomina; le righe di lean.jsonl più vecchie di 90 giorni. Le esportazioni sono solo contate: sono file tuoi.

Una volta al giorno Glut pulisce da solo all'avvio — qui e in `glut run` — e dice cosa ha liberato. L'output di build e le cache (una cartella con un CACHEDIR.TAG, sotto node_modules/ o il target/ di un progetto Cargo) non hanno alcuno snapshot.

I termini sono tenuti in `~/.glut/clean.json`. Dalla riga di comando: `glut clean`, `glut clean --dry-run`, `glut clean keep sessions 30d`; `glut clean --dry-run --json` scrive il rapporto per un programma.
