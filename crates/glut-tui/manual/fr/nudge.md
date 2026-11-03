Si Glut demande au modèle de continuer à la fin d'un tour, de tenir sa liste de tâches et d'en écrire une quand il travaille sans.

/nudge on — quand un tour s'arrête avec des points de la liste de tâches ouverts, ou sur des mots qui annoncent une étape (« ensuite je… », deux-points à la fin), Glut envoie au modèle une ligne à lui : continuer ou dire pourquoi il s'arrête ; le fil le montre discrètement. Quand la liste ne change pas pendant qu'il travaille, Glut la lui rappelle, et si rien ne change, une fois par un message à part ; s'il n'en a écrit aucune dans une exécution, Glut lui en demande une, une fois par tour
/nudge off — un tour s'arrête là où le modèle l'arrête : ni rappel de la liste ni demande d'une

La question de fin vient une fois par tour, jamais en mode plan, après une question qui t'est posée, dans un tour de boucle ou l'exécution d'un plan ; le rappel et la demande suivent le même choix, mais dans l'exécution d'un plan Glut rappelle toujours. /nudge seul montre le choix actuel. Gardé dans `~/.glut/prefs.json` ; `"nudge": false` dans `~/.glut/settings.json` le coupe pour chaque exécution.
