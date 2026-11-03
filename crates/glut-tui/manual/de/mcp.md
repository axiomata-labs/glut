Die MCP-Server sehen und wie jeder steht, bei einem gehosteten anmelden, einen hinzufügen oder entfernen.

/mcp — jeder Server: verbunden mit seinen Werkzeugen, wartend auf eine Anmeldung, oder fehlgeschlagen
/mcp login <name> — meldet bei einem gehosteten Server an; seine Adresse öffnet sich mit ⏎ in deinem Browser
/mcp login <name> <address> — über SSH: die Adresse, auf der der Browser landete, hier eingefügt
/mcp logout <name> — meldet ab und vergisst sein Token
/mcp add <name> <url> — fügt einen gehosteten Server zu `~/.glut/mcp.json` hinzu und verbindet ihn
/mcp add <name> -- <command> [args] — fügt einen Server hinzu, den Glut selbst startet
/mcp remove <name> — nimmt ihn aus `~/.glut/mcp.json` und aus der Session

Ein Server, der keine Anmeldung anbietet, die Glut nutzen kann, fragt nach einem Token, verdeckt getippt. Die eigenen Server eines Projekts ändert das Projekt: /mcp remove nimmt nur deine. `glut mcp` tut dasselbe außerhalb einer Session.

Während eines seiner Werkzeuge läuft, kann ein Server dich fragen: ein Formular (⇥ ↑↓ zwischen den Feldern, Leertaste kreuzt an, ⏎ sendet, Esc schließt, ⌃D lehnt ab) oder eine Seite zum Öffnen (o). Nur du antwortest, nie das Modell; ein Formular, das nach einem Passwort oder Key fragt, wird abgelehnt, und ist niemand da, wird jede Frage abgelehnt.
