//! Español.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "idioma: {name}",
    language_unkept: "idioma: {name} — solo para esta ejecución: {reason}",
    language_unknown: "no hay idioma {name} — {known}",
    language_title: "Idioma de Glut",
    language_now: "actual",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
    topics: &[
        "Sesión",
        "Modelo",
        "Vista",
        "Panes",
        "Cambios",
        "Proyecto",
        "Ajustes",
        "Entrada",
        "Editar la línea",
        "Ayuda",
    ],
    commands: &[
        ("clear", "empezar una sesión nueva"),
        (
            "resume",
            "abrir una sesión anterior de esta carpeta — las bifurcaciones bajo la suya",
        ),
        (
            "projects",
            "seguir en otro proyecto: las carpetas en las que trabajó Glut, o una por su ruta",
        ),
        ("compact", "resumir la conversación hasta ahora"),
        (
            "handoff",
            "/handoff write: lo hecho y lo que viene después; /handoff read en una sesión nueva",
        ),
        ("quit", "terminar la sesión"),
        ("exit", "terminar la sesión, como /quit"),
        (
            "model",
            "elegir un modelo del proveedor en uso, o /model <id>",
        ),
        ("provider", "cambiar de proveedor, o /provider <id>"),
        ("effort", "cuánto piensa el modelo, o /effort <level>"),
        (
            "context",
            "cuán llena está la ventana; /context 64k la ajusta",
        ),
        (
            "thinking",
            "el razonamiento del modelo: /thinking off, folded u open",
        ),
        (
            "panel",
            "el panel lateral con el plan, u ocultarlo (⌃B; ⌃G el gate)",
        ),
        (
            "meter",
            "contexto, tokens y coste por turno y herramienta, el ahorro de lean (⌃N)",
        ),
        (
            "agents",
            "los subagentes de la sesión: abrir el flujo de uno (también un clic en su ficha; ⌃X A: la pestaña \
             Agentes)",
        ),
        (
            "plan",
            "el plan que Glut envió desde el modo plan, entero, con su lista de tareas",
        ),
        (
            "scrollback",
            "la conversación al scrollback propio del terminal (F3)",
        ),
        (
            "split",
            "una sesión nueva junto al pane principal (hasta seis, dos filas de tres); en un repositorio git aquí o \
             en un worktree",
        ),
        (
            "swap",
            "el pane principal y el pane n (o este) intercambian sus lugares",
        ),
        (
            "close",
            "cerrar este pane (no el principal) — o terminar un agente en segundo plano; su sesión queda para /resume",
        ),
        (
            "send",
            "un mensaje a otro pane (primero su número, a partir de tres), recibido allí como si se escribiera; \
             /send plan le pasa tu plan para construir",
        ),
        (
            "role",
            "el rol de este pane: el prompt y las herramientas de un agente con nombre, no su modelo — reviewer, \
             tester, docs o los tuyos",
        ),
        (
            "bg",
            "un agente en segundo plano: una sesión sin pane, alcanzable por su nombre — /bg as reviewer",
        ),
        (
            "show",
            "traer un agente del segundo plano al frente como pane",
        ),
        (
            "hide",
            "mandar este pane al segundo plano, con su memoria (⌃X H)",
        ),
        (
            "peek",
            "leer el flujo de un agente en segundo plano sobre este pane; Esc lo cierra",
        ),
        (
            "diff",
            "revisar lo que cambió el último turno, hunk por hunk; /diff session (⌃R)",
        ),
        (
            "steps",
            "los pasos que cambiaron archivos: qué cambió cada uno, o los archivos como estaban",
        ),
        (
            "todo",
            "el TODO.md del proyecto (⌃Y): /todo <text> añade uno, /todo open lo edita, /todo clean quita los \
             completados",
        ),
        (
            "memory",
            "lo que guarda la memoria del proyecto: ⏎ muestra un hecho; /memory open edita su índice",
        ),
        (
            "skill",
            "iniciar un skill: /skill <name> [para qué] — /skill solo los lista (make-agent, make-skill, make-hook, …)",
        ),
        (
            "init",
            "un AGENTS.md breve para esta carpeta, o revisar su guía (/init --migrate vuelve a preguntar por \
             CLAUDE.md)",
        ),
        (
            "gate",
            "las reglas vigentes; retirar un «siempre» (⌃G: el gate, en vivo)",
        ),
        (
            "images",
            "cómo se dibujan las imágenes: la real donde el terminal puede, medios bloques en color real, o solo su \
             línea",
        ),
        (
            "links",
            "el texto de un enlace es el enlace donde el terminal habla OSC 8, si no la URL escrita detrás",
        ),
        (
            "motion",
            "cuánto se mueve la pantalla; calm y off mantienen quieto a Glimm («Reducir movimiento» de macOS cuenta \
             como off)",
        ),
        (
            "reader",
            "para un lector de pantalla: teclas en palabras, una pantalla sencilla, nada que se mueva, una línea en \
             cada cambio de estado",
        ),
        (
            "language",
            "el idioma en que Glut te habla: en de fr it es zh ja pt — el modelo responde en el que escribas",
        ),
        (
            "theme",
            "los colores de Glut, o los de un tema de Axiomata: /theme <name>",
        ),
        ("editor", "elegir tu editor, o /editor <command>"),
        (
            "cache",
            "cómo una sesión en reposo guarda su caché: volver a leer (ping), la hora de Anthropic (1h), o nada",
        ),
        (
            "footer",
            "las uniones del pie de página: flechas de Powerline o bloques simples (deducidos del terminal)",
        ),
        ("vim", "editar la entrada al estilo de vi: /vim on u off"),
        ("help", "comandos y teclas"),
    ],
    keys: &[
        ("⇧⇥", "el modo siguiente: plan → ask → build → auto"),
        ("⎋", "detener a Glut"),
        (
            "⏎",
            "mientras Glut trabaja: guiarlo, recibido antes de su siguiente paso",
        ),
        (
            "⇥",
            "mientras Glut trabaja: poner el mensaje en cola para después",
        ),
        ("⌥↑", "recuperar el último mensaje en espera"),
        ("⌃C ⌃C", "salir"),
        ("⌃F", "buscar en el flujo"),
        ("⌃O", "desplegar o plegar cada resultado de herramienta"),
        ("⌃T", "desplegar o plegar cada razonamiento"),
        ("⌃B", "el panel lateral con el plan, u ocultarlo"),
        (
            "⌃G",
            "el gate, en vivo: cada juicio, los hosts, los rechazos",
        ),
        (
            "⌃N",
            "el contador: contexto, tokens y coste por turno y herramienta",
        ),
        (
            "⌃X A",
            "la pestaña Agentes: cada subagente y los agentes en segundo plano, en vivo",
        ),
        (
            "⌃X ↑",
            "leer el flujo bloque a bloque: ⏎ pliega, c copia, ⎋ vuelve",
        ),
        (
            "⌃X ⇥",
            "el panel lateral toma las teclas: ←→ las pestañas, ↑↓ dentro, ⎋ vuelve",
        ),
        ("PgUp PgDn", "desplazar el flujo (⇧↑ ⇧↓ una línea)"),
        ("F3", "la conversación al scrollback del terminal"),
        ("F2", "dar el ratón al terminal y recuperarlo"),
        (
            "⌃P",
            "el pane siguiente, en orden de lectura (también ⌃]: ⌃6 en un teclado alemán)",
        ),
        ("⌃X 2", "un pane nuevo junto al principal (también ⌃X 3)"),
        (
            "⌃X }",
            "el pane principal más ancho (el ratón también arrastra su borde)",
        ),
        ("⌃X {", "el pane principal más estrecho"),
        ("⌃X 0", "cerrar este pane"),
        ("⌃X B", "un agente en segundo plano, eligiendo antes su rol"),
        ("⌃X H", "este pane al segundo plano"),
        ("⌃R", "revisar lo que cambió el último turno, hunk por hunk"),
        ("⎋⎋", "retroceder o bifurcar a antes de un mensaje tuyo"),
        ("⌃Y", "la pestaña Tareas: Espacio marca, a añade"),
        ("/", "los comandos"),
        ("@", "mencionar un archivo"),
        (
            "⌃V",
            "la imagen del portapapeles al mensaje (también un archivo de imagen arrastrado)",
        ),
        ("↑", "los mensajes enviados antes"),
        ("⇧⏎", "una línea nueva (también ⌥⏎)"),
        ("⌃X E", "editar la entrada en tu editor"),
        (
            "⌃A ⌃E",
            "el inicio de la línea, su final (también Inicio Fin)",
        ),
        (
            "⌥B ⌥F",
            "una palabra a la izquierda, una a la derecha (también ⌥← ⌥→)",
        ),
        ("⌃W", "borrar la palabra anterior (también ⌥⌫)"),
        ("⌃U", "borrar hasta el inicio de la línea"),
        ("⌃J", "una línea nueva"),
        ("⌃D", "salir, en una línea vacía mientras Glut descansa"),
        ("⌃K", "esta hoja de teclas"),
    ],
    help_commands: "Comandos",
    help_keys: "Teclas ({legend}; ⌃K las deja en el panel lateral)",
    help_also: "Además: !cmd ejecuta un comando y muestra su salida a Glut · !!cmd lo ejecuta solo para ti · un \
                pegado grande aparece como [pasted N lines] · arrastrar copia · un clic en la ficha de un subagente \
                lo abre",
    help_vim: "/vim on: ⎋ al modo Normal (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y con un movimiento o \
               dobles · números · p P · u ⌃R), i a I A o O para Insertar; ⎋ en modo Normal detiene a Glut",
    legend: "⌃ Control · ⇧ Mayúsculas · ⌥ Opción · ⏎ Retorno · ⇥ Tabulador · ⎋ Escape",
};
