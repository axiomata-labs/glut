Donner à cette session des dossiers à côté du dossier de travail, pour que Glut puisse travailler entre plusieurs projets.

/folder — les dossiers à côté du dossier de travail
/folder add <path> — en ajoute un pour cette session
/folder remove <path> — en retire un

Les outils de fichiers atteignent un tel dossier par son chemin complet, les commandes peuvent y écrire dans la sandbox, le Gate le juge comme le dossier de travail et l’annulation y rétablit aussi les fichiers. Toi seul ajoutes un dossier, jamais un modèle, un Skill ou un projet. Glut refuse le dossier personnel, un dossier qui contient le projet ou se trouve dedans, un dossier avec des secrets, et deux dossiers imbriqués l’un dans l’autre.

Les dossiers ne valent que pour cette session ; `glut run --folder <path>` en donne un à une exécution sans interface.
