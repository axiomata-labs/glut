Scegli l’editor in cui Glut apre i file e l’inserimento.

/editor — gli editor trovati sul tuo sistema
/editor <command> — un tuo comando, con `{file}` e `{line}` dove vanno

Senza `{file}` il file viene aggiunto in fondo. Un editor a finestra deve aspettare finché il file è chiuso (`code --wait`). ⌃X E modifica l’inserimento con esso; anche /todo open e /memory open lo usano. Conservato in `~/.glut/prefs.json`.

/editor nvim +{line} {file}
