Den anderen Glut-Fenstern auf diesem Rechner schreiben.

/sessions — ihre Panes und Agenten: Adresse, Projekt, Modell, Modus, arbeitet oder wartet; ⏎ setzt /send <address> in die Eingabe
/sessions off — dieses Fenster nimmt keine Nachrichten von ihnen an
/sessions on — und wieder

Eine Nachricht des Owners dort geht sofort herein; eine eines Modells dort nur, wenn es im Modus des Panes arbeitet, das sie erreicht, sonst wartet sie über der Eingabe: ⏎ lässt sie herein, Esc verwirft sie. Keine ist dein Wort an das Modell hier. Nur Glut spricht mit den anderen Fenstern — ein Befehl des Modells erreicht sie nie.

Auch die Modelle sehen einander: ihr Werkzeug `who` listet jeden Agenten hier und in den anderen Fenstern — Rolle, Modell, Ordner, Branch, beschäftigt oder frei — und die Zeile, die jeder für seine Arbeit gesetzt hat, in seinen eigenen Worten.

Ein Modell kann ein anderes um etwas bitten, hier oder in einem anderen Fenster: Der Zug, den seine Bitte dort startet, antwortet ihm — die letzte Antwort dieses Zuges kommt als Antwort zurück, während es an anderem weiterarbeitet. Ein beschäftigter Agent nimmt sie, sobald sein Zug endet; einer, der es nicht kann, sagt es. Antwortet niemand — du hast die Bitte mit Esc beiseitegelegt, das Pane wurde geschlossen, das Fenster endete —, sagt Glut es dem Bittenden.

Du siehst alles im Reiter Team (⌃X T): jeden Agenten hier und in den anderen Fenstern — beschäftigt, frei oder wartend auf dich, seinen Branch und seine Zeile — und was zwischen ihnen lief: Nachrichten, Bitten, Antworten und wer kam und ging.

Wo sich zwei Agenten in die Quere kommen, sagt Glut es ihnen: Ein Agent, der im selben Arbeitsbaum auf demselben Branch arbeitet wie ein anderer, der dort gerade arbeitet, oder einer, der eine Datei ändert, die ein anderer in der letzten Viertelstunde geändert hat, hört es einmal vor seinem nächsten Schritt — und du siehst den Hinweis in seinem Verlauf und im Reiter Team.

Ein Agent, den du gestartet hast, kann leiten: Mit `start_helpers` startet er eigene Helfer im Hintergrund — jeder mit Rolle, Modell und erster Aufgabe, in seinem Ordner oder in einer eigenen Kopie des Repos —, und ihre Antworten kommen zu ihm zurück. Ein Helfer hat den Modus und die „immer“-Regeln seiner Leitung, enger durch seine Rolle, und startet selbst keine Helfer. Im Modus build sagst du einmal Ja für das ganze Team, in auto starten sie sofort, in ask und plan schlägt die Leitung sie nur vor. Mit `take_work` holt die Leitung die Arbeit eines Helfers aus seiner Kopie — geprüft wie der Gewinner der Arena, nicht committet — und schließt ihn.
