Deja que este pane haga de un rol: el prompt y las herramientas de un agente con nombre, nunca su modelo.

/role — los roles para elegir
/role <name> — hace de ese rol desde ahora
/role none — de nuevo un todoterreno, con todas las herramientas

Glut trae reviewer, tester y docs; tus propios roles viven en `~/.glut/agents`, los de un proyecto en su `.glut/agents` (se leen solo en una carpeta de confianza). Las herramientas de un rol rigen de inmediato, su prompt desde el próximo turno. Un rol que restringe las herramientas necesita el Gate.
