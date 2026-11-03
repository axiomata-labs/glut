Einen Agenten im Hintergrund starten: eine Session ohne Pane, erreichbar über ihren Namen.

/bg — ein Agent namens agent (oder agent-2, …); in einem Git-Repository fragt Glut, wo er arbeitet
/bg <name> — ein Agent dieses Namens (Kleinbuchstaben, Ziffern und Bindestriche)
/bg as <role> — er spielt eine Rolle und ist nach ihr benannt
/bg here — er arbeitet in diesem Ordner
/bg tree [branch] — er arbeitet in einem neuen Worktree

/send <name> gibt ihm Arbeit, /peek <name> liest seinen Stream, /show <name> holt ihn nach vorn, /close <name> beendet ihn. Jeder Agent arbeitet und kostet für sich: Glut fragt einmal pro Fenster, bevor ein siebter startet. ⌃X B startet einen mit zuerst gewählter Rolle, ⌃X A listet sie.

/bg docs-check as docs
