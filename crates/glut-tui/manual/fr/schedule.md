Des plans qui tournent quand Glut est fermé : une tâche à une heure du jour ou à intervalle régulier.

/schedule — l’onglet Exécutions (⌃X R) : chaque plan avec sa prochaine heure et son dernier résultat
/schedule list — les plans dans le flux
/schedule add <name> [flags] <task> — crée un plan, ou modifie celui de ce nom
/schedule run <name> — le lance maintenant, à côté de Glut
/schedule pause <name> — le suspend jusqu’à resume
/schedule resume <name> — il tourne de nouveau à ses heures
/schedule remove <name> — le plan et son historique disparaissent ; ses sessions restent

Les flags sont ceux de `glut schedule add` :
- `--at 07:00` une heure du jour, avec `--days mo-fr` ou `sa,su` (tous les jours sans)
- `--every 2h` un intervalle de 10m à 7d, à la place de --at
- `--mode ask|build|plan|dont_ask|auto` ce qu’il peut faire sans règle ; ask s’il n’est pas nommé
- `--allow <rule>`, `--deny <rule>` des règles dans la forme de Claude Code, comme `Bash(cargo test:*)` ; répétable
- `--folder <path>` un dossier à côté du sien ; répétable
- `--max-usd 1`, `--max-minutes 30` le plus qu’une exécution peut coûter et durer : 1 $ et 30 min s’ils ne sont pas nommés
- `--max-tokens 1M` sur un abonnement, le plus de tokens qu’une exécution peut utiliser : 1M s’il n’est pas donné
- `--replace` les règles et dossiers donnés prennent la place de ceux du plan au lieu de s’y ajouter
- `--cwd <path>`, `--provider <id>`, `--model <id>` son dossier (celui-ci sinon), son fournisseur et son modèle, fixés à la création

La tâche est un message, ou /skill <name> [pour quoi]. launchd lance chaque exécution, seulement pendant que tu es connecté ; une heure manquée pendant la veille est rattrapée une fois au réveil. Personne n’est là pour répondre : toute demande est refusée et notée, alors autorise ce dont il a besoin. Une exécution est une session gardée à part, annoncée par une notification macOS. Dans l’onglet Exécutions : ⏎ ouvre un plan ou la session d’une exécution, r le lance maintenant, p le suspend ou le reprend, d le supprime.

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" lance les tests et signale ce qui échoue
