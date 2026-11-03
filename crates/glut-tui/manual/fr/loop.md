Lancer une tâche encore et encore dans un agent d’arrière-plan à elle, tant que Glut est ouvert.

/loop <interval> [fresh] [as <role>] [notify] <task> — lance une boucle ; le premier tour aussitôt
/loop — liste les boucles : intervalle, tours, le prochain tour et le coût jusqu’ici
/loop stop [name] — termine cette boucle, ou la seule
/loop rounds [n] — le plus de tours d’une boucle ; seul, montre la limite actuelle
/loop budget [usd] — le plus qu’une boucle peut coûter ; seul, montre la limite actuelle
/loop tokens [n] — le plus de tokens qu’une boucle sur un abonnement peut utiliser (5M, 500k) ; seul, montre la limite actuelle

Les tours ne se chevauchent jamais : l’intervalle est compté depuis la fin d’un tour, comme 30s, 5m, 2h ou 1h30m (de 10s à 24h). Un tour poursuit la conversation du tour précédent ; avec fresh, chaque tour en commence une nouvelle. La tâche est un message, ou /skill <name> [pour quoi], /compact ou /init — aucune autre commande.

L’agent s’appelle loop (ou d’après son rôle) et prend le mode de ce pane, les « toujours » que tu as donnés ici, ses dossiers à côté et son rôle ; as <role> ne peut nommer que le rôle que ce pane joue. Ses demandes attendent ta réponse. Une tuile vient quand un tour demande ou que la boucle se termine ; avec notify, après chaque tour.

Une boucle se termine après ses tours ou dès que ses tours coûtent son budget (par défaut 50 tours et 5 $, sinon `loop.rounds` et `loop.maxUsd` de `~/.glut/settings.json` ; un changement vaut pour les boucles lancées après lui), quand un tour échoue ou que tu l’arrêtes (⎋ dans son tour), ou quand son modèle appelle `end_loop` parce que la fin que ta tâche nommait est atteinte. Quand les prix du modèle sont inconnus, seuls les tours y mettent fin. Son agent reste ensuite dans l’onglet Agents, à lire ou à /close. x sur sa carte dans l’onglet Agents (⌃X A) termine la boucle.

/loop 10m lance les tests et corrige ce qui échoue ; arrête-toi quand tout passe
/loop 1h fresh as reviewer notify relis le dernier commit
