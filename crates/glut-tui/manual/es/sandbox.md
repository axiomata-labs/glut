Cómo se sujetan los comandos que Glut ejecuta, y si pueden escribir en las carpetas git.

/sandbox — el sandbox aquí (macOS: Seatbelt; Linux: bubblewrap y seccomp) y el interruptor de git
/sandbox git read-only — ningún comando escribe en una carpeta git: ninguno hace commit desde el sandbox
/sandbox git writable — los comandos pueden volver a escribir en las carpetas git, también hacer commit
/sandbox required on — ningún comando se ejecuta donde no hay sandbox (falta bubblewrap, un contenedor sin el derecho); off los deja ejecutarse, sujetos solo por las reglas

El sandbox deja que un comando escriba solo en tus carpetas, lea todo salvo tus secretos y los archivos .env, y llegue a la red solo por el proxy de Glut. En Linux no puede negar un nombre que aún no existe: lo que un comando crea y macOS niega — un .envrc nuevo, un .git, un .git/commondir — va a ~/.glut/quarantine en cuanto aparece, y otra vez tras el comando. Una herramienta tuya que se ejecute justo en ese momento aún podría encontrarlo; con las carpetas git de solo lectura, no se crea ninguno en ellas.

El interruptor vale en cada sesión y ventana desde el próximo comando, y se guarda en ~/.glut/sandbox.json — nunca lo escribe un comando. glut sandbox show y glut sandbox git read-only|writable hacen lo mismo fuera de Glut.
