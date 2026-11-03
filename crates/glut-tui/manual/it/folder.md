Dai a questa sessione cartelle accanto a quella di lavoro, così Glut può lavorare tra più progetti.

/folder — le cartelle accanto a quella di lavoro
/folder add <path> — ne aggiunge una per questa sessione
/folder remove <path> — ne toglie una

Gli strumenti dei file raggiungono una tale cartella dal suo percorso completo, i comandi possono scriverci dentro la sandbox, il Gate la giudica come la cartella di lavoro e anche undo ne ripristina i file. Solo tu aggiungi una cartella, mai un modello, una Skill o un progetto. Glut rifiuta la cartella home, una cartella che contiene il progetto o ci sta dentro, una con segreti e due cartelle una dentro l’altra.

Le cartelle valgono solo per questa sessione; `glut run --folder <path>` ne dà una a un’esecuzione headless.
