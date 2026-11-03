Die Keys sehen und festlegen, die in dieser Session vor dem Modell verborgen sind.

/secrets — jeder bisher verborgene Wert, mit Nummer, Art, Länge und wo er gesehen wurde (nie der Wert)
/secrets allow <n> — lässt Secret n für diese Session durch
/secrets allow <n> always — lässt es immer durch (sein Fingerabdruck wird gespeichert, nie der Wert)
/secrets add <name> <regex> — verbirgt ein eigenes Format, ab dem nächsten Ergebnis
/secrets remove <name> — entfernt ein eigenes Format
/secrets on|off — Maskierung an oder aus

Ein Key in der Antwort eines Werkzeugs erreicht das Modell als ‹secret n: …›; Glut setzt seinen Wert nur in eine Datei zurück, die ihn schon enthält. Bei einer Nachricht von dir, die aussieht, als enthielte sie einen Key, wird nachgefragt, bevor sie abgeschickt wird. Das Modell erfährt von einer Änderung mit deiner nächsten Nachricht. Deine Formate und Entscheidungen bleiben in `~/.glut/secrets.json` erhalten.

Die Maskierung schützt vor einem Key, der aus Versehen gezeigt wird; die Sandbox und das Netzwerk-Gate bleiben die Schranken.
