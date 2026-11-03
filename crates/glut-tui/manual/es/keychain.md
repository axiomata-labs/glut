Mantener el llavero fuera del alcance de los comandos que Glut ejecuta.

/keychain — si está abierto, y los programas que pueden pedirlo
/keychain add <program> — pone un programa en la lista (gh, o git y un subcomando: git push)
/keychain remove <program> — lo quita de la lista
/keychain open — lo abre a todo comando, hasta que termine esta sesión
/keychain close — lo vuelve a cerrar

Una clave o un inicio de sesión del llavero se alcanza más allá de toda regla de archivos del sandbox, así que un comando que Glut ejecuta encuentra el llavero cerrado. Un comando de un programa de la lista pregunta primero — en todo modo, también en `auto` — y se ejecuta con el llavero abierto en cuanto lo permites; un «siempre» lo convierte en una regla tuya. Sin nadie presente (un plan, `glut run`) solo una regla así lo abre. Cuando un comando falla por el llavero cerrado, Glut lo dice bajo su salida.

Permitir un comando da el llavero también a todo lo que inicia: un hook de git, una extensión de gh o un script de compilación puede leer entonces cualquier clave en él.

Solo tus propias reglas dan el llavero: una regla de permiso del `.glut/` de un proyecto deja ejecutar un comando, nunca con el llavero abierto. Una cadena lo obtiene solo si cada parte está en la lista o solo lee. Lo que `glut keychain open` mantiene abierto solo cuenta aquí, donde estás tú — nunca en `glut run` ni en la ejecución de un anfitrión.

Una regla tuya lo abre sin preguntar solo si nombra un programa y su subcomando (`gh pr *`, `git push *`) — nunca un programa entero (`gh *`). Una tarjeta 🔑 muestra el comando entero, ↑↓ para recorrerlo, y solo permite cuando lo has leído hasta el final.

La lista se guarda en `~/.glut/keychain.json`; `glut keychain open` mantiene el llavero abierto de una sesión a otra, `glut keychain close` lo cierra para siempre.
