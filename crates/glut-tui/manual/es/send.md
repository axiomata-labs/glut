Envía un mensaje a otro pane o agente en segundo plano, recibido allí como si lo hubieras escrito.

/send — una lista de a quién escribir: los panes y agentes de aquí, luego las otras ventanas de Glut; ⏎ pone /send <who> en la entrada
/send <text> — al único otro pane
/send <n|name> <text> — al pane n o al agente de ese nombre (necesario con tres o más)
/send plan — pasa el plan de este pane al otro pane para que construya
/send <n|name> plan — se lo pasa a ese

El mensaje empieza de inmediato donde la sesión descansa, y si no, tras su turno. Las teclas se quedan en este pane.
