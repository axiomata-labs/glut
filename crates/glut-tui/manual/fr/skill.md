Lancer un Skill : des instructions pour un genre de tâche, que le modèle charge et suit.

/skill — les Skills au choix
/skill <name> [pour quoi] — lance ce Skill sur ce qui suit son nom

Glut apporte make-agent, make-skill et make-hook pour écrire les tiens. Les tiens vivent dans `~/.glut/skills`, ceux d’un projet dans son `.glut/skills` (lus seulement dans un dossier de confiance) ; un Skill de même nom masque celui de Glut.

/skill make-hook bloque git push sur main
