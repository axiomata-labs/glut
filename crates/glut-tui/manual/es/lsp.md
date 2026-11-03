Si los servidores de lenguaje ayudan al modelo.

/lsp on — tras una edición, los errores del archivo llegan con el resultado, y el modelo tiene la herramienta lsp: dónde se define un símbolo, cada lugar donde se usa, su tipo y su documentación, y un renombre en todo el proyecto
/lsp off — ningún servidor corre: sus procesos terminan al instante, sin errores tras una edición, sin herramienta lsp
/lsp — la elección actual, y cada servidor de la sesión: listo, indexando el proyecto, o apagado para la sesión

Glut solo inicia servidores que nunca ejecutan el código del proyecto (rust-analyzer sin scripts de build ni macros procedurales, pyright y los servidores de TOML, YAML, JSON, CSS, HTML y Markdown), solo en el sandbox sin red, y solo donde el Gate permite su línea de comando. Un servidor arranca con la primera edición o pregunta de su lenguaje y se queda para la sesión; rust-analyzer tarda unos segundos en indexar un proyecto grande, mientras lsp lo dice y el modelo usa grep. Un renombre es un solo cambio: se pregunta una vez por todos sus archivos, todos o ninguno, con una instantánea antes. Se guarda en `~/.glut/prefs.json`; `"lsp": false` en `~/.glut/settings.json` lo apaga para cada ejecución, `glut run --no-lsp` para una.
