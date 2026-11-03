Pasa el trabajo a una sesión posterior de esta carpeta.

/handoff write — el modelo escribe lo hecho, cómo están las cosas y lo que viene después
/handoff read — toma el relevo de la carpeta en esta sesión

Un relevo se guarda en `~/.glut/handoffs`, uno por carpeta. Una vez leído se aparta, de modo que ninguna sesión posterior lo lee dos veces. Escribir uno es una llamada de pago aparte; la conversación se queda como está.
