Benachrichtigt werden, wenn ein Turn endet oder Glut fragt, während das Fenster nicht vorne ist.

/notify on — das Ende eines Turns und jede Frage
/notify asks — nur die Fragen
/notify off — nichts
/notify system — über die eigenen Benachrichtigungen von macOS (ein Klick darauf öffnet den Skript-Editor, nicht das Terminal)
/notify after <s> — das Ende eines Turns wird erst gemeldet, wenn er so viele Sekunden lief (standardmäßig 10, bis zu 3600)
/notify tiles on|off — die Kacheln oben rechts im Fenster

/notify allein zeigt, was gerade gilt. Glut nutzt die eigenen Benachrichtigungen des Terminals, wo es weiß, dass es sie hat, und die Glocke. Eine Benachrichtigung nennt Glut, das Projekt, das Pane und was geschah, nie die Worte des Modells. Bleibt in `~/.glut/prefs.json` erhalten, vor `notify` in `~/.glut/settings.json`.
