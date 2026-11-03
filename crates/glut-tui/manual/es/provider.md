Cambia a otro proveedor de modelos.

/provider — los proveedores que configuraste
/provider <id> — cambia a ese, con su modelo por defecto
/provider key [id] — cambia solo la clave de ese proveedor (sin id, la del proveedor por defecto)

Los proveedores se guardan en `~/.glut/providers.json`, sus claves en el Keychain (o en `~/.glut/credentials.json`); `glut providers add <id>` añade uno. La conversación sigue, y el próximo arranque empieza con el proveedor que elegiste.

La clave se escribe oculta y se prueba con la lista de modelos del proveedor antes de reemplazar la antigua; si falla, eliges si se guarda de todos modos, y si no la antigua se queda. La sesión toma una clave guardada al instante. Un proveedor que inicia sesión, no necesita clave o la toma de una variable de entorno te lo dice. En el shell: `glut providers key <id>`.
