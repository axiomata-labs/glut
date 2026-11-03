Écrire la session pour la lire ou la partager.

/export — du Markdown dans `~/.glut/exports`
/export html — un seul fichier HTML, ses styles dedans, sans script
/export jsonl — les messages en lignes JSON
/export copy — le Markdown dans le presse-papiers au lieu d’un fichier
/export full — ajoute les résultats d’outils entiers et le prompt système
/export <format> <path> — dans un fichier à toi (`~/` est ton dossier personnel, un chemin relatif est pris dans le dossier de travail)

Les mots vont dans n’importe quel ordre, le chemin en dernier. Sans full, l’export montre ce que le modèle a vu : les longs résultats raccourcis. Un fichier existant n’est jamais écrasé. Les clés que Glut a cachées restent cachées, mais ce que tu as tapé toi-même peut encore en contenir une : relis un export avant de le partager. Dans /resume, ⌃E exporte une session de la liste.
