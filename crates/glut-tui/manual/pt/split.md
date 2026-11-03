Abre uma sessão nova num pane ao lado do principal.

/split — um pane novo; num repositório git o Glut pergunta onde ele trabalha
/split here — ele trabalha nesta pasta, ao seu lado
/split tree [branch] — ele trabalha num Worktree novo, numa branch própria
/split resume — ele abre uma sessão anterior, escolhida da lista
/split as <role> — ele assume um papel (/role os lista)

Até seis panes ficam ao lado do principal, em duas fileiras de três. ⌃P percorre os panes, ⌃X 2 também divide, ⌃X } e ⌃X { deixam o pane principal mais largo ou mais estreito. Dois panes aqui mudam os mesmos arquivos; um Worktree os mantém separados até que /close pergunte como ele termina.

/split tree fix-login as reviewer
