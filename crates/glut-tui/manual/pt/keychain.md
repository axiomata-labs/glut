Manter o porta-chaves fora do alcance dos comandos que o Glut executa.

/keychain — se está aberto, e os programas que o podem pedir
/keychain add <program> — põe um programa na lista (gh, ou git e um subcomando: git push)
/keychain remove <program> — tira-o da lista
/keychain open — abre-o a todo comando, até esta sessão terminar
/keychain close — volta a fechá-lo

Uma chave ou um início de sessão no porta-chaves alcança-se para lá de toda regra de ficheiros da sandbox, por isso um comando que o Glut executa encontra o porta-chaves fechado. Um comando de um programa da lista pergunta primeiro — em todo modo, também em `auto` — e corre com o porta-chaves aberto assim que o permites; um «sempre» torna-o uma regra tua. Sem ninguém presente (um plano, `glut run`) só uma regra assim o abre. Quando um comando falha no porta-chaves fechado, o Glut di-lo sob a sua saída.

Permitir um comando dá o porta-chaves também a tudo o que ele inicia: um hook do git, uma extensão do gh ou um script de compilação pode então ler qualquer chave nele.

Só as suas próprias regras dão o porta-chaves: uma regra de permissão do `.glut/` de um projeto deixa um comando correr, nunca com o porta-chaves aberto. Uma cadeia só o recebe quando cada parte está na lista ou só lê. O que `glut keychain open` mantém aberto só conta aqui, onde você está — nunca em `glut run` nem na execução de um anfitrião.

Uma regra sua só o abre sem perguntar quando nomeia um programa e o seu subcomando (`gh pr *`, `git push *`) — nunca um programa inteiro (`gh *`). Um cartão 🔑 mostra o comando inteiro, ↑↓ para o percorrer, e só permite depois de o ler até ao fim.

A lista fica em `~/.glut/keychain.json`; `glut keychain open` mantém o porta-chaves aberto de uma sessão para outra, `glut keychain close` fecha-o de vez.
