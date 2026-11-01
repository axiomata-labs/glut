//! The commands' manual pages in Portuguese (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 66] = [
    (
        "clear",
        "Começa uma sessão nova e vazia nesta pasta.\n\
         \n\
         /clear — a conversa, a lista de tarefas do plano, o custo e o medidor recomeçam do zero\n\
         \n\
         A sessão que você deixa é guardada: /resume a abre de novo. Seu modelo, seu modo e seus ajustes continuam \
         como estão.",
    ),
    (
        "resume",
        "Abre uma sessão anterior desta pasta e continua nela.\n\
         \n\
         /resume — a lista das sessões guardadas desta pasta, a mais recente primeiro; uma ramificação fica sob a \
         sessão de onde veio\n\
         \n\
         Na lista: digite para filtrar, ⏎ abre uma, ⌫ apaga uma (com confirmação; as ramificações dela continuam como \
         sessões próprias), ⌃E escreve uma como Markdown em `~/.glut/exports`. Uma sessão aberta em outro pane se \
         alcança lá (⌃P).\n\
         \n\
         /search encontra uma sessão pelo que foi dito nela.",
    ),
    (
        "search",
        "Encontra uma sessão guardada pelas palavras dela: a lista do /resume, filtrada pelo texto da conversa.\n\
         \n\
         /search <text> — as sessões desta pasta cuja conversa contém o texto\n\
         \n\
         Na lista, ⌃A alterna entre esta pasta e todos os projetos. ⏎ abre uma sessão desta pasta; a de outro projeto \
         mostra a pasta dele — inicie o Glut lá para continuar nela.",
    ),
    (
        "btw",
        "Faz uma pergunta à parte sem perturbar a conversa: a resposta vem numa janela acima da entrada e nunca \
         passa a fazer parte da sessão.\n\
         \n\
         /btw <question> — pergunta sobre a sessão deste pane, mesmo enquanto o turno dele continua\n\
         /btw <name> <question> — pergunta sobre outro pane ou sobre um agente em segundo plano, pelo número ou pelo \
         nome\n\
         /btw — reabre a última resposta\n\
         \n\
         O modelo responde a partir da conversa como ela está e não executa nenhuma ferramenta: se ele precisasse \
         consultar algo, faça a pergunta como mensagem. O custo dela entra no custo da sessão. ⎋ fecha a janela, PgUp \
         PgDn a rolam.\n\
         \n\
         /btw reviewer como vai?",
    ),
    (
        "projects",
        "Continua em outro projeto: uma das pastas em que o Glut trabalhou, ou qualquer pasta pelo caminho.\n\
         \n\
         /projects — a lista: ★ fixado, ● este; ⏎ abre um, p fixa ou solta, x oculta um até que se trabalhe nele de \
         novo\n\
         /projects <path> — abre essa pasta (⇥ completa o nome de uma pasta)\n\
         \n\
         Enquanto um turno ou um agente ainda trabalha, o Glut pergunta antes de parar os dois e trocar. As sessões e \
         os panes desta janela são guardados. Numa janela aberta por um aplicativo hospedeiro, a pasta é ele quem \
         escolhe.",
    ),
    (
        "folder",
        "Dá a esta sessão pastas ao lado da de trabalho, para que o Glut trabalhe entre projetos.\n\
         \n\
         /folder — as pastas ao lado da de trabalho\n\
         /folder add <path> — adiciona uma para esta sessão\n\
         /folder remove <path> — tira uma\n\
         \n\
         As ferramentas de arquivos alcançam essa pasta pelo caminho completo, os comandos podem escrever nela dentro \
         do sandbox, o Gate a julga como a pasta de trabalho e o undo devolve os arquivos dela também. Só você \
         adiciona uma pasta, nunca um modelo, uma Skill ou um projeto. O Glut recusa a pasta pessoal, uma pasta que \
         contém o projeto ou fica dentro dele, uma com segredos e duas pastas uma dentro da outra.\n\
         \n\
         As pastas valem só para esta sessão; `glut run --folder <path>` dá uma a uma execução sem interface.",
    ),
    (
        "compact",
        "Resume a conversa até aqui, para que ocupe menos da janela do modelo.\n\
         \n\
         /compact — o modelo escreve um resumo que toma o lugar das mensagens mais antigas\n\
         \n\
         O prompt do sistema, sua última mensagem e os passos inteiros ficam; o que o resumo substitui é guardado no \
         armazenamento de resultados da sessão, nunca jogado fora. O Glut também compacta sozinho pouco antes de a \
         janela encher (`compact` em `~/.glut/settings.json`). /context mostra o quanto a janela está cheia.",
    ),
    (
        "handoff",
        "Passa o trabalho para uma sessão posterior desta pasta.\n\
         \n\
         /handoff write — o modelo escreve o que foi feito, como as coisas estão e o que vem a seguir\n\
         /handoff read — traz o handoff da pasta para esta sessão\n\
         \n\
         Um handoff fica em `~/.glut/handoffs`, um por pasta. Depois de lido ele é posto de lado, para que nenhuma \
         sessão posterior o leia duas vezes. Escrever um é uma chamada paga à parte; a conversa continua como está.",
    ),
    (
        "export",
        "Escreve a sessão para ler ou compartilhar.\n\
         \n\
         /export — Markdown em `~/.glut/exports`\n\
         /export html — um arquivo HTML, com os estilos dentro, sem script\n\
         /export jsonl — as mensagens como linhas JSON\n\
         /export copy — o Markdown na área de transferência, em vez de um arquivo\n\
         /export full — inclui os resultados inteiros das ferramentas e o prompt do sistema\n\
         /export <format> <path> — para um arquivo seu (`~/` é a sua pasta pessoal, um caminho relativo fica na pasta \
         de trabalho)\n\
         \n\
         As palavras vão em qualquer ordem, o caminho por último. Sem full, a exportação mostra o que o modelo viu: \
         resultados longos encurtados. Um arquivo existente nunca é sobrescrito. As chaves que o Glut ocultou \
         continuam ocultas, mas o que você mesmo digitou ainda pode conter uma: leia uma exportação antes de \
         compartilhá-la. No /resume, ⌃E exporta uma sessão da lista.",
    ),
    (
        "quit",
        "Encerra o Glut, com todos os panes e agentes em segundo plano.\n\
         \n\
         /quit — enquanto outro pane ou agente ainda trabalha, o Glut avisa primeiro: /quit de novo encerra tudo mesmo \
         assim\n\
         \n\
         ⌃C ⌃C faz o mesmo, e ⌃D numa linha vazia enquanto o Glut descansa. As sessões são guardadas para o /resume.",
    ),
    ("exit", "Encerra o Glut, como /quit faz."),
    (
        "model",
        "Escolhe o modelo com que esta sessão conversa.\n\
         \n\
         /model — os modelos do provedor em uso, da lista guardada dele\n\
         /model <id> — troca para esse modelo pelo id dele\n\
         \n\
         A conversa continua com o modelo novo, e a próxima inicialização começa com ele. Um modelo de outro provedor \
         se escolhe depois de /provider. `glut models refresh` renova as listas.",
    ),
    (
        "provider",
        "Troca para outro provedor de modelos.\n\
         \n\
         /provider — os provedores que você configurou\n\
         /provider <id> — troca para esse, no modelo padrão dele\n\
         /provider key [id] — muda só a chave desse provedor (sem id, a do provedor padrão)\n\
         \n\
         Os provedores ficam em `~/.glut/providers.json`, as chaves deles no Keychain (ou em \
         `~/.glut/credentials.json`); `glut providers add <id>` adiciona um. A conversa continua, e a próxima \
         inicialização começa com o provedor que você escolheu.\n\
         \n\
         A chave é digitada oculta e testada na lista de modelos do provedor antes de substituir a antiga; se falhar, \
         você escolhe se a mantém mesmo assim, senão a antiga fica. A sessão usa na hora uma chave guardada. Um \
         provedor que entra com uma conta, não precisa de chave ou a lê de uma variável de ambiente avisa isso. No \
         shell: `glut providers key <id>`.",
    ),
    (
        "login",
        "Entrar com uma assinatura em vez de uma chave — hoje o seu plano ChatGPT Plus ou Pro.\n\
         \n\
         /login — as assinaturas, cada uma com seu estado; ⏎ entra ou sai\n\
         /login <provider> — entra nela na hora, como /login chatgpt\n\
         /login <provider> <address> — via SSH: o endereço em que o navegador terminou, colado de volta\n\
         /login openrouter — o OpenRouter dá ao Glut uma chave própria, guardada como uma digitada; limite-a ou \
         apague-a em openrouter.ai/settings/keys\n\
         \n\
         O endereço é mostrado primeiro; ⏎ o abre no seu navegador. O Glut guarda o login no Keychain (ou em \
         `~/.glut/credentials.json`) e o renova sozinho; sair o revoga na OpenAI.\n\
         \n\
         No plano uma chamada não custa dinheiro, mas valem os limites de uso do plano: quando um é atingido, o turno \
         termina e o Glut aponta para Configurações do ChatGPT → Uso, onde você também limita o que cada app pode \
         usar. No shell: glut providers login|logout <provider>.",
    ),
    (
        "effort",
        "Define o quanto o modelo pensa antes de responder.\n\
         \n\
         /effort — os níveis para escolher\n\
         /effort <level> — none, minimal, low, medium, high, xhigh ou max\n\
         /effort default — o provedor decide\n\
         \n\
         Mais esforço costuma significar respostas melhores em trabalho difícil, porém mais lentas e a um custo maior. \
         Nem todo modelo conhece todos os níveis. O medidor (⌃N) mostra o nível em uso.",
    ),
    (
        "route",
        "Que hosts servem o modelo no OpenRouter (os outros servem cada modelo a partir de um só host).\n\
         \n\
         /route — a ordem que o Glut envia agora, cada host com o motivo, o preço e a velocidade\n\
         /route prefer <hosts> — estes primeiro, por esta ordem (xiaomi deepinfra)\n\
         /route avoid <hosts> — estes nunca\n\
         /route quant <levels> — só estas quantizações (fp8 bf16)\n\
         /route strict on|off — só os hosts preferidos, nenhum outro como recurso\n\
         /route off — as fixações deste modelo vão-se\n\
         /route rule on|off — a regra para todos os modelos\n\
         /route rule margin|uptime|speed|wait <n> — os seus números\n\
         \n\
         O OpenRouter serve um modelo a partir de muitos hosts, com quantizações, preços e velocidades diferentes. O \
         Glut lê os hosts de um modelo uma vez por dia e ordena-os pela sua regra: precisão total (bf16, fp16) antes \
         de fp8 salvo se custar mais de 40 % a mais, um host que não indica a quantização entre fp8 e fp4 — o do \
         fabricante do modelo como total —, fp4 só se mais ninguém o servir; um host abaixo de 95 % de \
         disponibilidade, abaixo de 30 tokens por segundo ou acima de 10 segundos até ao primeiro token vai para o \
         fim. A ordem vai com cada pedido, com recurso permitido, para que nenhum pedido falhe por causa da regra. As \
         fixações de um modelo vêm antes da regra. Tudo fica em `~/.glut/providers.json`.",
    ),
    (
        "context",
        "Mostra o quanto a janela do modelo está cheia, ou diz ao Glut o tamanho dela.\n\
         \n\
         /context — os tokens usados, a janela e de onde vem o tamanho dela, e o que os guias (`AGENTS.md`, \
         `CLAUDE.md`) custam a cada turno\n\
         /context <tokens> — define a janela deste modelo: 65536, 64k ou 1M\n\
         \n\
         Uma janela que você define é guardada em `~/.glut/providers.json` para este modelo e vale antes da lista do \
         provedor. O Glut compacta sozinho pouco antes de a janela encher.",
    ),
    (
        "thinking",
        "Escolhe como o raciocínio do modelo aparece no fluxo.\n\
         \n\
         /thinking off — oculto\n\
         /thinking folded — uma linha por pensamento; um clique abre um\n\
         /thinking open — cada pensamento mostrado por inteiro\n\
         \n\
         /thinking sozinho os recolhe. ⌃T abre ou recolhe todos os pensamentos de uma vez. Sua escolha é guardada em \
         `~/.glut/prefs.json`.",
    ),
    (
        "panel",
        "Mostra o painel lateral na aba Plano, ou o esconde.\n\
         \n\
         /panel — o painel lateral com as abas: Plano, Gate, Medidor, Agentes, Pendentes, Execuções e Teclas\n\
         \n\
         ⌃B faz o mesmo, ⌃X ⇥ entrega as teclas ao painel (←→ as abas, ↑↓ dentro delas, ⎋ volta). O painel precisa de \
         uma janela com pelo menos 100 colunas de largura. Se ele aparece ou não fica guardado para a próxima \
         inicialização.",
    ),
    (
        "meter",
        "Mostra o que a sessão usa: o contexto, os tokens e o custo.\n\
         \n\
         /meter — a aba Medidor do painel lateral\n\
         \n\
         Ela mostra o quanto a janela está cheia e quando o Glut compacta, os tokens da sessão de entrada, em cache e \
         de saída com o preço deles, cada turno e cada ferramenta, e o que a saída enxuta (lean) do Glut poupou. ⌃N \
         também a abre.",
    ),
    (
        "agents",
        "Mostra os subagentes que o Glut iniciou nesta sessão.\n\
         \n\
         /agents — a lista de subagentes, cada um sob o que o iniciou; ⏎ abre o fluxo próprio de um\n\
         \n\
         Um clique no cartão de um subagente também o abre. ⌃X A abre a aba Agentes: cada subagente e cada agente em \
         segundo plano, ao vivo — ⏎ abre um, x o para.",
    ),
    (
        "plan",
        "Lê por inteiro o plano que o Glut enviou do modo plan, com a lista de tarefas dele.\n\
         \n\
         /plan — o último plano desta sessão\n\
         \n\
         No modo plan (⇧⇥) o Glut lê e planeja sem mudar nada, e então envia o plano para você aprovar ou devolver. \
         /send plan o passa a outro pane para construir.",
    ),
    (
        "scrollback",
        "Põe a conversa no scrollback do próprio terminal, para rolar, buscar e copiar com as ferramentas do \
         terminal.\n\
         \n\
         /scrollback — imprime a conversa; ⏎ ou ⌃C volta ao Glut\n\
         \n\
         F3 faz o mesmo.",
    ),
    (
        "split",
        "Abre uma sessão nova num pane ao lado do principal.\n\
         \n\
         /split — um pane novo; num repositório git o Glut pergunta onde ele trabalha\n\
         /split here — ele trabalha nesta pasta, ao seu lado\n\
         /split tree [branch] — ele trabalha num Worktree novo, numa branch própria\n\
         /split resume — ele abre uma sessão anterior, escolhida da lista\n\
         /split as <role> — ele assume um papel (/role os lista)\n\
         \n\
         Até seis panes ficam ao lado do principal, em duas fileiras de três. ⌃P percorre os panes, ⌃X 2 também \
         divide, ⌃X } e ⌃X { deixam o pane principal mais largo ou mais estreito. Dois panes aqui mudam os mesmos \
         arquivos; um Worktree os mantém separados até que /close pergunte como ele termina.\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "swap",
        "Faz o pane principal e outro trocarem de lugar.\n\
         \n\
         /swap — este pane passa a ser o principal\n\
         /swap <n> — o pane n passa a ser o principal",
    ),
    (
        "close",
        "Fecha um pane, ou encerra um agente em segundo plano.\n\
         \n\
         /close — fecha este pane (nunca o principal); enquanto ele trabalha, um segundo /close o para e o fecha\n\
         /close <name> — encerra esse agente em segundo plano, com o turno dele parado\n\
         /close <n> — fecha o pane n de onde está, pelo número, nome ou papel; fica no seu pane\n\
         /close sozinho, se houver mais do que este pane para fechar — uma lista, este pane primeiro: ⏎ fecha o \
         escolhido\n\
         \n\
         A sessão é guardada para o /resume. Um pane num Worktree pergunta como o Worktree termina: guardá-lo com a \
         branch dele, incorporá-lo (fazer o merge dele na branch em que você está) ou descartá-lo. ⌃X 0 também fecha \
         este pane.",
    ),
    (
        "send",
        "Envia uma mensagem a outro pane ou agente em segundo plano, recebida lá como se você a tivesse digitado.\n\
         \n\
         /send — uma lista de a quem escrever: os panes e agentes daqui, depois as outras janelas do Glut; ⏎ põe \
         /send <who> na entrada\n\
         /send <text> — para o único outro pane\n\
         /send <n|name> <text> — para o pane n ou o agente desse nome (necessário com três ou mais)\n\
         /send plan — passa o plano deste pane ao outro pane para construir\n\
         /send <n|name> plan — o passa àquele\n\
         \n\
         A mensagem começa de imediato onde a sessão descansa, senão depois do turno dela. As teclas ficam neste pane.",
    ),
    (
        "role",
        "Faz este pane assumir um papel: o prompt e as ferramentas de um agente nomeado, nunca o modelo dele.\n\
         \n\
         /role — os papéis para escolher\n\
         /role <name> — assume esse papel daqui em diante\n\
         /role none — volta a ser um faz-tudo, com todas as ferramentas\n\
         \n\
         O Glut traz reviewer, tester e docs; seus próprios papéis ficam em `~/.glut/agents`, os de um projeto em \
         `.glut/agents` dele (lidos só numa pasta confiável). As ferramentas de um papel valem de imediato, o prompt \
         dele a partir do próximo turno. Um papel que restringe as ferramentas precisa do Gate.",
    ),
    (
        "bg",
        "Inicia um agente em segundo plano: uma sessão sem pane, alcançada pelo nome.\n\
         \n\
         /bg — um agente chamado agent (ou agent-2, …); num repositório git o Glut pergunta onde ele trabalha\n\
         /bg <name> — um agente com esse nome (letras minúsculas, dígitos e hifens)\n\
         /bg as <role> — ele assume um papel e recebe o nome dele\n\
         /bg here — ele trabalha nesta pasta\n\
         /bg tree [branch] — ele trabalha num Worktree novo\n\
         \n\
         /send <name> lhe dá trabalho, /peek <name> lê o fluxo dele, /show <name> o traz para a frente, /close <name> \
         o encerra. Cada agente trabalha e custa por conta própria: o Glut pergunta uma vez por janela antes de um \
         sétimo. ⌃X B inicia um com o papel escolhido, ⌃X A os lista.\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "Traz um agente em segundo plano para a frente como pane.\n\
         \n\
         /show <name> — o agente passa a ser o último pane da grade\n\
         \n\
         Quando a janela não tem mais espaço, use /hide ou /close num pane antes. ⌃X A lista os agentes em segundo \
         plano.",
    ),
    (
        "hide",
        "Manda este pane para o segundo plano, com a memória dele guardada.\n\
         \n\
         /hide — o pane sai da tela e continua trabalhando como agente em segundo plano\n\
         \n\
         Ele guarda um nome pelo qual se alcança (o papel dele, ou pane-<n>): /show <name> o traz de volta, /send \
         <name> e /peek <name> o alcançam. O pane principal e o último ficam. ⌃X H faz o mesmo.",
    ),
    (
        "peek",
        "Lê o fluxo de um agente em segundo plano sobre este pane, sem trazê-lo para a frente.\n\
         \n\
         /peek <name> — o fluxo dele, ao vivo, só para ler\n\
         /peek — o fecha de novo\n\
         \n\
         ⎋ também o fecha; /show <name> traz o agente para a frente.",
    ),
    (
        "loop",
        "Executa uma tarefa repetidas vezes num agente em segundo plano próprio, enquanto o Glut está aberto.\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — inicia um loop; a primeira rodada de imediato\n\
         /loop — lista os loops: intervalo, rodadas, a próxima rodada e o custo até agora\n\
         /loop stop [name] — encerra esse loop, ou o único\n\
         /loop rounds [n] — o máximo de rodadas de um loop; sozinho mostra o limite atual\n\
         /loop budget [usd] — o máximo que um loop pode custar; sozinho mostra o limite atual\n\
         /loop tokens [n] — o máximo de tokens que um loop numa assinatura pode usar (5M, 500k); sozinho mostra o \
         limite atual\n\
         \n\
         As rodadas nunca se sobrepõem: o intervalo é contado a partir do fim de uma rodada, como 30s, 5m, 2h ou 1h30m \
         (de 10s a 24h). Uma rodada continua com a conversa da rodada anterior; com fresh cada rodada começa uma nova. \
         A tarefa é uma mensagem, ou /skill <name> [para quê], /compact ou /init — nenhum outro comando.\n\
         \n\
         O agente se chama loop (ou leva o nome do papel dele) e assume o modo deste pane, o “sempre” que você deu \
         aqui, as pastas ao lado e o papel dele; as <role> só pode nomear o papel que este pane assume. As perguntas \
         dele esperam por você. Um cartão aparece quando uma rodada pergunta ou o loop termina; com notify, depois de \
         cada rodada.\n\
         \n\
         Um loop termina depois das rodadas dele ou quando as rodadas custam o orçamento dele (por padrão 50 rodadas e \
         US$ 5, senão `loop.rounds` e `loop.maxUsd` de `~/.glut/settings.json`; uma mudança vale para os loops \
         iniciados depois dela), quando uma rodada falha ou você a para (⎋ na rodada dele), ou quando o modelo dele \
         chama `end_loop` porque o fim que sua tarefa nomeou foi alcançado. Quando os preços do modelo são \
         desconhecidos, só as rodadas o encerram. O agente dele continua na aba Agentes depois, para ler ou /close. x \
         no cartão dele na aba Agentes (⌃X A) encerra o loop.\n\
         \n\
         /loop 10m rode os testes e conserte o que falhar; pare quando todos passarem\n\
         /loop 1h fresh as reviewer notify revise o último commit",
    ),
    (
        "schedule",
        "Planos que rodam com o Glut fechado: uma tarefa a uma hora do dia ou de tempos em tempos.\n\
         \n\
         /schedule — a aba Execuções (⌃X R): cada plano com a próxima hora dele e o último resultado\n\
         /schedule list — os planos no fluxo\n\
         /schedule add <name> [flags] <task> — cria um plano, ou muda o que tem esse nome\n\
         /schedule run <name> — o executa agora, ao lado do Glut\n\
         /schedule pause <name> — o segura até o resume\n\
         /schedule resume <name> — ele volta a rodar nas horas dele\n\
         /schedule remove <name> — o plano e o registro dele somem; as sessões dele ficam\n\
         \n\
         Os flags são os de `glut schedule add`:\n\
         - `--at 07:00` uma hora do dia, com `--days mo-fr` ou `sa,su` (todos os dias sem isso)\n\
         - `--every 2h` um intervalo de 10m a 7d, em vez de --at\n\
         - `--mode ask|build|plan|dont_ask|auto` o que ele pode fazer sem uma regra; ask se não for nomeado\n\
         - `--allow <rule>`, `--deny <rule>` regras na forma do Claude Code, como `Bash(cargo test:*)`; repetíveis\n\
         - `--folder <path>` uma pasta ao lado da dele; repetível\n\
         - `--max-usd 1`, `--max-minutes 30` o máximo que uma execução pode custar e durar: US$ 1 e 30 min se não for \
         nomeado\n\
         - `--max-tokens 1M` numa assinatura, o máximo de tokens que uma execução pode usar: 1M se não for dado\n\
         - `--replace` as regras e pastas dadas tomam o lugar das do plano, em vez de se juntarem a elas\n\
         - `--cwd <path>`, `--provider <id>`, `--model <id>` a pasta dele (esta, se não), o provedor e o modelo, \
         fixados quando é criado\n\
         \n\
         A tarefa é uma mensagem, ou /skill <name> [para quê]. O launchd inicia cada execução, só enquanto você está \
         com a sessão aberta no Mac; uma hora perdida com o Mac em repouso é recuperada uma vez ao despertar. Ninguém \
         está lá para responder: toda pergunta é recusada e anotada, então permita o que ele precisa. Uma execução é \
         uma sessão guardada própria, avisada por uma notificação do macOS. Na aba Execuções: ⏎ abre um plano ou a \
         sessão de uma execução, r o executa agora, p pausa ou retoma, d o apaga.\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" rode os testes e relate o que \
         falha",
    ),
    (
        "sessions",
        "Escrever às outras janelas do Glut desta máquina.\n\n/sessions — os seus panes e agentes: endereço, projeto, \
        modelo, modo, a trabalhar ou à espera; ⏎ põe /send <address> na entrada\n/sessions off — esta janela não \
        aceita as mensagens delas\n/sessions on — e de novo\n\nUma mensagem do dono de lá entra logo; a de um modelo \
        de lá só se trabalhar no modo do pane a que chega, senão espera acima da entrada: ⏎ deixa-a entrar, Esc \
        descarta-a. Nenhuma é a sua palavra ao modelo daqui. Só o Glut fala com as outras janelas — um comando do \
        modelo nunca as alcança.",
    ),
    (
        "name",
        "Dar nome a esta janela entre as janelas do Glut desta máquina.\n\n/name — o nome que tem agora\n/name <new> \
        — um novo: letras, dígitos, - _ e .\n\nAs outras janelas chegam a esta pelo nome, a um pane por name/2 ou \
        name/reviewer. Um nome que dá fica com a sessão e volta com /resume.",
    ),
    (
        "diff",
        "Revisa o que o último turno mudou, hunk por hunk.\n\
         \n\
         /diff — as mudanças do último turno que mudou arquivos\n\
         /diff session — todas as mudanças desta sessão\n\
         \n\
         Na revisão: ↑↓ um hunk, ←→ um arquivo, a aceita, r rejeita (o hunk volta, e o Glut lê o porquê com a sua \
         próxima mensagem), c comenta (enviado com a sua próxima mensagem), v marca um arquivo como revisado, s \
         alterna entre turno e sessão, ⎋ fecha. ⌃R também a abre; /review é o mesmo comando.",
    ),
    (
        "steps",
        "Mostra os passos que mudaram arquivos e devolve os arquivos.\n\
         \n\
         /steps — cada passo que mudou um arquivo; ⏎ num deles mostra o que mudou, ou devolve os arquivos ao que eram \
         antes dele\n\
         \n\
         Devolver os arquivos leva junto as suas edições desde então; a conversa fica. O Glut guarda um snapshot antes \
         de cada passo que muda algo; o que os arquivos eram logo antes também é guardado, então `glut undo` o traz de \
         volta. ⎋⎋ volta a conversa em vez disso.",
    ),
    (
        "todo",
        "Guarda as pendências do projeto no `TODO.md` dele, uma lista de verificação que você e o Glut usam.\n\
         \n\
         /todo — a aba Pendentes: Espaço marca um item, a adiciona um, o abre o editor\n\
         /todo <text> — adiciona um item ao fim da lista, também enquanto o Glut trabalha\n\
         /todo open — a lista no seu editor\n\
         /todo clean — tira os itens concluídos, com confirmação\n\
         \n\
         O resto do arquivo nunca é tocado. ⌃Y mostra ou esconde a aba.",
    ),
    (
        "memory",
        "Mostra o que a memória do projeto guarda: os fatos que o Glut anotou para as sessões seguintes.\n\
         \n\
         /memory — os fatos com as descrições deles; ⏎ mostra um por inteiro\n\
         /memory open — o índice (`MEMORY.md`) no seu editor\n\
         \n\
         A memória fica fora do repositório: em `~/.glut/memory/<project>/`, ou na memória do Claude Code onde o \
         Claude Code trabalha no repositório. O Glut a escreve com a ferramenta `memory` dele; lê-la aqui não consulta \
         nenhum modelo.",
    ),
    (
        "skill",
        "Inicia uma Skill: instruções para um tipo de tarefa, que o modelo carrega e segue.\n\
         \n\
         /skill — as Skills para escolher\n\
         /skill <name> [para quê] — executa essa Skill sobre o que vem depois do nome dela\n\
         \n\
         O Glut traz make-agent, make-skill e make-hook para você escrever as suas. As suas ficam em `~/.glut/skills`, \
         as de um projeto em `.glut/skills` dele (lidas só numa pasta confiável); uma de mesmo nome esconde a do \
         Glut.\n\
         \n\
         /skill make-hook bloqueie git push na main",
    ),
    (
        "init",
        "Escreve um `AGENTS.md` curto para esta pasta, ou verifica e encurta o guia que ela tem.\n\
         \n\
         /init — sem guia o Glut escreve o `AGENTS.md`; com um, ele o verifica e o mantém curto\n\
         /init --migrate — pergunta de novo se o `CLAUDE.md` passa para o `AGENTS.md`\n\
         \n\
         Com só um `CLAUDE.md`, o Glut pergunta uma vez se ele passa para o `AGENTS.md` (o `CLAUDE.md` então o \
         importa, um guia para todo harness); um não é guardado. Os guias vão junto a cada turno: /context mostra o \
         que custam.",
    ),
    (
        "gate",
        "Mostra as regras pelas quais o Gate decide e retira um “sempre”.\n\
         \n\
         /gate — as regras em vigor: seus “sempre” desta sessão, depois os de `~/.glut/settings.json` e os do `.glut/` \
         do projeto; ⏎ num “sempre” o retira, e o Glut pergunta de novo da próxima vez\n\
         \n\
         ⌃G abre a aba Gate: cada julgamento ao vivo, os hosts alcançados e as recusas. ⇧⇥ muda o modo.",
    ),
    (
        "mcp",
        "Mostra os servidores MCP e como cada um está, faz login num hospedado, adiciona ou remove um.\n\
         \n\
         /mcp — cada servidor: conectado com as ferramentas dele, esperando um login ou com falha\n\
         /mcp login <name> — faz login num servidor hospedado; o endereço dele abre no seu navegador com ⏎\n\
         /mcp login <name> <address> — por SSH: o endereço onde o navegador terminou, colado de volta\n\
         /mcp logout <name> — sai e esquece o token dele\n\
         /mcp add <name> <url> — adiciona um servidor hospedado a `~/.glut/mcp.json` e o conecta\n\
         /mcp add <name> -- <command> [args] — adiciona um servidor que o próprio Glut inicia\n\
         /mcp remove <name> — o tira de `~/.glut/mcp.json` e da sessão\n\
         \n\
         Um servidor que não oferece um login que o Glut possa usar pede um token, digitado oculto. Os servidores \
         próprios de um projeto são do projeto para mudar: /mcp remove tira só os seus. `glut mcp` faz o mesmo fora de \
         uma sessão.\n\
         \n\
         Enquanto uma das suas ferramentas corre, um servidor pode perguntar-lhe: um formulário (⇥ ↑↓ entre os \
         campos, espaço marca, ⏎ envia, Esc fecha, ⌃D recusa) ou uma página a abrir (o). Só você responde, nunca o \
         modelo; um formulário que pede uma palavra-passe ou uma chave é recusado e, sem ninguém presente, toda a \
         pergunta é recusada.",
    ),
    (
        "images",
        "Escolhe como as imagens são desenhadas no fluxo.\n\
         \n\
         /images inline — a imagem real onde o terminal consegue mostrar uma\n\
         /images blocks — meios-blocos em cor real\n\
         /images off — só a linha da imagem\n\
         \n\
         /images sozinho mostra a escolha atual. Por padrão o Glut desenha inline onde o terminal consegue e em blocos \
         nos outros. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "links",
        "Escolhe como os links aparecem no fluxo.\n\
         \n\
         /links clickable — o texto do link é o link, onde o terminal fala OSC 8\n\
         /links plain — a URL é escrita depois do texto\n\
         \n\
         /links sozinho mostra a escolha atual. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "notify",
        "Avisa quando um turno termina ou o Glut pergunta, enquanto a janela não está na frente.\n\
         \n\
         /notify on — o fim de um turno e cada pergunta\n\
         /notify asks — só as perguntas\n\
         /notify off — nada\n\
         /notify system — pelas notificações próprias do macOS (um clique numa delas abre o Editor de Script, não o \
         terminal)\n\
         /notify after <s> — o fim de um turno só é avisado depois de ele ter rodado tantos segundos (10 por padrão, \
         até 3600)\n\
         /notify tiles on|off — os cartões no canto superior direito da janela\n\
         \n\
         /notify sozinho mostra o que vale agora. O Glut usa as notificações do próprio terminal onde sabe que ele as \
         tem, e o sino. Uma notificação nomeia o Glut, o projeto, o pane e o que aconteceu, nunca as palavras do \
         modelo. Guardado em `~/.glut/prefs.json`, acima de `notify` em `~/.glut/settings.json`.",
    ),
    (
        "secrets",
        "Mostra e define as chaves ocultas do modelo nesta sessão.\n\
         \n\
         /secrets — cada valor oculto até agora, por número, tipo, tamanho e onde foi visto (nunca o valor)\n\
         /secrets allow <n> — deixa o segredo n passar nesta sessão\n\
         /secrets allow <n> always — deixa passar sempre (a impressão digital dele é guardada, nunca o valor)\n\
         /secrets add <name> <regex> — oculta um formato seu, a partir do próximo resultado\n\
         /secrets remove <name> — descarta um formato seu\n\
         /secrets on|off — liga ou desliga a ocultação\n\
         \n\
         Uma chave no que uma ferramenta responde chega ao modelo como ‹secret n: …›; o Glut só devolve o valor dela a \
         um arquivo que já a contém. Uma mensagem sua que parece conter uma chave gera uma pergunta antes de ser \
         enviada. O modelo fica sabendo de uma mudança com a sua próxima mensagem. Seus formatos e escolhas são \
         guardados em `~/.glut/secrets.json`.\n\
         \n\
         A ocultação protege contra uma chave mostrada por acidente; o sandbox e o Gate de rede continuam sendo as \
         barreiras.",
    ),
    (
        "keychain",
        "Manter o porta-chaves fora do alcance dos comandos que o Glut executa.\n\
         \n\
         /keychain — se está aberto, e os programas que o podem pedir\n\
         /keychain add <program> — põe um programa na lista (gh, ou git e um subcomando: git push)\n\
         /keychain remove <program> — tira-o da lista\n\
         /keychain open — abre-o a todo comando, até esta sessão terminar\n\
         /keychain close — volta a fechá-lo\n\
         \n\
         Uma chave ou um início de sessão no porta-chaves alcança-se para lá de toda regra de ficheiros da sandbox, \
         por isso um comando que o Glut executa encontra o porta-chaves fechado. Um comando de um programa da lista \
         pergunta primeiro — em todo modo, também em `auto` — e corre com o porta-chaves aberto assim que o permites; \
         um «sempre» torna-o uma regra tua. Sem ninguém presente (um plano, `glut run`) só uma regra assim \
         o abre. Quando um comando falha no porta-chaves fechado, o Glut di-lo sob a sua saída.\n\
         \n\
         Permitir um comando dá o porta-chaves também a tudo o que ele inicia: um hook do git, uma extensão do gh ou \
         um script de compilação pode então ler qualquer chave nele.\n\
         \n\
         Só as suas próprias regras dão o porta-chaves: uma regra de permissão do `.glut/` de um projeto deixa um \
         comando correr, nunca com o porta-chaves aberto. Uma cadeia só o recebe quando cada parte está na lista ou \
         só lê. O que `glut keychain open` mantém aberto só conta aqui, onde você está — nunca em `glut run` nem na \
         execução de um anfitrião.\n\
         \n\
         Uma regra sua só o abre sem perguntar quando nomeia um programa e o seu subcomando (`gh pr *`, `git push *`) \
         — nunca um programa inteiro (`gh *`). Um cartão 🔑 mostra o comando inteiro, ↑↓ para o percorrer, e só \
         permite depois de o ler até ao fim.\n\
         \n\
         A lista fica em `~/.glut/keychain.json`; `glut keychain open` mantém o porta-chaves aberto de uma sessão \
         para outra, `glut keychain close` fecha-o de vez.",
    ),
    (
        "sandbox",
        "Como são contidos os comandos que o Glut executa, e se podem escrever nas pastas git.\n\n/sandbox — a \
         sandbox aqui (macOS: Seatbelt; Linux: bubblewrap e seccomp) e o interruptor do git\n/sandbox git \
         read-only — nenhum comando escreve numa pasta git: nenhum faz commit a partir da sandbox\n/sandbox git \
         writable — os comandos podem voltar a escrever nas pastas git, também fazer commit\n\
         /sandbox required on — nenhum comando corre onde não há sandbox (falta o bubblewrap, um contentor \
         sem o direito); off deixa-os correr, contidos só pelas regras\n\nA sandbox deixa \
         um comando escrever só nas tuas pastas, ler tudo menos os teus segredos e os ficheiros .env, e chegar à \
         rede só pelo proxy do Glut. Em Linux não consegue recusar um nome que ainda não existe: o que um \
         comando cria e o macOS recusa — um .envrc novo, um .git, um .git/commondir — vai para \
         ~/.glut/quarantine assim que aparece, e mais uma vez depois do comando. Uma ferramenta tua que corra \
         precisamente nesse momento ainda o poderia encontrar; com as pastas git só de leitura, não se cria \
         nenhum nelas.\n\nO interruptor vale em cada sessão e janela a partir do próximo comando, e fica em \
         ~/.glut/sandbox.json — nunca escrito por um comando. glut sandbox show e glut sandbox git \
         read-only|writable fazem o mesmo fora do Glut.",
    ),
    (
        "classifier",
        "Deixa um segundo modelo julgar os comandos sobre os quais só o modo te perguntaria.\n\
         \n\
         /classifier — que modelo julga, ou que nenhum o faz\n\
         /classifier session — julga o modelo da sessão (enquanto não definires nenhum)\n\
         /classifier <provider> <model> — um modelo próprio, basta um rápido (openrouter deepseek/deepseek-v4.1-flash)\n\
         /classifier off — nenhum julga: és perguntado sempre\n\
         \n\
         O Glut conhece muitos comandos que apenas leem (ls, grep, git status, …) e executa-os em todos os modos. Um ciclo ou um pipeline deles (for f in *.md; do grep -c x \"$f\"; done) não o consegue julgar só pelo texto, por isso pergunta. O segundo modelo vê esse comando e a sua pasta — nunca a conversa, um ficheiro ou uma saída — e diz READS ou CHANGES. Só com READS o comando executa como um comando de leitura conhecido, na sandbox; tudo o resto, ou nenhuma resposta clara em segundos, pergunta-te como antes, com a sua opinião sob o comando. Uma regra tua, um comando pesado, o porta-chaves e a sandbox ficam como estão; o mesmo comando não é julgado duas vezes numa sessão.\n\
         \n\
         Um comando que deixou passar indica-o ao lado. A escolha fica em `~/.glut/classifier.json` e vale a partir do próximo turno; `glut run --classifier` define-a para uma execução.",
    ),
    (
        "ide",
        "O teu editor ao lado do Glut: Neovim com o plugin glut.nvim, VS Code (e Cursor, Windsurf) ou um IDE JetBrains com o plugin próprio do Glut.\n\
         \n\
         /ide — a que editor o Glut está ligado, e quais servem\n\
         /ide on — ligar (pergunta quando servem vários)\n\
         /ide off — desligar nesta sessão\n\
         \n\
         Ligado, cada mensagem tua leva o ficheiro ativo e a seleção (⧉ por baixo), enquanto nenhuma regra de leitura \
         retiver o ficheiro; o modelo pode perguntar que ficheiros estão abertos e o que dizem os servidores de \
         linguagem; o Glut lê um buffer como o editor o tem, também o não guardado, e uma pergunta do gate sobre uma \
         alteração aparece também como diff no editor — conta a primeira resposta. O Glut liga-se sozinho ao arrancar \
         onde serve um editor: o que arrancou o Glut, senão aquele cuja pasta contém a do Glut. \"⧉\" no rodapé diz \
         que está ligado.",
    ),
    (
        "clean",
        "Evitar que os dados do Glut em ~/.glut cresçam sem fim.\n\
         \n\
         /clean — o que o Glut guarda, item por item, e o que pode sair segundo os seus prazos\n\
         /clean now — limpa já, em segundo plano\n\
         /clean keep — os prazos\n\
         /clean keep <kind> <days>d — define um: snapshots, journals, sessions, results ou lean\n\
         \n\
         O que sai: os snapshots de uma execução mais antigos que o seu prazo (7 dias) — o depósito inteiro de uma \
         pasta quando não resta nenhum, exista a pasta ou não; o diário de uma execução inacabada (7 dias); uma \
         sessão não escrita há 90 dias, com o seu registo de eventos e os seus resultados longos; os resultados \
         longos sem sessão após 30 dias; as imagens que nenhuma sessão guardada nomeia; as linhas de lean.jsonl com \
         mais de 90 dias. As exportações são só contadas: são ficheiros seus.\n\
         \n\
         Uma vez por dia o Glut limpa sozinho ao arrancar — aqui e em `glut run` — e diz o que liberou. Saídas de \
         build e caches (uma pasta com um CACHEDIR.TAG, sob node_modules/ ou o target/ de um projeto Cargo) não têm \
         snapshots.\n\
         \n\
         Os prazos ficam em `~/.glut/clean.json`. Na linha de comandos: `glut clean`, `glut clean --dry-run`, `glut \
         clean keep sessions 30d`; `glut clean --dry-run --json` escreve o relatório para um programa.",
    ),
    (
        "motion",
        "Escolhe o quanto a tela se move.\n\
         \n\
         /motion full — o Glimm respira, faíscas sobem, a chama tremula\n\
         /motion calm — tudo fica parado; um quadro só é desenhado quando algo mudou\n\
         /motion off — como calm\n\
         \n\
         /motion sozinho mostra a escolha atual. O “Reduzir movimento” do macOS e o modo leitor mantêm a tela parada \
         qualquer que seja a sua escolha. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "rule",
        "Uma regra para esta sessão que o modelo segue a cada turno.\n\
         \n\
         /rule <text> — guardá-la, como «atualize a lista de tarefas a cada passo»\n\
         /rule — as regras atuais, numeradas\n\
         /rule remove <n> — retirar a regra n\n\
         \n\
         As regras vão com as instruções de cada turno, depois dos guias, até você retirá-las; /clear as mantém, uma \
         sessão nova começa sem elas. No máximo 20, de 500 caracteres cada. Para uma regra de toda sessão, escreva-a \
         em `~/.glut/AGENTS.md` ou no guia do projeto.",
    ),
    (
        "maxsteps",
        "Quantos passos um turno dá no máximo.\n\
         \n\
         /maxsteps — o limite atual\n\
         /maxsteps <n> — um turno para depois de n passos, de 1 a 1000\n\
         \n\
         Um passo é uma chamada ao modelo. No limite o turno para e diz isso; ⏎ na entrada vazia envia «Go on where \
         you stopped.» e o trabalho continua. Os orçamentos e o limite de tempo continuam sendo os freios reais. \
         Guardado em `~/.glut/prefs.json`; `maxTurns` em `~/.glut/settings.json` define para cada execução, `glut run \
         --max-turns` para uma.",
    ),
    (
        "nudge",
        "Se o Glut pede ao modelo que continue no fim de um turno, que se ate à lista de tarefas e que escreva uma \
         quando trabalha sem nenhuma.\n\
         \n\
         /nudge on — quando um turno termina com itens da lista de tarefas abertos, ou com palavras que anunciam um \
         passo («agora vou…», dois-pontos no fim), o Glut envia ao modelo uma linha própria: continuar ou dizer por \
         que para; o histórico mostra isso discretamente. Se a lista não muda enquanto ele trabalha, o Glut lembra, e \
         se nada mudar, uma vez numa mensagem própria; se numa execução não escreveu nenhuma, pede uma, uma vez por \
         turno\n\
         /nudge off — um turno termina onde o modelo o termina: sem lembrete da lista nem pedido de uma\n\
         \n\
         A pergunta do fim vem uma vez por turno, nunca no modo plan, depois de uma pergunta a você, numa rodada de um \
         loop ou na execução de um plano; o lembrete e o pedido seguem a mesma escolha, mas na execução de um plano o \
         Glut lembra sempre. /nudge sozinho mostra a escolha atual. Guardado em `~/.glut/prefs.json`; `\"nudge\": \
         false` em `~/.glut/settings.json` desliga para cada execução.",
    ),
    (
        "lsp",
        "Se os servidores de linguagem ajudam o modelo.\n\
         \n\
         /lsp on — após uma edição, os erros do arquivo vêm com o resultado, e o modelo tem a ferramenta lsp: onde um \
         símbolo é definido, cada lugar onde é usado, seu tipo e sua documentação, e uma renomeação em todo o \
         projeto\n\
         /lsp off — nenhum servidor roda: seus processos terminam na hora, sem erros após uma edição, sem ferramenta \
         lsp\n\
         /lsp — a escolha atual, e cada servidor da sessão: pronto, indexando o projeto, ou desligado na sessão\n\
         \n\
         O Glut só inicia servidores que nunca executam o código do projeto (rust-analyzer sem scripts de build nem \
         macros procedurais, pyright e os servidores de TOML, YAML, JSON, CSS, HTML e Markdown), só no sandbox sem \
         rede, e só onde o Gate permite sua linha de comando. Um servidor inicia na primeira edição ou pergunta da \
         sua linguagem e fica para a sessão; o rust-analyzer leva alguns segundos para indexar um projeto grande, \
         enquanto isso lsp o diz e o modelo usa grep. Uma renomeação é uma só mudança: perguntada uma vez para todos \
         os seus arquivos, todos ou nenhum, com um snapshot antes. Guardado em `~/.glut/prefs.json`; `\"lsp\": false` \
         em `~/.glut/settings.json` desliga para cada execução, `glut run --no-lsp` para uma.",
    ),
    (
        "reader",
        "Configura o Glut para um leitor de tela.\n\
         \n\
         /reader on — teclas em palavras, um rodapé simples, sem Glimm, molduras simples, nada se movendo e uma linha \
         de texto a cada mudança de estado\n\
         /reader off — a tela de sempre\n\
         \n\
         /reader sozinho mostra a escolha atual. Guardado em `~/.glut/prefs.json`; `glut --reader` o mantém ligado por \
         uma execução.",
    ),
    (
        "language",
        "Escolhe o idioma em que o Glut fala com você.\n\
         \n\
         /language — os idiomas para escolher\n\
         /language <code> — en, de, fr, it, es, zh, ja ou pt\n\
         \n\
         Sem escolha, o Glut fala o idioma do seu sistema. O modelo responde no idioma em que você escreve, qualquer \
         que seja a sua escolha aqui. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "theme",
        "Escolhe as cores do Glut.\n\
         \n\
         /theme — os temas para escolher\n\
         /theme <name> — glut, o próprio do Glut, ou um dos temas do Axiomata (catppuccin-mocha, github-dark, …)\n\
         \n\
         O Glut colore só o texto e mantém o fundo do terminal: um tema claro pede um terminal claro. Guardado em \
         `~/.glut/prefs.json`; numa janela aberta por um aplicativo hospedeiro, a escolha vale para aquela sessão.\n\
         \n\
         Cada tema dá às partes do rodapé, aos nomes das ferramentas segundo o seu tipo (ler, escrever, comandos, o \
         que sai), aos panes e agentes e à linha acima da entrada as suas próprias cores suaves; o que precisa de si \
         fica vivo. O modo de leitura e `NO_COLOR` mantêm-no sóbrio.",
    ),
    (
        "editor",
        "Escolhe o editor em que o Glut abre arquivos e a entrada.\n\
         \n\
         /editor — os editores encontrados no seu sistema\n\
         /editor <command> — o seu próprio comando, com `{file}` e `{line}` onde eles entram\n\
         \n\
         Sem `{file}`, o arquivo é acrescentado ao fim. Um editor com janela precisa esperar até o arquivo ser fechado \
         (`code --wait`). ⌃X E edita a entrada nele; /todo open e /memory open também o usam. Guardado em \
         `~/.glut/prefs.json`.\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "Escolhe como uma sessão em repouso mantém o cache de prompt do provedor, para que o próximo turno comece \
         aquecido.\n\
         \n\
         /cache ping — o Glut lê o cache de novo a cada poucos minutos enquanto a sessão descansa, por até 30 \
         minutos\n\
         /cache 1h — a Anthropic o mantém por uma hora (cada escrita custa o dobro); com outros provedores a leitura o \
         mantém\n\
         /cache off — não mantido: uma pausa além dos poucos minutos do provedor começa fria\n\
         \n\
         /cache sozinho mostra a escolha atual; ping é o padrão. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "footer",
        "Escolhe como o rodapé junta as suas partes.\n\
         \n\
         /footer powerline — as setas do Powerline\n\
         /footer plain — blocos simples\n\
         \n\
         /footer sozinho mostra a escolha atual e o porquê. Sem escolha, o Glut deduz do terminal se ele desenha os \
         glifos do Powerline. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "vim",
        "Edita a entrada no estilo do vi.\n\
         \n\
         /vim on — ⎋ vai para o modo Normal (movimentos, operadores, contagens, u e ⌃R), i a I A o O voltam para \
         Inserir\n\
         /vim off — as teclas de edição de sempre\n\
         \n\
         /vim sozinho o liga. ⎋ no modo Normal para o Glut. Guardado em `~/.glut/prefs.json`.",
    ),
    (
        "help",
        "Cada comando e cada tecla, numa caixa própria.\n\
         \n\
         /help — os comandos com uma linha cada; digitar filtra, ↑↓ escolhem, ⏎ abre a página inteira de um\n\
         /help <command> — a página desse comando de imediato, como /help loop\n\
         \n\
         Numa página, ↑↓ e PgUp PgDn rolam, ⎋ volta à lista e dali fecha a caixa. A última entrada traz cada tecla por \
         tema, !cmd e o modo vi; ⌃K mantém as teclas no painel lateral.",
    ),
];
