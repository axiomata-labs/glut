Voir et régler les clés cachées au modèle dans cette session.

/secrets — chaque valeur cachée jusqu’ici, par numéro, genre, longueur et lieu où elle a été vue (jamais la valeur)
/secrets allow <n> — laisse passer le secret n pour cette session
/secrets allow <n> always — le laisse passer toujours (son empreinte est gardée, jamais la valeur)
/secrets add <name> <regex> — cache un format à toi, dès le prochain résultat
/secrets remove <name> — retire un format à toi
/secrets on|off — le masquage activé ou désactivé

Une clé dans ce que répond un outil parvient au modèle sous la forme ‹secret n: …› ; Glut ne remet sa valeur que dans un fichier qui la contient déjà. Un message de toi qui semble contenir une clé fait l’objet d’une question avant de partir. Le modèle apprend un changement avec ton prochain message. Tes formats et tes choix sont gardés dans `~/.glut/secrets.json`.

Le masquage protège d’une clé montrée par accident ; la sandbox et le Gate réseau restent les barrières.
