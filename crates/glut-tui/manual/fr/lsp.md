Si les serveurs de langage aident le modèle.

/lsp on — après une modification, les erreurs du fichier viennent avec le résultat, et le modèle a l'outil lsp : où un symbole est défini, chaque endroit où il sert, son type et sa doc, et un renommage dans tout le projet
/lsp off — aucun serveur ne tourne : leurs processus s'arrêtent aussitôt, plus d'erreurs après une modification, plus d'outil lsp
/lsp — le choix actuel, et chaque serveur de la session : prêt, indexe le projet, ou arrêté pour la session

Glut ne démarre que des serveurs qui n'exécutent jamais le code du projet (rust-analyzer sans scripts de build ni macros procédurales, pyright, et les serveurs de TOML, YAML, JSON, CSS, HTML et Markdown), seulement dans le bac à sable sans réseau, et seulement là où le Gate permet leur ligne de commande. Un serveur démarre à la première modification ou question de son langage et reste pour la session ; rust-analyzer met quelques secondes à indexer un grand projet, lsp le dit et le modèle utilise grep. Un renommage est un seul changement : demandé une fois pour tous ses fichiers, tous ou aucun, un instantané avant. Gardé dans `~/.glut/prefs.json` ; `"lsp": false` dans `~/.glut/settings.json` l'éteint pour chaque exécution, `glut run --no-lsp` pour une.
