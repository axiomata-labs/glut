Come sono tenuti i comandi che Glut esegue, e se possono scrivere nelle cartelle git.

/sandbox — la sandbox qui (macOS: Seatbelt; Linux: bubblewrap e seccomp) e l’interruttore git
/sandbox git read-only — nessun comando scrive in una cartella git: nessuno fa commit dalla sandbox
/sandbox git writable — i comandi possono di nuovo scrivere nelle cartelle git, anche fare commit
/sandbox required on — nessun comando gira dove non c’è una sandbox (manca bubblewrap, un container senza il diritto); off li lascia girare, tenuti solo dalle regole

La sandbox lascia che un comando scriva solo nelle tue cartelle, legga tutto tranne i tuoi segreti e i file .env, e raggiunga la rete solo tramite il proxy di Glut. Su Linux non può rifiutare un nome che ancora non c’è: ciò che un comando crea e macOS rifiuta — un nuovo .envrc, un .git, un .git/commondir — va in ~/.glut/quarantine appena compare, e ancora una volta dopo il comando. Uno strumento tuo che gira proprio in quel momento potrebbe ancora trovarlo; con le cartelle git in sola lettura, lì non se ne crea nessuno.

L’interruttore vale in ogni sessione e finestra dal prossimo comando, ed è tenuto in ~/.glut/sandbox.json — mai scritto da un comando. glut sandbox show e glut sandbox git read-only|writable fanno lo stesso fuori da Glut.
