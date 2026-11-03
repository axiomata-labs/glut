Zu einem anderen Modell-Provider wechseln.

/provider — die Provider, die du eingerichtet hast
/provider <id> — wechselt zu diesem, auf sein Standardmodell
/provider key [id] — ändert nur den Schlüssel dieses Providers (ohne id den des Standard-Providers)

Die Provider liegen in `~/.glut/providers.json`, ihre Keys im Schlüsselbund (oder in `~/.glut/credentials.json`); `glut providers add <id>` fügt einen hinzu. Das Gespräch geht weiter, und der nächste Start beginnt mit dem Provider, den du gewählt hast.

Der Schlüssel wird verdeckt eingegeben und an der Modellliste des Providers geprüft, bevor er den alten ersetzt; schlägt das fehl, entscheidest du, ob er trotzdem bleibt, sonst bleibt der alte. Die Session nimmt einen behaltenen Schlüssel sofort. Ein Provider, der sich anmeldet, keinen Schlüssel braucht oder ihn aus einer Umgebungsvariable nimmt, sagt es dir. In der Shell: `glut providers key <id>`.
