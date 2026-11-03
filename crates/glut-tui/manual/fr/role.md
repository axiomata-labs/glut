Faire jouer un rôle à ce pane : le prompt et les outils d’un agent nommé, jamais son modèle.

/role — les rôles au choix
/role <name> — joue ce rôle à partir de maintenant
/role none — de nouveau un généraliste, avec tous les outils

Glut apporte reviewer, tester et docs ; tes propres rôles vivent dans `~/.glut/agents`, ceux d’un projet dans son `.glut/agents` (lus seulement dans un dossier de confiance). Les outils d’un rôle prennent effet aussitôt, son prompt dès le tour suivant. Un rôle qui restreint les outils a besoin du Gate.
