//! Italiano.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "lingua: {name}",
    language_unkept: "lingua: {name} — solo per questa esecuzione: {reason}",
    language_unknown: "nessuna lingua {name} — {known}",
    language_title: "Lingua di Glut",
    language_now: "attuale",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} pane",
    },
    topics: &[
        "Sessione",
        "Modello",
        "Vista",
        "Pane",
        "Modifiche",
        "Progetto",
        "Impostazioni",
        "Inserimento",
        "Modificare la riga",
        "Aiuto",
    ],
    commands: &[
        ("clear", "iniziare una nuova sessione"),
        (
            "resume",
            "aprire una sessione precedente di questa cartella — le biforcazioni sotto la loro",
        ),
        (
            "projects",
            "continuare in un altro progetto: le cartelle in cui Glut ha lavorato, o una dal suo percorso",
        ),
        ("compact", "riassumere la conversazione finora"),
        (
            "handoff",
            "/handoff write: cosa è stato fatto e cosa viene dopo; /handoff read in una nuova sessione",
        ),
        ("quit", "terminare la sessione"),
        ("exit", "terminare la sessione, come /quit"),
        (
            "model",
            "scegliere un modello del provider in uso, o /model <id>",
        ),
        ("provider", "cambiare provider, o /provider <id>"),
        (
            "effort",
            "quanto a fondo pensa il modello, o /effort <level>",
        ),
        (
            "context",
            "quanto è piena la finestra; /context 64k la imposta",
        ),
        (
            "thinking",
            "il ragionamento del modello: /thinking off, folded o open",
        ),
        (
            "panel",
            "il pannello laterale con il piano, o nasconderlo (⌃B; ⌃G il gate)",
        ),
        (
            "meter",
            "contesto, token e costo per turno e strumento, i risparmi di lean (⌃N)",
        ),
        (
            "agents",
            "i subagenti della sessione: aprire il flusso di uno (anche un clic sulla sua tessera; ⌃X A: la scheda Agenti)",
        ),
        (
            "plan",
            "il piano che Glut ha inviato dalla modalità piano, intero, con il suo elenco di attività",
        ),
        (
            "scrollback",
            "la conversazione nello scrollback del terminale (F3)",
        ),
        (
            "split",
            "una nuova sessione accanto al pane principale (fino a sei, due file da tre); in un repository git qui o in un worktree",
        ),
        (
            "swap",
            "il pane principale e il pane n (o questo) si scambiano di posto",
        ),
        (
            "close",
            "chiudere questo pane (non il principale) — o terminare un agente in background; la sua sessione resta per /resume",
        ),
        (
            "send",
            "un messaggio a un altro pane (prima il suo numero, da tre in su), ricevuto là come se fosse digitato; /send plan gli passa il tuo piano da costruire",
        ),
        (
            "role",
            "il ruolo di questo pane: prompt e strumenti di un agente con nome, non il suo modello — reviewer, tester, docs o i tuoi",
        ),
        (
            "bg",
            "un agente in background: una sessione senza pane, raggiungibile per nome — /bg as reviewer",
        ),
        (
            "show",
            "portare in primo piano come pane un agente dal background",
        ),
        (
            "hide",
            "mandare questo pane in background, con la sua memoria (⌃X H)",
        ),
        (
            "peek",
            "leggere il flusso di un agente in background sopra questo pane; Esc lo chiude",
        ),
        (
            "diff",
            "rivedere cosa ha cambiato l’ultimo turno, hunk per hunk; /diff session (⌃R)",
        ),
        (
            "steps",
            "i passi che hanno cambiato file: cosa ha cambiato ciascuno, o ripristinare i file",
        ),
        (
            "todo",
            "il TODO.md del progetto (⌃Y): /todo <text> ne aggiunge uno, /todo open lo modifica, /todo clean elimina quelli completati",
        ),
        (
            "memory",
            "cosa tiene la memoria del progetto: ⏎ mostra un fatto; /memory open ne modifica l’indice",
        ),
        (
            "skill",
            "avviare una skill: /skill <name> [a che scopo] — /skill da solo li elenca (make-agent, make-skill, make-hook, …)",
        ),
        (
            "init",
            "un breve AGENTS.md per questa cartella, o controllarne la guida (/init --migrate chiede di nuovo per CLAUDE.md)",
        ),
        (
            "gate",
            "le regole in vigore; ritirare un «sempre» (⌃G: il gate, dal vivo)",
        ),
        (
            "images",
            "come vengono disegnate le immagini: quella vera dove il terminale può, mezzi blocchi a colori reali, o solo la loro riga",
        ),
        (
            "links",
            "il testo di un link è il link dove il terminale parla OSC 8, altrimenti l’URL scritto dopo",
        ),
        (
            "motion",
            "quanto si muove lo schermo; calm e off tengono fermo Glimm (in macOS «Riduci movimento» conta come off)",
        ),
        (
            "reader",
            "per uno screen reader: tasti a parole, uno schermo semplice, niente che si muove, una riga a ogni cambio di stato",
        ),
        (
            "language",
            "la lingua in cui Glut ti parla: en de fr it es zh ja pt — il modello risponde in quella in cui scrivi",
        ),
        (
            "theme",
            "i colori di Glut, o quelli di un tema di Axiomata: /theme <name>",
        ),
        ("editor", "scegliere il tuo editor, o /editor <command>"),
        (
            "cache",
            "come una sessione a riposo mantiene la sua cache: rilettura (ping), l’ora di Anthropic (1h), o per niente",
        ),
        (
            "footer",
            "le giunzioni del piè di pagina: frecce Powerline o blocchi semplici (dedotti dal terminale)",
        ),
        (
            "vim",
            "modificare l’inserimento alla maniera di vi: /vim on o off",
        ),
        ("help", "comandi e tasti"),
    ],
    keys: &[
        ("⇧⇥", "la modalità successiva: plan → ask → build → auto"),
        ("⎋", "fermare Glut"),
        (
            "⏎",
            "mentre Glut lavora: guidarlo, preso in carico prima del passo successivo",
        ),
        (
            "⇥",
            "mentre Glut lavora: mettere il messaggio in coda per dopo",
        ),
        ("⌥↑", "riprendere l’ultimo messaggio in attesa"),
        ("⌃C ⌃C", "uscire"),
        ("⌃F", "cercare nel flusso"),
        ("⌃O", "aprire o chiudere ogni risultato di strumento"),
        ("⌃T", "aprire o chiudere ogni ragionamento"),
        ("⌃B", "il pannello laterale con il piano, o nasconderlo"),
        (
            "⌃G",
            "il gate, dal vivo: ogni giudizio, gli host, i rifiuti",
        ),
        (
            "⌃N",
            "il contatore: contesto, token e costo per turno e strumento",
        ),
        (
            "⌃X A",
            "la scheda Agenti: ogni subagente e gli agenti in background, dal vivo",
        ),
        (
            "⌃X ↑",
            "leggere il flusso blocco per blocco: ⏎ apre o chiude, c copia, ⎋ indietro",
        ),
        (
            "⌃X ⇥",
            "il pannello laterale prende i tasti: ←→ le schede, ↑↓ al loro interno, ⎋ indietro",
        ),
        ("PgUp PgDn", "scorrere il flusso (⇧↑ ⇧↓ una riga)"),
        ("F3", "la conversazione nello scrollback del terminale"),
        ("F2", "dare il mouse al terminale e riprenderlo"),
        (
            "⌃P",
            "il pane successivo, a giro in ordine di lettura (anche ⌃]: ⌃6 su una tastiera tedesca)",
        ),
        ("⌃X 2", "un nuovo pane accanto al principale (anche ⌃X 3)"),
        (
            "⌃X }",
            "il pane principale più largo (anche trascinandone il bordo col mouse)",
        ),
        ("⌃X {", "il pane principale più stretto"),
        ("⌃X 0", "chiudere questo pane"),
        ("⌃X B", "un agente in background, scelto prima il suo ruolo"),
        ("⌃X H", "questo pane in background"),
        (
            "⌃R",
            "rivedere cosa ha cambiato l’ultimo turno, hunk per hunk",
        ),
        (
            "⎋⎋",
            "tornare indietro o biforcare a prima di un tuo messaggio",
        ),
        ("⌃Y", "la scheda Attività: Spazio spunta, a aggiunge"),
        ("/", "i comandi"),
        ("@", "menzionare un file"),
        (
            "⌃V",
            "l’immagine degli appunti nel messaggio (anche un file immagine trascinato)",
        ),
        ("↑", "i messaggi inviati prima"),
        ("⇧⏎", "una nuova riga (anche ⌥⏎)"),
        ("⌃X E", "modificare l’inserimento nel tuo editor"),
        (
            "⌃A ⌃E",
            "l’inizio della riga, la sua fine (anche Home Fine)",
        ),
        (
            "⌥B ⌥F",
            "una parola a sinistra, una parola a destra (anche ⌥← ⌥→)",
        ),
        ("⌃W", "cancellare la parola prima (anche ⌥⌫)"),
        ("⌃U", "cancellare fino all’inizio della riga"),
        ("⌃J", "una nuova riga"),
        ("⌃D", "uscire, su una riga vuota mentre Glut riposa"),
        ("⌃K", "questo foglio dei tasti"),
    ],
    help_commands: "Comandi",
    help_keys: "Tasti ({legend}; ⌃K li tiene nel pannello laterale)",
    help_also: "Inoltre: !cmd esegue un comando e mostra a Glut il suo output · !!cmd lo esegue solo per te · un \
                grande incolla appare come [pasted N lines] · trascina per copiare · un clic sulla tessera di un \
                subagente lo apre",
    help_vim: "/vim on: ⎋ alla modalità Normale (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y con un \
               movimento o doppi · numeri · p P · u ⌃R), i a I A o O per la modalità Inserimento; ⎋ in modalità \
               Normale ferma Glut",
    legend: "⌃ Controllo · ⇧ Maiuscole · ⌥ Opzione · ⏎ Invio · ⇥ Tab · ⎋ Esc",
};
