Wie die Befehle gehalten werden, die Glut ausführt, und ob sie Git-Ordner schreiben dürfen.

/sandbox — die Sandbox hier (macOS: Seatbelt; Linux: bubblewrap und seccomp) und der Git-Schalter
/sandbox git read-only — kein Befehl schreibt in einen Git-Ordner: aus der Sandbox committet keiner
/sandbox git writable — Befehle dürfen wieder in Git-Ordner schreiben, auch committen
/sandbox required on — kein Befehl läuft, wo es keine Sandbox gibt (bubblewrap fehlt, ein Container ohne das Recht); off lässt sie laufen, nur von den Regeln gehalten

Die Sandbox lässt einen Befehl nur in deinen Ordnern schreiben, alles lesen außer deinen Geheimnissen und .env-Dateien, und ins Netz nur über Gluts Proxy. Unter Linux kann sie einen Namen, den es noch nicht gibt, nicht verbieten: Was ein Befehl anlegt, das macOS verweigert — ein neues .envrc, ein .git, eine .git/commondir —, kommt nach ~/.glut/quarantine, sobald es auftaucht, und nach dem Befehl noch einmal. Ein Werkzeug von dir, das genau in diesem Moment läuft, könnte es trotzdem antreffen; sind Git-Ordner schreibgeschützt, entsteht darin gar keins.

Der Schalter gilt in jeder Session und jedem Fenster ab dem nächsten Befehl und liegt in ~/.glut/sandbox.json — kein Befehl schreibt ihn. glut sandbox show und glut sandbox git read-only|writable tun dasselbe außerhalb von Glut.
