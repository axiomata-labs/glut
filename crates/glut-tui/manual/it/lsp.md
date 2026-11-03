Se i server di linguaggio aiutano il modello.

/lsp on — dopo una modifica gli errori del file arrivano con il risultato, e il modello ha lo strumento lsp: dove un simbolo è definito, ogni punto in cui è usato, il suo tipo e la sua documentazione, e una rinomina in tutto il progetto
/lsp off — nessun server gira: i loro processi finiscono subito, niente errori dopo una modifica, niente strumento lsp
/lsp — la scelta attuale, e ogni server della sessione: pronto, indicizza il progetto, o spento per la sessione

Glut avvia solo server che non eseguono mai il codice del progetto (rust-analyzer senza script di build né macro procedurali, pyright e i server di TOML, YAML, JSON, CSS, HTML e Markdown), solo nella sandbox senza rete, e solo dove il Gate permette la loro riga di comando. Un server parte alla prima modifica o domanda del suo linguaggio e resta per la sessione; rust-analyzer impiega alcuni secondi per indicizzare un progetto grande, intanto lsp lo dice e il modello usa grep. Una rinomina è un solo cambiamento: chiesta una volta per tutti i suoi file, tutti o nessuno, con un'istantanea prima. Salvato in `~/.glut/prefs.json`; `"lsp": false` in `~/.glut/settings.json` lo spegne per ogni esecuzione, `glut run --no-lsp` per una.
