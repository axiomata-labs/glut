Executa a mesma tarefa em dois a quatro modelos ao mesmo tempo e os compara (a arena).

/arena — os modelos de todos os provedores; a barra de espaço marca de dois a quatro, ⏎ os coloca na entrada para acrescentar a tarefa
/arena <model>, <model>: <task> — o mesmo numa linha (provedor/modelo, ou um nome que sirva a um modelo)
/arena say <text> — um seguimento a cada modelo que respondeu, quando todos acabam (não no banco); tempo, custo e tokens somam-se ao longo das rondas
/arena take <n> — as mudanças do modelo n para a sua pasta, sem commit
/arena card — o cartão de comparação de novo: os números e a verificação de cada modelo, abaixo o diff dele (←/→ outro modelo, s dois lado a lado, c verificar, t levar)
/arena check <command> — a verificação para este repositório (cargo test, npm test …), executada no worktree de cada modelo, no sandbox dele; guardada, roda sozinha depois da próxima arena
/arena tasks — os conjuntos de tarefas da bancada de testes em ~/.glut/arena/tasks; /arena tasks <set> executa um com os modelos da última arena (ou /arena tasks <set>: <model>, <model>; com x3 depois do conjunto, cada tarefa três vezes seguidas (até x5), a tabela mostra então 2/3; com --background os modelos correm como agentes em segundo plano, sem pane, o progresso no separador Agents), tarefa após tarefa, cada uma julgada pela sua própria verificação
/arena results — os resultados de cada conjunto (tarefas aprovadas, Ø tempo e custo) e as vitórias da arena livre
/arena blind on|off — a arena livre às cegas (por omissão): os modelos têm letras numa ordem sorteada, os nomes chegam quando levares um ou terminares a arena
/arena judge — depois da tua escolha, o modelo do pane de onde a arena começou comenta as soluções em duas ou três frases (sem ferramentas; nunca antes, para não influenciar a escolha)
/arena end — fecha os panes da arena e remove os worktrees deles (pergunta antes)

Cada modelo trabalha num pane e num worktree próprios, que partem da sua pasta como ela está, com arquivos sem commit e novos incluídos (sem arquivo .env, sem link), em auto: só comandos pesados perguntam. Um modelo para depois de 30 minutos ou 2 $. Os modelos na nuvem rodam ao mesmo tempo, os locais (Ollama, LM Studio) um depois do outro, quando os demais terminam. Quando todos acabam, os números deles aparecem no pane de onde a arena foi iniciada, e /arena card abre o cartão de comparação: tempo, custo, tokens, chamadas e o que cada um mudou. /arena take confere se as mudanças ainda cabem na sua pasta; se não couberem, nada é escrito.

/arena sonnet, gpt-5: make the failing test pass
