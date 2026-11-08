Escribir a las otras ventanas de Glut de esta máquina.

/sessions — sus panes y agentes: dirección, proyecto, modelo, modo, trabajando o esperando; ⏎ pone /send <address> en la entrada
/sessions off — esta ventana no acepta sus mensajes
/sessions on — y de nuevo

Un mensaje del propietario de allí entra enseguida; el de un modelo de allí solo si trabaja en el modo del pane al que llega, si no espera sobre la entrada: ⏎ lo deja entrar, Esc lo descarta. Ninguno es tu palabra al modelo de aquí. Solo Glut habla con las otras ventanas — un comando del modelo nunca las alcanza.

Los modelos también se ven entre sí: su herramienta `who` lista cada agente aquí y en las otras ventanas — rol, modelo, carpeta, rama, ocupado o libre — y la línea que cada uno puso sobre lo que hace, con sus propias palabras.

Un modelo puede pedirle algo a otro, aquí o en otra ventana: el turno que su petición inicia allí le responde — la última respuesta de ese turno vuelve como respuesta, mientras sigue con otro trabajo. Un agente ocupado la toma en cuanto termina su turno; uno que no puede hacerlo lo dice. Si nadie va a responder — apartaste la petición con Esc, el pane se cerró, la ventana terminó — Glut se lo dice a quien pidió.
