Se os servidores de linguagem ajudam o modelo.

/lsp on — após uma edição, os erros do arquivo vêm com o resultado, e o modelo tem a ferramenta lsp: onde um símbolo é definido, cada lugar onde é usado, seu tipo e sua documentação, e uma renomeação em todo o projeto
/lsp off — nenhum servidor roda: seus processos terminam na hora, sem erros após uma edição, sem ferramenta lsp
/lsp — a escolha atual, e cada servidor da sessão: pronto, indexando o projeto, ou desligado na sessão

O Glut só inicia servidores que nunca executam o código do projeto (rust-analyzer sem scripts de build nem macros procedurais, pyright e os servidores de TOML, YAML, JSON, CSS, HTML e Markdown), só no sandbox sem rede, e só onde o Gate permite sua linha de comando. Um servidor inicia na primeira edição ou pergunta da sua linguagem e fica para a sessão; o rust-analyzer leva alguns segundos para indexar um projeto grande, enquanto isso lsp o diz e o modelo usa grep. Uma renomeação é uma só mudança: perguntada uma vez para todos os seus arquivos, todos ou nenhum, com um snapshot antes. Guardado em `~/.glut/prefs.json`; `"lsp": false` em `~/.glut/settings.json` desliga para cada execução, `glut run --no-lsp` para uma.
