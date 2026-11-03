Elige cómo una sesión en reposo mantiene la caché de prompts del proveedor, para que el próximo turno empiece caliente.

/cache ping — Glut vuelve a leer la caché cada pocos minutos mientras la sesión descansa, hasta 30 minutos
/cache 1h — Anthropic la guarda una hora (cada escritura cuesta el doble); en otros proveedores se lee para mantenerla
/cache off — no se mantiene: una pausa de más de los pocos minutos del proveedor empieza en frío

/cache solo muestra la elección actual; ping es el valor por defecto. Se guarda en `~/.glut/prefs.json`.
