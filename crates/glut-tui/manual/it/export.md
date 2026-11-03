Scrivi la sessione per leggerla o condividerla.

/export — Markdown in `~/.glut/exports`
/export html — un solo file HTML, con gli stili dentro, senza script
/export jsonl — i messaggi come righe JSON
/export copy — il Markdown negli appunti invece che in un file
/export full — aggiunge i risultati interi degli strumenti e il prompt di sistema
/export <format> <path> — in un file tuo (`~/` è la tua home, un percorso relativo sta nella cartella di lavoro)

Le parole vanno in qualsiasi ordine, il percorso per ultimo. Senza full l’esportazione mostra ciò che il modello ha visto: i risultati lunghi accorciati. Un file esistente non viene mai sovrascritto. Le chiavi che Glut ha nascosto restano nascoste, ma ciò che hai digitato tu potrebbe contenerne una: leggi un’esportazione prima di condividerla. In /resume, ⌃E esporta una sessione dell’elenco.
