Si Glut cherche une fois par jour une version plus récente et l'indique dans le pied.

/update on — une fois par jour, en arrière-plan, Glut lit le numéro de la dernière version dans `https://glut-ai.com/latest.txt` ; s'il est plus récent que celui-ci, le pied l'indique discrètement (`↑ 0.2.0`)
/update off — Glut ne demande rien, et le pied n'indique rien

/update seul dit si un Glut plus récent est sorti et donne la commande qui met celui-ci à jour, selon la façon dont il a été installé (`brew upgrade glut`, `cargo install glut-tui` ou le script d'installation). Glut ne se met jamais à jour lui-même et ne demande jamais pendant l'exécution d'un plan, un tour de boucle, `glut run`, `glut acp` ou l'arène. Gardé dans `~/.glut/prefs.json` ; `"updateCheck": false` dans `~/.glut/settings.json` le désactive là où /update n'a rien dit d'autre.
