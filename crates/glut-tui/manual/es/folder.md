Da a esta sesión carpetas junto a la de trabajo, para que Glut pueda trabajar entre proyectos.

/folder — las carpetas junto a la de trabajo
/folder add <path> — añade una para esta sesión
/folder remove <path> — quita una

Las herramientas de archivos alcanzan una carpeta así por su ruta completa, los comandos pueden escribir allí dentro del sandbox, el Gate la juzga como la carpeta de trabajo y deshacer también devuelve sus archivos. Solo tú añades una carpeta, nunca un modelo, un Skill ni un proyecto. Glut rechaza la carpeta personal, una carpeta que contiene el proyecto o queda dentro de él, una con secretos y dos carpetas una dentro de la otra.

Las carpetas valen solo para esta sesión; `glut run --folder <path>` da una a una ejecución sin interfaz.
