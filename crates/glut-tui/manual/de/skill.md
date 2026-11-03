Einen Skill starten: Anweisungen für eine Art von Aufgabe, die das Modell lädt und befolgt.

/skill — die Skills zur Auswahl
/skill <name> [wofür] — führt diesen Skill auf das aus, was nach seinem Namen folgt

Glut bringt make-agent, make-skill und make-hook mit, um eigene zu schreiben. Deine liegen in `~/.glut/skills`, die eines Projekts in dessen `.glut/skills` (nur in einem vertrauten Ordner gelesen); einer desselben Namens verdeckt den von Glut.

/skill make-hook blockiere git push auf main
