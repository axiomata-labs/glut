Empêcher les données de Glut dans ~/.glut de grossir sans fin.

/clean — ce que Glut garde, poste par poste, et ce qui peut partir selon vos délais
/clean now — nettoie tout de suite, en arrière-plan
/clean keep — les délais
/clean keep <kind> <days>d — en fixe un : snapshots, journals, sessions, results ou lean

Ce qui part : les instantanés d'une exécution plus vieux que leur délai (7 jours) — tout le dépôt d'un dossier quand il n'en reste aucun, le dossier présent ou non ; le journal d'une exécution inachevée (7 jours) ; une session non écrite depuis 90 jours, avec son journal d'événements et ses longs résultats ; les longs résultats sans session après 30 jours ; les images qu'aucune session gardée ne cite ; les lignes de lean.jsonl de plus de 90 jours. Les exports sont seulement comptés : ce sont vos fichiers.

Une fois par jour, Glut nettoie de lui-même au démarrage — ici et dans `glut run` — et dit ce qu'il a libéré. Les sorties de build et les caches (un dossier avec un CACHEDIR.TAG, sous node_modules/ ou le target/ d'un projet Cargo) n'ont aucun instantané.

Les délais sont gardés dans `~/.glut/clean.json`. En ligne de commande : `glut clean`, `glut clean --dry-run`, `glut clean keep sessions 30d` ; `glut clean --dry-run --json` écrit le rapport pour un programme.
