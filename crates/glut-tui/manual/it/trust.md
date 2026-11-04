Se le impostazioni .glut proprie di questa cartella contano per intero.

/trust — se questa cartella è fidata, e cosa aggiungono allora le sue impostazioni
/trust on — la rende fidata: contano le sue regole di permesso, la sua modalità, i suoi skill, agenti e comandi
/trust off — non si fida più: contano solo le sue regole di divieto e di domanda
/trust list — ogni cartella fidata

Il .glut/settings.json di un progetto può tenere più segreti e chiedere o vietare di più — questo conta in ogni cartella. Ciò che permetterebbe di più a un modello, le sue regole di permesso e la sua modalità, conta solo dove ti sei fidato della cartella, e così gli skill, gli agenti e i comandi del suo .glut. Leggi ciò che /trust mostra prima di dire on: il .glut di un repository clonato è la parola di qualcun altro. La sua modalità non va mai oltre la tua, e i suoi hook non girano mai, fidata o no.

La modifica conta dalla prossima sessione della cartella (glut -c riprende la conversazione). Mai la cartella home, la radice o una cartella che non è solo tua (di un altro utente, o una che chiunque può scrivere, come /tmp). L’elenco è tenuto in ~/.glut/trusted.json, che nessun comando eseguito da Glut scrive; glut trust show|add|remove|list fa lo stesso fuori da Glut.
