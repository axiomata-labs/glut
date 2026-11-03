Ton éditeur à côté de Glut : Neovim avec le plugin glut.nvim, VS Code (et Cursor, Windsurf) ou un IDE JetBrains avec le plugin propre à Glut.

/ide — à quel éditeur Glut est relié, et lesquels conviennent
/ide on — relier (demande si plusieurs conviennent)
/ide off — couper pour cette session

Relié, chacun de tes messages porte le fichier actif et la sélection (⧉ en dessous), tant qu'aucune règle de lecture ne retient le fichier ; le modèle peut demander quels fichiers sont ouverts et ce que disent les serveurs de langage ; Glut lit un tampon tel que l'éditeur le tient, non enregistré compris, et une question du gate sur une modification s'affiche aussi en diff dans l'éditeur — la première réponse compte. Glut se relie seul au démarrage là où un éditeur convient : celui qui a lancé Glut, sinon celui dont le dossier contient celui de Glut. « ⧉ » dans le pied dit qu'il est relié.
