//! The commands' manual pages in French (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 62] = [
    (
        "clear",
        "Commencer une nouvelle session vide dans ce dossier.\n\
         \n\
         /clear — la conversation, la liste de tâches du plan, le coût et le compteur repartent de zéro\n\
         \n\
         La session que tu quittes est gardée : /resume la rouvre. Ton modèle, ton mode et tes réglages restent comme \
         ils sont.",
    ),
    (
        "resume",
        "Ouvrir une session antérieure de ce dossier et la poursuivre.\n\
         \n\
         /resume — la liste des sessions gardées de ce dossier, la plus récente d’abord ; une bifurcation figure sous \
         la session dont elle vient\n\
         \n\
         Dans la liste : tape pour filtrer, ⏎ en ouvre une, ⌫ en supprime une (après confirmation ; ses bifurcations \
         restent des sessions à part entière), ⌃E en écrit une en Markdown dans `~/.glut/exports`. Une session ouverte \
         dans un autre pane s’atteint là-bas (⌃P).\n\
         \n\
         /search retrouve une session d’après ce qui s’y est dit.",
    ),
    (
        "search",
        "Retrouver une session gardée par ses mots : la liste de /resume, filtrée par le texte de sa conversation.\n\
         \n\
         /search <text> — les sessions de ce dossier dont la conversation contient le texte\n\
         \n\
         Dans la liste, ⌃A bascule entre ce dossier et tous les projets. ⏎ ouvre une session de ce dossier ; celle \
         d’un autre projet nomme son dossier — lance Glut là-bas pour la poursuivre.",
    ),
    (
        "btw",
        "Poser une question en aparté sans déranger la conversation : la réponse vient dans une fenêtre au-dessus de \
         la saisie et ne fait jamais partie de la session.\n\
         \n\
         /btw <question> — interroge la session de ce pane, même pendant que son tour continue\n\
         /btw <name> <question> — interroge un autre pane ou un agent en arrière-plan, par son numéro ou son nom\n\
         /btw — rouvre la dernière réponse\n\
         \n\
         Le modèle répond d’après la conversation telle qu’elle est et n’utilise aucun outil : s’il devait chercher \
         quelque chose, pose-lui plutôt la question comme un message. Son coût compte dans le coût de la session. ⎋ \
         ferme la fenêtre, PgUp PgDn la font défiler.\n\
         \n\
         /btw reviewer où en es-tu ?",
    ),
    (
        "projects",
        "Continuer dans un autre projet : l’un des dossiers où Glut a travaillé, ou n’importe quel dossier par son \
         chemin.\n\
         \n\
         /projects — la liste : ★ épinglé, ● celui-ci ; ⏎ en ouvre un, p l’épingle ou le désépingle, x en masque un \
         jusqu’à ce qu’on y retravaille\n\
         /projects <path> — ouvre ce dossier (⇥ complète le nom d’un dossier)\n\
         \n\
         Tant qu’un tour ou un agent travaille encore, Glut demande avant de les arrêter et de changer. Les sessions \
         et les panes de cette fenêtre sont gardés. Dans une fenêtre ouverte par une application hôte, le dossier est \
         à l’hôte de le choisir.",
    ),
    (
        "folder",
        "Donner à cette session des dossiers à côté du dossier de travail, pour que Glut puisse travailler entre \
         plusieurs projets.\n\
         \n\
         /folder — les dossiers à côté du dossier de travail\n\
         /folder add <path> — en ajoute un pour cette session\n\
         /folder remove <path> — en retire un\n\
         \n\
         Les outils de fichiers atteignent un tel dossier par son chemin complet, les commandes peuvent y écrire dans \
         la sandbox, le Gate le juge comme le dossier de travail et l’annulation y rétablit aussi les fichiers. Toi \
         seul ajoutes un dossier, jamais un modèle, un Skill ou un projet. Glut refuse le dossier personnel, un \
         dossier qui contient le projet ou se trouve dedans, un dossier avec des secrets, et deux dossiers imbriqués \
         l’un dans l’autre.\n\
         \n\
         Les dossiers ne valent que pour cette session ; `glut run --folder <path>` en donne un à une exécution sans \
         interface.",
    ),
    (
        "compact",
        "Résumer la conversation jusqu’ici, pour qu’elle prenne moins de place dans la fenêtre du modèle.\n\
         \n\
         /compact — le modèle écrit un résumé qui prend la place des anciens messages\n\
         \n\
         Le prompt système, ton dernier message et les étapes entières restent ; ce que le résumé remplace est gardé \
         dans le stock de résultats de la session, jamais jeté. Glut compacte aussi de lui-même peu avant que la \
         fenêtre soit pleine (`compact` dans `~/.glut/settings.json`). /context montre à quel point la fenêtre est \
         pleine.",
    ),
    (
        "handoff",
        "Passer le travail à une session ultérieure de ce dossier.\n\
         \n\
         /handoff write — le modèle écrit ce qui a été fait, où en sont les choses et ce qui vient ensuite\n\
         /handoff read — reprend le handoff du dossier dans cette session\n\
         \n\
         Un handoff est gardé dans `~/.glut/handoffs`, un par dossier. Une fois lu, il est mis de côté, si bien \
         qu’aucune session ultérieure ne le lit deux fois. En écrire un est un appel payant à part ; la conversation \
         reste telle qu’elle est.",
    ),
    (
        "export",
        "Écrire la session pour la lire ou la partager.\n\
         \n\
         /export — du Markdown dans `~/.glut/exports`\n\
         /export html — un seul fichier HTML, ses styles dedans, sans script\n\
         /export jsonl — les messages en lignes JSON\n\
         /export copy — le Markdown dans le presse-papiers au lieu d’un fichier\n\
         /export full — ajoute les résultats d’outils entiers et le prompt système\n\
         /export <format> <path> — dans un fichier à toi (`~/` est ton dossier personnel, un chemin relatif est pris \
         dans le dossier de travail)\n\
         \n\
         Les mots vont dans n’importe quel ordre, le chemin en dernier. Sans full, l’export montre ce que le modèle a \
         vu : les longs résultats raccourcis. Un fichier existant n’est jamais écrasé. Les clés que Glut a cachées \
         restent cachées, mais ce que tu as tapé toi-même peut encore en contenir une : relis un export avant de le \
         partager. Dans /resume, ⌃E exporte une session de la liste.",
    ),
    (
        "quit",
        "Terminer Glut, avec chaque pane et chaque agent en arrière-plan.\n\
         \n\
         /quit — tant qu’un autre pane ou un agent travaille encore, Glut le dit d’abord : /quit une seconde fois les \
         termine aussi\n\
         \n\
         ⌃C ⌃C fait de même, et ⌃D sur une ligne vide pendant que Glut se repose. Les sessions sont gardées pour \
         /resume.",
    ),
    ("exit", "Terminer Glut, comme le fait /quit."),
    (
        "model",
        "Choisir le modèle avec lequel cette session parle.\n\
         \n\
         /model — les modèles du fournisseur utilisé, d’après sa liste gardée\n\
         /model <id> — passe à ce modèle par son id\n\
         \n\
         La conversation continue avec le nouveau modèle, et le prochain démarrage commence avec lui. Un modèle d’un \
         autre fournisseur se choisit après /provider. `glut models refresh` renouvelle les listes.",
    ),
    (
        "provider",
        "Passer à un autre fournisseur de modèles.\n\
         \n\
         /provider — les fournisseurs que tu as configurés\n\
         /provider <id> — passe à celui-ci, sur son modèle par défaut\n\
         /provider key [id] — ne change que la clé de ce fournisseur (sans id, celle du fournisseur par défaut)\n\
         \n\
         Les fournisseurs sont gardés dans `~/.glut/providers.json`, leurs clés dans le Trousseau (ou dans \
         `~/.glut/credentials.json`) ; `glut providers add <id>` en ajoute un. La conversation continue, et le \
         prochain démarrage commence avec le fournisseur que tu as choisi.\n\
         \n\
         La clé se tape masquée et s'essaie sur la liste de modèles du fournisseur avant de remplacer l'ancienne ; si \
         cela échoue, tu choisis de la garder quand même, sinon l'ancienne reste. La session prend aussitôt une clé \
         gardée. Un fournisseur qui se connecte, n'a pas besoin de clé ou la prend dans une variable d'environnement \
         te le dit. Dans le shell : `glut providers key <id>`.",
    ),
    (
        "login",
        "Te connecter avec un abonnement au lieu d'une clé — aujourd'hui ton abonnement ChatGPT Plus ou Pro.\n\
         \n\
         /login — les abonnements, chacun avec son état ; ⏎ connecte ou déconnecte\n\
         /login <provider> — s'y connecte tout de suite, comme /login chatgpt\n\
         /login <provider> <address> — via SSH : l'adresse sur laquelle le navigateur s'est arrêté, recollée\n\
         /login openrouter — OpenRouter donne à Glut une clé à lui, gardée comme une clé tapée ; limite-la ou \
         supprime-la sur openrouter.ai/settings/keys\n\
         \n\
         L'adresse est d'abord affichée ; ⏎ l'ouvre dans ton navigateur. Glut garde la connexion dans le trousseau \
         (ou `~/.glut/credentials.json`) et la renouvelle tout seul ; la déconnexion la révoque chez OpenAI.\n\
         \n\
         Avec l'abonnement, un appel ne coûte rien, mais les limites d'usage de l'abonnement s'appliquent : quand \
         l'une est atteinte, le tour s'arrête et Glut renvoie vers Réglages ChatGPT → Utilisation, où tu limites \
         aussi ce que chaque app peut utiliser. Dans le shell : glut providers login|logout <provider>.",
    ),
    (
        "effort",
        "Régler à quel point le modèle réfléchit avant de répondre.\n\
         \n\
         /effort — les niveaux au choix\n\
         /effort <level> — none, minimal, low, medium, high, xhigh ou max\n\
         /effort default — le fournisseur décide\n\
         \n\
         Plus d’effort donne en général de meilleures réponses sur un travail difficile, mais plus lentes et plus \
         chères. Tous les modèles ne connaissent pas tous les niveaux. Le compteur (⌃N) montre le niveau utilisé.",
    ),
    (
        "context",
        "Voir à quel point la fenêtre du modèle est pleine, ou dire à Glut quelle taille elle a.\n\
         \n\
         /context — les tokens utilisés, la fenêtre et d’où vient sa taille, et ce que coûtent à chaque tour les \
         guides (`AGENTS.md`, `CLAUDE.md`)\n\
         /context <tokens> — règle la fenêtre de ce modèle : 65536, 64k ou 1M\n\
         \n\
         Une fenêtre que tu règles est gardée dans `~/.glut/providers.json` pour ce modèle et prime sur la liste du \
         fournisseur. Glut compacte de lui-même peu avant que la fenêtre soit pleine.",
    ),
    (
        "thinking",
        "Choisir comment la réflexion du modèle apparaît dans le flux.\n\
         \n\
         /thinking off — masquée\n\
         /thinking folded — une ligne par pensée ; un clic en ouvre une\n\
         /thinking open — chaque pensée montrée en entier\n\
         \n\
         /thinking seul les replie. ⌃T ouvre ou replie toutes les pensées d’un coup. Ton choix est gardé dans \
         `~/.glut/prefs.json`.",
    ),
    (
        "panel",
        "Afficher le panneau latéral sur son onglet Plan, ou le masquer.\n\
         \n\
         /panel — le panneau latéral et ses onglets : Plan, Gate, Compteur, Agents, À faire, Exécutions et Touches\n\
         \n\
         ⌃B fait de même, ⌃X ⇥ donne les touches au panneau (←→ les onglets, ↑↓ dedans, ⎋ retour). Le panneau demande \
         une fenêtre large d’au moins 100 colonnes. Qu’il soit affiché ou non est gardé pour le prochain démarrage.",
    ),
    (
        "meter",
        "Voir ce que la session consomme : le contexte, les tokens et le coût.\n\
         \n\
         /meter — l’onglet Compteur du panneau latéral\n\
         \n\
         Il montre à quel point la fenêtre est pleine et quand Glut compacte, les tokens de la session en entrée, en \
         cache et en sortie avec leur prix, chaque tour et chaque outil, et ce que la sortie lean de Glut a épargné. \
         ⌃N l’ouvre aussi.",
    ),
    (
        "agents",
        "Voir les sous-agents que Glut a lancés dans cette session.\n\
         \n\
         /agents — la liste des sous-agents, chacun sous celui qui l’a lancé ; ⏎ ouvre le flux propre de l’un d’eux\n\
         \n\
         Un clic sur la tuile d’un sous-agent l’ouvre aussi. ⌃X A ouvre l’onglet Agents : chaque sous-agent et chaque \
         agent en arrière-plan, en direct — ⏎ en ouvre un, x l’arrête.",
    ),
    (
        "plan",
        "Lire en entier le plan que Glut a envoyé depuis le mode plan, avec sa liste de tâches.\n\
         \n\
         /plan — le dernier plan de cette session\n\
         \n\
         En mode plan (⇧⇥), Glut lit et planifie sans rien changer, puis envoie son plan pour que tu l’approuves ou le \
         renvoies. /send plan le passe à un autre pane pour le construire.",
    ),
    (
        "scrollback",
        "Mettre la conversation dans l’historique propre du terminal, pour la faire défiler, la chercher et la copier \
         avec les outils du terminal.\n\
         \n\
         /scrollback — imprime la conversation ; ⏎ ou ⌃C revient à Glut\n\
         \n\
         F3 fait de même.",
    ),
    (
        "split",
        "Ouvrir une nouvelle session dans un pane à côté du principal.\n\
         \n\
         /split — un nouveau pane ; dans un dépôt git, Glut demande où il travaille\n\
         /split here — il travaille dans ce dossier, à côté de toi\n\
         /split tree [branch] — il travaille dans un nouveau Worktree, sur une branche à lui\n\
         /split resume — il ouvre une session antérieure, choisie dans la liste\n\
         /split as <role> — il joue un rôle (/role les liste)\n\
         \n\
         Jusqu’à six panes se tiennent à côté du principal, en deux rangées de trois. ⌃P fait le tour des panes, ⌃X 2 \
         divise aussi, ⌃X } et ⌃X { rendent le pane principal plus large ou plus étroit. Deux panes ici modifient les \
         mêmes fichiers ; un Worktree les tient séparés jusqu’à ce que /close demande comment il se termine.\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "swap",
        "Faire échanger leurs places au pane principal et à un autre.\n\
         \n\
         /swap — ce pane devient le principal\n\
         /swap <n> — le pane n devient le principal",
    ),
    (
        "close",
        "Fermer un pane, ou terminer un agent en arrière-plan.\n\
         \n\
         /close — ferme ce pane (jamais le principal) ; pendant qu’il travaille, un second /close l’arrête et le \
         ferme\n\
         /close <name> — termine cet agent en arrière-plan, son tour arrêté\n\
         /close <n> — ferme le pane n depuis là où tu es, par son numéro, son nom ou son rôle ; tu restes dans ton \
         pane\n\
         /close seul, s'il y a plus que ce pane à fermer — une liste, ce pane en premier : ⏎ ferme celui choisi\n\
         \n\
         La session est gardée pour /resume. Un pane dans un Worktree demande comment le Worktree se termine : le \
         garder avec sa branche, le reprendre (le fusionner dans la branche où tu es) ou l’abandonner. ⌃X 0 ferme \
         aussi ce pane.",
    ),
    (
        "send",
        "Envoyer un message à un autre pane ou à un agent en arrière-plan, reçu là-bas comme si tu l’avais tapé.\n\
         \n\
         /send — une liste de destinataires : les panes et agents d'ici, puis les autres fenêtres Glut ; ⏎ met /send \
         <who> dans la saisie\n\
         /send <text> — au seul autre pane\n\
         /send <n|name> <text> — au pane n ou à l’agent de ce nom (nécessaire à partir de trois panes)\n\
         /send plan — passe le plan de ce pane à l’autre pane pour le construire\n\
         /send <n|name> plan — le passe à celui-là\n\
         \n\
         Le message démarre aussitôt là où la session se repose, sinon après son tour. Les touches restent dans ce \
         pane.",
    ),
    (
        "role",
        "Faire jouer un rôle à ce pane : le prompt et les outils d’un agent nommé, jamais son modèle.\n\
         \n\
         /role — les rôles au choix\n\
         /role <name> — joue ce rôle à partir de maintenant\n\
         /role none — de nouveau un généraliste, avec tous les outils\n\
         \n\
         Glut apporte reviewer, tester et docs ; tes propres rôles vivent dans `~/.glut/agents`, ceux d’un projet dans \
         son `.glut/agents` (lus seulement dans un dossier de confiance). Les outils d’un rôle prennent effet \
         aussitôt, son prompt dès le tour suivant. Un rôle qui restreint les outils a besoin du Gate.",
    ),
    (
        "bg",
        "Lancer un agent en arrière-plan : une session sans pane, joignable par son nom.\n\
         \n\
         /bg — un agent nommé agent (ou agent-2, …) ; dans un dépôt git, Glut demande où il travaille\n\
         /bg <name> — un agent de ce nom (minuscules, chiffres et tirets)\n\
         /bg as <role> — il joue un rôle et porte son nom\n\
         /bg here — il travaille dans ce dossier\n\
         /bg tree [branch] — il travaille dans un nouveau Worktree\n\
         \n\
         /send <name> lui donne du travail, /peek <name> lit son flux, /show <name> le ramène au premier plan, /close \
         <name> le termine. Chaque agent travaille et coûte pour lui-même : Glut demande une fois par fenêtre avant un \
         septième. ⌃X B en lance un avec son rôle choisi, ⌃X A les liste.\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "Ramener un agent de l’arrière-plan au premier plan comme pane.\n\
         \n\
         /show <name> — l’agent devient le dernier pane de la grille\n\
         \n\
         Quand la fenêtre n’a plus de place, /hide ou /close d’abord un pane. ⌃X A liste les agents en arrière-plan.",
    ),
    (
        "hide",
        "Envoyer ce pane en arrière-plan, sa mémoire gardée.\n\
         \n\
         /hide — le pane quitte l’écran et continue de travailler comme agent en arrière-plan\n\
         \n\
         Il garde un nom pour qu’on le joigne (son rôle, ou pane-<n>) : /show <name> le ramène, /send <name> et /peek \
         <name> l’atteignent. Le pane principal et le dernier restent. ⌃X H fait de même.",
    ),
    (
        "peek",
        "Lire le flux d’un agent en arrière-plan par-dessus ce pane, sans le ramener au premier plan.\n\
         \n\
         /peek <name> — son flux, en direct, à lire seulement\n\
         /peek — le referme\n\
         \n\
         ⎋ le ferme aussi ; /show <name> ramène l’agent au premier plan.",
    ),
    (
        "loop",
        "Lancer une tâche encore et encore dans un agent d’arrière-plan à elle, tant que Glut est ouvert.\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — lance une boucle ; le premier tour aussitôt\n\
         /loop — liste les boucles : intervalle, tours, le prochain tour et le coût jusqu’ici\n\
         /loop stop [name] — termine cette boucle, ou la seule\n\
         /loop rounds [n] — le plus de tours d’une boucle ; seul, montre la limite actuelle\n\
         /loop budget [usd] — le plus qu’une boucle peut coûter ; seul, montre la limite actuelle\n\
         /loop tokens [n] — le plus de tokens qu’une boucle sur un abonnement peut utiliser (5M, 500k) ; seul, montre \
         la limite actuelle\n\
         \n\
         Les tours ne se chevauchent jamais : l’intervalle est compté depuis la fin d’un tour, comme 30s, 5m, 2h ou \
         1h30m (de 10s à 24h). Un tour poursuit la conversation du tour précédent ; avec fresh, chaque tour en \
         commence une nouvelle. La tâche est un message, ou /skill <name> [pour quoi], /compact ou /init — aucune \
         autre commande.\n\
         \n\
         L’agent s’appelle loop (ou d’après son rôle) et prend le mode de ce pane, les « toujours » que tu as donnés \
         ici, ses dossiers à côté et son rôle ; as <role> ne peut nommer que le rôle que ce pane joue. Ses demandes \
         attendent ta réponse. Une tuile vient quand un tour demande ou que la boucle se termine ; avec notify, après \
         chaque tour.\n\
         \n\
         Une boucle se termine après ses tours ou dès que ses tours coûtent son budget (par défaut 50 tours et 5 $, \
         sinon `loop.rounds` et `loop.maxUsd` de `~/.glut/settings.json` ; un changement vaut pour les boucles lancées \
         après lui), quand un tour échoue ou que tu l’arrêtes (⎋ dans son tour), ou quand son modèle appelle \
         `end_loop` parce que la fin que ta tâche nommait est atteinte. Quand les prix du modèle sont inconnus, seuls \
         les tours y mettent fin. Son agent reste ensuite dans l’onglet Agents, à lire ou à /close. x sur sa carte \
         dans l’onglet Agents (⌃X A) termine la boucle.\n\
         \n\
         /loop 10m lance les tests et corrige ce qui échoue ; arrête-toi quand tout passe\n\
         /loop 1h fresh as reviewer notify relis le dernier commit",
    ),
    (
        "schedule",
        "Des plans qui tournent quand Glut est fermé : une tâche à une heure du jour ou à intervalle régulier.\n\
         \n\
         /schedule — l’onglet Exécutions (⌃X R) : chaque plan avec sa prochaine heure et son dernier résultat\n\
         /schedule list — les plans dans le flux\n\
         /schedule add <name> [flags] <task> — crée un plan, ou modifie celui de ce nom\n\
         /schedule run <name> — le lance maintenant, à côté de Glut\n\
         /schedule pause <name> — le suspend jusqu’à resume\n\
         /schedule resume <name> — il tourne de nouveau à ses heures\n\
         /schedule remove <name> — le plan et son historique disparaissent ; ses sessions restent\n\
         \n\
         Les flags sont ceux de `glut schedule add` :\n\
         - `--at 07:00` une heure du jour, avec `--days mo-fr` ou `sa,su` (tous les jours sans)\n\
         - `--every 2h` un intervalle de 10m à 7d, à la place de --at\n\
         - `--mode ask|build|plan|dont_ask|auto` ce qu’il peut faire sans règle ; ask s’il n’est pas nommé\n\
         - `--allow <rule>`, `--deny <rule>` des règles dans la forme de Claude Code, comme `Bash(cargo test:*)` ; \
         répétable\n\
         - `--folder <path>` un dossier à côté du sien ; répétable\n\
         - `--max-usd 1`, `--max-minutes 30` le plus qu’une exécution peut coûter et durer : 1 $ et 30 min s’ils ne \
         sont pas nommés\n\
         - `--max-tokens 1M` sur un abonnement, le plus de tokens qu’une exécution peut utiliser : 1M s’il n’est pas \
         donné\n\
         - `--replace` les règles et dossiers donnés prennent la place de ceux du plan au lieu de s’y ajouter\n\
         - `--cwd <path>`, `--provider <id>`, `--model <id>` son dossier (celui-ci sinon), son fournisseur et son \
         modèle, fixés à la création\n\
         \n\
         La tâche est un message, ou /skill <name> [pour quoi]. launchd lance chaque exécution, seulement pendant que \
         tu es connecté ; une heure manquée pendant la veille est rattrapée une fois au réveil. Personne n’est là pour \
         répondre : toute demande est refusée et notée, alors autorise ce dont il a besoin. Une exécution est une \
         session gardée à part, annoncée par une notification macOS. Dans l’onglet Exécutions : ⏎ ouvre un plan ou la \
         session d’une exécution, r le lance maintenant, p le suspend ou le reprend, d le supprime.\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" lance les tests et signale ce qui \
         échoue",
    ),
    (
        "sessions",
        "Écrire aux autres fenêtres Glut de cette machine.\n\n/sessions — leurs panes et agents : adresse, projet, \
        modèle, mode, au travail ou en attente ; ⏎ met /send <address> dans la saisie\n/sessions off — cette fenêtre \
        ne prend plus leurs messages\n/sessions on — et de nouveau\n\nUn message du propriétaire là-bas entre \
        aussitôt ; celui d'un modèle là-bas seulement s'il travaille dans le mode du pane qu'il atteint, sinon il \
        attend au-dessus de la saisie : ⏎ le laisse entrer, Esc le jette. Aucun n'est ta parole au modèle d'ici. Seul \
        Glut parle aux autres fenêtres — une commande du modèle ne les atteint jamais.",
    ),
    (
        "name",
        "Nommer cette fenêtre parmi les fenêtres Glut de cette machine.\n\n/name — le nom qu'a cette fenêtre\n/name \
        <new> — un nouveau : lettres, chiffres, - _ et .\n\nLes autres fenêtres atteignent celle-ci par son nom, un \
        pane par name/2 ou name/reviewer. Un nom que tu donnes reste avec la session et revient avec /resume.",
    ),
    (
        "diff",
        "Revoir ce que le dernier tour a changé, hunk par hunk.\n\
         \n\
         /diff — les changements du dernier tour qui a changé des fichiers\n\
         /diff session — chaque changement de cette session\n\
         \n\
         Dans la revue : ↑↓ un hunk, ←→ un fichier, a accepte, r rejette (le hunk est rétabli, et Glut lit pourquoi \
         avec ton prochain message), c commente (envoyé avec ton prochain message), v marque un fichier comme revu, s \
         bascule entre tour et session, ⎋ ferme. ⌃R l’ouvre aussi ; /review est la même commande.",
    ),
    (
        "steps",
        "Voir les étapes qui ont changé des fichiers, et rétablir des fichiers.\n\
         \n\
         /steps — chaque étape qui a changé un fichier ; ⏎ sur l’une montre ce qu’elle a changé, ou rétablit les \
         fichiers d’avant elle\n\
         \n\
         Rétablir des fichiers emporte avec soi tes modifications depuis ; la conversation reste. Glut garde un \
         snapshot avant chaque étape qui change quelque chose ; l’état des fichiers juste avant est gardé aussi, si \
         bien que `glut undo` le ramène. ⎋⎋ revient en arrière dans la conversation à la place.",
    ),
    (
        "todo",
        "Tenir les tâches du projet dans son `TODO.md`, une liste à cocher que toi et Glut utilisez tous deux.\n\
         \n\
         /todo — l’onglet À faire : Espace coche un élément, a en ajoute un, o ouvre l’éditeur\n\
         /todo <text> — ajoute un élément à la fin de la liste, même pendant que Glut travaille\n\
         /todo open — la liste dans ton éditeur\n\
         /todo clean — retire les éléments terminés, après confirmation\n\
         \n\
         Le reste du fichier n’est jamais touché. ⌃Y montre ou masque l’onglet.",
    ),
    (
        "memory",
        "Voir ce que garde la mémoire du projet : les faits que Glut a notés pour les sessions suivantes.\n\
         \n\
         /memory — les faits avec leurs descriptions ; ⏎ en montre un en entier\n\
         /memory open — l’index (`MEMORY.md`) dans ton éditeur\n\
         \n\
         La mémoire se trouve hors du dépôt : dans `~/.glut/memory/<project>/`, ou dans la mémoire de Claude Code là \
         où Claude Code travaille dans le dépôt. Glut l’écrit avec son outil `memory` ; la lire ici ne sollicite aucun \
         modèle.",
    ),
    (
        "skill",
        "Lancer un Skill : des instructions pour un genre de tâche, que le modèle charge et suit.\n\
         \n\
         /skill — les Skills au choix\n\
         /skill <name> [pour quoi] — lance ce Skill sur ce qui suit son nom\n\
         \n\
         Glut apporte make-agent, make-skill et make-hook pour écrire les tiens. Les tiens vivent dans \
         `~/.glut/skills`, ceux d’un projet dans son `.glut/skills` (lus seulement dans un dossier de confiance) ; un \
         Skill de même nom masque celui de Glut.\n\
         \n\
         /skill make-hook bloque git push sur main",
    ),
    (
        "init",
        "Écrire un court `AGENTS.md` pour ce dossier, ou vérifier et raccourcir le guide qu’il a.\n\
         \n\
         /init — sans guide, Glut écrit `AGENTS.md` ; avec un guide, il le vérifie et le garde court\n\
         /init --migrate — redemande si `CLAUDE.md` passe dans `AGENTS.md`\n\
         \n\
         Avec un `CLAUDE.md` seul, Glut demande une fois s’il passe dans `AGENTS.md` (`CLAUDE.md` l’importe alors, un \
         seul guide pour tous les harness) ; un non est retenu. Les guides accompagnent chaque tour : /context montre \
         ce qu’ils coûtent.",
    ),
    (
        "gate",
        "Voir les règles d’après lesquelles le Gate décide, et retirer un « toujours ».\n\
         \n\
         /gate — les règles en vigueur : tes « toujours » de cette session, puis ceux de `~/.glut/settings.json` et du \
         `.glut/` du projet ; ⏎ sur un « toujours » le retire, et Glut redemande la prochaine fois\n\
         \n\
         ⌃G ouvre l’onglet Gate : chaque jugement en direct, les hôtes atteints et les refus. ⇧⇥ change le mode.",
    ),
    (
        "mcp",
        "Voir les serveurs MCP et l’état de chacun, se connecter à un serveur hébergé, en ajouter ou en retirer un.\n\
         \n\
         /mcp — chaque serveur : connecté avec ses outils, en attente d’une connexion, ou en échec\n\
         /mcp login <name> — se connecte à un serveur hébergé ; son adresse s’ouvre dans ton navigateur sur ⏎\n\
         /mcp login <name> <address> — par SSH : l’adresse où le navigateur a fini, collée en retour\n\
         /mcp logout <name> — se déconnecte et oublie son token\n\
         /mcp add <name> <url> — ajoute un serveur hébergé à `~/.glut/mcp.json` et le connecte\n\
         /mcp add <name> -- <command> [args] — ajoute un serveur que Glut lance lui-même\n\
         /mcp remove <name> — le retire de `~/.glut/mcp.json` et de la session\n\
         \n\
         Un serveur qui n’offre aucune connexion que Glut sache utiliser demande un token, saisi masqué. Les serveurs \
         propres à un projet sont au projet de les changer : /mcp remove ne retire que les tiens. `glut mcp` fait de \
         même hors d’une session.\n\
         \n\
         Pendant qu'un de ses outils tourne, un serveur peut te demander : un formulaire (⇥ ↑↓ entre les champs, \
         espace coche, ⏎ envoie, Esc ferme, ⌃D refuse) ou une page à ouvrir (o). Toi seul y réponds, jamais le modèle \
         ; un formulaire qui demande un mot de passe ou une clé est refusé, et sans personne toute question est \
         refusée.",
    ),
    (
        "images",
        "Choisir comment les images sont dessinées dans le flux.\n\
         \n\
         /images inline — la vraie image là où le terminal peut en montrer une\n\
         /images blocks — des demi-blocs en couleurs vraies\n\
         /images off — seulement la ligne de l’image\n\
         \n\
         /images seul montre le choix actuel. Par défaut, Glut dessine en ligne là où le terminal le peut et en blocs \
         ailleurs. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "links",
        "Choisir comment les liens apparaissent dans le flux.\n\
         \n\
         /links clickable — le texte du lien est le lien, là où le terminal parle OSC 8\n\
         /links plain — l’URL est écrite après le texte\n\
         \n\
         /links seul montre le choix actuel. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "notify",
        "Être prévenu quand un tour se termine ou que Glut demande, tant que la fenêtre n’est pas au premier plan.\n\
         \n\
         /notify on — la fin d’un tour et chaque demande\n\
         /notify asks — seulement les demandes\n\
         /notify off — rien\n\
         /notify system — par les notifications propres de macOS (un clic sur l’une ouvre Éditeur de script, pas le \
         terminal)\n\
         /notify after <s> — la fin d’un tour n’est annoncée que si elle a duré autant de secondes (10 par défaut, \
         jusqu’à 3600)\n\
         /notify tiles on|off — les tuiles en haut à droite de la fenêtre\n\
         \n\
         /notify seul montre ce qui vaut maintenant. Glut utilise les notifications propres du terminal là où il sait \
         qu’il les a, et la cloche. Une notification nomme Glut, le projet, le pane et ce qui s’est passé, jamais les \
         mots du modèle. Gardé dans `~/.glut/prefs.json`, avant `notify` dans `~/.glut/settings.json`.",
    ),
    (
        "secrets",
        "Voir et régler les clés cachées au modèle dans cette session.\n\
         \n\
         /secrets — chaque valeur cachée jusqu’ici, par numéro, genre, longueur et lieu où elle a été vue (jamais la \
         valeur)\n\
         /secrets allow <n> — laisse passer le secret n pour cette session\n\
         /secrets allow <n> always — le laisse passer toujours (son empreinte est gardée, jamais la valeur)\n\
         /secrets add <name> <regex> — cache un format à toi, dès le prochain résultat\n\
         /secrets remove <name> — retire un format à toi\n\
         /secrets on|off — le masquage activé ou désactivé\n\
         \n\
         Une clé dans ce que répond un outil parvient au modèle sous la forme ‹secret n: …› ; Glut ne remet sa valeur \
         que dans un fichier qui la contient déjà. Un message de toi qui semble contenir une clé fait l’objet d’une \
         question avant de partir. Le modèle apprend un changement avec ton prochain message. Tes formats et tes choix \
         sont gardés dans `~/.glut/secrets.json`.\n\
         \n\
         Le masquage protège d’une clé montrée par accident ; la sandbox et le Gate réseau restent les barrières.",
    ),
    (
        "keychain",
        "Tenir le trousseau hors de portée des commandes que Glut exécute.\n\
         \n\
         /keychain — s'il est ouvert, et les programmes qui peuvent le demander\n\
         /keychain add <program> — met un programme sur la liste (gh, ou git et une sous-commande : git push)\n\
         /keychain remove <program> — le retire de la liste\n\
         /keychain open — l'ouvre à toute commande, jusqu'à la fin de cette session\n\
         /keychain close — le referme\n\
         \n\
         Une clé ou une connexion dans le trousseau s'atteint par-delà toutes les règles de fichiers du bac à sable : \
         une commande que Glut exécute trouve donc le trousseau fermé. Une commande d'un programme listé demande \
         d'abord — dans tous les modes, `auto` aussi — et s'exécute avec le trousseau ouvert dès que tu l'autorises ; \
         un « toujours » en fait une règle à toi. Quand personne n'est là (un plan, `glut run`), seule \
         une telle règle l'ouvre. Quand une commande échoue sur le trousseau fermé, Glut le dit sous sa sortie.\n\
         \n\
         Autoriser une commande donne aussi le trousseau à tout ce qu'elle lance : un hook git, une extension gh ou \
         un script de build peut alors y lire n'importe quelle clé.\n\
         \n\
         Seules vos propres règles donnent le trousseau : une règle d'autorisation du `.glut/` d'un projet laisse une \
         commande s'exécuter, jamais avec le trousseau ouvert. Une chaîne ne l'obtient que si chaque partie est \
         listée ou ne fait que lire. Ce que `glut keychain open` garde ouvert ne compte qu'ici, où vous êtes — jamais \
         dans `glut run` ni dans l'exécution d'un hôte.\n\
         \n\
         Une de vos règles ne l'ouvre sans demander que si elle nomme un programme et sa sous-commande (`gh pr *`, \
         `git push *`) — jamais un programme entier (`gh *`). Une carte 🔑 montre toute la commande, ↑↓ pour la \
         parcourir, et n'autorise qu'une fois lue jusqu'au bout.\n\
         \n\
         La liste est gardée dans `~/.glut/keychain.json` ; `glut keychain open` garde le trousseau ouvert d'une \
         session à l'autre, `glut keychain close` le ferme pour de bon.",
    ),
    (
        "clean",
        "Empêcher les données de Glut dans ~/.glut de grossir sans fin.\n\
         \n\
         /clean — ce que Glut garde, poste par poste, et ce qui peut partir selon vos délais\n\
         /clean now — nettoie tout de suite, en arrière-plan\n\
         /clean keep — les délais\n\
         /clean keep <kind> <days>d — en fixe un : snapshots, journals, sessions, results ou lean\n\
         \n\
         Ce qui part : les instantanés d'une exécution plus vieux que leur délai (7 jours) — tout le dépôt d'un \
         dossier quand il n'en reste aucun, le dossier présent ou non ; le journal d'une exécution inachevée (7 \
         jours) ; une session non écrite depuis 90 jours, avec son journal d'événements et ses longs résultats ; les \
         longs résultats sans session après 30 jours ; les images qu'aucune session gardée ne cite ; les lignes de \
         lean.jsonl de plus de 90 jours. Les exports sont seulement comptés : ce sont vos fichiers.\n\
         \n\
         Une fois par jour, Glut nettoie de lui-même au démarrage — ici et dans `glut run` — et dit ce qu'il a \
         libéré. Les sorties de build et les caches (un dossier avec un CACHEDIR.TAG, sous node_modules/ ou le \
         target/ d'un projet Cargo) n'ont aucun instantané.\n\
         \n\
         Les délais sont gardés dans `~/.glut/clean.json`. En ligne de commande : `glut clean`, `glut clean \
         --dry-run`, `glut clean keep sessions 30d` ; `glut clean --dry-run --json` écrit le rapport pour un \
         programme.",
    ),
    (
        "motion",
        "Choisir à quel point l’écran bouge.\n\
         \n\
         /motion full — Glimm respire, des étincelles montent, la flamme vacille\n\
         /motion calm — tout reste immobile ; une image n’est dessinée que si quelque chose a changé\n\
         /motion off — comme calm\n\
         \n\
         /motion seul montre le choix actuel. « Réduire les animations » de macOS et le mode lecteur tiennent l’écran \
         immobile quoi que tu choisisses. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "rule",
        "Une règle pour cette session que le modèle suit à chaque tour.\n\
         \n\
         /rule <text> — la garder, comme « mets à jour la liste de tâches à chaque étape »\n\
         /rule — les règles actuelles, numérotées\n\
         /rule remove <n> — retirer la règle n\n\
         \n\
         Les règles accompagnent les instructions de chaque tour, après les guides, jusqu'à ce que tu les retires ; \
         /clear les garde, une nouvelle session commence sans. Au plus 20, de 500 caractères chacune. Pour une règle \
         de chaque session, écris-la dans `~/.glut/AGENTS.md` ou le guide du projet.",
    ),
    (
        "maxsteps",
        "Combien d'étapes un tour fait au plus.\n\
         \n\
         /maxsteps — la limite actuelle\n\
         /maxsteps <n> — un tour s'arrête après n étapes, de 1 à 1000\n\
         \n\
         Une étape est un appel du modèle. À la limite le tour s'arrête et le dit ; ⏎ sur la saisie vide envoie « Go \
         on where you stopped. » et le travail continue. Les budgets et la limite de temps restent les vrais freins. \
         Gardé dans `~/.glut/prefs.json` ; `maxTurns` dans `~/.glut/settings.json` le fixe pour chaque exécution, \
         `glut run --max-turns` pour une.",
    ),
    (
        "nudge",
        "Si Glut demande au modèle de continuer à la fin d'un tour, de tenir sa liste de tâches et d'en écrire une \
         quand il travaille sans.\n\
         \n\
         /nudge on — quand un tour s'arrête avec des points de la liste de tâches ouverts, ou sur des mots qui \
         annoncent une étape (« ensuite je… », deux-points à la fin), Glut envoie au modèle une ligne à lui : \
         continuer ou dire pourquoi il s'arrête ; le fil le montre discrètement. Quand la liste ne change pas pendant \
         qu'il travaille, Glut la lui rappelle, et si rien ne change, une fois par un message à part ; s'il n'en a \
         écrit aucune dans une exécution, Glut lui en demande une, une fois par tour\n\
         /nudge off — un tour s'arrête là où le modèle l'arrête : ni rappel de la liste ni demande d'une\n\
         \n\
         La question de fin vient une fois par tour, jamais en mode plan, après une question qui t'est posée, dans un \
         tour de boucle ou l'exécution d'un plan ; le rappel et la demande suivent le même choix, mais dans \
         l'exécution d'un plan Glut rappelle toujours. /nudge seul montre le choix actuel. Gardé dans \
         `~/.glut/prefs.json` ; `\"nudge\": false` dans `~/.glut/settings.json` le coupe pour chaque exécution.",
    ),
    (
        "lsp",
        "Si les serveurs de langage aident le modèle.\n\
         \n\
         /lsp on — après une modification, les erreurs du fichier viennent avec le résultat, et le modèle a l'outil \
         code : où un symbole est défini, chaque endroit où il sert, son type et sa doc, et un renommage dans tout le \
         projet\n\
         /lsp off — aucun serveur ne tourne : leurs processus s'arrêtent aussitôt, plus d'erreurs après une \
         modification, plus d'outil code\n\
         /lsp — le choix actuel, et chaque serveur de la session : prêt, indexe le projet, ou arrêté pour la session\n\
         \n\
         Glut ne démarre que des serveurs qui n'exécutent jamais le code du projet (rust-analyzer sans scripts de \
         build ni macros procédurales, pyright, et les serveurs de TOML, YAML, JSON, CSS, HTML et Markdown), seulement \
         dans le bac à sable sans réseau, et seulement là où le Gate permet leur ligne de commande. Un serveur \
         démarre à la première modification ou question de son langage et reste pour la session ; rust-analyzer met \
         quelques secondes à indexer un grand projet, code le dit et le modèle utilise grep. Un renommage est un seul \
         changement : demandé une fois pour tous ses fichiers, tous ou aucun, un instantané avant. Gardé dans \
         `~/.glut/prefs.json` ; `\"lsp\": false` dans `~/.glut/settings.json` l'éteint pour chaque exécution, `glut \
         run --no-lsp` pour une.",
    ),
    (
        "reader",
        "Configurer Glut pour un lecteur d’écran.\n\
         \n\
         /reader on — les touches en mots, un pied de page sobre, pas de Glimm, des cadres simples, rien qui bouge et \
         une ligne de texte à chaque changement d’état\n\
         /reader off — l’écran habituel\n\
         \n\
         /reader seul montre le choix actuel. Gardé dans `~/.glut/prefs.json` ; `glut --reader` le maintient activé \
         pour une exécution.",
    ),
    (
        "language",
        "Choisir la langue dans laquelle Glut te parle.\n\
         \n\
         /language — les langues au choix\n\
         /language <code> — en, de, fr, it, es, zh, ja ou pt\n\
         \n\
         Sans choix, Glut parle la langue de ton système. Le modèle répond dans la langue où tu écris, quoi que tu \
         choisisses ici. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "theme",
        "Choisir les couleurs de Glut.\n\
         \n\
         /theme — les thèmes au choix\n\
         /theme <name> — glut, celui de Glut, ou l’un des thèmes d’Axiomata (catppuccin-mocha, github-dark, …)\n\
         \n\
         Glut ne colore que le texte et garde le fond du terminal : un thème clair demande un terminal clair. Gardé \
         dans `~/.glut/prefs.json` ; dans une fenêtre ouverte par une application hôte, le choix vaut pour cette \
         session.\n\
         \n\
         Chaque thème donne aux parties du pied de page, aux noms des outils selon leur genre (lire, écrire, \
         commandes, ce qui sort), aux panes et aux agents et à la ligne au-dessus de la saisie leurs propres couleurs \
         douces ; ce qui a besoin de toi reste vif. Le mode lecture et `NO_COLOR` restent sobres.",
    ),
    (
        "editor",
        "Choisir l’éditeur dans lequel Glut ouvre les fichiers et la saisie.\n\
         \n\
         /editor — les éditeurs trouvés sur ton système\n\
         /editor <command> — ta propre commande, avec `{file}` et `{line}` là où ils vont\n\
         \n\
         Sans `{file}`, le fichier est ajouté à la fin. Un éditeur à fenêtre doit attendre que le fichier soit fermé \
         (`code --wait`). ⌃X E y modifie la saisie ; /todo open et /memory open s’en servent aussi. Gardé dans \
         `~/.glut/prefs.json`.\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "Choisir comment une session au repos garde le cache de prompt du fournisseur, pour que le tour suivant \
         démarre au chaud.\n\
         \n\
         /cache ping — Glut relit le cache toutes les quelques minutes pendant que la session se repose, jusqu’à 30 \
         minutes\n\
         /cache 1h — Anthropic le garde une heure (chaque écriture coûte le double) ; chez les autres fournisseurs, il \
         est relu pour être gardé\n\
         /cache off — pas gardé : une pause au-delà des quelques minutes du fournisseur repart à froid\n\
         \n\
         /cache seul montre le choix actuel ; ping est la valeur par défaut. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "footer",
        "Choisir comment le pied de page joint ses parties.\n\
         \n\
         /footer powerline — les flèches de Powerline\n\
         /footer plain — des blocs simples\n\
         \n\
         /footer seul montre le choix actuel et pourquoi. Sans choix, Glut devine d’après le terminal s’il dessine les \
         glyphes de Powerline. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "vim",
        "Modifier la saisie à la manière de vi.\n\
         \n\
         /vim on — ⎋ passe en mode Normal (déplacements, opérateurs, comptes, u et ⌃R), i a I A o O reviennent en mode \
         Insertion\n\
         /vim off — les touches d’édition habituelles\n\
         \n\
         /vim seul l’active. ⎋ en mode Normal arrête Glut. Gardé dans `~/.glut/prefs.json`.",
    ),
    (
        "help",
        "Chaque commande et chaque touche, dans une boîte à part.\n\
         \n\
         /help — les commandes avec une ligne chacune ; taper les filtre, ↑↓ choisissent, ⏎ ouvre la page entière de \
         l’une d’elles\n\
         /help <command> — la page de cette commande aussitôt, comme /help loop\n\
         \n\
         Sur une page, ↑↓ et PgUp PgDn font défiler, ⎋ revient à la liste et ferme la boîte depuis là. La dernière \
         entrée contient chaque touche par sujet, !cmd et le mode vi ; ⌃K garde les touches dans le panneau latéral.",
    ),
];
