Être prévenu quand un tour se termine ou que Glut demande, tant que la fenêtre n’est pas au premier plan.

/notify on — la fin d’un tour et chaque demande
/notify asks — seulement les demandes
/notify off — rien
/notify system — par les notifications propres de macOS (un clic sur l’une ouvre Éditeur de script, pas le terminal)
/notify after <s> — la fin d’un tour n’est annoncée que si elle a duré autant de secondes (10 par défaut, jusqu’à 3600)
/notify tiles on|off — les tuiles en haut à droite de la fenêtre

/notify seul montre ce qui vaut maintenant. Glut utilise les notifications propres du terminal là où il sait qu’il les a, et la cloche. Une notification nomme Glut, le projet, le pane et ce qui s’est passé, jamais les mots du modèle. Gardé dans `~/.glut/prefs.json`, avant `notify` dans `~/.glut/settings.json`.
