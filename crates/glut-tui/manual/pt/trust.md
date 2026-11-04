Se as próprias definições .glut desta pasta contam por inteiro.

/trust — se esta pasta é de confiança, e o que as suas próprias definições acrescentam então
/trust on — confia nela: contam as suas regras de permissão, o seu modo, skills, agentes e comandos
/trust off — deixa de confiar nela: só contam as suas regras de proibição e de pergunta
/trust list — cada pasta de confiança

O .glut/settings.json de um projeto pode guardar mais segredos e perguntar ou proibir mais — isso conta em cada pasta. O que permitiria mais a um modelo, as suas regras de permissão e o seu modo, só conta onde confiaste na pasta, tal como os skills, agentes e comandos do seu .glut. Lê o que /trust mostra antes de dizer on: o .glut de um repositório clonado é a palavra de outra pessoa. O seu modo nunca vai além do teu, e os seus hooks nunca correm, de confiança ou não.

A mudança conta a partir da próxima sessão da pasta (glut -c retoma a conversa). Nunca a pasta pessoal, a raiz ou uma pasta que não seja só tua (de outro utilizador, ou uma que qualquer um pode escrever, como /tmp). A lista fica em ~/.glut/trusted.json, que nenhum comando executado pelo Glut escreve; glut trust show|add|remove|list faz o mesmo fora do Glut.
