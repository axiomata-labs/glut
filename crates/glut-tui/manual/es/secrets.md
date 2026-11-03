Mira y ajusta las claves ocultas al modelo en esta sesión.

/secrets — cada valor oculto hasta ahora, por número, tipo, longitud y dónde se vio (nunca el valor)
/secrets allow <n> — deja pasar el secreto n durante esta sesión
/secrets allow <n> always — lo deja pasar siempre (se guarda su huella, nunca el valor)
/secrets add <name> <regex> — oculta un formato tuyo, desde el próximo resultado
/secrets remove <name> — quita un formato tuyo
/secrets on|off — el enmascarado activado o desactivado

Una clave en lo que responde una herramienta llega al modelo como ‹secret n: …›; Glut devuelve su valor solo a un archivo que ya lo contiene. De un mensaje tuyo que parezca contener una clave se pregunta antes de enviarlo. El modelo se entera de un cambio con tu próximo mensaje. Tus formatos y elecciones se guardan en `~/.glut/secrets.json`.

El enmascarado protege contra una clave mostrada por accidente; el sandbox y el Gate de red siguen siendo las barreras.
