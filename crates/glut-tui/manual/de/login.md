Mit einem Abo statt mit einem Schlüssel anmelden — heute mit deinem ChatGPT-Plus- oder -Pro-Abo.

/login — die Abos, jedes mit seinem Stand; ⏎ meldet an oder ab
/login <provider> — meldet sich sofort bei diesem an, etwa /login chatgpt
/login <provider> <address> — über SSH: die Adresse, auf der der Browser endete, zurück eingefügt
/login openrouter — OpenRouter gibt Glut einen eigenen Key, gehalten wie ein getippter; begrenzen oder löschen unter openrouter.ai/settings/keys

Die Adresse wird zuerst gezeigt; ⏎ öffnet sie in deinem Browser. Glut bewahrt die Anmeldung in der Keychain auf (oder in `~/.glut/credentials.json`) und erneuert sie selbst; Abmelden widerruft sie bei OpenAI.

Mit dem Abo kostet ein Aufruf kein Geld, aber die Nutzungsgrenzen des Abos gelten: Ist eine erreicht, endet der Turn und Glut verweist auf ChatGPT-Einstellungen → Nutzung, wo du auch begrenzt, was jede App nutzen darf. In der Shell: glut providers login|logout <provider>.
