Escolhe o editor em que o Glut abre arquivos e a entrada.

/editor — os editores encontrados no seu sistema
/editor <command> — o seu próprio comando, com `{file}` e `{line}` onde eles entram

Sem `{file}`, o arquivo é acrescentado ao fim. Um editor com janela precisa esperar até o arquivo ser fechado (`code --wait`). ⌃X E edita a entrada nele; /todo open e /memory open também o usam. Guardado em `~/.glut/prefs.json`.

/editor nvim +{line} {file}
