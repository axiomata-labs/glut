Scegli come una sessione a riposo mantiene la cache dei prompt del provider, così il turno successivo parte a caldo.

/cache ping — Glut rilegge la cache ogni pochi minuti mentre la sessione riposa, fino a 30 minuti
/cache 1h — Anthropic la tiene un’ora (ogni scrittura costa il doppio); gli altri provider vengono letti per mantenerla
/cache off — non mantenuta: una pausa oltre i pochi minuti del provider riparte a freddo

/cache da solo mostra la scelta attuale; ping è il predefinito. Conservato in `~/.glut/prefs.json`.
