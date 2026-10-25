//! Français.

use glut::lang::Plural;

use super::Texts;

pub const TEXTS: Texts = Texts {
    language_set: "langue : {name}",
    language_unkept: "langue : {name} — pour cette exécution seulement : {reason}",
    language_unknown: "aucune langue {name} — {known}",
    language_title: "Langue de Glut",
    language_now: "actuelle",
    panes_count: Plural {
        one: "{n} pane",
        other: "{n} panes",
    },
    topics: &[
        "Session",
        "Modèle",
        "Affichage",
        "Panes",
        "Modifications",
        "Projet",
        "Réglages",
        "Saisie",
        "Modifier la ligne",
        "Aide",
    ],
    commands: &[
        ("clear", "commencer une nouvelle session"),
        (
            "resume",
            "ouvrir une session antérieure de ce dossier — les bifurcations sous la leur",
        ),
        (
            "projects",
            "continuer dans un autre projet : les dossiers où Glut a travaillé, ou un par son chemin",
        ),
        ("compact", "résumer la conversation jusqu’ici"),
        (
            "handoff",
            "/handoff write : ce qui est fait et ce qui vient ensuite ; /handoff read dans une nouvelle session",
        ),
        ("quit", "terminer la session"),
        ("exit", "terminer la session, comme /quit"),
        (
            "model",
            "choisir un modèle du fournisseur utilisé, ou /model <id>",
        ),
        ("provider", "changer de fournisseur, ou /provider <id>"),
        (
            "effort",
            "à quel point le modèle réfléchit, ou /effort <level>",
        ),
        (
            "context",
            "à quel point la fenêtre est pleine ; /context 64k la règle",
        ),
        (
            "thinking",
            "la réflexion du modèle : /thinking off, folded ou open",
        ),
        (
            "panel",
            "le panneau latéral sur le plan, ou le masquer (⌃B ; ⌃G le gate)",
        ),
        (
            "meter",
            "contexte, tokens et coût par tour et par outil, les économies de lean (⌃N)",
        ),
        (
            "agents",
            "les sous-agents de la session : ouvrir le flux de l’un d’eux (un clic sur sa tuile aussi ; ⌃X A : \
             l’onglet Agents)",
        ),
        (
            "plan",
            "le plan que Glut a envoyé depuis le mode plan, entier, avec sa liste de tâches",
        ),
        (
            "scrollback",
            "la conversation dans l’historique du terminal (F3)",
        ),
        (
            "split",
            "une nouvelle session à côté du pane principal (jusqu’à six, deux rangées de trois) ; dans un dépôt git \
             ici ou dans un worktree",
        ),
        (
            "swap",
            "le pane principal et le pane n (ou celui-ci) échangent leurs places",
        ),
        (
            "close",
            "fermer ce pane (pas le principal) — ou arrêter un agent en arrière-plan ; sa session reste pour /resume",
        ),
        (
            "send",
            "un message à un autre pane (à partir de trois panes, son numéro d’abord), reçu là-bas comme s’il était \
             tapé ; /send plan lui passe ton plan à construire",
        ),
        (
            "role",
            "le rôle que joue ce pane : le prompt et les outils d’un agent nommé, pas son modèle — reviewer, \
             tester, docs ou les tiens",
        ),
        (
            "bg",
            "un agent en arrière-plan : une session sans pane, joignable par son nom — /bg as reviewer",
        ),
        (
            "show",
            "ramener un agent de l’arrière-plan au premier plan comme pane",
        ),
        (
            "hide",
            "envoyer ce pane en arrière-plan, sa mémoire gardée (⌃X H)",
        ),
        (
            "peek",
            "lire le flux d’un agent en arrière-plan par-dessus ce pane ; Esc le ferme",
        ),
        (
            "diff",
            "revoir ce que le dernier tour a changé, hunk par hunk ; /diff session (⌃R)",
        ),
        (
            "steps",
            "les étapes qui ont changé des fichiers : ce que chacune a changé, ou les fichiers rétablis",
        ),
        (
            "todo",
            "le TODO.md du projet (⌃Y) : /todo <text> en ajoute un, /todo open le modifie, /todo clean retire les \
             tâches terminées",
        ),
        (
            "memory",
            "ce que garde la mémoire du projet : ⏎ montre un fait ; /memory open modifie son index",
        ),
        (
            "skill",
            "lancer un skill : /skill <name> [pour quoi] — /skill seul les liste (make-agent, make-skill, \
             make-hook, …)",
        ),
        (
            "init",
            "un court AGENTS.md pour ce dossier, ou vérifier son guide (/init --migrate redemande pour CLAUDE.md)",
        ),
        (
            "gate",
            "les règles en vigueur ; retirer un « toujours » (⌃G : le gate, en direct)",
        ),
        (
            "images",
            "comment les images sont dessinées : la vraie où le terminal le peut, des demi-blocs en couleurs \
             vraies, ou leur ligne seule",
        ),
        (
            "links",
            "le texte d’un lien est le lien là où le terminal parle OSC 8, sinon l’URL écrite après",
        ),
        (
            "motion",
            "à quel point l’écran bouge ; calm et off tiennent Glimm immobile (« Réduire les animations » de macOS \
             compte comme off)",
        ),
        (
            "reader",
            "pour un lecteur d’écran : touches en mots, un écran sobre, rien ne bouge, une ligne à chaque \
             changement d’état",
        ),
        (
            "language",
            "la langue dans laquelle Glut te parle : en de fr it es zh ja pt — le modèle répond dans celle où tu écris",
        ),
        (
            "theme",
            "les couleurs de Glut, ou celles d’un thème d’Axiomata : /theme <name>",
        ),
        ("editor", "choisir ton éditeur, ou /editor <command>"),
        (
            "cache",
            "comment une session au repos garde son cache : relu (ping), l’heure d’Anthropic (1h), ou pas du tout",
        ),
        (
            "footer",
            "les jonctions du pied de page : flèches Powerline ou blocs simples (devinés d’après le terminal)",
        ),
        (
            "vim",
            "modifier la saisie à la manière de vi : /vim on ou off",
        ),
        ("help", "commandes et touches"),
    ],
    keys: &[
        ("⇧⇥", "le mode suivant : plan → ask → build → auto"),
        ("⎋", "arrêter Glut"),
        (
            "⏎",
            "pendant que Glut travaille : l’orienter, pris en compte avant son étape suivante",
        ),
        (
            "⇥",
            "pendant que Glut travaille : mettre le message en attente pour après",
        ),
        ("⌥↑", "reprendre le dernier message en attente"),
        ("⌃C ⌃C", "quitter"),
        ("⌃F", "chercher dans le flux"),
        ("⌃O", "déplier ou replier chaque résultat d’outil"),
        ("⌃T", "ouvrir ou replier chaque réflexion"),
        ("⌃B", "le panneau latéral sur le plan, ou le masquer"),
        (
            "⌃G",
            "le gate, en direct : chaque jugement, les hôtes, les refus",
        ),
        (
            "⌃N",
            "le compteur : contexte, tokens et coût par tour et par outil",
        ),
        (
            "⌃X A",
            "l’onglet Agents : chaque sous-agent et les agents en arrière-plan, en direct",
        ),
        (
            "⌃X ↑",
            "lire le flux bloc par bloc : ⏎ replie, c copie, ⎋ retour",
        ),
        (
            "⌃X ⇥",
            "le panneau latéral prend les touches : ←→ les onglets, ↑↓ dedans, ⎋ retour",
        ),
        ("PgUp PgDn", "faire défiler le flux (⇧↑ ⇧↓ une ligne)"),
        ("F3", "la conversation dans l’historique du terminal"),
        ("F2", "donner la souris au terminal et la reprendre"),
        (
            "⌃P",
            "le pane suivant, à tour de rôle dans l’ordre de lecture (⌃] aussi : ⌃6 sur un clavier allemand)",
        ),
        ("⌃X 2", "un nouveau pane à côté du principal (⌃X 3 aussi)"),
        (
            "⌃X }",
            "le pane principal plus large (la souris tire aussi sa bordure)",
        ),
        ("⌃X {", "le pane principal plus étroit"),
        ("⌃X 0", "fermer ce pane"),
        ("⌃X B", "un agent en arrière-plan, son rôle choisi d’abord"),
        ("⌃X H", "ce pane en arrière-plan"),
        (
            "⌃R",
            "revoir ce que le dernier tour a changé, hunk par hunk",
        ),
        (
            "⎋⎋",
            "revenir en arrière ou bifurquer avant un de tes messages",
        ),
        ("⌃Y", "l’onglet Tâches : Espace coche, a ajoute"),
        ("/", "les commandes"),
        ("@", "mentionner un fichier"),
        (
            "⌃V",
            "l’image du presse-papiers dans le message (un fichier image déposé aussi)",
        ),
        ("↑", "les messages envoyés avant"),
        ("⇧⏎", "une nouvelle ligne (⌥⏎ aussi)"),
        ("⌃X E", "modifier la saisie dans ton éditeur"),
        ("⌃A ⌃E", "le début de la ligne, sa fin (Début Fin aussi)"),
        ("⌥B ⌥F", "un mot à gauche, un mot à droite (⌥← ⌥→ aussi)"),
        ("⌃W", "supprimer le mot d’avant (⌥⌫ aussi)"),
        ("⌃U", "supprimer jusqu’au début de la ligne"),
        ("⌃J", "une nouvelle ligne"),
        (
            "⌃D",
            "quitter, sur une ligne vide pendant que Glut se repose",
        ),
        ("⌃K", "cette feuille de touches"),
    ],
    help_commands: "Commandes",
    help_keys: "Touches ({legend} ; ⌃K les garde dans le panneau latéral)",
    help_also: "Aussi : !cmd lance une commande et montre sa sortie à Glut · !!cmd la lance pour toi seul · un \
                grand collage apparaît comme [pasted N lines] · glisser pour copier · un clic sur la tuile d’un \
                sous-agent l’ouvre",
    help_vim: "/vim on : ⎋ vers le mode Normal (h j k l w b e 0 ^ $ gg G · x r ~ J D C s S · d c y avec un \
               mouvement ou doublés · nombres · p P · u ⌃R), i a I A o O pour le mode Insertion ; ⎋ en mode Normal \
               arrête Glut",
    legend: "⌃ Contrôle · ⇧ Maj · ⌥ Option · ⏎ Retour · ⇥ Tab · ⎋ Échap",
};
