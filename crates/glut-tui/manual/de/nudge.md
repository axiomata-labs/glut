Ob Glut das Modell weitermachen lässt, an seine Aufgabenliste erinnert und bittet, eine zu schreiben, wenn es keine hat.

/nudge on — endet ein Turn mit offenen Punkten der Aufgabenliste oder mit Worten, die einen nächsten Schritt ankündigen („als Nächstes…“, ein Doppelpunkt am Ende), schickt Glut dem Modell eine eigene Zeile: weitermachen oder sagen, warum es aufhört; der Verlauf zeigt das leise. Bleibt die Liste unverändert, erinnert Glut daran, und bleibt das ohne Folgen, einmal mit einer eigenen Nachricht; hat das Modell in einem Lauf keine geschrieben, bittet Glut um eine, einmal pro Turn
/nudge off — ein Turn endet, wo das Modell ihn beendet: keine Erinnerung an die Liste, keine Bitte darum

Das Nachfragen am Ende kommt einmal pro Turn, nie im Plan-Modus, nach einer Frage an dich, in einer Runde einer Schleife oder im Lauf eines Zeitplans; Erinnerung und Bitte folgen derselben Wahl, nur im Lauf eines Zeitplans erinnert Glut immer. /nudge allein zeigt die jetzige Wahl. Gehalten in `~/.glut/prefs.json`; `"nudge": false` in `~/.glut/settings.json` schaltet es für jeden Lauf ab.
