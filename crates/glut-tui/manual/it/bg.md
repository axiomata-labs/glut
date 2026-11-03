Avvia un agente in background: una sessione senza pane, raggiunta per nome.

/bg — un agente di nome agent (o agent-2, …); in un repository git Glut chiede dove lavora
/bg <name> — un agente con quel nome (lettere minuscole, cifre e trattini)
/bg as <role> — recita un ruolo, e prende il nome da esso
/bg here — lavora in questa cartella
/bg tree [branch] — lavora in un nuovo Worktree

/send <name> gli dà lavoro, /peek <name> ne legge il flusso, /show <name> lo porta in primo piano, /close <name> lo termina. Ogni agente lavora e costa per conto suo: Glut chiede una volta per finestra prima di un settimo. ⌃X B ne avvia uno con il ruolo scelto prima, ⌃X A li elenca.

/bg docs-check as docs
