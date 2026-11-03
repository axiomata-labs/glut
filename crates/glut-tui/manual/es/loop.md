Ejecuta una tarea una y otra vez en un agente en segundo plano propio, mientras Glut está abierto.

/loop <interval> [fresh] [as <role>] [notify] <task> — inicia un bucle; la primera ronda de inmediato
/loop — lista los bucles: intervalo, rondas, la próxima ronda y el coste hasta ahora
/loop stop [name] — termina ese bucle, o el único
/loop rounds [n] — las rondas máximas de un bucle; solo, muestra el límite actual
/loop budget [usd] — lo máximo que puede costar un bucle; solo, muestra el límite actual
/loop tokens [n] — el máximo de tokens que puede usar un bucle en una suscripción (5M, 500k); solo, muestra el límite actual

Las rondas nunca se solapan: el intervalo se cuenta desde el fin de una ronda, como 30s, 5m, 2h o 1h30m (de 10s a 24h). Una ronda sigue con la conversación de la ronda anterior; con fresh cada ronda empieza una nueva. La tarea es un mensaje, o /skill <name> [what for], /compact o /init — ningún otro comando.

El agente se llama loop (o como su rol) y toma el modo de este pane, el «siempre» que diste aquí, sus carpetas junto a la de trabajo y su rol; as <role> solo puede nombrar el rol que hace este pane. Sus preguntas esperan a que respondas. Una ficha aparece cuando una ronda pregunta o el bucle termina; con notify, tras cada ronda.

Un bucle termina tras sus rondas o en cuanto sus rondas cuestan su presupuesto (por defecto 50 rondas y 5 $, y si no `loop.rounds` y `loop.maxUsd` de `~/.glut/settings.json`; un cambio cuenta para los bucles iniciados después), cuando una ronda falla o tú lo detienes (⎋ en su ronda), o cuando su modelo llama a `end_loop` porque se alcanzó el final que nombró tu tarea. Cuando los precios del modelo son desconocidos, solo las rondas lo terminan. Su agente se queda después en la pestaña Agentes, para leerlo o hacerle /close. x en su tarjeta de la pestaña Agentes (⌃X A) termina el bucle.

/loop 10m ejecuta los tests y corrige lo que falle; para cuando todos pasen
/loop 1h fresh as reviewer notify revisa el último commit
