Executa uma tarefa repetidas vezes num agente em segundo plano próprio, enquanto o Glut está aberto.

/loop <interval> [fresh] [as <role>] [notify] <task> — inicia um loop; a primeira rodada de imediato
/loop — lista os loops: intervalo, rodadas, a próxima rodada e o custo até agora
/loop stop [name] — encerra esse loop, ou o único
/loop rounds [n] — o máximo de rodadas de um loop; sozinho mostra o limite atual
/loop budget [usd] — o máximo que um loop pode custar; sozinho mostra o limite atual
/loop tokens [n] — o máximo de tokens que um loop numa assinatura pode usar (5M, 500k); sozinho mostra o limite atual

As rodadas nunca se sobrepõem: o intervalo é contado a partir do fim de uma rodada, como 30s, 5m, 2h ou 1h30m (de 10s a 24h). Uma rodada continua com a conversa da rodada anterior; com fresh cada rodada começa uma nova. A tarefa é uma mensagem, ou /skill <name> [para quê], /compact ou /init — nenhum outro comando.

O agente se chama loop (ou leva o nome do papel dele) e assume o modo deste pane, o “sempre” que você deu aqui, as pastas ao lado e o papel dele; as <role> só pode nomear o papel que este pane assume. As perguntas dele esperam por você. Um cartão aparece quando uma rodada pergunta ou o loop termina; com notify, depois de cada rodada.

Um loop termina depois das rodadas dele ou quando as rodadas custam o orçamento dele (por padrão 50 rodadas e US$ 5, senão `loop.rounds` e `loop.maxUsd` de `~/.glut/settings.json`; uma mudança vale para os loops iniciados depois dela), quando uma rodada falha ou você a para (⎋ na rodada dele), ou quando o modelo dele chama `end_loop` porque o fim que sua tarefa nomeou foi alcançado. Quando os preços do modelo são desconhecidos, só as rodadas o encerram. O agente dele continua na aba Agentes depois, para ler ou /close. x no cartão dele na aba Agentes (⌃X A) encerra o loop.

/loop 10m rode os testes e conserte o que falhar; pare quando todos passarem
/loop 1h fresh as reviewer notify revise o último commit
