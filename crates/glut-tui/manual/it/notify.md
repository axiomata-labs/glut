Vieni avvisato quando un turno finisce o Glut chiede, mentre la finestra non è in primo piano.

/notify on — la fine di un turno e ogni domanda
/notify asks — solo le domande
/notify off — niente
/notify system — tramite le notifiche di macOS (un clic su una apre Script Editor, non il terminale)
/notify after <s> — la fine di un turno è annunciata solo dopo che è durato questi secondi (10 di norma, fino a 3600)
/notify tiles on|off — le tessere in alto a destra nella finestra

/notify da solo mostra cosa vale ora. Glut usa le notifiche del terminale dove sa di averle, e il campanello. Una notifica nomina Glut, il progetto, il pane e cosa è successo, mai le parole del modello. Conservato in `~/.glut/prefs.json`, prima di `notify` in `~/.glut/settings.json`.
