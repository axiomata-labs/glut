Se o Glut procura uma vez por dia uma versão mais nova e a mostra no rodapé.

/update on — uma vez por dia, em segundo plano, o Glut lê o número da versão mais nova em `https://glut-ai.com/latest.txt`; se for mais nova que esta, o rodapé a mostra discretamente (`↑ 0.2.0`)
/update off — o Glut não pergunta nada, e o rodapé não mostra nada

/update sozinho diz se há um Glut mais novo e o comando que atualiza este, conforme foi instalado (`brew upgrade glut`, `cargo install glut-tui` ou o script de instalação). O Glut nunca se atualiza sozinho e nunca pergunta na execução de um plano, numa rodada de um loop, em `glut run`, `glut acp` ou na arena. Guardado em `~/.glut/prefs.json`; `"updateCheck": false` em `~/.glut/settings.json` desliga onde /update não disse outra coisa.
