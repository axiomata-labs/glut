Quali host servono il modello su OpenRouter (gli altri servono ogni modello da un solo host).

/route — l'ordine che Glut invia ora, ogni host con il motivo, il prezzo e la velocità
/route prefer <hosts> — questi per primi, in quest'ordine (xiaomi deepinfra)
/route avoid <hosts> — questi mai
/route quant <levels> — solo queste quantizzazioni (fp8 bf16)
/route strict on|off — solo gli host preferiti, nessun altro di riserva
/route off — le scelte fisse di questo modello se ne vanno
/route rule on|off — la regola per ogni modello
/route rule margin|uptime|speed|wait <n> — i suoi numeri

OpenRouter serve un modello da molti host, con quantizzazioni, prezzi e velocità diversi. Glut legge gli host di un modello una volta al giorno e li ordina con la sua regola: precisione piena (bf16, fp16) prima di fp8 salvo costi più del 40 % in più, un host che non indica la quantizzazione tra fp8 e fp4 — quello del produttore del modello come pieno —, fp4 solo se nessun altro lo serve; un host sotto il 95 % di disponibilità, sotto 30 token al secondo o oltre 10 secondi al primo token va in fondo. L'ordine parte con ogni richiesta, riserva permessa, così nessuna richiesta fallisce per la regola. Le scelte fisse di un modello vengono prima della regola. Tutto sta in `~/.glut/providers.json`.
