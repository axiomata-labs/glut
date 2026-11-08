Escribir a las otras ventanas de Glut de esta máquina.

/sessions — sus panes y agentes: dirección, proyecto, modelo, modo, trabajando o esperando; ⏎ pone /send <address> en la entrada
/sessions off — esta ventana no acepta sus mensajes
/sessions on — y de nuevo

Un mensaje del propietario de allí entra enseguida; el de un modelo de allí solo si trabaja en el modo del pane al que llega, si no espera sobre la entrada: ⏎ lo deja entrar, Esc lo descarta. Ninguno es tu palabra al modelo de aquí. Solo Glut habla con las otras ventanas — un comando del modelo nunca las alcanza.

Los modelos también se ven entre sí: su herramienta `who` lista cada agente aquí y en las otras ventanas — rol, modelo, carpeta, rama, ocupado o libre — y la línea que cada uno puso sobre lo que hace, con sus propias palabras.

Un modelo puede pedirle algo a otro, aquí o en otra ventana: el turno que su petición inicia allí le responde — la última respuesta de ese turno vuelve como respuesta, mientras sigue con otro trabajo. Un agente ocupado la toma en cuanto termina su turno; uno que no puede hacerlo lo dice. Si nadie va a responder — apartaste la petición con Esc, el pane se cerró, la ventana terminó — Glut se lo dice a quien pidió.

Lo ves todo en la pestaña Equipo (⌃X T): cada agente aquí y en las otras ventanas — ocupado, libre o esperándote, su rama y su línea — y lo que pasó entre ellos: mensajes, peticiones, respuestas, y quién llegó y se fue.

Donde dos agentes se estorban, Glut se lo dice: un agente trabajando en el mismo árbol de trabajo y en la misma rama que otro que trabaja allí, o uno que cambia un archivo que otro cambió en el último cuarto de hora, lo oye una vez antes de su siguiente paso — y tú ves la nota en su flujo y en la pestaña Equipo.

Un agente que iniciaste puede dirigir: con `start_helpers` inicia sus propios ayudantes en segundo plano — cada uno con un rol, un modelo, su primera tarea, en su carpeta o en una copia del repositorio propia — y sus respuestas vuelven a él. Un ayudante tiene el modo y las reglas «siempre» de su líder, más estrechos según su rol, y no inicia ayudantes. En modo build dices sí una vez para todo el equipo; en auto arrancan enseguida; en ask y plan el líder solo los propone. Con `take_work` el líder trae el trabajo de un ayudante desde su copia — comprobado como el ganador de la arena, sin commit — y lo cierra.
