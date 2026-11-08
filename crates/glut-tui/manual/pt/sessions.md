Escrever às outras janelas do Glut desta máquina.

/sessions — os seus panes e agentes: endereço, projeto, modelo, modo, a trabalhar ou à espera; ⏎ põe /send <address> na entrada
/sessions off — esta janela não aceita as mensagens delas
/sessions on — e de novo

Uma mensagem do dono de lá entra logo; a de um modelo de lá só se trabalhar no modo do pane a que chega, senão espera acima da entrada: ⏎ deixa-a entrar, Esc descarta-a. Nenhuma é a sua palavra ao modelo daqui. Só o Glut fala com as outras janelas — um comando do modelo nunca as alcança.

Os modelos também se veem: a ferramenta `who` lista cada agente aqui e nas outras janelas — papel, modelo, pasta, branch, ocupado ou livre — e a linha que cada um definiu sobre o que faz, com as suas próprias palavras.

Um modelo pode pedir algo a outro, aqui ou noutra janela: o turno que o pedido inicia lá responde-lhe — a última resposta desse turno volta como resposta, enquanto ele continua com outro trabalho. Um agente ocupado aceita-o assim que o seu turno termina; um que não consegue diz isso. Se ninguém vai responder — puseste o pedido de lado com Esc, o pane foi fechado, a janela terminou — o Glut diz isso a quem pediu.

Você vê tudo na aba Equipe (⌃X T): cada agente aqui e nas outras janelas — ocupado, livre ou esperando por você, seu branch e sua linha — e o que passou entre eles: mensagens, pedidos, respostas, e quem chegou e saiu.

Onde dois agentes se atrapalham, o Glut avisa: um agente trabalhando na mesma árvore de trabalho e no mesmo branch que outro que trabalha ali, ou um que altera um arquivo que outro alterou no último quarto de hora, ouve isso uma vez antes do próximo passo — e você vê o aviso no fluxo dele e na aba Equipe.
