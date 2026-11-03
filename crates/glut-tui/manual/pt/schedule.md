Planos que rodam com o Glut fechado: uma tarefa a uma hora do dia ou de tempos em tempos.

/schedule — a aba Execuções (⌃X R): cada plano com a próxima hora dele e o último resultado
/schedule list — os planos no fluxo
/schedule add <name> [flags] <task> — cria um plano, ou muda o que tem esse nome
/schedule run <name> — o executa agora, ao lado do Glut
/schedule pause <name> — o segura até o resume
/schedule resume <name> — ele volta a rodar nas horas dele
/schedule remove <name> — o plano e o registro dele somem; as sessões dele ficam

Os flags são os de `glut schedule add`:
- `--at 07:00` uma hora do dia, com `--days mo-fr` ou `sa,su` (todos os dias sem isso)
- `--every 2h` um intervalo de 10m a 7d, em vez de --at
- `--mode ask|build|plan|dont_ask|auto` o que ele pode fazer sem uma regra; ask se não for nomeado
- `--allow <rule>`, `--deny <rule>` regras na forma do Claude Code, como `Bash(cargo test:*)`; repetíveis
- `--folder <path>` uma pasta ao lado da dele; repetível
- `--max-usd 1`, `--max-minutes 30` o máximo que uma execução pode custar e durar: US$ 1 e 30 min se não for nomeado
- `--max-tokens 1M` numa assinatura, o máximo de tokens que uma execução pode usar: 1M se não for dado
- `--replace` as regras e pastas dadas tomam o lugar das do plano, em vez de se juntarem a elas
- `--cwd <path>`, `--provider <id>`, `--model <id>` a pasta dele (esta, se não), o provedor e o modelo, fixados quando é criado

A tarefa é uma mensagem, ou /skill <name> [para quê]. O launchd inicia cada execução, só enquanto você está com a sessão aberta no Mac; uma hora perdida com o Mac em repouso é recuperada uma vez ao despertar. Ninguém está lá para responder: toda pergunta é recusada e anotada, então permita o que ele precisa. Uma execução é uma sessão guardada própria, avisada por uma notificação do macOS. Na aba Execuções: ⏎ abre um plano ou a sessão de uma execução, r o executa agora, p pausa ou retoma, d o apaga.

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" rode os testes e relate o que falha
