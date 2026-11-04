Ob die eigenen .glut-Einstellungen dieses Ordners ganz zählen.

/trust — ob diesem Ordner vertraut wird und was seine eigenen Einstellungen dann hinzufügen
/trust on — vertraut ihm: Seine Erlaubnis-Regeln, sein Modus, seine Skills, Agenten und Befehle zählen
/trust off — vertraut ihm nicht mehr: Nur seine Verbots- und Frage-Regeln zählen
/trust list — jeder vertraute Ordner

Die .glut/settings.json eines Projekts darf mehr geheim halten und mehr fragen oder verbieten — das zählt in jedem Ordner. Was einem Modell mehr erlauben würde, seine Erlaubnis-Regeln und sein Modus, zählt nur, wo du dem Ordner vertraut hast, ebenso die Skills, Agenten und Befehle in seinem .glut. Lies, was /trust zeigt, bevor du on sagst: Das .glut eines geklonten Repositorys ist das Wort eines anderen. Sein Modus geht nie über deinen eigenen hinaus, und seine Hooks laufen nie, ob vertraut oder nicht.

Die Änderung zählt ab der nächsten Session des Ordners (glut -c nimmt das Gespräch wieder auf). Nie der Home-Ordner, die Wurzel oder ein Ordner, der nicht dir allein gehört (der eines anderen Benutzers oder einer, den jeder schreiben darf, wie /tmp). Die Liste steht in ~/.glut/trusted.json, die kein Befehl schreibt, den Glut ausführt; glut trust show|add|remove|list tut dasselbe außerhalb von Glut.
