Si los propios ajustes .glut de esta carpeta cuentan por completo.

/trust — si esta carpeta es de confianza, y qué añaden entonces sus propios ajustes
/trust on — confía en ella: cuentan sus reglas de permiso, su modo, sus skills, agentes y comandos
/trust off — deja de confiar en ella: solo cuentan sus reglas de prohibición y de pregunta
/trust list — cada carpeta de confianza

El .glut/settings.json de un proyecto puede guardar más secretos y preguntar o prohibir más — eso cuenta en cada carpeta. Lo que permitiría más a un modelo, sus reglas de permiso y su modo, solo cuenta donde confiaste en la carpeta, igual que los skills, agentes y comandos de su .glut. Lee lo que muestra /trust antes de decir on: el .glut de un repositorio clonado es la palabra de otro. Su modo nunca va más allá del tuyo, y sus hooks nunca se ejecutan, de confianza o no.

El cambio cuenta desde la próxima sesión de la carpeta (glut -c retoma la conversación). Nunca la carpeta personal, la raíz ni una carpeta que no sea solo tuya (la de otro usuario, o una que cualquiera puede escribir, como /tmp). La lista se guarda en ~/.glut/trusted.json, que ningún comando que Glut ejecuta escribe; glut trust show|add|remove|list hace lo mismo fuera de Glut.
