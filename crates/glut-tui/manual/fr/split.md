Ouvrir une nouvelle session dans un pane à côté du principal.

/split — un nouveau pane ; dans un dépôt git, Glut demande où il travaille
/split here — il travaille dans ce dossier, à côté de toi
/split tree [branch] — il travaille dans un nouveau Worktree, sur une branche à lui
/split resume — il ouvre une session antérieure, choisie dans la liste
/split as <role> — il joue un rôle (/role les liste)

Jusqu’à six panes se tiennent à côté du principal, en deux rangées de trois. ⌃P fait le tour des panes, ⌃X 2 divise aussi, ⌃X } et ⌃X { rendent le pane principal plus large ou plus étroit. Deux panes ici modifient les mêmes fichiers ; un Worktree les tient séparés jusqu’à ce que /close demande comment il se termine.

/split tree fix-login as reviewer
