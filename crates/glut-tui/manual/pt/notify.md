Avisa quando um turno termina ou o Glut pergunta, enquanto a janela não está na frente.

/notify on — o fim de um turno e cada pergunta
/notify asks — só as perguntas
/notify off — nada
/notify system — pelas notificações próprias do macOS (um clique numa delas abre o Editor de Script, não o terminal)
/notify after <s> — o fim de um turno só é avisado depois de ele ter rodado tantos segundos (10 por padrão, até 3600)
/notify tiles on|off — os cartões no canto superior direito da janela

/notify sozinho mostra o que vale agora. O Glut usa as notificações do próprio terminal onde sabe que ele as tem, e o sino. Uma notificação nomeia o Glut, o projeto, o pane e o que aconteceu, nunca as palavras do modelo. Guardado em `~/.glut/prefs.json`, acima de `notify` em `~/.glut/settings.json`.
