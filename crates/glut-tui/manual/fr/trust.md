Si les propres réglages .glut de ce dossier comptent en entier.

/trust — si ce dossier est de confiance, et ce que ses propres réglages ajoutent alors
/trust on — lui fait confiance : ses règles d’autorisation, son mode, ses skills, agents et commandes comptent
/trust off — ne lui fait plus confiance : seules ses règles d’interdiction et de question comptent
/trust list — chaque dossier de confiance

Le .glut/settings.json d’un projet peut garder plus de secrets et demander ou refuser davantage — cela compte dans chaque dossier. Ce qui permettrait plus à un modèle, ses règles d’autorisation et son mode, ne compte que là où vous avez fait confiance au dossier, de même que les skills, agents et commandes de son .glut. Lisez ce que /trust montre avant de dire on : le .glut d’un dépôt cloné est la parole de quelqu’un d’autre. Son mode ne dépasse jamais le vôtre, et ses hooks ne s’exécutent jamais, de confiance ou non.

Le changement compte dès la prochaine session du dossier (glut -c reprend la conversation). Jamais le dossier personnel, la racine, ni un dossier qui n’est pas à vous seul (celui d’un autre utilisateur, ou un que tout le monde peut écrire, comme /tmp). La liste est gardée dans ~/.glut/trusted.json, qu’aucune commande exécutée par Glut n’écrit ; glut trust show|add|remove|list fait de même hors de Glut.
