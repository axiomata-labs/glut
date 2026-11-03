Lancer la même tâche sur deux à quatre modèles à la fois et les comparer (l’arène).

/arena — les modèles de tous les fournisseurs ; Espace en marque de deux à quatre, ⏎ les met dans la saisie pour ajouter la tâche
/arena <model>, <model>: <task> — la même chose en une ligne (fournisseur/modèle, ou un nom qui correspond à un seul modèle)
/arena say <text> — une relance à chaque modèle qui a répondu, quand tous ont fini (pas sur le banc) ; durée, coût et tokens s’additionnent au fil des tours
/arena take <n> — les changements du modèle n dans ton dossier, sans commit
/arena card — la carte de comparaison de nouveau : les chiffres et la vérification de chaque modèle, son diff en dessous (←/→ un autre modèle, s deux côte à côte, c vérifier, t reprendre)
/arena check <command> — la vérification pour ce dépôt (cargo test, npm test …), lancée dans le worktree de chaque modèle, dans son sandbox ; mémorisée, elle se lance toute seule après la prochaine arène
/arena tasks — les jeux de tâches du banc d’essai dans ~/.glut/arena/tasks ; /arena tasks <set> en lance un sur les modèles de la dernière arène (ou /arena tasks <set>: <model>, <model> ; avec x3 après le jeu, chaque tâche trois fois de suite (jusqu’à x5), le tableau indique alors 2/3 ; avec --background ses modèles tournent en agents d’arrière-plan, sans pane, leur progression dans l’onglet Agents), tâche après tâche, chacune jugée par sa propre vérification
/arena results — les résultats de chaque jeu (tâches réussies, Ø durée et coût) et les victoires de l’arène libre
/arena blind on|off — l’arène libre à l’aveugle (par défaut) : ses modèles portent des lettres dans un ordre tiré au sort, leurs noms viennent dès que tu en reprends un ou termines l’arène
/arena judge — après ton choix, le modèle du pane d’où l’arène est partie commente les solutions en deux ou trois phrases (sans outils ; jamais avant, pour ne pas influencer ton choix)
/arena end — ferme les panes de l’arène et supprime leurs worktrees (demande d’abord)

Chaque modèle travaille dans un pane et un worktree à lui, qui part de ton dossier tel qu’il est, fichiers non commités et nouveaux compris (pas de fichier .env, pas de lien), en auto : seules les commandes lourdes demandent. Un modèle s’arrête après 30 minutes ou 2 $. Les modèles cloud tournent en même temps, les locaux (Ollama, LM Studio) l’un après l’autre une fois les autres terminés. Quand tous ont fini, leurs chiffres s’affichent dans le pane d’où l’arène a démarré, et /arena card ouvre la carte de comparaison : durée, coût, tokens, appels et ce que chacun a changé. /arena take vérifie que les changements vont encore dans ton dossier ; sinon, rien n’est écrit.

/arena sonnet, gpt-5: make the failing test pass
