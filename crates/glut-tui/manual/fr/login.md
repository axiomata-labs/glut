Te connecter avec un abonnement au lieu d'une clé — aujourd'hui ton abonnement ChatGPT Plus ou Pro.

/login — les abonnements, chacun avec son état ; ⏎ connecte ou déconnecte
/login <provider> — s'y connecte tout de suite, comme /login chatgpt
/login <provider> <address> — via SSH : l'adresse sur laquelle le navigateur s'est arrêté, recollée
/login openrouter — OpenRouter donne à Glut une clé à lui, gardée comme une clé tapée ; limite-la ou supprime-la sur openrouter.ai/settings/keys

L'adresse est d'abord affichée ; ⏎ l'ouvre dans ton navigateur. Glut garde la connexion dans le trousseau (ou `~/.glut/credentials.json`) et la renouvelle tout seul ; la déconnexion la révoque chez OpenAI.

Avec l'abonnement, un appel ne coûte rien, mais les limites d'usage de l'abonnement s'appliquent : quand l'une est atteinte, le tour s'arrête et Glut renvoie vers Réglages ChatGPT → Utilisation, où tu limites aussi ce que chaque app peut utiliser. Dans le shell : glut providers login|logout <provider>.
