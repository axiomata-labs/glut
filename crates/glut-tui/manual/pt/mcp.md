Mostra os servidores MCP e como cada um está, faz login num hospedado, adiciona ou remove um.

/mcp — cada servidor: conectado com as ferramentas dele, esperando um login ou com falha
/mcp login <name> — faz login num servidor hospedado; o endereço dele abre no seu navegador com ⏎
/mcp login <name> <address> — por SSH: o endereço onde o navegador terminou, colado de volta
/mcp logout <name> — sai e esquece o token dele
/mcp add <name> <url> — adiciona um servidor hospedado a `~/.glut/mcp.json` e o conecta
/mcp add <name> -- <command> [args] — adiciona um servidor que o próprio Glut inicia
/mcp remove <name> — o tira de `~/.glut/mcp.json` e da sessão

Um servidor que não oferece um login que o Glut possa usar pede um token, digitado oculto. Os servidores próprios de um projeto são do projeto para mudar: /mcp remove tira só os seus. `glut mcp` faz o mesmo fora de uma sessão.

Enquanto uma das suas ferramentas corre, um servidor pode perguntar-lhe: um formulário (⇥ ↑↓ entre os campos, espaço marca, ⏎ envia, Esc fecha, ⌃D recusa) ou uma página a abrir (o). Só você responde, nunca o modelo; um formulário que pede uma palavra-passe ou uma chave é recusado e, sem ninguém presente, toda a pergunta é recusada.
