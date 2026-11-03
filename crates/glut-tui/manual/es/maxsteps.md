Cuántos pasos da como máximo un turno.

/maxsteps — el límite actual
/maxsteps <n> — un turno se detiene tras n pasos, de 1 a 1000

Un paso es una llamada al modelo. En el límite el turno se detiene y lo dice; ⏎ con la entrada vacía envía «Go on where you stopped.» y el trabajo sigue. Los presupuestos y el límite de tiempo siguen siendo los frenos reales. Guardado en `~/.glut/prefs.json`; `maxTurns` en `~/.glut/settings.json` lo fija para cada ejecución, `glut run --max-turns` para una.
