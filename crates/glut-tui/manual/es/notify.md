Recibe un aviso cuando un turno termina o Glut pregunta, mientras la ventana no está al frente.

/notify on — el fin de un turno y cada pregunta
/notify asks — solo las preguntas
/notify off — nada
/notify system — con las notificaciones propias de macOS (un clic en una abre Script Editor, no el terminal)
/notify after <s> — el fin de un turno se avisa solo si duró tantos segundos (10 por defecto, hasta 3600)
/notify tiles on|off — las fichas arriba a la derecha de la ventana

/notify solo muestra lo que rige ahora. Glut usa las notificaciones propias del terminal donde sabe que las tiene, y la campana. Una notificación nombra a Glut, el proyecto, el pane y lo que pasó, nunca las palabras del modelo. Se guarda en `~/.glut/prefs.json`, por encima de `notify` en `~/.glut/settings.json`.
