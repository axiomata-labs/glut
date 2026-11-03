Iniciar sesión con una suscripción en lugar de una clave — hoy tu plan ChatGPT Plus o Pro.

/login — las suscripciones, cada una con su estado; ⏎ inicia o cierra la sesión
/login <provider> — inicia sesión en esa al momento, como /login chatgpt
/login <provider> <address> — por SSH: la dirección en la que terminó el navegador, pegada de vuelta
/login openrouter — OpenRouter le da a Glut una clave propia, guardada como una escrita; limítala o bórrala en openrouter.ai/settings/keys

Primero se muestra la dirección; ⏎ la abre en tu navegador. Glut guarda la sesión en el llavero (o en `~/.glut/credentials.json`) y la renueva solo; cerrar sesión la revoca en OpenAI.

Con el plan, una llamada no cuesta dinero, pero rigen los límites de uso del plan: cuando se alcanza uno, el turno termina y Glut remite a Ajustes de ChatGPT → Uso, donde también limitas lo que puede usar cada app. En la shell: glut providers login|logout <provider>.
