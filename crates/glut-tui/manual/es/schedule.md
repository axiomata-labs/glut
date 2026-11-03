Planes que se ejecutan con Glut cerrado: una tarea a una hora del día o cada cierto tiempo.

/schedule — la pestaña Ejecuciones (⌃X R): cada plan con su próxima hora y su último resultado
/schedule list — los planes en el flujo
/schedule add <name> [flags] <task> — crea un plan, o cambia el de ese nombre
/schedule run <name> — lo ejecuta ahora, junto a Glut
/schedule pause <name> — lo retiene hasta resume
/schedule resume <name> — vuelve a ejecutarse a sus horas
/schedule remove <name> — el plan y su registro se van; sus sesiones se quedan

Los flags son los de `glut schedule add`:
- `--at 07:00` una hora del día, con `--days mo-fr` o `sa,su` (cada día si no se da)
- `--every 2h` un intervalo de 10m a 7d, en lugar de --at
- `--mode ask|build|plan|dont_ask|auto` lo que puede hacer sin una regla; ask si no se nombra
- `--allow <rule>`, `--deny <rule>` reglas en la forma de Claude Code, como `Bash(cargo test:*)`; repetible
- `--folder <path>` una carpeta junto a la suya; repetible
- `--max-usd 1`, `--max-minutes 30` lo máximo que una ejecución puede costar y durar: 1 $ y 30 min si no se nombran
- `--max-tokens 1M` en una suscripción, el máximo de tokens que puede usar una ejecución: 1M si no se indica
- `--replace` las reglas y carpetas dadas ocupan el lugar de las del plan en lugar de sumarse a ellas
- `--cwd <path>`, `--provider <id>`, `--model <id>` su carpeta (esta si no), su proveedor y su modelo, fijados al crearlo

La tarea es un mensaje, o /skill <name> [what for]. launchd inicia cada ejecución, solo mientras tienes la sesión abierta; una hora perdida durmiendo se recupera una vez al despertar. No hay nadie para responder: toda pregunta se rechaza y se anota, así que permite lo que necesite. Una ejecución es una sesión guardada propia, avisada con una notificación de macOS. En la pestaña Ejecuciones: ⏎ abre un plan o la sesión de una ejecución, r lo ejecuta ahora, p lo pausa o lo reanuda, d lo elimina.

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" ejecuta los tests e informa de lo que falle
