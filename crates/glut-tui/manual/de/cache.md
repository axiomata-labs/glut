Wählen, wie eine ruhende Session den Prompt-Cache des Providers hält, damit der nächste Turn warm startet.

/cache ping — Glut liest den Cache alle paar Minuten erneut, solange die Session ruht, bis zu 30 Minuten
/cache 1h — Anthropic hält ihn eine Stunde (jedes Schreiben kostet das Doppelte); andere Provider werden gelesen, um ihn zu halten
/cache off — nicht gehalten: eine Pause über die wenigen Minuten des Providers hinaus startet kalt

/cache allein zeigt die jetzige Wahl; ping ist der Standard. Bleibt in `~/.glut/prefs.json` erhalten.
