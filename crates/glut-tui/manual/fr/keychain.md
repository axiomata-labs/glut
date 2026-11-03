Tenir le trousseau hors de portée des commandes que Glut exécute.

/keychain — s'il est ouvert, et les programmes qui peuvent le demander
/keychain add <program> — met un programme sur la liste (gh, ou git et une sous-commande : git push)
/keychain remove <program> — le retire de la liste
/keychain open — l'ouvre à toute commande, jusqu'à la fin de cette session
/keychain close — le referme

Une clé ou une connexion dans le trousseau s'atteint par-delà toutes les règles de fichiers du bac à sable : une commande que Glut exécute trouve donc le trousseau fermé. Une commande d'un programme listé demande d'abord — dans tous les modes, `auto` aussi — et s'exécute avec le trousseau ouvert dès que tu l'autorises ; un « toujours » en fait une règle à toi. Quand personne n'est là (un plan, `glut run`), seule une telle règle l'ouvre. Quand une commande échoue sur le trousseau fermé, Glut le dit sous sa sortie.

Autoriser une commande donne aussi le trousseau à tout ce qu'elle lance : un hook git, une extension gh ou un script de build peut alors y lire n'importe quelle clé.

Seules vos propres règles donnent le trousseau : une règle d'autorisation du `.glut/` d'un projet laisse une commande s'exécuter, jamais avec le trousseau ouvert. Une chaîne ne l'obtient que si chaque partie est listée ou ne fait que lire. Ce que `glut keychain open` garde ouvert ne compte qu'ici, où vous êtes — jamais dans `glut run` ni dans l'exécution d'un hôte.

Une de vos règles ne l'ouvre sans demander que si elle nomme un programme et sa sous-commande (`gh pr *`, `git push *`) — jamais un programme entier (`gh *`). Une carte 🔑 montre toute la commande, ↑↓ pour la parcourir, et n'autorise qu'une fois lue jusqu'au bout.

La liste est gardée dans `~/.glut/keychain.json` ; `glut keychain open` garde le trousseau ouvert d'une session à l'autre, `glut keychain close` le ferme pour de bon.
