//! Português (Brasil).

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "idioma: {name}",
    language_unkept: "idioma: {name} — só nesta execução: {reason}",
    language_unknown: "nenhum idioma {name} — {known}",
    language_title: "Idioma do Glut",
    language_now: "atual",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
    topics: &[
        "Sessão",
        "Modelo",
        "Visualização",
        "Panes",
        "Alterações",
        "Projeto",
        "Ajustes",
        "Entrada",
        "Editar a linha",
        "Ajuda",
    ],
    commands: &[
        ("clear", "começar uma sessão nova"),
        (
            "resume",
            "abrir uma sessão anterior desta pasta — as ramificações sob a delas",
        ),
        (
            "projects",
            "continuar em outro projeto: as pastas em que o Glut trabalhou, ou uma pelo caminho",
        ),
        ("compact", "resumir a conversa até aqui"),
        (
            "handoff",
            "/handoff write: o que foi feito e o que vem a seguir; /handoff read numa sessão nova",
        ),
        ("quit", "encerrar a sessão"),
        ("exit", "encerrar a sessão, como /quit"),
        (
            "model",
            "escolher um modelo do provedor em uso, ou /model <id>",
        ),
        ("provider", "trocar de provedor, ou /provider <id>"),
        ("effort", "o quanto o modelo pensa, ou /effort <level>"),
        (
            "context",
            "o quanto a janela está cheia; /context 64k a define",
        ),
        (
            "thinking",
            "o raciocínio do modelo: /thinking off, folded ou open",
        ),
        (
            "panel",
            "o painel lateral com o plano, ou escondê-lo (⌃B; ⌃G o gate)",
        ),
        (
            "meter",
            "contexto, tokens e custo por turno e ferramenta, a economia do lean (⌃N)",
        ),
        (
            "agents",
            "os subagentes da sessão: abrir o fluxo de um deles (um clique no cartão dele também; ⌃X A: a aba Agentes)",
        ),
        (
            "plan",
            "o plano que o Glut enviou do modo plano, inteiro, com a lista de tarefas",
        ),
        (
            "scrollback",
            "a conversa para o scrollback do próprio terminal (F3)",
        ),
        (
            "split",
            "uma sessão nova ao lado do pane principal (até seis, duas fileiras de três); num repositório git aqui ou num worktree",
        ),
        (
            "swap",
            "o pane principal e o pane n (ou este) trocam de lugar",
        ),
        (
            "close",
            "fechar este pane (não o principal) — ou encerrar um agente em segundo plano; a sessão dele fica para /resume",
        ),
        (
            "send",
            "uma mensagem a outro pane (primeiro o número, a partir de três), recebida lá como se fosse digitada; /send plan passa a ele o seu plano para construir",
        ),
        (
            "role",
            "o papel deste pane: o prompt e as ferramentas de um agente nomeado, não o modelo dele — reviewer, tester, docs ou os seus",
        ),
        (
            "bg",
            "um agente em segundo plano: uma sessão sem pane, alcançada pelo nome — /bg as reviewer",
        ),
        (
            "show",
            "trazer um agente do segundo plano para a frente como pane",
        ),
        (
            "hide",
            "mandar este pane para o segundo plano, com a memória dele (⌃X H)",
        ),
        (
            "peek",
            "ler o fluxo de um agente em segundo plano sobre este pane; Esc fecha",
        ),
        (
            "diff",
            "revisar o que o último turno mudou, hunk por hunk; /diff session (⌃R)",
        ),
        (
            "steps",
            "os passos que mudaram arquivos: o que cada um mudou, ou os arquivos de volta",
        ),
        (
            "todo",
            "o TODO.md do projeto (⌃Y): /todo <text> adiciona um, /todo open o edita, /todo clean tira os concluídos",
        ),
        (
            "memory",
            "o que a memória do projeto guarda: ⏎ mostra um fato; /memory open edita o índice dela",
        ),
        (
            "skill",
            "iniciar uma skill: /skill <name> [para quê] — /skill sozinho os lista (make-agent, make-skill, make-hook, …)",
        ),
        (
            "init",
            "um AGENTS.md curto para esta pasta, ou verificar o guia dela (/init --migrate pergunta de novo sobre CLAUDE.md)",
        ),
        (
            "gate",
            "as regras em vigor; retirar um “sempre” (⌃G: o gate, ao vivo)",
        ),
        (
            "images",
            "como as imagens são desenhadas: a real onde o terminal consegue, meios-blocos em cor real, ou só a linha delas",
        ),
        (
            "links",
            "o texto de um link é o link onde o terminal fala OSC 8, senão a URL escrita depois",
        ),
        (
            "motion",
            "o quanto a tela se move; calm e off mantêm o Glimm parado (o “Reduzir movimento” do macOS conta como off)",
        ),
        (
            "reader",
            "para um leitor de tela: teclas em palavras, uma tela simples, nada se movendo, uma linha a cada mudança de estado",
        ),
        (
            "language",
            "o idioma em que o Glut fala com você: en de fr it es zh ja pt — o modelo responde no idioma em que você escreve",
        ),
        (
            "theme",
            "as cores do Glut, ou as de um tema do Axiomata: /theme <name>",
        ),
        ("editor", "escolher o seu editor, ou /editor <command>"),
        (
            "cache",
            "como uma sessão em repouso mantém o cache: ler de novo (ping), a hora da Anthropic (1h), ou nada",
        ),
        (
            "footer",
            "as junções do rodapé: setas do Powerline ou blocos simples (deduzidos do terminal)",
        ),
        ("vim", "editar a entrada no estilo do vi: /vim on ou off"),
        ("help", "comandos e teclas"),
    ],
    keys: &[
        ("⇧⇥", "o próximo modo: plan → ask → build → auto"),
        ("⎋", "parar o Glut"),
        (
            "⏎",
            "enquanto o Glut trabalha: orientá-lo, recebido antes do próximo passo",
        ),
        (
            "⇥",
            "enquanto o Glut trabalha: pôr a mensagem na fila para depois",
        ),
        ("⌥↑", "pegar de volta a última mensagem em espera"),
        ("⌃C ⌃C", "sair"),
        ("⌃F", "buscar no fluxo"),
        ("⌃O", "abrir ou recolher todos os resultados de ferramentas"),
        ("⌃T", "abrir ou recolher todos os raciocínios"),
        ("⌃B", "o painel lateral com o plano, ou escondê-lo"),
        (
            "⌃G",
            "o gate, ao vivo: cada julgamento, os hosts, as recusas",
        ),
        (
            "⌃N",
            "o medidor: contexto, tokens e custo por turno e ferramenta",
        ),
        (
            "⌃X A",
            "a aba Agentes: cada subagente e os agentes em segundo plano, ao vivo",
        ),
        (
            "⌃X ↑",
            "ler o fluxo bloco a bloco: ⏎ recolhe, c copia, ⎋ volta",
        ),
        (
            "⌃X ⇥",
            "o painel lateral fica com as teclas: ←→ as abas, ↑↓ dentro delas, ⎋ volta",
        ),
        ("PgUp PgDn", "rolar o fluxo (⇧↑ ⇧↓ uma linha)"),
        ("F3", "a conversa para o scrollback do terminal"),
        ("F2", "passar o mouse para o terminal e de volta"),
        (
            "⌃P",
            "o próximo pane, em ordem de leitura (⌃] também: ⌃6 num teclado alemão)",
        ),
        ("⌃X 2", "um pane novo ao lado do principal (⌃X 3 também)"),
        (
            "⌃X }",
            "o pane principal mais largo (o mouse também arrasta a borda dele)",
        ),
        ("⌃X {", "o pane principal mais estreito"),
        ("⌃X 0", "fechar este pane"),
        (
            "⌃X B",
            "um agente em segundo plano, com o papel escolhido antes",
        ),
        ("⌃X H", "este pane para o segundo plano"),
        ("⌃R", "revisar o que o último turno mudou, hunk por hunk"),
        ("⎋⎋", "voltar ou ramificar para antes de uma mensagem sua"),
        ("⌃Y", "a aba Tarefas: Espaço marca, a adiciona"),
        ("/", "os comandos"),
        ("@", "mencionar um arquivo"),
        (
            "⌃V",
            "a imagem da área de transferência na mensagem (também um arquivo de imagem arrastado)",
        ),
        ("↑", "as mensagens enviadas antes"),
        ("⇧⏎", "uma linha nova (⌥⏎ também)"),
        ("⌃X E", "editar a entrada no seu editor"),
        ("⌃A ⌃E", "o início da linha, o fim dela (Home End também)"),
        (
            "⌥B ⌥F",
            "uma palavra à esquerda, uma à direita (⌥← ⌥→ também)",
        ),
        ("⌃W", "apagar a palavra anterior (⌥⌫ também)"),
        ("⌃U", "apagar até o início da linha"),
        ("⌃J", "uma linha nova"),
        ("⌃D", "sair, numa linha vazia enquanto o Glut descansa"),
        ("⌃K", "esta folha de teclas"),
    ],
    help_commands: "Comandos",
    help_keys: "Teclas ({legend}; ⌃K as mantém no painel lateral)",
    help_also: "Também: !cmd executa um comando e mostra a saída ao Glut · !!cmd o executa só para você · uma \
                colagem grande aparece como [pasted N lines] · arrastar copia · um clique no cartão de um subagente \
                o abre",
    help_vim: "/vim on: ⎋ para o modo Normal (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y com um movimento \
               ou dobrados · contagens · p P · u ⌃R), i a I A o O para Inserir; ⎋ no modo Normal para o Glut",
    legend: "⌃ Control · ⇧ Shift · ⌥ Option · ⏎ Return · ⇥ Tab · ⎋ Esc",
};
