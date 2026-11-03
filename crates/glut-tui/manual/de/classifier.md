Lass ein zweites Modell die Befehle beurteilen, nach denen dich sonst nur der Modus fragen würde.

/classifier — welches Modell beurteilt, oder dass keins es tut
/classifier session — das Modell der Session beurteilt (solange du keins setzt)
/classifier <provider> <model> — ein eigenes Modell, ein schnelles genügt (openrouter deepseek/deepseek-v4.1-flash)
/classifier off — keins beurteilt: du wirst jedes Mal gefragt

Glut kennt viele Befehle, die nur lesen (ls, grep, git status, …), und führt sie in jedem Modus aus. Eine Schleife oder eine Pipeline daraus (for f in *.md; do grep -c x "$f"; done) kann es an ihrem Text allein nicht beurteilen, also fragt es. Das zweite Modell sieht diesen Befehl und seinen Ordner — nie das Gespräch, eine Datei oder eine Ausgabe — und sagt READS oder CHANGES. Nur bei READS läuft der Befehl wie ein gelisteter Lesebefehl, in der Sandbox; alles andere, oder keine klare Antwort binnen Sekunden, fragt dich wie bisher, sein Wort unter dem Befehl. Eine Regel von dir, ein heftiger Befehl, der Schlüsselbund und die Sandbox bleiben, wie sie sind; derselbe Befehl wird in einer Session nicht zweimal beurteilt.

Ein Befehl, den es laufen ließ, sagt das neben sich. Die Wahl steht in `~/.glut/classifier.json` und gilt ab dem nächsten Turn; `glut run --classifier` setzt sie für einen Lauf.
