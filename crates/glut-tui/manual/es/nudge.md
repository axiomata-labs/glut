Si Glut pide al modelo que siga al final de un turno, que se ciña a su lista de tareas y que escriba una cuando trabaja sin ninguna.

/nudge on — cuando un turno termina con puntos de la lista de tareas abiertos, o con palabras que anuncian un paso («a continuación…», dos puntos al final), Glut envía al modelo una línea propia: seguir o decir por qué para; el historial lo muestra discretamente. Si la lista no cambia mientras trabaja, Glut le recuerda, y si eso no cambia nada, una vez con un mensaje propio; si en una ejecución no ha escrito ninguna, le pide una, una vez por turno
/nudge off — un turno termina donde el modelo lo termina: sin recordatorio de la lista ni petición de una

La pregunta del final viene una vez por turno, nunca en modo plan, tras una pregunta a ti, en una ronda de un bucle o en la ejecución de un plan; el recordatorio y la petición siguen la misma elección, pero en la ejecución de un plan Glut siempre recuerda. /nudge solo muestra la elección actual. Guardado en `~/.glut/prefs.json`; `"nudge": false` en `~/.glut/settings.json` lo apaga para cada ejecución.
