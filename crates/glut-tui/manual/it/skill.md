Avvia una Skill: istruzioni per un tipo di compito, che il modello carica e segue.

/skill — le Skill tra cui scegliere
/skill <name> [a che scopo] — esegue quella Skill su ciò che segue il suo nome

Glut porta make-agent, make-skill e make-hook per scrivere le tue. Le tue stanno in `~/.glut/skills`, quelle di un progetto in `.glut/skills` (lette solo in una cartella fidata); una con lo stesso nome nasconde quella di Glut.

/skill make-hook blocca git push su main
