Combien d'étapes un tour fait au plus.

/maxsteps — la limite actuelle
/maxsteps <n> — un tour s'arrête après n étapes, de 1 à 1000

Une étape est un appel du modèle. À la limite le tour s'arrête et le dit ; ⏎ sur la saisie vide envoie « Go on where you stopped. » et le travail continue. Les budgets et la limite de temps restent les vrais freins. Gardé dans `~/.glut/prefs.json` ; `maxTurns` dans `~/.glut/settings.json` le fixe pour chaque exécution, `glut run --max-turns` pour une.
