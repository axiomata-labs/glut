Den Schlüsselbund außer Reichweite der Befehle halten, die Glut ausführt.

/keychain — ob er offen ist, und die Programme, die nach ihm fragen dürfen
/keychain add <program> — setzt ein Programm auf die Liste (gh, oder git und ein Unterbefehl: git push)
/keychain remove <program> — nimmt es von der Liste
/keychain open — öffnet ihn für jeden Befehl, bis diese Session endet
/keychain close — schließt ihn wieder

Ein Key oder eine Anmeldung im Schlüsselbund ist an jeder Dateiregel der Sandbox vorbei erreichbar, darum findet ein Befehl, den Glut ausführt, den Schlüsselbund zu. Ein Befehl eines gelisteten Programms fragt zuerst — in jedem Modus, auch in `auto` — und läuft mit offenem Schlüsselbund, sobald du es erlaubst; ein „immer“ macht daraus eine Regel von dir. Ist niemand da (ein Zeitplan, `glut run`), öffnet ihn nur eine solche Regel. Scheitert ein Befehl am geschlossenen Schlüsselbund, sagt Glut das unter seiner Ausgabe.

Wer einen Befehl erlaubt, gibt den Schlüsselbund auch allem, was er startet: ein git-Hook, eine gh-Erweiterung oder ein Build-Skript kann dann jeden Key darin lesen.

Den Schlüsselbund geben nur deine eigenen Regeln frei: eine Erlauben-Regel aus dem `.glut/` eines Projekts lässt einen Befehl laufen, aber nie mit offenem Schlüsselbund. Eine Kette bekommt ihn nur, wenn jeder Teil gelistet ist oder nur liest. Was `glut keychain open` offen hält, gilt nur hier, wo du sitzt — nie in `glut run` oder dem Lauf eines Hosts.

Eine Regel von dir öffnet ihn ohne Frage nur, wenn sie ein Programm und seinen Unterbefehl nennt (`gh pr *`, `git push *`) — nie ein ganzes Programm (`gh *`). Eine 🔑-Karte zeigt den ganzen Befehl, ↑↓ blättert, und erlauben lässt er sich erst, wenn du ihn bis zum Ende gelesen hast.

Die Liste bleibt in `~/.glut/keychain.json`; `glut keychain open` hält den Schlüsselbund über Sessions hinweg offen, `glut keychain close` schließt ihn für immer.
