Ejecuta la misma tarea en dos a cuatro modelos a la vez y los compara (la arena).

/arena — los modelos de todos los proveedores; la barra espaciadora marca de dos a cuatro, ⏎ los pone en la entrada para añadir la tarea
/arena <model>, <model>: <task> — lo mismo en una línea (proveedor/modelo, o un nombre que encaje con un modelo)
/arena say <text> — una repregunta a cada modelo que respondió, cuando todos acaban (no en el banco); tiempo, coste y tokens se suman a lo largo de las rondas
/arena take <n> — los cambios del modelo n a tu carpeta, sin commit
/arena card — la tarjeta de comparación otra vez: las cifras y la comprobación de cada modelo, debajo su diff (←/→ otro modelo, s dos lado a lado, c comprobar, t tomar)
/arena check <command> — la comprobación para este repositorio (cargo test, npm test …), ejecutada en el worktree de cada modelo, en su sandbox; guardada, se ejecuta sola tras la próxima arena
/arena tasks — los conjuntos de tareas del banco de pruebas en ~/.glut/arena/tasks; /arena tasks <set> ejecuta uno con los modelos de la última arena (o /arena tasks <set>: <model>, <model>; con x3 tras el conjunto, cada tarea tres veces seguidas (hasta x5), la tabla muestra entonces 2/3; con --background sus modelos corren como agentes en segundo plano, sin pane, su progreso en la pestaña Agents), tarea tras tarea, cada una juzgada por su propia comprobación
/arena results — los resultados de cada conjunto (tareas superadas, Ø tiempo y coste) y las victorias de la arena libre
/arena blind on|off — la arena libre a ciegas (por defecto): sus modelos llevan letras en un orden sorteado, sus nombres llegan en cuanto tomes uno o termines la arena
/arena judge — tras tu elección, el modelo del pane desde el que se inició la arena comenta las soluciones en dos o tres frases (sin herramientas; nunca antes, para no teñir tu elección)
/arena end — cierra los panes de la arena y elimina sus worktrees (pregunta antes)

Cada modelo trabaja en un pane y un worktree propios que parten de tu carpeta tal como está, con los archivos sin commit y nuevos incluidos (sin archivo .env, sin enlaces), en auto: solo preguntan los comandos pesados. Un modelo se detiene a los 30 minutos o a los 2 $. Los modelos en la nube se ejecutan a la vez, los locales (Ollama, LM Studio) uno tras otro cuando los demás han terminado. Cuando todos acaban, sus cifras quedan en el pane desde el que se inició la arena, y /arena card abre la tarjeta de comparación: tiempo, coste, tokens, llamadas y lo que cambió cada uno. /arena take comprueba que los cambios aún encajan en tu carpeta; si no encajan, no se escribe nada.

/arena sonnet, gpt-5: make the failing test pass
