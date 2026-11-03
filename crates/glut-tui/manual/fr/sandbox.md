Comment sont tenues les commandes que Glut exécute, et si elles peuvent écrire dans les dossiers git.

/sandbox — le bac à sable ici (macOS : Seatbelt ; Linux : bubblewrap et seccomp) et le commutateur git
/sandbox git read-only — aucune commande n’écrit dans un dossier git : aucune ne commite depuis le bac à sable
/sandbox git writable — les commandes peuvent de nouveau écrire dans les dossiers git, et commiter
/sandbox required on — aucune commande ne s’exécute là où il n’y a pas de bac à sable (bubblewrap absent, un conteneur sans le droit) ; off les laisse s’exécuter, tenues par les seules règles

Le bac à sable ne laisse une commande écrire que dans tes dossiers, tout lire sauf tes secrets et les fichiers .env, et n’atteindre le réseau que par le proxy de Glut. Sous Linux, il ne peut pas refuser un nom qui n’existe pas encore : ce qu’une commande crée et que macOS refuse — un nouveau .envrc, un .git, un .git/commondir — part dans ~/.glut/quarantine dès qu’il apparaît, et encore une fois après la commande. Un outil à toi qui s’exécute à cet instant précis pourrait encore le rencontrer ; avec les dossiers git en lecture seule, aucun n’y est créé.

Le commutateur vaut dans chaque session et chaque fenêtre dès la prochaine commande, et se garde dans ~/.glut/sandbox.json — jamais écrit par une commande. glut sandbox show et glut sandbox git read-only|writable font de même hors de Glut.
