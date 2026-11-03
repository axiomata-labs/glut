Laisse un second modèle juger les commandes sur lesquelles seul le mode t’interrogerait.

/classifier — quel modèle juge, ou qu’aucun ne le fait
/classifier session — le modèle de la session juge (tant que tu n’en fixes aucun)
/classifier <provider> <model> — un modèle à part, un rapide suffit (openrouter deepseek/deepseek-v4.1-flash)
/classifier off — aucun ne juge : on te demande à chaque fois

Glut connaît beaucoup de commandes qui ne font que lire (ls, grep, git status, …) et les exécute dans tous les modes. Une boucle ou un pipeline de celles-ci (for f in *.md; do grep -c x "$f"; done), il ne peut pas en juger par son seul texte, alors il demande. Le second modèle voit cette commande et son dossier — jamais la conversation, un fichier ou une sortie — et répond READS ou CHANGES. Seulement sur READS la commande s’exécute comme une commande de lecture connue, dans le bac à sable ; tout le reste, ou aucune réponse claire en quelques secondes, te demande comme avant, son avis sous la commande. Une règle à toi, une commande lourde, le trousseau et le bac à sable restent tels quels ; la même commande n’est pas jugée deux fois dans une session.

Une commande qu’il a laissé passer le dit à côté. Le choix est gardé dans `~/.glut/classifier.json` et vaut dès le prochain tour ; `glut run --classifier` le fixe pour une exécution.
