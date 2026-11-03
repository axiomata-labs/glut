Voir à quel point la fenêtre du modèle est pleine, ou dire à Glut quelle taille elle a.

/context — les tokens utilisés, la fenêtre et d’où vient sa taille, et ce que coûtent à chaque tour les guides (`AGENTS.md`, `CLAUDE.md`)
/context <tokens> — règle la fenêtre de ce modèle : 65536, 64k ou 1M

Une fenêtre que tu règles est gardée dans `~/.glut/providers.json` pour ce modèle et prime sur la liste du fournisseur. Glut compacte de lui-même peu avant que la fenêtre soit pleine.
