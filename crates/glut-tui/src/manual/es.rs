//! The commands' manual pages in Spanish (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 65] = [
    (
        "clear",
        "Empieza una sesión nueva y vacía en esta carpeta.\n\
         \n\
         /clear — la conversación, la lista de tareas del plan, el coste y el contador empiezan desde cero\n\
         \n\
         La sesión que dejas se conserva: /resume la abre de nuevo. Tu modelo, tu modo y tus ajustes siguen como \
         están.",
    ),
    (
        "resume",
        "Abre una sesión anterior de esta carpeta y sigue con ella.\n\
         \n\
         /resume — la lista de las sesiones guardadas de esta carpeta, la más reciente primero; una bifurcación \
         aparece bajo la sesión de la que viene\n\
         \n\
         En la lista: escribe para filtrar, ⏎ abre una, ⌫ borra una (antes pregunta; sus bifurcaciones se quedan como \
         sesiones propias), ⌃E la escribe como Markdown en `~/.glut/exports`. A una sesión abierta en otro pane se \
         llega desde allí (⌃P).\n\
         \n\
         /search encuentra una sesión por lo que se dijo en ella.",
    ),
    (
        "search",
        "Encuentra una sesión guardada por sus palabras: la lista de /resume, filtrada por el texto de su \
         conversación.\n\
         \n\
         /search <text> — las sesiones de esta carpeta cuya conversación contiene el texto\n\
         \n\
         En la lista, ⌃A alterna entre esta carpeta y todos los proyectos. ⏎ abre una sesión de esta carpeta; una de \
         otro proyecto nombra su carpeta: inicia Glut allí para seguir con ella.",
    ),
    (
        "btw",
        "Haz una pregunta al margen sin molestar a la conversación: la respuesta llega en una ventana sobre la \
         entrada y nunca pasa a formar parte de la sesión.\n\
         \n\
         /btw <question> — pregunta sobre la sesión de este pane, incluso mientras su turno sigue\n\
         /btw <name> <question> — pregunta sobre otro pane o un agente en segundo plano, por su número o su nombre\n\
         /btw — vuelve a abrir la última respuesta\n\
         \n\
         El modelo responde con la conversación tal como está y no ejecuta ninguna herramienta: si necesitara \
         consultar algo, hazle la pregunta como mensaje. Su coste cuenta en el coste de la sesión. ⎋ cierra la \
         ventana, PgUp PgDn la desplazan.\n\
         \n\
         /btw reviewer ¿cómo vas?",
    ),
    (
        "projects",
        "Sigue en otro proyecto: una de las carpetas en las que trabajó Glut, o cualquier carpeta por su ruta.\n\
         \n\
         /projects — la lista: ★ fijado, ● este; ⏎ abre uno, p lo fija o lo suelta, x lo oculta hasta que se vuelva a \
         trabajar en él\n\
         /projects <path> — abre esa carpeta (⇥ completa el nombre de una carpeta)\n\
         \n\
         Mientras un turno o un agente sigue trabajando, Glut pregunta antes de detenerlos y cambiar. Las sesiones y \
         los panes de esta ventana se conservan. En una ventana abierta por una aplicación anfitriona, la carpeta la \
         elige ella.",
    ),
    (
        "folder",
        "Da a esta sesión carpetas junto a la de trabajo, para que Glut pueda trabajar entre proyectos.\n\
         \n\
         /folder — las carpetas junto a la de trabajo\n\
         /folder add <path> — añade una para esta sesión\n\
         /folder remove <path> — quita una\n\
         \n\
         Las herramientas de archivos alcanzan una carpeta así por su ruta completa, los comandos pueden escribir allí \
         dentro del sandbox, el Gate la juzga como la carpeta de trabajo y deshacer también devuelve sus archivos. \
         Solo tú añades una carpeta, nunca un modelo, un Skill ni un proyecto. Glut rechaza la carpeta personal, una \
         carpeta que contiene el proyecto o queda dentro de él, una con secretos y dos carpetas una dentro de la \
         otra.\n\
         \n\
         Las carpetas valen solo para esta sesión; `glut run --folder <path>` da una a una ejecución sin interfaz.",
    ),
    (
        "compact",
        "Resume la conversación hasta ahora, para que ocupe menos de la ventana del modelo.\n\
         \n\
         /compact — el modelo escribe un resumen que ocupa el lugar de los mensajes más antiguos\n\
         \n\
         El prompt del sistema, tu último mensaje y los pasos completos se quedan; lo que el resumen sustituye se \
         guarda en el almacén de resultados de la sesión, nunca se tira. Glut también compacta por sí mismo poco antes \
         de que la ventana se llene (`compact` en `~/.glut/settings.json`). /context muestra cuán llena está la \
         ventana.",
    ),
    (
        "handoff",
        "Pasa el trabajo a una sesión posterior de esta carpeta.\n\
         \n\
         /handoff write — el modelo escribe lo hecho, cómo están las cosas y lo que viene después\n\
         /handoff read — toma el relevo de la carpeta en esta sesión\n\
         \n\
         Un relevo se guarda en `~/.glut/handoffs`, uno por carpeta. Una vez leído se aparta, de modo que ninguna \
         sesión posterior lo lee dos veces. Escribir uno es una llamada de pago aparte; la conversación se queda como \
         está.",
    ),
    (
        "export",
        "Escribe la sesión para leerla o compartirla.\n\
         \n\
         /export — Markdown en `~/.glut/exports`\n\
         /export html — un solo archivo HTML, con sus estilos dentro y sin script\n\
         /export jsonl — los mensajes como líneas JSON\n\
         /export copy — el Markdown al portapapeles en lugar de a un archivo\n\
         /export full — añade los resultados completos de las herramientas y el prompt del sistema\n\
         /export <format> <path> — a un archivo tuyo (`~/` es tu carpeta personal, una ruta relativa queda en la \
         carpeta de trabajo)\n\
         \n\
         Las palabras van en cualquier orden, la ruta al final. Sin full la exportación muestra lo que vio el modelo: \
         los resultados largos, acortados. Un archivo existente nunca se sobrescribe. Las claves que Glut ocultó \
         siguen ocultas, pero lo que escribiste tú mismo aún puede contener una: lee una exportación antes de \
         compartirla. En /resume, ⌃E exporta una sesión de la lista.",
    ),
    (
        "quit",
        "Termina Glut, con todos los panes y agentes en segundo plano.\n\
         \n\
         /quit — mientras otro pane o agente sigue trabajando, Glut lo avisa primero: /quit otra vez termina también \
         ese trabajo\n\
         \n\
         ⌃C ⌃C hace lo mismo, y ⌃D en una línea vacía mientras Glut descansa. Las sesiones se conservan para /resume.",
    ),
    ("exit", "Termina Glut, como hace /quit."),
    (
        "model",
        "Elige el modelo con el que habla esta sesión.\n\
         \n\
         /model — los modelos del proveedor en uso, de su lista guardada\n\
         /model <id> — cambia a ese modelo por su id\n\
         \n\
         La conversación sigue con el modelo nuevo, y el próximo arranque empieza con él. Un modelo de otro proveedor \
         se elige después de /provider. `glut models refresh` renueva las listas.",
    ),
    (
        "provider",
        "Cambia a otro proveedor de modelos.\n\
         \n\
         /provider — los proveedores que configuraste\n\
         /provider <id> — cambia a ese, con su modelo por defecto\n\
         /provider key [id] — cambia solo la clave de ese proveedor (sin id, la del proveedor por defecto)\n\
         \n\
         Los proveedores se guardan en `~/.glut/providers.json`, sus claves en el Keychain (o en \
         `~/.glut/credentials.json`); `glut providers add <id>` añade uno. La conversación sigue, y el próximo \
         arranque empieza con el proveedor que elegiste.\n\
         \n\
         La clave se escribe oculta y se prueba con la lista de modelos del proveedor antes de reemplazar la antigua; \
         si falla, eliges si se guarda de todos modos, y si no la antigua se queda. La sesión toma una clave guardada \
         al instante. Un proveedor que inicia sesión, no necesita clave o la toma de una variable de entorno te lo \
         dice. En el shell: `glut providers key <id>`.",
    ),
    (
        "login",
        "Iniciar sesión con una suscripción en lugar de una clave — hoy tu plan ChatGPT Plus o Pro.\n\
         \n\
         /login — las suscripciones, cada una con su estado; ⏎ inicia o cierra la sesión\n\
         /login <provider> — inicia sesión en esa al momento, como /login chatgpt\n\
         /login <provider> <address> — por SSH: la dirección en la que terminó el navegador, pegada de vuelta\n\
         /login openrouter — OpenRouter le da a Glut una clave propia, guardada como una escrita; limítala o bórrala \
         en openrouter.ai/settings/keys\n\
         \n\
         Primero se muestra la dirección; ⏎ la abre en tu navegador. Glut guarda la sesión en el llavero (o en \
         `~/.glut/credentials.json`) y la renueva solo; cerrar sesión la revoca en OpenAI.\n\
         \n\
         Con el plan, una llamada no cuesta dinero, pero rigen los límites de uso del plan: cuando se alcanza uno, el \
         turno termina y Glut remite a Ajustes de ChatGPT → Uso, donde también limitas lo que puede usar cada app. En \
         la shell: glut providers login|logout <provider>.",
    ),
    (
        "effort",
        "Ajusta cuánto piensa el modelo antes de responder.\n\
         \n\
         /effort — los niveles para elegir\n\
         /effort <level> — none, minimal, low, medium, high, xhigh o max\n\
         /effort default — decide el proveedor\n\
         \n\
         Más esfuerzo suele significar mejores respuestas en trabajo difícil, más lentas y de mayor coste. No todos \
         los modelos conocen todos los niveles. El contador (⌃N) muestra el nivel en uso.",
    ),
    (
        "route",
        "Qué hosts sirven el modelo en OpenRouter (otros sirven cada modelo desde un solo host).\n\
         \n\
         /route — el orden que Glut envía ahora, cada host con su motivo, precio y velocidad\n\
         /route prefer <hosts> — estos primero, en este orden (xiaomi deepinfra)\n\
         /route avoid <hosts> — estos nunca\n\
         /route quant <levels> — solo estas cuantizaciones (fp8 bf16)\n\
         /route strict on|off — solo los hosts preferidos, ningún otro de respaldo\n\
         /route off — las fijaciones de este modelo se van\n\
         /route rule on|off — la regla para todos los modelos\n\
         /route rule margin|uptime|speed|wait <n> — sus números\n\
         \n\
         OpenRouter sirve un modelo desde muchos hosts, con cuantizaciones, precios y velocidades distintos. Glut lee \
         los hosts de un modelo una vez al día y los ordena según su regla: precisión completa (bf16, fp16) antes que \
         fp8 salvo que cueste más de un 40 % más, un host que no indica cuantización entre fp8 y fp4 — el del \
         fabricante del modelo como completo —, fp4 solo si nadie más lo sirve; un host con menos de 95 % de \
         disponibilidad, menos de 30 tokens por segundo o más de 10 segundos hasta el primer token va al final. El \
         orden va con cada petición con respaldo permitido, así que ninguna petición falla por la regla. Las \
         fijaciones de un modelo van antes que la regla. Todo se guarda en `~/.glut/providers.json`.",
    ),
    (
        "context",
        "Mira cuán llena está la ventana del modelo, o dile a Glut qué tamaño tiene.\n\
         \n\
         /context — los tokens usados, la ventana y de dónde viene su tamaño, y lo que cuestan en cada turno las guías \
         (`AGENTS.md`, `CLAUDE.md`)\n\
         /context <tokens> — fija la ventana de este modelo: 65536, 64k o 1M\n\
         \n\
         Una ventana que fijas se guarda en `~/.glut/providers.json` para este modelo y cuenta antes que la lista del \
         proveedor. Glut compacta por sí mismo poco antes de que la ventana se llene.",
    ),
    (
        "thinking",
        "Elige cómo se muestra el razonamiento del modelo en el flujo.\n\
         \n\
         /thinking off — oculto\n\
         /thinking folded — una línea por pensamiento; un clic abre uno\n\
         /thinking open — cada pensamiento mostrado entero\n\
         \n\
         /thinking solo los pliega. ⌃T abre o pliega todos los pensamientos a la vez. Tu elección se guarda en \
         `~/.glut/prefs.json`.",
    ),
    (
        "panel",
        "Muestra el panel lateral en su pestaña Plan, u ocúltalo.\n\
         \n\
         /panel — el panel lateral con sus pestañas: Plan, Gate, Contador, Agentes, Pendientes, Ejecuciones y Teclas\n\
         \n\
         ⌃B hace lo mismo, ⌃X ⇥ pasa las teclas al panel (←→ las pestañas, ↑↓ dentro, ⎋ vuelve). El panel necesita una \
         ventana de al menos 100 columnas de ancho. Que se muestre o no se guarda para el próximo arranque.",
    ),
    (
        "meter",
        "Mira lo que usa la sesión: el contexto, los tokens y el coste.\n\
         \n\
         /meter — la pestaña Contador del panel lateral\n\
         \n\
         Muestra cuán llena está la ventana y cuándo compacta Glut, los tokens de la sesión de entrada, en caché y de \
         salida con su precio, cada turno y cada herramienta, y lo que ahorró la salida lean de Glut. ⌃N también la \
         abre.",
    ),
    (
        "agents",
        "Mira los subagentes que Glut inició en esta sesión.\n\
         \n\
         /agents — la lista de subagentes, cada uno bajo el que lo inició; ⏎ abre el flujo propio de uno\n\
         \n\
         Un clic en la ficha de un subagente también lo abre. ⌃X A abre la pestaña Agentes: cada subagente y cada \
         agente en segundo plano, en vivo — ⏎ abre uno, x lo detiene.",
    ),
    (
        "plan",
        "Lee entero el plan que Glut envió desde el modo plan, con su lista de tareas.\n\
         \n\
         /plan — el último plan de esta sesión\n\
         \n\
         En el modo plan (⇧⇥) Glut lee y planifica sin cambiar nada, y luego envía su plan para que lo apruebes o lo \
         devuelvas. /send plan se lo pasa a otro pane para que construya.",
    ),
    (
        "scrollback",
        "Pasa la conversación al scrollback propio del terminal, para desplazarla, buscarla y copiarla con las \
         herramientas del terminal.\n\
         \n\
         /scrollback — imprime la conversación; ⏎ o ⌃C vuelve a Glut\n\
         \n\
         F3 hace lo mismo.",
    ),
    (
        "split",
        "Abre una sesión nueva en un pane junto al principal.\n\
         \n\
         /split — un pane nuevo; en un repositorio git Glut pregunta dónde trabaja\n\
         /split here — trabaja en esta carpeta, a tu lado\n\
         /split tree [branch] — trabaja en un Worktree nuevo, en una rama propia\n\
         /split resume — abre una sesión anterior, elegida de la lista\n\
         /split as <role> — hace de un rol (/role los lista)\n\
         \n\
         Hasta seis panes quedan junto al principal, en dos filas de tres. ⌃P recorre los panes, ⌃X 2 también divide, \
         ⌃X } y ⌃X { hacen el pane principal más ancho o más estrecho. Dos panes aquí cambian los mismos archivos; un \
         Worktree los mantiene separados hasta que /close pregunta cómo termina.\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "swap",
        "Deja que el pane principal y otro intercambien sus lugares.\n\
         \n\
         /swap — este pane pasa a ser el principal\n\
         /swap <n> — el pane n pasa a ser el principal",
    ),
    (
        "close",
        "Cierra un pane, o termina un agente en segundo plano.\n\
         \n\
         /close — cierra este pane (nunca el principal); mientras trabaja, un segundo /close lo detiene y lo cierra\n\
         /close <name> — termina ese agente en segundo plano, con su turno detenido\n\
         /close <n> — cierra el pane n desde donde estás, por su número, nombre o rol; te quedas en tu pane\n\
         /close solo, si hay más que este pane para cerrar — una lista, este pane primero: ⏎ cierra el elegido\n\
         \n\
         La sesión se conserva para /resume. Un pane en un Worktree pregunta cómo termina el Worktree: conservarlo con \
         su rama, asumirlo (fusionarlo en la rama en la que estás) o descartarlo. ⌃X 0 también cierra este pane.",
    ),
    (
        "send",
        "Envía un mensaje a otro pane o agente en segundo plano, recibido allí como si lo hubieras escrito.\n\
         \n\
         /send — una lista de a quién escribir: los panes y agentes de aquí, luego las otras ventanas de Glut; ⏎ pone \
         /send <who> en la entrada\n\
         /send <text> — al único otro pane\n\
         /send <n|name> <text> — al pane n o al agente de ese nombre (necesario con tres o más)\n\
         /send plan — pasa el plan de este pane al otro pane para que construya\n\
         /send <n|name> plan — se lo pasa a ese\n\
         \n\
         El mensaje empieza de inmediato donde la sesión descansa, y si no, tras su turno. Las teclas se quedan en \
         este pane.",
    ),
    (
        "role",
        "Deja que este pane haga de un rol: el prompt y las herramientas de un agente con nombre, nunca su modelo.\n\
         \n\
         /role — los roles para elegir\n\
         /role <name> — hace de ese rol desde ahora\n\
         /role none — de nuevo un todoterreno, con todas las herramientas\n\
         \n\
         Glut trae reviewer, tester y docs; tus propios roles viven en `~/.glut/agents`, los de un proyecto en su \
         `.glut/agents` (se leen solo en una carpeta de confianza). Las herramientas de un rol rigen de inmediato, su \
         prompt desde el próximo turno. Un rol que restringe las herramientas necesita el Gate.",
    ),
    (
        "bg",
        "Inicia un agente en segundo plano: una sesión sin pane, alcanzable por su nombre.\n\
         \n\
         /bg — un agente llamado agent (o agent-2, …); en un repositorio git Glut pregunta dónde trabaja\n\
         /bg <name> — un agente con ese nombre (letras minúsculas, dígitos y guiones)\n\
         /bg as <role> — hace de un rol y se llama como él\n\
         /bg here — trabaja en esta carpeta\n\
         /bg tree [branch] — trabaja en un Worktree nuevo\n\
         \n\
         /send <name> le da trabajo, /peek <name> lee su flujo, /show <name> lo trae al frente, /close <name> lo \
         termina. Cada agente trabaja y cuesta por su cuenta: Glut pregunta una vez por ventana antes de un séptimo. \
         ⌃X B inicia uno con su rol elegido antes, ⌃X A los lista.\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "Trae un agente en segundo plano al frente como pane.\n\
         \n\
         /show <name> — el agente pasa a ser el último pane de la cuadrícula\n\
         \n\
         Cuando la ventana ya no tiene sitio, antes haz /hide o /close a un pane. ⌃X A lista los agentes en segundo \
         plano.",
    ),
    (
        "hide",
        "Manda este pane al segundo plano, con su memoria.\n\
         \n\
         /hide — el pane sale de la pantalla y sigue trabajando como agente en segundo plano\n\
         \n\
         Conserva un nombre por el que alcanzarlo (su rol, o pane-<n>): /show <name> lo trae de vuelta, /send <name> y \
         /peek <name> lo alcanzan. El pane principal y el último se quedan. ⌃X H hace lo mismo.",
    ),
    (
        "peek",
        "Lee el flujo de un agente en segundo plano sobre este pane, sin traerlo al frente.\n\
         \n\
         /peek <name> — su flujo, en vivo, solo para leer\n\
         /peek — lo cierra de nuevo\n\
         \n\
         ⎋ también lo cierra; /show <name> trae el agente al frente.",
    ),
    (
        "loop",
        "Ejecuta una tarea una y otra vez en un agente en segundo plano propio, mientras Glut está abierto.\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — inicia un bucle; la primera ronda de inmediato\n\
         /loop — lista los bucles: intervalo, rondas, la próxima ronda y el coste hasta ahora\n\
         /loop stop [name] — termina ese bucle, o el único\n\
         /loop rounds [n] — las rondas máximas de un bucle; solo, muestra el límite actual\n\
         /loop budget [usd] — lo máximo que puede costar un bucle; solo, muestra el límite actual\n\
         /loop tokens [n] — el máximo de tokens que puede usar un bucle en una suscripción (5M, 500k); solo, muestra \
         el límite actual\n\
         \n\
         Las rondas nunca se solapan: el intervalo se cuenta desde el fin de una ronda, como 30s, 5m, 2h o 1h30m (de \
         10s a 24h). Una ronda sigue con la conversación de la ronda anterior; con fresh cada ronda empieza una nueva. \
         La tarea es un mensaje, o /skill <name> [what for], /compact o /init — ningún otro comando.\n\
         \n\
         El agente se llama loop (o como su rol) y toma el modo de este pane, el «siempre» que diste aquí, sus \
         carpetas junto a la de trabajo y su rol; as <role> solo puede nombrar el rol que hace este pane. Sus \
         preguntas esperan a que respondas. Una ficha aparece cuando una ronda pregunta o el bucle termina; con \
         notify, tras cada ronda.\n\
         \n\
         Un bucle termina tras sus rondas o en cuanto sus rondas cuestan su presupuesto (por defecto 50 rondas y 5 $, \
         y si no `loop.rounds` y `loop.maxUsd` de `~/.glut/settings.json`; un cambio cuenta para los bucles iniciados \
         después), cuando una ronda falla o tú lo detienes (⎋ en su ronda), o cuando su modelo llama a `end_loop` \
         porque se alcanzó el final que nombró tu tarea. Cuando los precios del modelo son desconocidos, solo las \
         rondas lo terminan. Su agente se queda después en la pestaña Agentes, para leerlo o hacerle /close. x en su \
         tarjeta de la pestaña Agentes (⌃X A) termina el bucle.\n\
         \n\
         /loop 10m ejecuta los tests y corrige lo que falle; para cuando todos pasen\n\
         /loop 1h fresh as reviewer notify revisa el último commit",
    ),
    (
        "schedule",
        "Planes que se ejecutan con Glut cerrado: una tarea a una hora del día o cada cierto tiempo.\n\
         \n\
         /schedule — la pestaña Ejecuciones (⌃X R): cada plan con su próxima hora y su último resultado\n\
         /schedule list — los planes en el flujo\n\
         /schedule add <name> [flags] <task> — crea un plan, o cambia el de ese nombre\n\
         /schedule run <name> — lo ejecuta ahora, junto a Glut\n\
         /schedule pause <name> — lo retiene hasta resume\n\
         /schedule resume <name> — vuelve a ejecutarse a sus horas\n\
         /schedule remove <name> — el plan y su registro se van; sus sesiones se quedan\n\
         \n\
         Los flags son los de `glut schedule add`:\n\
         - `--at 07:00` una hora del día, con `--days mo-fr` o `sa,su` (cada día si no se da)\n\
         - `--every 2h` un intervalo de 10m a 7d, en lugar de --at\n\
         - `--mode ask|build|plan|dont_ask|auto` lo que puede hacer sin una regla; ask si no se nombra\n\
         - `--allow <rule>`, `--deny <rule>` reglas en la forma de Claude Code, como `Bash(cargo test:*)`; repetible\n\
         - `--folder <path>` una carpeta junto a la suya; repetible\n\
         - `--max-usd 1`, `--max-minutes 30` lo máximo que una ejecución puede costar y durar: 1 $ y 30 min si no se \
         nombran\n\
         - `--max-tokens 1M` en una suscripción, el máximo de tokens que puede usar una ejecución: 1M si no se indica\n\
         - `--replace` las reglas y carpetas dadas ocupan el lugar de las del plan en lugar de sumarse a ellas\n\
         - `--cwd <path>`, `--provider <id>`, `--model <id>` su carpeta (esta si no), su proveedor y su modelo, \
         fijados al crearlo\n\
         \n\
         La tarea es un mensaje, o /skill <name> [what for]. launchd inicia cada ejecución, solo mientras tienes la \
         sesión abierta; una hora perdida durmiendo se recupera una vez al despertar. No hay nadie para responder: \
         toda pregunta se rechaza y se anota, así que permite lo que necesite. Una ejecución es una sesión guardada \
         propia, avisada con una notificación de macOS. En la pestaña Ejecuciones: ⏎ abre un plan o la sesión de una \
         ejecución, r lo ejecuta ahora, p lo pausa o lo reanuda, d lo elimina.\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" ejecuta los tests e informa de lo \
         que falle",
    ),
    (
        "sessions",
        "Escribir a las otras ventanas de Glut de esta máquina.\n\n/sessions — sus panes y agentes: dirección, \
        proyecto, modelo, modo, trabajando o esperando; ⏎ pone /send <address> en la entrada\n/sessions off — esta \
        ventana no acepta sus mensajes\n/sessions on — y de nuevo\n\nUn mensaje del propietario de allí entra \
        enseguida; el de un modelo de allí solo si trabaja en el modo del pane al que llega, si no espera sobre la \
        entrada: ⏎ lo deja entrar, Esc lo descarta. Ninguno es tu palabra al modelo de aquí. Solo Glut habla con las \
        otras ventanas — un comando del modelo nunca las alcanza.",
    ),
    (
        "name",
        "Dar nombre a esta ventana entre las ventanas de Glut de esta máquina.\n\n/name — el nombre que tiene \
        ahora\n/name <new> — uno nuevo: letras, dígitos, - _ y .\n\nLas otras ventanas llegan a esta por su nombre, a \
        un pane por name/2 o name/reviewer. Un nombre que das se guarda con la sesión y vuelve con /resume.",
    ),
    (
        "diff",
        "Revisa lo que cambió el último turno, hunk por hunk.\n\
         \n\
         /diff — los cambios del último turno que cambió archivos\n\
         /diff session — cada cambio de esta sesión\n\
         \n\
         En la revisión: ↑↓ un hunk, ←→ un archivo, a acepta, r rechaza (el hunk vuelve atrás, y Glut lee por qué con \
         tu próximo mensaje), c comenta (se envía con tu próximo mensaje), v marca un archivo como revisado, s alterna \
         entre turno y sesión, ⎋ cierra. ⌃R también la abre; /review es el mismo comando.",
    ),
    (
        "steps",
        "Mira los pasos que cambiaron archivos, y devuelve los archivos a como estaban.\n\
         \n\
         /steps — cada paso que cambió un archivo; ⏎ sobre uno muestra lo que cambió, o devuelve los archivos a como \
         estaban antes de él\n\
         \n\
         Devolver los archivos se lleva consigo tus ediciones posteriores; la conversación se queda. Glut guarda una \
         instantánea antes de cada paso que cambia algo; lo que los archivos eran justo antes también se guarda, así \
         que `glut undo` lo trae de vuelta. ⎋⎋ retrocede la conversación en cambio.",
    ),
    (
        "todo",
        "Guarda los pendientes del proyecto en su `TODO.md`, una lista de comprobación que usáis tú y Glut.\n\
         \n\
         /todo — la pestaña Pendientes: Espacio marca un elemento, a añade uno, o abre el editor\n\
         /todo <text> — añade un elemento al final de la lista, también mientras Glut trabaja\n\
         /todo open — la lista en tu editor\n\
         /todo clean — quita los elementos hechos, preguntando antes\n\
         \n\
         El resto del archivo nunca se toca. ⌃Y muestra u oculta la pestaña.",
    ),
    (
        "memory",
        "Mira lo que guarda la memoria del proyecto: los hechos que Glut anotó para sesiones posteriores.\n\
         \n\
         /memory — los hechos con sus descripciones; ⏎ muestra uno entero\n\
         /memory open — el índice (`MEMORY.md`) en tu editor\n\
         \n\
         La memoria queda fuera del repositorio: en `~/.glut/memory/<project>/`, o en la memoria de Claude Code allí \
         donde Claude Code trabaja en el repositorio. Glut la escribe con su herramienta `memory`; leerla aquí no \
         consulta a ningún modelo.",
    ),
    (
        "skill",
        "Inicia un Skill: instrucciones para un tipo de tarea, que el modelo carga y sigue.\n\
         \n\
         /skill — los Skills para elegir\n\
         /skill <name> [what for] — ejecuta ese Skill sobre lo que sigue a su nombre\n\
         \n\
         Glut trae make-agent, make-skill y make-hook para escribir los tuyos. Los tuyos viven en `~/.glut/skills`, \
         los de un proyecto en su `.glut/skills` (se leen solo en una carpeta de confianza); uno con el mismo nombre \
         oculta el de Glut.\n\
         \n\
         /skill make-hook bloquea git push en main",
    ),
    (
        "init",
        "Escribe un `AGENTS.md` breve para esta carpeta, o revisa y acorta la guía que ya tiene.\n\
         \n\
         /init — sin guía, Glut escribe `AGENTS.md`; con una, la revisa y la mantiene breve\n\
         /init --migrate — vuelve a preguntar si `CLAUDE.md` pasa a `AGENTS.md`\n\
         \n\
         Con solo un `CLAUDE.md`, Glut pregunta una vez si pasa a `AGENTS.md` (`CLAUDE.md` lo importa entonces, una \
         guía para todos los harness); un no se recuerda. Las guías van en cada turno: /context muestra lo que \
         cuestan.",
    ),
    (
        "gate",
        "Mira las reglas por las que decide el Gate, y retira un «siempre».\n\
         \n\
         /gate — las reglas vigentes: tus «siempre» de esta sesión, luego las de `~/.glut/settings.json` y las del \
         `.glut/` del proyecto; ⏎ sobre un «siempre» lo retira, y Glut vuelve a preguntar la próxima vez\n\
         \n\
         ⌃G abre la pestaña Gate: cada juicio en vivo, los hosts alcanzados y los rechazos. ⇧⇥ cambia el modo.",
    ),
    (
        "mcp",
        "Mira los servidores MCP y cómo está cada uno, inicia sesión en uno alojado, añade o quita uno.\n\
         \n\
         /mcp — cada servidor: conectado con sus herramientas, esperando un inicio de sesión, o fallido\n\
         /mcp login <name> — inicia sesión en un servidor alojado; su dirección se abre en tu navegador con ⏎\n\
         /mcp login <name> <address> — por SSH: la dirección en la que acabó el navegador, pegada de vuelta\n\
         /mcp logout <name> — cierra la sesión y olvida su token\n\
         /mcp add <name> <url> — añade un servidor alojado a `~/.glut/mcp.json` y lo conecta\n\
         /mcp add <name> -- <command> [args] — añade un servidor que Glut inicia él mismo\n\
         /mcp remove <name> — lo quita de `~/.glut/mcp.json` y de la sesión\n\
         \n\
         Un servidor que no ofrece un inicio de sesión que Glut pueda usar pide un token, escrito oculto. Los \
         servidores propios de un proyecto los cambia el proyecto: /mcp remove solo quita los tuyos. `glut mcp` hace \
         lo mismo fuera de una sesión.\n\
         \n\
         Mientras una de sus herramientas trabaja, un servidor puede preguntarte: un formulario (⇥ ↑↓ entre los \
         campos, espacio marca, ⏎ envía, Esc cierra, ⌃D rechaza) o una página que abrir (o). Solo tú respondes, nunca \
         el modelo; un formulario que pide una contraseña o una clave se rechaza, y sin nadie presente toda pregunta \
         se rechaza.",
    ),
    (
        "images",
        "Elige cómo se dibujan las imágenes en el flujo.\n\
         \n\
         /images inline — la imagen real donde el terminal puede mostrarla\n\
         /images blocks — medios bloques en color real\n\
         /images off — solo la línea de la imagen\n\
         \n\
         /images solo muestra la elección actual. Por defecto Glut dibuja en línea donde el terminal puede y en \
         bloques en el resto. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "links",
        "Elige cómo se muestran los enlaces en el flujo.\n\
         \n\
         /links clickable — el texto del enlace es el enlace, donde el terminal habla OSC 8\n\
         /links plain — la URL se escribe detrás del texto\n\
         \n\
         /links solo muestra la elección actual. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "notify",
        "Recibe un aviso cuando un turno termina o Glut pregunta, mientras la ventana no está al frente.\n\
         \n\
         /notify on — el fin de un turno y cada pregunta\n\
         /notify asks — solo las preguntas\n\
         /notify off — nada\n\
         /notify system — con las notificaciones propias de macOS (un clic en una abre Script Editor, no el \
         terminal)\n\
         /notify after <s> — el fin de un turno se avisa solo si duró tantos segundos (10 por defecto, hasta 3600)\n\
         /notify tiles on|off — las fichas arriba a la derecha de la ventana\n\
         \n\
         /notify solo muestra lo que rige ahora. Glut usa las notificaciones propias del terminal donde sabe que las \
         tiene, y la campana. Una notificación nombra a Glut, el proyecto, el pane y lo que pasó, nunca las palabras \
         del modelo. Se guarda en `~/.glut/prefs.json`, por encima de `notify` en `~/.glut/settings.json`.",
    ),
    (
        "secrets",
        "Mira y ajusta las claves ocultas al modelo en esta sesión.\n\
         \n\
         /secrets — cada valor oculto hasta ahora, por número, tipo, longitud y dónde se vio (nunca el valor)\n\
         /secrets allow <n> — deja pasar el secreto n durante esta sesión\n\
         /secrets allow <n> always — lo deja pasar siempre (se guarda su huella, nunca el valor)\n\
         /secrets add <name> <regex> — oculta un formato tuyo, desde el próximo resultado\n\
         /secrets remove <name> — quita un formato tuyo\n\
         /secrets on|off — el enmascarado activado o desactivado\n\
         \n\
         Una clave en lo que responde una herramienta llega al modelo como ‹secret n: …›; Glut devuelve su valor solo \
         a un archivo que ya lo contiene. De un mensaje tuyo que parezca contener una clave se pregunta antes de \
         enviarlo. El modelo se entera de un cambio con tu próximo mensaje. Tus formatos y elecciones se guardan en \
         `~/.glut/secrets.json`.\n\
         \n\
         El enmascarado protege contra una clave mostrada por accidente; el sandbox y el Gate de red siguen siendo las \
         barreras.",
    ),
    (
        "keychain",
        "Mantener el llavero fuera del alcance de los comandos que Glut ejecuta.\n\
         \n\
         /keychain — si está abierto, y los programas que pueden pedirlo\n\
         /keychain add <program> — pone un programa en la lista (gh, o git y un subcomando: git push)\n\
         /keychain remove <program> — lo quita de la lista\n\
         /keychain open — lo abre a todo comando, hasta que termine esta sesión\n\
         /keychain close — lo vuelve a cerrar\n\
         \n\
         Una clave o un inicio de sesión del llavero se alcanza más allá de toda regla de archivos del sandbox, así \
         que un comando que Glut ejecuta encuentra el llavero cerrado. Un comando de un programa de la lista pregunta \
         primero — en todo modo, también en `auto` — y se ejecuta con el llavero abierto en cuanto lo permites; un \
         «siempre» lo convierte en una regla tuya. Sin nadie presente (un plan, `glut run`) solo una regla \
         así lo abre. Cuando un comando falla por el llavero cerrado, Glut lo dice bajo su salida.\n\
         \n\
         Permitir un comando da el llavero también a todo lo que inicia: un hook de git, una extensión de gh o un \
         script de compilación puede leer entonces cualquier clave en él.\n\
         \n\
         Solo tus propias reglas dan el llavero: una regla de permiso del `.glut/` de un proyecto deja ejecutar un \
         comando, nunca con el llavero abierto. Una cadena lo obtiene solo si cada parte está en la lista o solo lee. \
         Lo que `glut keychain open` mantiene abierto solo cuenta aquí, donde estás tú — nunca en `glut run` ni en la \
         ejecución de un anfitrión.\n\
         \n\
         Una regla tuya lo abre sin preguntar solo si nombra un programa y su subcomando (`gh pr *`, `git push *`) — \
         nunca un programa entero (`gh *`). Una tarjeta 🔑 muestra el comando entero, ↑↓ para recorrerlo, y solo \
         permite cuando lo has leído hasta el final.\n\
         \n\
         La lista se guarda en `~/.glut/keychain.json`; `glut keychain open` mantiene el llavero abierto de una \
         sesión a otra, `glut keychain close` lo cierra para siempre.",
    ),
    (
        "classifier",
        "Deja que un segundo modelo juzgue los comandos por los que solo el modo te preguntaría.\n\
         \n\
         /classifier — qué modelo juzga, o que ninguno lo hace\n\
         /classifier session — juzga el modelo de la sesión (mientras no fijes ninguno)\n\
         /classifier <provider> <model> — un modelo propio, basta uno rápido (openrouter deepseek/deepseek-v4.1-flash)\n\
         /classifier off — ninguno juzga: se te pregunta cada vez\n\
         \n\
         Glut conoce muchos comandos que solo leen (ls, grep, git status, …) y los ejecuta en todos los modos. Un bucle o una tubería de ellos (for f in *.md; do grep -c x \"$f\"; done) no puede juzgarlo solo por su texto, así que pregunta. El segundo modelo ve ese comando y su carpeta — nunca la conversación, un archivo ni una salida — y dice READS o CHANGES. Solo con READS el comando se ejecuta como un comando de lectura conocido, en el sandbox; cualquier otra cosa, o ninguna respuesta clara en segundos, te pregunta como antes, con su opinión bajo el comando. Una regla tuya, un comando pesado, el llavero y el sandbox quedan como están; el mismo comando no se juzga dos veces en una sesión.\n\
         \n\
         Un comando que dejó pasar lo indica a su lado. La elección se guarda en `~/.glut/classifier.json` y vale desde el próximo turno; `glut run --classifier` la fija para una ejecución.",
    ),
    (
        "ide",
        "Tu editor junto a Glut: Neovim con el plugin glut.nvim (vendrán otros).\n\
         \n\
         /ide — a qué editor está conectado Glut y cuáles encajan\n\
         /ide on — conectar (pregunta si encajan varios)\n\
         /ide off — desconectar en esta sesión\n\
         \n\
         Conectado, cada mensaje tuyo lleva el archivo activo y la selección (⧉ debajo), mientras ninguna regla de \
         lectura retenga el archivo; el modelo puede preguntar qué archivos están abiertos y qué dicen los servidores \
         de lenguaje; Glut lee un búfer como lo tiene el editor, también lo no guardado, y una pregunta del gate \
         sobre un cambio aparece también como diff en el editor — cuenta la primera respuesta. Glut se conecta solo \
         al arrancar donde encaja un editor: el que arrancó Glut, si no aquel cuya carpeta contiene la de Glut. \"⧉\" \
         en el pie dice que está conectado.",
    ),
    (
        "clean",
        "Evitar que los datos de Glut en ~/.glut crezcan sin fin.\n\
         \n\
         /clean — lo que Glut guarda, partida por partida, y lo que puede irse según tus plazos\n\
         /clean now — limpia ya, en segundo plano\n\
         /clean keep — los plazos\n\
         /clean keep <kind> <days>d — fija uno: snapshots, journals, sessions, results o lean\n\
         \n\
         Lo que se va: las instantáneas de una ejecución más antiguas que su plazo (7 días) — el almacén entero de \
         una carpeta cuando no queda ninguna, esté la carpeta o no; el diario de una ejecución sin terminar (7 días); \
         una sesión sin escribir desde hace 90 días, con su registro de eventos y sus resultados largos; los \
         resultados largos sin sesión tras 30 días; las imágenes que ninguna sesión guardada nombra; las líneas de \
         lean.jsonl de más de 90 días. Las exportaciones solo se cuentan: son tus archivos.\n\
         \n\
         Una vez al día Glut limpia solo al arrancar — aquí y en `glut run` — y dice lo que liberó. La salida de \
         compilación y las cachés (una carpeta con un CACHEDIR.TAG, bajo node_modules/ o el target/ de un proyecto \
         Cargo) no tienen instantáneas.\n\
         \n\
         Los plazos se guardan en `~/.glut/clean.json`. En la línea de comandos: `glut clean`, `glut clean \
         --dry-run`, `glut clean keep sessions 30d`; `glut clean --dry-run --json` escribe el informe para un \
         programa.",
    ),
    (
        "motion",
        "Elige cuánto se mueve la pantalla.\n\
         \n\
         /motion full — Glimm respira, suben chispas, la llama parpadea\n\
         /motion calm — todo se queda quieto; se dibuja un cuadro solo cuando algo cambió\n\
         /motion off — como calm\n\
         \n\
         /motion solo muestra la elección actual. «Reducir movimiento» de macOS y el modo lector mantienen quieta la \
         pantalla elijas lo que elijas. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "rule",
        "Una regla para esta sesión que el modelo sigue en cada turno.\n\
         \n\
         /rule <text> — guardarla, como «actualiza la lista de tareas en cada paso»\n\
         /rule — las reglas actuales, numeradas\n\
         /rule remove <n> — retirar la regla n\n\
         \n\
         Las reglas van con las instrucciones de cada turno, tras las guías, hasta que las retires; /clear las \
         conserva, una sesión nueva empieza sin ellas. Como máximo 20, de 500 caracteres cada una. Para una regla de \
         cada sesión, escríbela en `~/.glut/AGENTS.md` o en la guía del proyecto.",
    ),
    (
        "maxsteps",
        "Cuántos pasos da como máximo un turno.\n\
         \n\
         /maxsteps — el límite actual\n\
         /maxsteps <n> — un turno se detiene tras n pasos, de 1 a 1000\n\
         \n\
         Un paso es una llamada al modelo. En el límite el turno se detiene y lo dice; ⏎ con la entrada vacía envía \
         «Go on where you stopped.» y el trabajo sigue. Los presupuestos y el límite de tiempo siguen siendo los \
         frenos reales. Guardado en `~/.glut/prefs.json`; `maxTurns` en `~/.glut/settings.json` lo fija para cada \
         ejecución, `glut run --max-turns` para una.",
    ),
    (
        "nudge",
        "Si Glut pide al modelo que siga al final de un turno, que se ciña a su lista de tareas y que escriba una \
         cuando trabaja sin ninguna.\n\
         \n\
         /nudge on — cuando un turno termina con puntos de la lista de tareas abiertos, o con palabras que anuncian un \
         paso («a continuación…», dos puntos al final), Glut envía al modelo una línea propia: seguir o decir por qué \
         para; el historial lo muestra discretamente. Si la lista no cambia mientras trabaja, Glut le recuerda, y si \
         eso no cambia nada, una vez con un mensaje propio; si en una ejecución no ha escrito ninguna, le pide una, \
         una vez por turno\n\
         /nudge off — un turno termina donde el modelo lo termina: sin recordatorio de la lista ni petición de una\n\
         \n\
         La pregunta del final viene una vez por turno, nunca en modo plan, tras una pregunta a ti, en una ronda de un \
         bucle o en la ejecución de un plan; el recordatorio y la petición siguen la misma elección, pero en la \
         ejecución de un plan Glut siempre recuerda. /nudge solo muestra la elección actual. Guardado en \
         `~/.glut/prefs.json`; `\"nudge\": false` en `~/.glut/settings.json` lo apaga para cada ejecución.",
    ),
    (
        "lsp",
        "Si los servidores de lenguaje ayudan al modelo.\n\
         \n\
         /lsp on — tras una edición, los errores del archivo llegan con el resultado, y el modelo tiene la herramienta \
         lsp: dónde se define un símbolo, cada lugar donde se usa, su tipo y su documentación, y un renombre en todo \
         el proyecto\n\
         /lsp off — ningún servidor corre: sus procesos terminan al instante, sin errores tras una edición, sin \
         herramienta lsp\n\
         /lsp — la elección actual, y cada servidor de la sesión: listo, indexando el proyecto, o apagado para la \
         sesión\n\
         \n\
         Glut solo inicia servidores que nunca ejecutan el código del proyecto (rust-analyzer sin scripts de build ni \
         macros procedurales, pyright y los servidores de TOML, YAML, JSON, CSS, HTML y Markdown), solo en el sandbox \
         sin red, y solo donde el Gate permite su línea de comando. Un servidor arranca con la primera edición o \
         pregunta de su lenguaje y se queda para la sesión; rust-analyzer tarda unos segundos en indexar un proyecto \
         grande, mientras lsp lo dice y el modelo usa grep. Un renombre es un solo cambio: se pregunta una vez por \
         todos sus archivos, todos o ninguno, con una instantánea antes. Se guarda en `~/.glut/prefs.json`; `\"lsp\": \
         false` en `~/.glut/settings.json` lo apaga para cada ejecución, `glut run --no-lsp` para una.",
    ),
    (
        "reader",
        "Prepara Glut para un lector de pantalla.\n\
         \n\
         /reader on — teclas en palabras, un pie de página sencillo, sin Glimm, marcos simples, nada que se mueva y \
         una línea de texto en cada cambio de estado\n\
         /reader off — la pantalla habitual\n\
         \n\
         /reader solo muestra la elección actual. Se guarda en `~/.glut/prefs.json`; `glut --reader` lo mantiene \
         activado durante una ejecución.",
    ),
    (
        "language",
        "Elige el idioma en que Glut te habla.\n\
         \n\
         /language — los idiomas para elegir\n\
         /language <code> — en, de, fr, it, es, zh, ja o pt\n\
         \n\
         Sin una elección Glut habla el idioma de tu sistema. El modelo responde en el idioma en que escribas, elijas \
         lo que elijas aquí. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "theme",
        "Elige los colores de Glut.\n\
         \n\
         /theme — los temas para elegir\n\
         /theme <name> — glut, el propio de Glut, o uno de los temas de Axiomata (catppuccin-mocha, github-dark, …)\n\
         \n\
         Glut colorea solo el texto y mantiene el fondo del terminal: un tema claro quiere un terminal claro. Se \
         guarda en `~/.glut/prefs.json`; en una ventana abierta por una aplicación anfitriona, la elección vale para \
         esa sesión.\n\
         \n\
         Cada tema da a las partes del pie, a los nombres de las herramientas según su tipo (leer, escribir, \
         comandos, lo que sale), a los panes y agentes y a la línea sobre la entrada sus propios colores suaves; lo \
         que te necesita sigue vivo. El modo lectura y `NO_COLOR` lo mantienen sobrio.",
    ),
    (
        "editor",
        "Elige el editor en el que Glut abre archivos y la entrada.\n\
         \n\
         /editor — los editores que se encuentran en tu sistema\n\
         /editor <command> — tu propio comando, con `{file}` y `{line}` donde van\n\
         \n\
         Sin `{file}` el archivo se añade al final. Un editor con ventana debe esperar hasta que se cierre el archivo \
         (`code --wait`). ⌃X E edita la entrada en él; /todo open y /memory open también lo usan. Se guarda en \
         `~/.glut/prefs.json`.\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "Elige cómo una sesión en reposo mantiene la caché de prompts del proveedor, para que el próximo turno empiece \
         caliente.\n\
         \n\
         /cache ping — Glut vuelve a leer la caché cada pocos minutos mientras la sesión descansa, hasta 30 minutos\n\
         /cache 1h — Anthropic la guarda una hora (cada escritura cuesta el doble); en otros proveedores se lee para \
         mantenerla\n\
         /cache off — no se mantiene: una pausa de más de los pocos minutos del proveedor empieza en frío\n\
         \n\
         /cache solo muestra la elección actual; ping es el valor por defecto. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "footer",
        "Elige cómo une el pie de página sus partes.\n\
         \n\
         /footer powerline — las flechas de Powerline\n\
         /footer plain — bloques simples\n\
         \n\
         /footer solo muestra la elección actual y el porqué. Sin una elección Glut deduce del terminal si dibuja los \
         glifos de Powerline. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "vim",
        "Edita la entrada al estilo de vi.\n\
         \n\
         /vim on — ⎋ pasa al modo Normal (movimientos, operadores, contadores, u y ⌃R), i a I A o O vuelven a \
         Insertar\n\
         /vim off — las teclas de edición habituales\n\
         \n\
         /vim solo lo activa. ⎋ en el modo Normal detiene a Glut. Se guarda en `~/.glut/prefs.json`.",
    ),
    (
        "help",
        "Cada comando y cada tecla, en una caja propia.\n\
         \n\
         /help — los comandos con una línea cada uno; escribir los filtra, ↑↓ eligen, ⏎ abre la página entera de uno\n\
         /help <command> — la página de ese comando de inmediato, como /help loop\n\
         \n\
         En una página ↑↓ y PgUp PgDn desplazan, ⎋ vuelve a la lista y desde ella cierra la caja. La última entrada \
         contiene cada tecla por tema, !cmd y el modo vi; ⌃K deja las teclas en el panel lateral.",
    ),
];
