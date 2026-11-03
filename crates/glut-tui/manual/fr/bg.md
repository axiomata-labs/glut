Lancer un agent en arrière-plan : une session sans pane, joignable par son nom.

/bg — un agent nommé agent (ou agent-2, …) ; dans un dépôt git, Glut demande où il travaille
/bg <name> — un agent de ce nom (minuscules, chiffres et tirets)
/bg as <role> — il joue un rôle et porte son nom
/bg here — il travaille dans ce dossier
/bg tree [branch] — il travaille dans un nouveau Worktree

/send <name> lui donne du travail, /peek <name> lit son flux, /show <name> le ramène au premier plan, /close <name> le termine. Chaque agent travaille et coûte pour lui-même : Glut demande une fois par fenêtre avant un septième. ⌃X B en lance un avec son rôle choisi, ⌃X A les liste.

/bg docs-check as docs
