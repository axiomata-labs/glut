Eine neue Session in einem Pane neben dem Haupt-Pane öffnen.

/split — ein neues Pane; in einem Git-Repository fragt Glut, wo es arbeitet
/split here — es arbeitet in diesem Ordner, neben dir
/split tree [branch] — es arbeitet in einem neuen Worktree auf einem eigenen Branch
/split resume — es öffnet eine frühere Session, aus der Liste gewählt
/split as <role> — es spielt eine Rolle (/role listet sie)

Bis zu sechs Panes stehen neben dem Haupt-Pane, in zwei Reihen zu drei. ⌃P geht reihum durch die Panes, ⌃X 2 teilt ebenfalls, ⌃X } und ⌃X { machen das Haupt-Pane breiter oder schmaler. Zwei Panes hier ändern dieselben Dateien; ein Worktree hält sie auseinander, bis /close fragt, wie er endet.

/split tree fix-login as reviewer
