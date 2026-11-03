Den Editor wählen, in dem Glut Dateien und die Eingabe öffnet.

/editor — die Editoren, die auf deinem System gefunden wurden
/editor <command> — dein eigener Befehl, mit `{file}` und `{line}` an den passenden Stellen

Fehlt `{file}`, wird die Datei am Ende angehängt. Ein Editor mit Fenster muss warten, bis die Datei geschlossen wird (`code --wait`). ⌃X E bearbeitet die Eingabe darin; /todo open und /memory open nutzen ihn ebenfalls. Bleibt in `~/.glut/prefs.json` erhalten.

/editor nvim +{line} {file}
