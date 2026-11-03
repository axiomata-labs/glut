Piani che girano mentre Glut è chiuso: un compito a un’ora del giorno o ogni tanto.

/schedule — la scheda Esecuzioni (⌃X R): ogni piano con la sua prossima ora e il suo ultimo risultato
/schedule list — i piani nel flusso
/schedule add <name> [flags] <task> — crea un piano, o modifica quello con quel nome
/schedule run <name> — lo esegue subito, accanto a Glut
/schedule pause <name> — lo sospende fino a resume
/schedule resume <name> — torna a girare alle sue ore
/schedule remove <name> — il piano e il suo registro spariscono; le sue sessioni restano

I flag sono quelli di `glut schedule add`:
- `--at 07:00` un’ora del giorno, con `--days mo-fr` o `sa,su` (senza, ogni giorno)
- `--every 2h` un intervallo da 10m a 7d, al posto di --at
- `--mode ask|build|plan|dont_ask|auto` cosa può fare senza una regola; ask se non nominato
- `--allow <rule>`, `--deny <rule>` regole nella forma di Claude Code, come `Bash(cargo test:*)`; ripetibili
- `--folder <path>` una cartella accanto alla sua; ripetibile
- `--max-usd 1`, `--max-minutes 30` il massimo che un’esecuzione può costare e durare: 1 $ e 30 min se non nominati
- `--max-tokens 1M` su un abbonamento, il massimo di token che un’esecuzione può usare: 1M se non indicato
- `--replace` le regole e le cartelle date prendono il posto di quelle del piano invece di aggiungersi
- `--cwd <path>`, `--provider <id>`, `--model <id>` la sua cartella (altrimenti questa), provider e modello, fissati alla creazione

Il compito è un messaggio, oppure /skill <name> [a che scopo]. launchd avvia ogni esecuzione, solo mentre hai effettuato l’accesso; un’ora persa durante la sospensione viene recuperata una volta al risveglio. Là non c’è nessuno a rispondere: ogni domanda viene rifiutata e annotata, perciò consenti ciò che serve. Un’esecuzione è una sessione conservata a sé, annunciata da una notifica di macOS. Nella scheda Esecuzioni: ⏎ apre un piano o la sessione di un’esecuzione, r la esegue subito, p la sospende o riprende, d la elimina.

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" esegui i test e riferisci cosa fallisce
