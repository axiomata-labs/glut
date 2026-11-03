Welche Hosts das Modell bei OpenRouter bedienen (andere bedienen jedes Modell von einem Host).

/route — die Reihenfolge, die Glut jetzt schickt, jeder Host mit Grund, Preis und Tempo
/route prefer <hosts> — diese zuerst, in dieser Reihenfolge (xiaomi deepinfra)
/route avoid <hosts> — diese nie
/route quant <levels> — nur diese Quantisierungen (fp8 bf16)
/route strict on|off — nur die bevorzugten Hosts, kein anderer als Ausweich
/route off — die Festlegungen dieses Modells gehen
/route rule on|off — die Regel für jedes Modell
/route rule margin|uptime|speed|wait <n> — ihre Zahlen

OpenRouter bedient ein Modell von vielen Hosts, verschieden quantisiert, verschieden teuer und schnell. Glut liest die Hosts eines Modells einmal am Tag und ordnet sie nach seiner Regel: volle Genauigkeit (bf16, fp16) vor fp8, außer sie kostet mehr als 40 % mehr, ein Host ohne Angabe zwischen fp8 und fp4 — der des Modell-Herstellers als voll —, fp4 nur, wenn sonst niemand das Modell bedient; ein Host unter 95 % Verfügbarkeit, unter 30 Tokens pro Sekunde oder über 10 Sekunden bis zum ersten Token kommt ans Ende. Die Reihenfolge geht mit jeder Anfrage, Ausweichen erlaubt, damit eine Anfrage nie an der Regel scheitert. Die Festlegungen eines Modells gehen vor die Regel. Alles steht in `~/.glut/providers.json`.
