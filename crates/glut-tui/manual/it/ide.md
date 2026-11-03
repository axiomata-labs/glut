Il tuo editor accanto a Glut: Neovim con il plugin glut.nvim, VS Code (e Cursor, Windsurf) o un IDE JetBrains con il plugin proprio di Glut.

/ide — a quale editor è collegato Glut, e quali vanno bene
/ide on — collega (chiede se ne vanno bene diversi)
/ide off — scollega per questa sessione

Collegato, ogni tuo messaggio porta il file attivo e la selezione (⧉ sotto), finché nessuna regola di lettura trattiene il file; il modello può chiedere quali file sono aperti e cosa dicono i language server; Glut legge un buffer come lo tiene l'editor, anche il non salvato, e una domanda del gate su una modifica appare anche come diff nell'editor — conta la prima risposta. Glut si collega da solo all'avvio dove va bene un editor: quello che ha avviato Glut, altrimenti quello la cui cartella contiene quella di Glut. "⧉" nel piè di pagina dice che è collegato.
