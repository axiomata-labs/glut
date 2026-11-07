Tornare a prima di uno dei tuoi messaggi: la conversazione, i file o entrambi.

/rewind — i tuoi messaggi, il più recente prima; ⏎ su uno chiede cosa torna indietro: conversazione e file, solo la conversazione, solo i file, o una diramazione (una nuova sessione da lì). ⎋⎋ apre la stessa lista.

Non si perde nulla: ciò che il ritorno taglia resta una sessione a sé sotto questa, «(before rewind)» in /resume, e la cartella com’era viene conservata con essa. Aprendo quella sessione, Glut chiede una volta se tornano anche i suoi file. I file vengono dagli snapshot di Glut, quindi torna anche ciò che un comando ha cambiato; lo stato subito prima viene prima messo da parte (`glut undo`). Oltre una compattazione possono tornare solo i file.
