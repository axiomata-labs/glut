Inicia un agente en segundo plano: una sesión sin pane, alcanzable por su nombre.

/bg — un agente llamado agent (o agent-2, …); en un repositorio git Glut pregunta dónde trabaja
/bg <name> — un agente con ese nombre (letras minúsculas, dígitos y guiones)
/bg as <role> — hace de un rol y se llama como él
/bg here — trabaja en esta carpeta
/bg tree [branch] — trabaja en un Worktree nuevo

/send <name> le da trabajo, /peek <name> lee su flujo, /show <name> lo trae al frente, /close <name> lo termina. Cada agente trabaja y cuesta por su cuenta: Glut pregunta una vez por ventana antes de un séptimo. ⌃X B inicia uno con su rol elegido antes, ⌃X A los lista.

/bg docs-check as docs
