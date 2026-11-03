Esegui lo stesso compito su due-quattro modelli insieme e confrontali (l’arena).

/arena — i modelli di ogni provider; la barra spaziatrice ne segna da due a quattro, ⏎ li mette nell’input per aggiungere il compito
/arena <model>, <model>: <task> — lo stesso in una riga (provider/modello, o un nome che corrisponde a un solo modello)
/arena say <text> — una domanda in più a ogni modello che ha risposto, quando tutti hanno finito (non sul banco); tempo, costo e token si sommano nei giri
/arena take <n> — le modifiche del modello n nella tua cartella, senza commit
/arena card — la scheda di confronto di nuovo: i numeri e il controllo di ogni modello, sotto il suo diff (←/→ un altro modello, s due affiancati, c controllare, t prendere)
/arena check <command> — il controllo per questo repository (cargo test, npm test …), eseguito nel worktree di ogni modello, nel suo sandbox; memorizzato, parte da solo dopo la prossima arena
/arena tasks — i set di compiti del banco di prova in ~/.glut/arena/tasks; /arena tasks <set> ne avvia uno sui modelli dell’ultima arena (o /arena tasks <set>: <model>, <model>; con x3 dopo il set, ogni compito tre volte di fila (fino a x5), la tabella mostra allora 2/3; con --background i modelli girano come agenti in background, senza pane, il loro avanzamento nella scheda Agents), compito dopo compito, ciascuno giudicato dal proprio controllo
/arena results — i risultati di ogni set (compiti superati, Ø tempo e costo) e le vittorie dell’arena libera
/arena blind on|off — l’arena libera alla cieca (predefinito): i suoi modelli hanno lettere in un ordine estratto, i nomi arrivano appena ne prendi uno o chiudi l’arena
/arena judge — dopo la tua scelta, il modello del pane da cui è partita l’arena commenta le soluzioni in due o tre frasi (senza strumenti; mai prima, per non influenzare la tua scelta)
/arena end — chiude i pane dell’arena e rimuove i loro worktree (chiede prima)

Ogni modello lavora in un pane e in un worktree propri che partono dalla tua cartella così com’è, file non committati e nuovi compresi (nessun file .env, nessun link), in auto: chiedono solo i comandi pesanti. Un modello si ferma dopo 30 minuti o 2 $. I modelli cloud girano insieme, quelli locali (Ollama, LM Studio) uno dopo l’altro quando gli altri hanno finito. Quando tutti hanno finito, i loro numeri compaiono nel pane da cui è partita l’arena e /arena card apre la scheda di confronto: tempo, costo, token, chiamate e cosa ha cambiato ciascuno. /arena take controlla che le modifiche si adattino ancora alla tua cartella; se non si adattano, non viene scritto nulla.

/arena sonnet, gpt-5: make the failing test pass
