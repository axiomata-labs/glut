//! The commands' manual pages in Italian (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 57] = [
    (
        "clear",
        "Inizia una nuova sessione vuota in questa cartella.\n\
         \n\
         /clear — la conversazione, l’elenco delle attività del piano, il costo e il contatore ripartono da zero\n\
         \n\
         La sessione che lasci viene conservata: /resume la riapre. Modello, modalità e impostazioni restano come \
         sono.",
    ),
    (
        "resume",
        "Apri una sessione precedente di questa cartella e vai avanti da lì.\n\
         \n\
         /resume — l’elenco delle sessioni conservate di questa cartella, le più recenti per prime; una biforcazione \
         sta sotto la sessione da cui nasce\n\
         \n\
         Nell’elenco: scrivi per filtrare, ⏎ ne apre una, ⌫ ne elimina una (prima chiede conferma; le sue biforcazioni \
         restano sessioni a sé), ⌃E ne scrive una in Markdown in `~/.glut/exports`. Una sessione aperta in un altro \
         pane si raggiunge là (⌃P).\n\
         \n\
         /search trova una sessione da ciò che vi è stato detto.",
    ),
    (
        "search",
        "Trova una sessione conservata dalle sue parole: l’elenco di /resume, filtrato dal testo della sua \
         conversazione.\n\
         \n\
         /search <text> — le sessioni di questa cartella la cui conversazione contiene il testo\n\
         \n\
         Nell’elenco, ⌃A passa tra questa cartella e tutti i progetti. ⏎ apre una sessione di questa cartella; una di \
         un altro progetto indica la sua cartella — avvia Glut là per proseguirla.",
    ),
    (
        "btw",
        "Fai una domanda a margine senza disturbare la conversazione: la risposta arriva in una finestra sopra \
         l’input e non diventa mai parte della sessione.\n\
         \n\
         /btw <question> — chiede della sessione di questo pane, anche mentre il suo turno prosegue\n\
         /btw <name> <question> — chiede di un altro pane o di un agente in background, per numero o nome\n\
         /btw — riapre l’ultima risposta\n\
         \n\
         Il modello risponde da ciò che la conversazione contiene ora e non esegue alcuno strumento: se dovesse \
         cercare qualcosa, chiedilo come messaggio. Il suo costo conta nel costo della sessione. ⎋ chiude la finestra, \
         PgUp PgDn la scorrono.\n\
         \n\
         /btw reviewer a che punto sei?",
    ),
    (
        "projects",
        "Prosegui in un altro progetto: una delle cartelle in cui Glut ha lavorato, o una qualsiasi dal suo \
         percorso.\n\
         \n\
         /projects — l’elenco: ★ fissato, ● questo; ⏎ ne apre uno, p lo fissa o lo libera, x lo nasconde finché non si \
         lavora di nuovo in esso\n\
         /projects <path> — apre quella cartella (⇥ completa il nome di una cartella)\n\
         \n\
         Mentre un turno o un agente lavora ancora, Glut chiede prima di fermarli e cambiare. Le sessioni e i pane di \
         questa finestra restano. In una finestra aperta da un’app ospite, la cartella la sceglie l’ospite.",
    ),
    (
        "folder",
        "Dai a questa sessione cartelle accanto a quella di lavoro, così Glut può lavorare tra più progetti.\n\
         \n\
         /folder — le cartelle accanto a quella di lavoro\n\
         /folder add <path> — ne aggiunge una per questa sessione\n\
         /folder remove <path> — ne toglie una\n\
         \n\
         Gli strumenti dei file raggiungono una tale cartella dal suo percorso completo, i comandi possono scriverci \
         dentro la sandbox, il Gate la giudica come la cartella di lavoro e anche undo ne ripristina i file. Solo tu \
         aggiungi una cartella, mai un modello, una Skill o un progetto. Glut rifiuta la cartella home, una cartella \
         che contiene il progetto o ci sta dentro, una con segreti e due cartelle una dentro l’altra.\n\
         \n\
         Le cartelle valgono solo per questa sessione; `glut run --folder <path>` ne dà una a un’esecuzione headless.",
    ),
    (
        "compact",
        "Riassumi la conversazione finora, così occupa meno della finestra del modello.\n\
         \n\
         /compact — il modello scrive un riassunto che prende il posto dei messaggi più vecchi\n\
         \n\
         Il prompt di sistema, il tuo ultimo messaggio e i passi interi restano; ciò che il riassunto sostituisce \
         viene conservato nell’archivio dei risultati della sessione, mai buttato. Glut compatta anche da solo poco \
         prima che la finestra sia piena (`compact` in `~/.glut/settings.json`). /context mostra quanto è piena la \
         finestra.",
    ),
    (
        "handoff",
        "Passa il lavoro a una sessione successiva di questa cartella.\n\
         \n\
         /handoff write — il modello scrive cosa è stato fatto, a che punto stanno le cose e cosa viene dopo\n\
         /handoff read — porta il passaggio di consegne della cartella in questa sessione\n\
         \n\
         Un passaggio di consegne è conservato in `~/.glut/handoffs`, uno per cartella. Una volta letto si sposta da \
         parte, così nessuna sessione successiva lo legge due volte. Scriverne uno è una chiamata a pagamento a sé; la \
         conversazione resta com’è.",
    ),
    (
        "export",
        "Scrivi la sessione per leggerla o condividerla.\n\
         \n\
         /export — Markdown in `~/.glut/exports`\n\
         /export html — un solo file HTML, con gli stili dentro, senza script\n\
         /export jsonl — i messaggi come righe JSON\n\
         /export copy — il Markdown negli appunti invece che in un file\n\
         /export full — aggiunge i risultati interi degli strumenti e il prompt di sistema\n\
         /export <format> <path> — in un file tuo (`~/` è la tua home, un percorso relativo sta nella cartella di \
         lavoro)\n\
         \n\
         Le parole vanno in qualsiasi ordine, il percorso per ultimo. Senza full l’esportazione mostra ciò che il \
         modello ha visto: i risultati lunghi accorciati. Un file esistente non viene mai sovrascritto. Le chiavi che \
         Glut ha nascosto restano nascoste, ma ciò che hai digitato tu potrebbe contenerne una: leggi un’esportazione \
         prima di condividerla. In /resume, ⌃E esporta una sessione dell’elenco.",
    ),
    (
        "quit",
        "Termina Glut, con ogni pane e agente in background.\n\
         \n\
         /quit — mentre un altro pane o agente lavora ancora, Glut lo dice prima: un secondo /quit termina anche \
         loro\n\
         \n\
         ⌃C ⌃C fa lo stesso, e ⌃D su una riga vuota mentre Glut riposa. Le sessioni sono conservate per /resume.",
    ),
    ("exit", "Termina Glut, come fa /quit."),
    (
        "model",
        "Scegli il modello con cui parla questa sessione.\n\
         \n\
         /model — i modelli del provider in uso, dal suo elenco conservato\n\
         /model <id> — passa a quel modello dal suo id\n\
         \n\
         La conversazione prosegue con il nuovo modello, e il prossimo avvio comincia con esso. Un modello di un altro \
         provider si sceglie dopo /provider. `glut models refresh` rinnova gli elenchi.",
    ),
    (
        "provider",
        "Passa a un altro provider di modelli.\n\
         \n\
         /provider — i provider che hai configurato\n\
         /provider <id> — passa a quello, sul suo modello predefinito\n\
         /provider key [id] — cambia solo la chiave di quel provider (senza id, quella del provider predefinito)\n\
         \n\
         I provider sono conservati in `~/.glut/providers.json`, le loro chiavi nel Keychain (o in \
         `~/.glut/credentials.json`); `glut providers add <id>` ne aggiunge uno. La conversazione prosegue, e il \
         prossimo avvio comincia con il provider scelto.\n\
         \n\
         La chiave si digita nascosta e viene provata sulla lista di modelli del provider prima di sostituire quella \
         vecchia; se fallisce, scegli se tenerla comunque, altrimenti resta la vecchia. La sessione usa subito una \
         chiave salvata. Un provider che accede con un account, non ha bisogno di chiave o la prende da una variabile \
         d'ambiente te lo dice. Nella shell: `glut providers key <id>`.",
    ),
    (
        "login",
        "Accedere con un abbonamento invece che con una chiave — oggi il tuo piano ChatGPT Plus o Pro.\n\
         \n\
         /login — gli abbonamenti, ciascuno con il suo stato; ⏎ accede o esce\n\
         /login <provider> — accede subito a quello, come /login chatgpt\n\
         /login <provider> <address> — via SSH: l'indirizzo su cui si è fermato il browser, incollato di nuovo\n\
         /login openrouter — OpenRouter dà a Glut una chiave sua, conservata come una digitata; limitala o cancellala \
         su openrouter.ai/settings/keys\n\
         \n\
         Prima viene mostrato l'indirizzo; ⏎ lo apre nel tuo browser. Glut conserva l'accesso nel portachiavi (o in \
         `~/.glut/credentials.json`) e lo rinnova da solo; uscire lo revoca presso OpenAI.\n\
         \n\
         Con il piano una chiamata non costa denaro, ma valgono i limiti d'uso del piano: quando uno viene raggiunto \
         il turno finisce e Glut rimanda a Impostazioni di ChatGPT → Utilizzo, dove limiti anche ciò che ogni app può \
         usare. Nella shell: glut providers login|logout <provider>.",
    ),
    (
        "effort",
        "Imposta quanto a fondo il modello pensa prima di rispondere.\n\
         \n\
         /effort — i livelli tra cui scegliere\n\
         /effort <level> — none, minimal, low, medium, high, xhigh o max\n\
         /effort default — decide il provider\n\
         \n\
         Più impegno di solito significa risposte migliori sul lavoro difficile, più lente e a un costo maggiore. Non \
         ogni modello conosce ogni livello. Il contatore (⌃N) mostra il livello in uso.",
    ),
    (
        "context",
        "Guarda quanto è piena la finestra del modello, o di’ a Glut quanto è grande.\n\
         \n\
         /context — i Token usati, la finestra e da dove viene la sua dimensione, e quanto costano a ogni turno le \
         guide (`AGENTS.md`, `CLAUDE.md`)\n\
         /context <tokens> — imposta la finestra di questo modello: 65536, 64k o 1M\n\
         \n\
         Una finestra che imposti è conservata in `~/.glut/providers.json` per questo modello e vale prima dell’elenco \
         del provider. Glut compatta da solo poco prima che la finestra sia piena.",
    ),
    (
        "thinking",
        "Scegli come appare il ragionamento del modello nel flusso.\n\
         \n\
         /thinking off — nascosto\n\
         /thinking folded — una riga per pensiero; un clic ne apre uno\n\
         /thinking open — ogni pensiero mostrato per intero\n\
         \n\
         /thinking da solo li ripiega. ⌃T apre o ripiega tutti i pensieri in una volta. La tua scelta è conservata in \
         `~/.glut/prefs.json`.",
    ),
    (
        "panel",
        "Mostra il pannello laterale sulla sua scheda Piano, o nascondilo.\n\
         \n\
         /panel — il pannello laterale con le sue schede: Piano, Gate, Contatore, Agenti, Da fare, Esecuzioni e \
         Tasti\n\
         \n\
         ⌃B fa lo stesso, ⌃X ⇥ passa i tasti al pannello (←→ le schede, ↑↓ al loro interno, ⎋ indietro). Il pannello \
         richiede una finestra larga almeno 100 colonne. Se si mostra o no viene conservato per il prossimo avvio.",
    ),
    (
        "meter",
        "Guarda cosa usa la sessione: il contesto, i Token e il costo.\n\
         \n\
         /meter — la scheda Contatore del pannello laterale\n\
         \n\
         Mostra quanto è piena la finestra e quando Glut compatta, i Token della sessione in ingresso, in cache e in \
         uscita con il loro prezzo, ogni turno e ogni strumento, e ciò che l’output lean di Glut ha risparmiato. Anche \
         ⌃N lo apre.",
    ),
    (
        "agents",
        "Guarda i subagenti che Glut ha avviato in questa sessione.\n\
         \n\
         /agents — l’elenco dei subagenti, ciascuno sotto quello che l’ha avviato; ⏎ ne apre il flusso\n\
         \n\
         Anche un clic sulla tessera di un subagente lo apre. ⌃X A apre la scheda Agenti: ogni subagente e ogni agente \
         in background, dal vivo — ⏎ ne apre uno, x lo ferma.",
    ),
    (
        "plan",
        "Leggi per intero il piano che Glut ha inviato dalla modalità piano, con il suo elenco di attività.\n\
         \n\
         /plan — l’ultimo piano di questa sessione\n\
         \n\
         Nella modalità piano (⇧⇥) Glut legge e pianifica senza cambiare nulla, poi invia il suo piano perché tu lo \
         approvi o lo rimandi indietro. /send plan lo passa a un altro pane da costruire.",
    ),
    (
        "scrollback",
        "Metti la conversazione nello scrollback del terminale, per scorrerla, cercarla e copiarla con gli strumenti \
         del terminale.\n\
         \n\
         /scrollback — stampa la conversazione; ⏎ o ⌃C torna a Glut\n\
         \n\
         F3 fa lo stesso.",
    ),
    (
        "split",
        "Apri una nuova sessione in un pane accanto a quello principale.\n\
         \n\
         /split — un nuovo pane; in un repository git Glut chiede dove lavora\n\
         /split here — lavora in questa cartella, accanto a te\n\
         /split tree [branch] — lavora in un nuovo Worktree su un branch tutto suo\n\
         /split resume — apre una sessione precedente, scelta dall’elenco\n\
         /split as <role> — recita un ruolo (/role li elenca)\n\
         \n\
         Fino a sei pane stanno accanto a quello principale, in due file da tre. ⌃P passa da un pane all’altro, anche \
         ⌃X 2 divide, ⌃X } e ⌃X { rendono il pane principale più largo o più stretto. Due pane qui cambiano gli stessi \
         file; un Worktree li tiene separati finché /close chiede come finisce.\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "swap",
        "Fai scambiare di posto il pane principale e un altro.\n\
         \n\
         /swap — questo pane diventa quello principale\n\
         /swap <n> — il pane n diventa quello principale",
    ),
    (
        "close",
        "Chiudi un pane, o termina un agente in background.\n\
         \n\
         /close — chiude questo pane (mai quello principale); mentre lavora, un secondo /close lo ferma e lo chiude\n\
         /close <name> — termina quell’agente in background, con il suo turno fermato\n\
         \n\
         La sessione è conservata per /resume. Un pane in un Worktree chiede come finisce il Worktree: tenerlo con il \
         suo branch, assumerlo (unirlo con merge al branch su cui sei) o scartarlo. Anche ⌃X 0 chiude questo pane.",
    ),
    (
        "send",
        "Invia un messaggio a un altro pane o agente in background, ricevuto là come se lo avessi digitato.\n\
         \n\
         /send <text> — all’unico altro pane\n\
         /send <n|name> <text> — al pane n o all’agente con quel nome (necessario da tre in su)\n\
         /send plan — passa il piano di questo pane all’altro pane da costruire\n\
         /send <n|name> plan — lo passa a quello\n\
         \n\
         Il messaggio parte subito dove la sessione riposa, altrimenti dopo il suo turno. I tasti restano in questo \
         pane.",
    ),
    (
        "role",
        "Fai recitare un ruolo a questo pane: il prompt e gli strumenti di un agente con nome, mai il suo modello.\n\
         \n\
         /role — i ruoli tra cui scegliere\n\
         /role <name> — recita quel ruolo da ora in poi\n\
         /role none — di nuovo un tuttofare, con ogni strumento\n\
         \n\
         Glut porta reviewer, tester e docs; i tuoi ruoli stanno in `~/.glut/agents`, quelli di un progetto in \
         `.glut/agents` (letti solo in una cartella fidata). Gli strumenti di un ruolo valgono subito, il suo prompt \
         dal turno successivo. Un ruolo che restringe gli strumenti richiede il Gate.",
    ),
    (
        "bg",
        "Avvia un agente in background: una sessione senza pane, raggiunta per nome.\n\
         \n\
         /bg — un agente di nome agent (o agent-2, …); in un repository git Glut chiede dove lavora\n\
         /bg <name> — un agente con quel nome (lettere minuscole, cifre e trattini)\n\
         /bg as <role> — recita un ruolo, e prende il nome da esso\n\
         /bg here — lavora in questa cartella\n\
         /bg tree [branch] — lavora in un nuovo Worktree\n\
         \n\
         /send <name> gli dà lavoro, /peek <name> ne legge il flusso, /show <name> lo porta in primo piano, /close \
         <name> lo termina. Ogni agente lavora e costa per conto suo: Glut chiede una volta per finestra prima di un \
         settimo. ⌃X B ne avvia uno con il ruolo scelto prima, ⌃X A li elenca.\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "Porta in primo piano un agente in background come pane.\n\
         \n\
         /show <name> — l’agente diventa l’ultimo pane della griglia\n\
         \n\
         Quando la finestra non ha più posto, prima fai /hide o /close su un pane. ⌃X A elenca gli agenti in \
         background.",
    ),
    (
        "hide",
        "Manda questo pane in background, con la sua memoria conservata.\n\
         \n\
         /hide — il pane lascia lo schermo e continua a lavorare come agente in background\n\
         \n\
         Conserva un nome con cui essere raggiunto (il suo ruolo, o pane-<n>): /show <name> lo riporta indietro, /send \
         <name> e /peek <name> lo raggiungono. Il pane principale e l’ultimo restano. ⌃X H fa lo stesso.",
    ),
    (
        "peek",
        "Leggi il flusso di un agente in background sopra questo pane, senza portarlo in primo piano.\n\
         \n\
         /peek <name> — il suo flusso, dal vivo, solo da leggere\n\
         /peek — lo richiude\n\
         \n\
         Anche ⎋ lo chiude; /show <name> porta l’agente in primo piano.",
    ),
    (
        "loop",
        "Esegui un compito più e più volte in un agente in background tutto suo, finché Glut è aperto.\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — avvia un ciclo; il primo giro subito\n\
         /loop — elenca i cicli: intervallo, giri, il prossimo giro e il costo finora\n\
         /loop stop [name] — termina quel ciclo, o l’unico\n\
         /loop rounds [n] — il massimo di giri di un ciclo; da solo mostra il limite attuale\n\
         /loop budget [usd] — il massimo che un ciclo può costare; da solo mostra il limite attuale\n\
         /loop tokens [n] — il massimo di token che un ciclo su un abbonamento può usare (5M, 500k); da solo mostra \
         il limite attuale\n\
         \n\
         I giri non si sovrappongono mai: l’intervallo è contato dalla fine di un giro, come 30s, 5m, 2h o 1h30m (da \
         10s a 24h). Un giro prosegue con la conversazione del giro precedente; con fresh ogni giro ne comincia una \
         nuova. Il compito è un messaggio, oppure /skill <name> [a che scopo], /compact o /init — nessun altro \
         comando.\n\
         \n\
         L’agente si chiama loop (o come il suo ruolo) e prende la modalità di questo pane, il «sempre» che hai dato \
         qui, le sue cartelle accanto e il suo ruolo; as <role> può nominare solo il ruolo che questo pane recita. Le \
         sue domande aspettano te. Una tessera arriva quando un giro chiede o il ciclo termina; con notify dopo ogni \
         giro.\n\
         \n\
         Un ciclo termina dopo i suoi giri o quando i suoi giri costano il suo budget (di norma 50 giri e 5 $, \
         altrimenti `loop.rounds` e `loop.maxUsd` di `~/.glut/settings.json`; una modifica vale per i cicli avviati \
         dopo), quando un giro fallisce o lo fermi tu (⎋ nel suo giro), oppure quando il suo modello chiama `end_loop` \
         perché la fine che il tuo compito indicava è raggiunta. Quando i prezzi del modello sono sconosciuti, lo \
         terminano solo i giri. Il suo agente resta poi nella scheda Agenti, da leggere o da /close. x sulla sua voce \
         nella scheda Agenti (⌃X A) termina il ciclo.\n\
         \n\
         /loop 10m esegui i test e correggi ciò che fallisce; fermati quando passano tutti\n\
         /loop 1h fresh as reviewer notify rivedi l’ultimo commit",
    ),
    (
        "schedule",
        "Piani che girano mentre Glut è chiuso: un compito a un’ora del giorno o ogni tanto.\n\
         \n\
         /schedule — la scheda Esecuzioni (⌃X R): ogni piano con la sua prossima ora e il suo ultimo risultato\n\
         /schedule list — i piani nel flusso\n\
         /schedule add <name> [flags] <task> — crea un piano, o modifica quello con quel nome\n\
         /schedule run <name> — lo esegue subito, accanto a Glut\n\
         /schedule pause <name> — lo sospende fino a resume\n\
         /schedule resume <name> — torna a girare alle sue ore\n\
         /schedule remove <name> — il piano e il suo registro spariscono; le sue sessioni restano\n\
         \n\
         I flag sono quelli di `glut schedule add`:\n\
         - `--at 07:00` un’ora del giorno, con `--days mo-fr` o `sa,su` (senza, ogni giorno)\n\
         - `--every 2h` un intervallo da 10m a 7d, al posto di --at\n\
         - `--mode ask|build|plan|dont_ask|auto` cosa può fare senza una regola; ask se non nominato\n\
         - `--allow <rule>`, `--deny <rule>` regole nella forma di Claude Code, come `Bash(cargo test:*)`; \
         ripetibili\n\
         - `--folder <path>` una cartella accanto alla sua; ripetibile\n\
         - `--max-usd 1`, `--max-minutes 30` il massimo che un’esecuzione può costare e durare: 1 $ e 30 min se non \
         nominati\n\
         - `--max-tokens 1M` su un abbonamento, il massimo di token che un’esecuzione può usare: 1M se non indicato\n\
         - `--replace` le regole e le cartelle date prendono il posto di quelle del piano invece di aggiungersi\n\
         - `--cwd <path>`, `--provider <id>`, `--model <id>` la sua cartella (altrimenti questa), provider e modello, \
         fissati alla creazione\n\
         \n\
         Il compito è un messaggio, oppure /skill <name> [a che scopo]. launchd avvia ogni esecuzione, solo mentre hai \
         effettuato l’accesso; un’ora persa durante la sospensione viene recuperata una volta al risveglio. Là non c’è \
         nessuno a rispondere: ogni domanda viene rifiutata e annotata, perciò consenti ciò che serve. Un’esecuzione è \
         una sessione conservata a sé, annunciata da una notifica di macOS. Nella scheda Esecuzioni: ⏎ apre un piano o \
         la sessione di un’esecuzione, r la esegue subito, p la sospende o riprende, d la elimina.\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" esegui i test e riferisci cosa \
         fallisce",
    ),
    (
        "diff",
        "Rivedi cosa ha cambiato l’ultimo turno, hunk per hunk.\n\
         \n\
         /diff — le modifiche dell’ultimo turno che ha cambiato file\n\
         /diff session — ogni modifica di questa sessione\n\
         \n\
         Nella revisione: ↑↓ un hunk, ←→ un file, a accetta, r rifiuta (l’hunk torna com’era, e Glut ne legge il \
         motivo con il tuo prossimo messaggio), c commenta (inviato con il tuo prossimo messaggio), v segna un file \
         come rivisto, s passa tra turno e sessione, ⎋ chiude. Anche ⌃R la apre; /review è lo stesso comando.",
    ),
    (
        "steps",
        "Guarda i passi che hanno cambiato file, e ripristina i file.\n\
         \n\
         /steps — ogni passo che ha cambiato un file; ⏎ su uno mostra cosa ha cambiato, o riporta i file a prima di \
         esso\n\
         \n\
         Ripristinare i file si porta dietro le tue modifiche successive; la conversazione resta. Glut conserva uno \
         snapshot prima di ogni passo che cambia qualcosa; anche com’erano i file subito prima viene conservato, così \
         `glut undo` lo riporta indietro. ⎋⎋ riavvolge invece la conversazione.",
    ),
    (
        "todo",
        "Tieni le attività del progetto nel suo `TODO.md`, un elenco di controllo che usate sia tu sia Glut.\n\
         \n\
         /todo — la scheda Da fare: Spazio spunta una voce, a ne aggiunge una, o apre l’editor\n\
         /todo <text> — aggiunge una voce in fondo all’elenco, anche mentre Glut lavora\n\
         /todo open — l’elenco nel tuo editor\n\
         /todo clean — elimina le voci completate, chiedendo prima\n\
         \n\
         Il resto del file non viene mai toccato. ⌃Y mostra o nasconde la scheda.",
    ),
    (
        "memory",
        "Guarda cosa conserva la memoria del progetto: i fatti che Glut ha annotato per le sessioni successive.\n\
         \n\
         /memory — i fatti con le loro descrizioni; ⏎ ne mostra uno per intero\n\
         /memory open — l’indice (`MEMORY.md`) nel tuo editor\n\
         \n\
         La memoria sta fuori dal repository: in `~/.glut/memory/<project>/`, oppure nella memoria di Claude Code dove \
         Claude Code lavora nel repository. Glut la scrive con il suo strumento `memory`; leggerla qui non interroga \
         alcun modello.",
    ),
    (
        "skill",
        "Avvia una Skill: istruzioni per un tipo di compito, che il modello carica e segue.\n\
         \n\
         /skill — le Skill tra cui scegliere\n\
         /skill <name> [a che scopo] — esegue quella Skill su ciò che segue il suo nome\n\
         \n\
         Glut porta make-agent, make-skill e make-hook per scrivere le tue. Le tue stanno in `~/.glut/skills`, quelle \
         di un progetto in `.glut/skills` (lette solo in una cartella fidata); una con lo stesso nome nasconde quella \
         di Glut.\n\
         \n\
         /skill make-hook blocca git push su main",
    ),
    (
        "init",
        "Scrivi un breve `AGENTS.md` per questa cartella, o controlla e accorcia la guida che ha.\n\
         \n\
         /init — senza una guida Glut scrive `AGENTS.md`; con una la controlla e la tiene breve\n\
         /init --migrate — chiede di nuovo se `CLAUDE.md` passa in `AGENTS.md`\n\
         \n\
         Con solo un `CLAUDE.md`, Glut chiede una volta se passa in `AGENTS.md` (`CLAUDE.md` lo importa allora, una \
         guida per ogni harness); un no viene ricordato. Le guide viaggiano con ogni turno: /context mostra quanto \
         costano.",
    ),
    (
        "gate",
        "Guarda le regole con cui decide il Gate, e ritira un «sempre».\n\
         \n\
         /gate — le regole in vigore: i tuoi «sempre» di questa sessione, poi quelli di `~/.glut/settings.json` e del \
         `.glut/` del progetto; ⏎ su un «sempre» lo ritira, e Glut chiede di nuovo la prossima volta\n\
         \n\
         ⌃G apre la scheda Gate: ogni giudizio dal vivo, gli host raggiunti e i rifiuti. ⇧⇥ cambia la modalità.",
    ),
    (
        "mcp",
        "Guarda i server MCP e lo stato di ciascuno, accedi a uno ospitato, aggiungine o rimuovine uno.\n\
         \n\
         /mcp — ogni server: connesso con i suoi strumenti, in attesa di un accesso, o fallito\n\
         /mcp login <name> — accede a un server ospitato; il suo indirizzo si apre nel browser con ⏎\n\
         /mcp login <name> <address> — via SSH: l’indirizzo su cui il browser è finito, incollato di nuovo\n\
         /mcp logout <name> — esce e dimentica il suo Token\n\
         /mcp add <name> <url> — aggiunge un server ospitato a `~/.glut/mcp.json` e lo connette\n\
         /mcp add <name> -- <command> [args] — aggiunge un server che Glut avvia da sé\n\
         /mcp remove <name> — lo toglie da `~/.glut/mcp.json` e dalla sessione\n\
         \n\
         Un server che non offre un accesso utilizzabile da Glut chiede un Token, digitato nascosto. I server propri \
         di un progetto sono del progetto da cambiare: /mcp remove toglie solo i tuoi. `glut mcp` fa lo stesso fuori \
         da una sessione.",
    ),
    (
        "images",
        "Scegli come vengono disegnate le immagini nel flusso.\n\
         \n\
         /images inline — l’immagine vera dove il terminale può mostrarne una\n\
         /images blocks — mezzi blocchi a colori reali\n\
         /images off — solo la riga dell’immagine\n\
         \n\
         /images da solo mostra la scelta attuale. Di norma Glut disegna inline dove il terminale può e a blocchi \
         altrove. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "links",
        "Scegli come appaiono i link nel flusso.\n\
         \n\
         /links clickable — il testo del link è il link, dove il terminale parla OSC 8\n\
         /links plain — l’URL è scritto dopo il testo\n\
         \n\
         /links da solo mostra la scelta attuale. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "notify",
        "Vieni avvisato quando un turno finisce o Glut chiede, mentre la finestra non è in primo piano.\n\
         \n\
         /notify on — la fine di un turno e ogni domanda\n\
         /notify asks — solo le domande\n\
         /notify off — niente\n\
         /notify system — tramite le notifiche di macOS (un clic su una apre Script Editor, non il terminale)\n\
         /notify after <s> — la fine di un turno è annunciata solo dopo che è durato questi secondi (10 di norma, fino \
         a 3600)\n\
         /notify tiles on|off — le tessere in alto a destra nella finestra\n\
         \n\
         /notify da solo mostra cosa vale ora. Glut usa le notifiche del terminale dove sa di averle, e il campanello. \
         Una notifica nomina Glut, il progetto, il pane e cosa è successo, mai le parole del modello. Conservato in \
         `~/.glut/prefs.json`, prima di `notify` in `~/.glut/settings.json`.",
    ),
    (
        "secrets",
        "Guarda e imposta le chiavi nascoste al modello in questa sessione.\n\
         \n\
         /secrets — ogni valore nascosto finora, per numero, tipo, lunghezza e dove è stato visto (mai il valore)\n\
         /secrets allow <n> — lascia passare il segreto n per questa sessione\n\
         /secrets allow <n> always — lo lascia passare sempre (se ne conserva l’impronta, mai il valore)\n\
         /secrets add <name> <regex> — nasconde un formato tuo, dal prossimo risultato in poi\n\
         /secrets remove <name> — elimina un formato tuo\n\
         /secrets on|off — mascheramento acceso o spento\n\
         \n\
         Una chiave in ciò che risponde uno strumento arriva al modello come ‹secret n: …›; Glut rimette il suo valore \
         solo in un file che già lo contiene. Un tuo messaggio che sembra contenere una chiave viene chiesto prima di \
         partire. Il modello sa di una modifica con il tuo prossimo messaggio. I tuoi formati e le tue scelte sono \
         conservati in `~/.glut/secrets.json`.\n\
         \n\
         Il mascheramento protegge da una chiave mostrata per caso; la sandbox e il Gate di rete restano le barriere.",
    ),
    (
        "motion",
        "Scegli quanto si muove lo schermo.\n\
         \n\
         /motion full — Glimm respira, le scintille salgono, la fiamma tremola\n\
         /motion calm — tutto sta fermo; un fotogramma è disegnato solo quando qualcosa cambia\n\
         /motion off — come calm\n\
         \n\
         /motion da solo mostra la scelta attuale. «Riduci movimento» di macOS e la modalità reader tengono fermo lo \
         schermo qualunque cosa scelga. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "rule",
        "Una regola per questa sessione che il modello segue a ogni turno.\n\
         \n\
         /rule <text> — tenerla, come «aggiorna la lista dei compiti a ogni passo»\n\
         /rule — le regole attuali, numerate\n\
         /rule remove <n> — ritirare la regola n\n\
         \n\
         Le regole vanno con le istruzioni di ogni turno, dopo le guide, finché non le ritiri; /clear le tiene, una \
         nuova sessione inizia senza. Al massimo 20, di 500 caratteri ciascuna. Per una regola di ogni sessione, \
         scrivila in `~/.glut/AGENTS.md` o nella guida del progetto.",
    ),
    (
        "maxsteps",
        "Quanti passi fa al massimo un turno.\n\
         \n\
         /maxsteps — il limite attuale\n\
         /maxsteps <n> — un turno si ferma dopo n passi, da 1 a 1000\n\
         \n\
         Un passo è una chiamata al modello. Al limite il turno si ferma e lo dice; ⏎ sull'input vuoto invia «Go on \
         where you stopped.» e il lavoro continua. I budget e il limite di tempo restano i veri freni. Tenuto in \
         `~/.glut/prefs.json`; `maxTurns` in `~/.glut/settings.json` lo fissa per ogni esecuzione, `glut run \
         --max-turns` per una.",
    ),
    (
        "nudge",
        "Se Glut chiede al modello di continuare alla fine di un turno, di stare alla sua lista di compiti e di \
         scriverne una quando lavora senza.\n\
         \n\
         /nudge on — quando un turno finisce con punti della lista di compiti aperti, o con parole che annunciano un \
         passo («ora passo…», due punti alla fine), Glut manda al modello una riga sua: continuare o dire perché si \
         ferma; il flusso lo mostra in silenzio. Se la lista non cambia mentre lavora, Glut gliela ricorda, e se non \
         cambia nulla, una volta con un messaggio a parte; se in \
         un'esecuzione non ne ha scritta nessuna, gliene chiede una, una volta per turno\n\
         /nudge off — un turno finisce dove il modello lo finisce: nessun richiamo alla lista, nessuna richiesta\n\
         \n\
         La domanda alla fine viene una volta per turno, mai in modalità plan, dopo una domanda a te, in un giro di un \
         ciclo o nell'esecuzione di un piano; il richiamo e la richiesta seguono la stessa scelta, ma nell'esecuzione \
         di un piano Glut ricorda sempre. /nudge da solo \
         mostra la scelta attuale. Tenuto in `~/.glut/prefs.json`; `\"nudge\": false` in `~/.glut/settings.json` lo \
         spegne per ogni esecuzione.",
    ),
    (
        "reader",
        "Configura Glut per uno screen reader.\n\
         \n\
         /reader on — tasti a parole, un piè di pagina semplice, niente Glimm, cornici semplici, niente che si muove e \
         una riga di testo a ogni cambio di stato\n\
         /reader off — lo schermo consueto\n\
         \n\
         /reader da solo mostra la scelta attuale. Conservato in `~/.glut/prefs.json`; `glut --reader` lo tiene acceso \
         per una sola esecuzione.",
    ),
    (
        "language",
        "Scegli la lingua in cui Glut ti parla.\n\
         \n\
         /language — le lingue tra cui scegliere\n\
         /language <code> — en, de, fr, it, es, zh, ja o pt\n\
         \n\
         Senza una scelta Glut parla la lingua del tuo sistema. Il modello risponde nella lingua in cui scrivi, \
         qualunque cosa scelga qui. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "theme",
        "Scegli i colori di Glut.\n\
         \n\
         /theme — i temi tra cui scegliere\n\
         /theme <name> — glut, quello proprio di Glut, o uno dei temi di Axiomata (catppuccin-mocha, github-dark, …)\n\
         \n\
         Glut colora solo il testo e tiene lo sfondo del terminale: un tema chiaro vuole un terminale chiaro. \
         Conservato in `~/.glut/prefs.json`; in una finestra aperta da un’app ospite, la scelta vale per quella \
         sessione.",
    ),
    (
        "editor",
        "Scegli l’editor in cui Glut apre i file e l’inserimento.\n\
         \n\
         /editor — gli editor trovati sul tuo sistema\n\
         /editor <command> — un tuo comando, con `{file}` e `{line}` dove vanno\n\
         \n\
         Senza `{file}` il file viene aggiunto in fondo. Un editor a finestra deve aspettare finché il file è chiuso \
         (`code --wait`). ⌃X E modifica l’inserimento con esso; anche /todo open e /memory open lo usano. Conservato \
         in `~/.glut/prefs.json`.\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "Scegli come una sessione a riposo mantiene la cache dei prompt del provider, così il turno successivo parte a \
         caldo.\n\
         \n\
         /cache ping — Glut rilegge la cache ogni pochi minuti mentre la sessione riposa, fino a 30 minuti\n\
         /cache 1h — Anthropic la tiene un’ora (ogni scrittura costa il doppio); gli altri provider vengono letti per \
         mantenerla\n\
         /cache off — non mantenuta: una pausa oltre i pochi minuti del provider riparte a freddo\n\
         \n\
         /cache da solo mostra la scelta attuale; ping è il predefinito. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "footer",
        "Scegli come il piè di pagina unisce le sue parti.\n\
         \n\
         /footer powerline — le frecce di Powerline\n\
         /footer plain — blocchi semplici\n\
         \n\
         /footer da solo mostra la scelta attuale e perché. Senza una scelta Glut deduce dal terminale se disegna i \
         glifi di Powerline. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "vim",
        "Modifica l’inserimento alla maniera di vi.\n\
         \n\
         /vim on — ⎋ passa alla modalità Normal (movimenti, operatori, conteggi, u e ⌃R), i a I A o O tornano a \
         Insert\n\
         /vim off — i consueti tasti di modifica\n\
         \n\
         /vim da solo lo attiva. ⎋ nella modalità Normal ferma Glut. Conservato in `~/.glut/prefs.json`.",
    ),
    (
        "help",
        "Ogni comando e ogni tasto, in un riquadro a sé.\n\
         \n\
         /help — i comandi con una riga ciascuno; scrivere filtra, ↑↓ scelgono, ⏎ apre la pagina intera di uno\n\
         /help <command> — la pagina di quel comando subito, come /help loop\n\
         \n\
         In una pagina ↑↓ e PgUp PgDn scorrono, ⎋ torna all’elenco e da lì chiude il riquadro. L’ultima voce contiene \
         ogni tasto per tema, !cmd e la modalità vi; ⌃K tiene i tasti nel pannello laterale.",
    ),
];
