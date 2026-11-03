Elige el editor en el que Glut abre archivos y la entrada.

/editor — los editores que se encuentran en tu sistema
/editor <command> — tu propio comando, con `{file}` y `{line}` donde van

Sin `{file}` el archivo se añade al final. Un editor con ventana debe esperar hasta que se cierre el archivo (`code --wait`). ⌃X E edita la entrada en él; /todo open y /memory open también lo usan. Se guarda en `~/.glut/prefs.json`.

/editor nvim +{line} {file}
