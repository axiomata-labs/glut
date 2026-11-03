Dein Editor neben Glut: Neovim mit dem Plugin glut.nvim, VS Code (auch Cursor, Windsurf) oder eine JetBrains-IDE mit Gluts eigenem Plugin.

/ide — mit welchem Editor Glut verbunden ist, und welche passen
/ide on — verbinden (fragt, wenn mehrere passen)
/ide off — für diese Session trennen

Verbunden trägt jede deiner Nachrichten die aktive Datei und die Markierung mit (⧉ darunter), solange keine Leseregel die Datei zurückhält; das Modell kann fragen, welche Dateien offen sind und was die Sprachserver sagen; Glut liest einen Buffer so, wie der Editor ihn hält, auch Ungespeichertes, und eine Frage des Gates zu einer Änderung erscheint auch als Diff im Editor — die erste Antwort zählt. Glut verbindet sich beim Start selbst, wo ein Editor passt: der, der Glut gestartet hat, sonst der, dessen Ordner Glut's Ordner enthält. "⧉" in der Fußzeile zeigt die Verbindung.
