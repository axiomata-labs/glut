Passer à un autre fournisseur de modèles.

/provider — les fournisseurs que tu as configurés
/provider <id> — passe à celui-ci, sur son modèle par défaut
/provider key [id] — ne change que la clé de ce fournisseur (sans id, celle du fournisseur par défaut)

Les fournisseurs sont gardés dans `~/.glut/providers.json`, leurs clés dans le Trousseau (ou dans `~/.glut/credentials.json`) ; `glut providers add <id>` en ajoute un. La conversation continue, et le prochain démarrage commence avec le fournisseur que tu as choisi.

La clé se tape masquée et s'essaie sur la liste de modèles du fournisseur avant de remplacer l'ancienne ; si cela échoue, tu choisis de la garder quand même, sinon l'ancienne reste. La session prend aussitôt une clé gardée. Un fournisseur qui se connecte, n'a pas besoin de clé ou la prend dans une variable d'environnement te le dit. Dans le shell : `glut providers key <id>`.
