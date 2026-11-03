Mira los servidores MCP y cómo está cada uno, inicia sesión en uno alojado, añade o quita uno.

/mcp — cada servidor: conectado con sus herramientas, esperando un inicio de sesión, o fallido
/mcp login <name> — inicia sesión en un servidor alojado; su dirección se abre en tu navegador con ⏎
/mcp login <name> <address> — por SSH: la dirección en la que acabó el navegador, pegada de vuelta
/mcp logout <name> — cierra la sesión y olvida su token
/mcp add <name> <url> — añade un servidor alojado a `~/.glut/mcp.json` y lo conecta
/mcp add <name> -- <command> [args] — añade un servidor que Glut inicia él mismo
/mcp remove <name> — lo quita de `~/.glut/mcp.json` y de la sesión

Un servidor que no ofrece un inicio de sesión que Glut pueda usar pide un token, escrito oculto. Los servidores propios de un proyecto los cambia el proyecto: /mcp remove solo quita los tuyos. `glut mcp` hace lo mismo fuera de una sesión.

Mientras una de sus herramientas trabaja, un servidor puede preguntarte: un formulario (⇥ ↑↓ entre los campos, espacio marca, ⏎ envía, Esc cierra, ⌃D rechaza) o una página que abrir (o). Solo tú respondes, nunca el modelo; un formulario que pide una contraseña o una clave se rechaza, y sin nadie presente toda pregunta se rechaza.
