Voir les serveurs MCP et l’état de chacun, se connecter à un serveur hébergé, en ajouter ou en retirer un.

/mcp — chaque serveur : connecté avec ses outils, en attente d’une connexion, ou en échec
/mcp login <name> — se connecte à un serveur hébergé ; son adresse s’ouvre dans ton navigateur sur ⏎
/mcp login <name> <address> — par SSH : l’adresse où le navigateur a fini, collée en retour
/mcp logout <name> — se déconnecte et oublie son token
/mcp add <name> <url> — ajoute un serveur hébergé à `~/.glut/mcp.json` et le connecte
/mcp add <name> -- <command> [args] — ajoute un serveur que Glut lance lui-même
/mcp remove <name> — le retire de `~/.glut/mcp.json` et de la session

Un serveur qui n’offre aucune connexion que Glut sache utiliser demande un token, saisi masqué. Les serveurs propres à un projet sont au projet de les changer : /mcp remove ne retire que les tiens. `glut mcp` fait de même hors d’une session.

Pendant qu'un de ses outils tourne, un serveur peut te demander : un formulaire (⇥ ↑↓ entre les champs, espace coche, ⏎ envoie, Esc ferme, ⌃D refuse) ou une page à ouvrir (o). Toi seul y réponds, jamais le modèle ; un formulaire qui demande un mot de passe ou une clé est refusé, et sans personne toute question est refusée.
