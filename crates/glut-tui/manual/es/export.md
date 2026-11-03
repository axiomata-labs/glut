Escribe la sesión para leerla o compartirla.

/export — Markdown en `~/.glut/exports`
/export html — un solo archivo HTML, con sus estilos dentro y sin script
/export jsonl — los mensajes como líneas JSON
/export copy — el Markdown al portapapeles en lugar de a un archivo
/export full — añade los resultados completos de las herramientas y el prompt del sistema
/export <format> <path> — a un archivo tuyo (`~/` es tu carpeta personal, una ruta relativa queda en la carpeta de trabajo)

Las palabras van en cualquier orden, la ruta al final. Sin full la exportación muestra lo que vio el modelo: los resultados largos, acortados. Un archivo existente nunca se sobrescribe. Las claves que Glut ocultó siguen ocultas, pero lo que escribiste tú mismo aún puede contener una: lee una exportación antes de compartirla. En /resume, ⌃E exporta una sesión de la lista.
