Passa a un altro provider di modelli.

/provider — i provider che hai configurato
/provider <id> — passa a quello, sul suo modello predefinito
/provider key [id] — cambia solo la chiave di quel provider (senza id, quella del provider predefinito)

I provider sono conservati in `~/.glut/providers.json`, le loro chiavi nel Keychain (o in `~/.glut/credentials.json`); `glut providers add <id>` ne aggiunge uno. La conversazione prosegue, e il prossimo avvio comincia con il provider scelto.

La chiave si digita nascosta e viene provata sulla lista di modelli del provider prima di sostituire quella vecchia; se fallisce, scegli se tenerla comunque, altrimenti resta la vecchia. La sessione usa subito una chiave salvata. Un provider che accede con un account, non ha bisogno di chiave o la prende da una variabile d'ambiente te lo dice. Nella shell: `glut providers key <id>`.
