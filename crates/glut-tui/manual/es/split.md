Abre una sesión nueva en un pane junto al principal.

/split — un pane nuevo; en un repositorio git Glut pregunta dónde trabaja
/split here — trabaja en esta carpeta, a tu lado
/split tree [branch] — trabaja en un Worktree nuevo, en una rama propia
/split resume — abre una sesión anterior, elegida de la lista
/split as <role> — hace de un rol (/role los lista)

Hasta seis panes quedan junto al principal, en dos filas de tres. ⌃P recorre los panes, ⌃X 2 también divide, ⌃X } y ⌃X { hacen el pane principal más ancho o más estrecho. Dos panes aquí cambian los mismos archivos; un Worktree los mantiene separados hasta que /close pregunta cómo termina.

/split tree fix-login as reviewer
