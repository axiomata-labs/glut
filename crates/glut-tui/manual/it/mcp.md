Guarda i server MCP e lo stato di ciascuno, accedi a uno ospitato, aggiungine o rimuovine uno.

/mcp — ogni server: connesso con i suoi strumenti, in attesa di un accesso, o fallito
/mcp login <name> — accede a un server ospitato; il suo indirizzo si apre nel browser con ⏎
/mcp login <name> <address> — via SSH: l’indirizzo su cui il browser è finito, incollato di nuovo
/mcp logout <name> — esce e dimentica il suo Token
/mcp add <name> <url> — aggiunge un server ospitato a `~/.glut/mcp.json` e lo connette
/mcp add <name> -- <command> [args] — aggiunge un server che Glut avvia da sé
/mcp remove <name> — lo toglie da `~/.glut/mcp.json` e dalla sessione

Un server che non offre un accesso utilizzabile da Glut chiede un Token, digitato nascosto. I server propri di un progetto sono del progetto da cambiare: /mcp remove toglie solo i tuoi. `glut mcp` fa lo stesso fuori da una sessione.

Mentre uno dei suoi strumenti è in corso, un server può chiederti: un modulo (⇥ ↑↓ tra i campi, spazio spunta, ⏎ invia, Esc chiude, ⌃D rifiuta) o una pagina da aprire (o). Rispondi solo tu, mai il modello; un modulo che chiede una password o una chiave viene rifiutato, e se non c'è nessuno ogni domanda viene rifiutata.
