Evitar que los datos de Glut en ~/.glut crezcan sin fin.

/clean — lo que Glut guarda, partida por partida, y lo que puede irse según tus plazos
/clean now — limpia ya, en segundo plano
/clean keep — los plazos
/clean keep <kind> <days>d — fija uno: snapshots, journals, sessions, results o lean

Lo que se va: las instantáneas de una ejecución más antiguas que su plazo (7 días) — el almacén entero de una carpeta cuando no queda ninguna, esté la carpeta o no; el diario de una ejecución sin terminar (7 días); una sesión sin escribir desde hace 90 días, con su registro de eventos y sus resultados largos; los resultados largos sin sesión tras 30 días; las imágenes que ninguna sesión guardada nombra; las líneas de lean.jsonl de más de 90 días. Las exportaciones solo se cuentan: son tus archivos.

Una vez al día Glut limpia solo al arrancar — aquí y en `glut run` — y dice lo que liberó. La salida de compilación y las cachés (una carpeta con un CACHEDIR.TAG, bajo node_modules/ o el target/ de un proyecto Cargo) no tienen instantáneas.

Los plazos se guardan en `~/.glut/clean.json`. En la línea de comandos: `glut clean`, `glut clean --dry-run`, `glut clean keep sessions 30d`; `glut clean --dry-run --json` escribe el informe para un programa.
