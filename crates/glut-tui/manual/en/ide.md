Your editor beside Glut: Neovim with the plugin glut.nvim, VS Code (and Cursor, Windsurf) or a JetBrains IDE with Glut's own plugin.

/ide — which editor Glut is connected to, and which fit
/ide on — connect (asks when several fit)
/ide off — disconnect for this session

Connected, every message of yours carries the active file and the selection (⧉ under it), as long as no read rule holds the file back; the model can ask which files are open and what the language servers say; Glut reads a buffer as the editor holds it, unsaved changes too, and a question of the gate about an edit shows as a diff in the editor too — the first answer counts. Glut connects by itself as it starts where one editor fits: the one that started Glut, else the one whose folder holds Glut's. "⧉" in the footer says it is connected.
