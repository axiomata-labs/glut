Dieser Session Ordner neben dem Arbeitsordner geben, damit Glut projektübergreifend arbeiten kann.

/folder — die Ordner neben dem Arbeitsordner
/folder add <path> — fügt für diese Session einen hinzu
/folder remove <path> — nimmt einen weg

Die Dateiwerkzeuge erreichen so einen Ordner über seinen vollen Pfad, Befehle dürfen dort innerhalb der Sandbox schreiben, das Gate beurteilt ihn wie den Arbeitsordner, und Undo holt auch seine Dateien zurück. Nur du fügst einen Ordner hinzu, nie ein Modell, ein Skill oder ein Projekt. Glut lehnt den Home-Ordner ab, einen Ordner, der das Projekt enthält oder darin liegt, einen mit Secrets und zwei Ordner, die ineinander liegen.

Die Ordner gelten nur für diese Session; `glut run --folder <path>` gibt einem Lauf ohne Oberfläche einen.
