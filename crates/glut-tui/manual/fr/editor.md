Choisir l’éditeur dans lequel Glut ouvre les fichiers et la saisie.

/editor — les éditeurs trouvés sur ton système
/editor <command> — ta propre commande, avec `{file}` et `{line}` là où ils vont

Sans `{file}`, le fichier est ajouté à la fin. Un éditeur à fenêtre doit attendre que le fichier soit fermé (`code --wait`). ⌃X E y modifie la saisie ; /todo open et /memory open s’en servent aussi. Gardé dans `~/.glut/prefs.json`.

/editor nvim +{line} {file}
