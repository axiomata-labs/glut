Passa il lavoro a una sessione successiva di questa cartella.

/handoff write — il modello scrive cosa è stato fatto, a che punto stanno le cose e cosa viene dopo
/handoff read — porta il passaggio di consegne della cartella in questa sessione

Un passaggio di consegne è conservato in `~/.glut/handoffs`, uno per cartella. Una volta letto si sposta da parte, così nessuna sessione successiva lo legge due volte. Scriverne uno è una chiamata a pagamento a sé; la conversazione resta com’è.
