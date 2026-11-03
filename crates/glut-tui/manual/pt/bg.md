Inicia um agente em segundo plano: uma sessão sem pane, alcançada pelo nome.

/bg — um agente chamado agent (ou agent-2, …); num repositório git o Glut pergunta onde ele trabalha
/bg <name> — um agente com esse nome (letras minúsculas, dígitos e hifens)
/bg as <role> — ele assume um papel e recebe o nome dele
/bg here — ele trabalha nesta pasta
/bg tree [branch] — ele trabalha num Worktree novo

/send <name> lhe dá trabalho, /peek <name> lê o fluxo dele, /show <name> o traz para a frente, /close <name> o encerra. Cada agente trabalha e custa por conta própria: o Glut pergunta uma vez por janela antes de um sétimo. ⌃X B inicia um com o papel escolhido, ⌃X A os lista.

/bg docs-check as docs
