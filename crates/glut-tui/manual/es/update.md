Si Glut busca una vez al día una versión más nueva y la muestra en el pie.

/update on — una vez al día, en segundo plano, Glut lee el número de la versión más nueva de `https://glut-ai.com/latest.txt`; si es más nueva que esta, el pie la muestra discretamente (`↑ 0.1.2`)
/update off — Glut no pregunta nada y el pie no muestra nada

/update solo dice si hay un Glut más nuevo y el comando que actualiza este, según cómo se instaló (`brew upgrade glut`, `cargo install glut-tui` o el script de instalación). Glut nunca se actualiza solo y nunca pregunta en la ejecución de un plan, una ronda de un bucle, `glut run`, `glut acp` o la arena. Guardado en `~/.glut/prefs.json`; `"updateCheck": false` en `~/.glut/settings.json` lo apaga donde /update no dijo otra cosa.
