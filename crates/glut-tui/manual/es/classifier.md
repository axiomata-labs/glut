Deja que un segundo modelo juzgue los comandos por los que solo el modo te preguntaría.

/classifier — qué modelo juzga, o que ninguno lo hace
/classifier session — juzga el modelo de la sesión (mientras no fijes ninguno)
/classifier <provider> <model> — un modelo propio, basta uno rápido (openrouter deepseek/deepseek-v4.1-flash)
/classifier off — ninguno juzga: se te pregunta cada vez

Glut conoce muchos comandos que solo leen (ls, grep, git status, …) y los ejecuta en todos los modos. Un bucle o una tubería de ellos (for f in *.md; do grep -c x "$f"; done) no puede juzgarlo solo por su texto, así que pregunta. El segundo modelo ve ese comando y su carpeta — nunca la conversación, un archivo ni una salida — y dice READS o CHANGES. Solo con READS el comando se ejecuta como un comando de lectura conocido, en el sandbox; cualquier otra cosa, o ninguna respuesta clara en segundos, te pregunta como antes, con su opinión bajo el comando. Una regla tuya, un comando pesado, el llavero y el sandbox quedan como están; el mismo comando no se juzga dos veces en una sesión.

Un comando que dejó pasar lo indica a su lado. La elección se guarda en `~/.glut/classifier.json` y vale desde el próximo turno; `glut run --classifier` la fija para una ejecución.
