Guarda e imposta le chiavi nascoste al modello in questa sessione.

/secrets — ogni valore nascosto finora, per numero, tipo, lunghezza e dove è stato visto (mai il valore)
/secrets allow <n> — lascia passare il segreto n per questa sessione
/secrets allow <n> always — lo lascia passare sempre (se ne conserva l’impronta, mai il valore)
/secrets add <name> <regex> — nasconde un formato tuo, dal prossimo risultato in poi
/secrets remove <name> — elimina un formato tuo
/secrets on|off — mascheramento acceso o spento

Una chiave in ciò che risponde uno strumento arriva al modello come ‹secret n: …›; Glut rimette il suo valore solo in un file che già lo contiene. Un tuo messaggio che sembra contenere una chiave viene chiesto prima di partire. Il modello sa di una modifica con il tuo prossimo messaggio. I tuoi formati e le tue scelte sono conservati in `~/.glut/secrets.json`.

Il mascheramento protegge da una chiave mostrata per caso; la sandbox e il Gate di rete restano le barriere.
