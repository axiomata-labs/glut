Esegui un compito più e più volte in un agente in background tutto suo, finché Glut è aperto.

/loop <interval> [fresh] [as <role>] [notify] <task> — avvia un ciclo; il primo giro subito
/loop — elenca i cicli: intervallo, giri, il prossimo giro e il costo finora
/loop stop [name] — termina quel ciclo, o l’unico
/loop rounds [n] — il massimo di giri di un ciclo; da solo mostra il limite attuale
/loop budget [usd] — il massimo che un ciclo può costare; da solo mostra il limite attuale
/loop tokens [n] — il massimo di token che un ciclo su un abbonamento può usare (5M, 500k); da solo mostra il limite attuale

I giri non si sovrappongono mai: l’intervallo è contato dalla fine di un giro, come 30s, 5m, 2h o 1h30m (da 10s a 24h). Un giro prosegue con la conversazione del giro precedente; con fresh ogni giro ne comincia una nuova. Il compito è un messaggio, oppure /skill <name> [a che scopo], /compact o /init — nessun altro comando.

L’agente si chiama loop (o come il suo ruolo) e prende la modalità di questo pane, il «sempre» che hai dato qui, le sue cartelle accanto e il suo ruolo; as <role> può nominare solo il ruolo che questo pane recita. Le sue domande aspettano te. Una tessera arriva quando un giro chiede o il ciclo termina; con notify dopo ogni giro.

Un ciclo termina dopo i suoi giri o quando i suoi giri costano il suo budget (di norma 50 giri e 5 $, altrimenti `loop.rounds` e `loop.maxUsd` di `~/.glut/settings.json`; una modifica vale per i cicli avviati dopo), quando un giro fallisce o lo fermi tu (⎋ nel suo giro), oppure quando il suo modello chiama `end_loop` perché la fine che il tuo compito indicava è raggiunta. Quando i prezzi del modello sono sconosciuti, lo terminano solo i giri. Il suo agente resta poi nella scheda Agenti, da leggere o da /close. x sulla sua voce nella scheda Agenti (⌃X A) termina il ciclo.

/loop 10m esegui i test e correggi ciò che fallisce; fermati quando passano tutti
/loop 1h fresh as reviewer notify rivedi l’ultimo commit
