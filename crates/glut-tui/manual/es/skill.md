Inicia un Skill: instrucciones para un tipo de tarea, que el modelo carga y sigue.

/skill — los Skills para elegir
/skill <name> [what for] — ejecuta ese Skill sobre lo que sigue a su nombre

Glut trae make-agent, make-skill y make-hook para escribir los tuyos. Los tuyos viven en `~/.glut/skills`, los de un proyecto en su `.glut/skills` (se leen solo en una carpeta de confianza); uno con el mismo nombre oculta el de Glut.

/skill make-hook bloquea git push en main
