Mira cuán llena está la ventana del modelo, o dile a Glut qué tamaño tiene.

/context — los tokens usados, la ventana y de dónde viene su tamaño, y lo que cuestan en cada turno las guías (`AGENTS.md`, `CLAUDE.md`)
/context <tokens> — fija la ventana de este modelo: 65536, 64k o 1M

Una ventana que fijas se guarda en `~/.glut/providers.json` para este modelo y cuenta antes que la lista del proveedor. Glut compacta por sí mismo poco antes de que la ventana se llene.
