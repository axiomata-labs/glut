Passa o trabalho para uma sessão posterior desta pasta.

/handoff write — o modelo escreve o que foi feito, como as coisas estão e o que vem a seguir
/handoff read — traz o handoff da pasta para esta sessão

Um handoff fica em `~/.glut/handoffs`, um por pasta. Depois de lido ele é posto de lado, para que nenhuma sessão posterior o leia duas vezes. Escrever um é uma chamada paga à parte; a conversa continua como está.
