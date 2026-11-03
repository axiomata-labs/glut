Resume la conversación hasta ahora, para que ocupe menos de la ventana del modelo.

/compact — el modelo escribe un resumen que ocupa el lugar de los mensajes más antiguos

El prompt del sistema, tu último mensaje y los pasos completos se quedan; lo que el resumen sustituye se guarda en el almacén de resultados de la sesión, nunca se tira. Glut también compacta por sí mismo poco antes de que la ventana se llene (`compact` en `~/.glut/settings.json`). /context muestra cuán llena está la ventana.
