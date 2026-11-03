Choisir comment une session au repos garde le cache de prompt du fournisseur, pour que le tour suivant démarre au chaud.

/cache ping — Glut relit le cache toutes les quelques minutes pendant que la session se repose, jusqu’à 30 minutes
/cache 1h — Anthropic le garde une heure (chaque écriture coûte le double) ; chez les autres fournisseurs, il est relu pour être gardé
/cache off — pas gardé : une pause au-delà des quelques minutes du fournisseur repart à froid

/cache seul montre le choix actuel ; ping est la valeur par défaut. Gardé dans `~/.glut/prefs.json`.
