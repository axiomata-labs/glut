Tu editor junto a Glut: Neovim con el plugin glut.nvim, VS Code (y Cursor, Windsurf) o un IDE de JetBrains con el plugin propio de Glut.

/ide — a qué editor está conectado Glut y cuáles encajan
/ide on — conectar (pregunta si encajan varios)
/ide off — desconectar en esta sesión

Conectado, cada mensaje tuyo lleva el archivo activo y la selección (⧉ debajo), mientras ninguna regla de lectura retenga el archivo; el modelo puede preguntar qué archivos están abiertos y qué dicen los servidores de lenguaje; Glut lee un búfer como lo tiene el editor, también lo no guardado, y una pregunta del gate sobre un cambio aparece también como diff en el editor — cuenta la primera respuesta. Glut se conecta solo al arrancar donde encaja un editor: el que arrancó Glut, si no aquel cuya carpeta contiene la de Glut. "⧉" en el pie dice que está conectado.
