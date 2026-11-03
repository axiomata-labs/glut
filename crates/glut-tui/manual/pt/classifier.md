Deixa um segundo modelo julgar os comandos sobre os quais só o modo te perguntaria.

/classifier — que modelo julga, ou que nenhum o faz
/classifier session — julga o modelo da sessão (enquanto não definires nenhum)
/classifier <provider> <model> — um modelo próprio, basta um rápido (openrouter deepseek/deepseek-v4.1-flash)
/classifier off — nenhum julga: és perguntado sempre

O Glut conhece muitos comandos que apenas leem (ls, grep, git status, …) e executa-os em todos os modos. Um ciclo ou um pipeline deles (for f in *.md; do grep -c x "$f"; done) não o consegue julgar só pelo texto, por isso pergunta. O segundo modelo vê esse comando e a sua pasta — nunca a conversa, um ficheiro ou uma saída — e diz READS ou CHANGES. Só com READS o comando executa como um comando de leitura conhecido, na sandbox; tudo o resto, ou nenhuma resposta clara em segundos, pergunta-te como antes, com a sua opinião sob o comando. Uma regra tua, um comando pesado, o porta-chaves e a sandbox ficam como estão; o mesmo comando não é julgado duas vezes numa sessão.

Um comando que deixou passar indica-o ao lado. A escolha fica em `~/.glut/classifier.json` e vale a partir do próximo turno; `glut run --classifier` define-a para uma execução.
