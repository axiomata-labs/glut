Accedere con un abbonamento invece che con una chiave — oggi il tuo piano ChatGPT Plus o Pro.

/login — gli abbonamenti, ciascuno con il suo stato; ⏎ accede o esce
/login <provider> — accede subito a quello, come /login chatgpt
/login <provider> <address> — via SSH: l'indirizzo su cui si è fermato il browser, incollato di nuovo
/login openrouter — OpenRouter dà a Glut una chiave sua, conservata come una digitata; limitala o cancellala su openrouter.ai/settings/keys

Prima viene mostrato l'indirizzo; ⏎ lo apre nel tuo browser. Glut conserva l'accesso nel portachiavi (o in `~/.glut/credentials.json`) e lo rinnova da solo; uscire lo revoca presso OpenAI.

Con il piano una chiamata non costa denaro, ma valgono i limiti d'uso del piano: quando uno viene raggiunto il turno finisce e Glut rimanda a Impostazioni di ChatGPT → Utilizzo, dove limiti anche ciò che ogni app può usare. Nella shell: glut providers login|logout <provider>.
