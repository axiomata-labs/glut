Revenir avant l’un de tes messages : la conversation, les fichiers, ou les deux.

/rewind — tes messages, le plus récent d’abord ; ⏎ sur l’un demande ce qui revient : conversation et fichiers, la conversation seule, les fichiers seuls, ou une bifurcation (une nouvelle session à partir de là). ⎋⎋ ouvre la même liste.

Rien n’est perdu : ce que le retour coupe reste une session à part sous celle-ci, « (before rewind) » dans /resume, et le dossier tel qu’il était est gardé avec elle. En ouvrant cette session, Glut demande une fois si ses fichiers reviennent aussi. Les fichiers viennent des snapshots de Glut : ce qu’une commande a changé revient aussi ; l’état juste avant est d’abord mis de côté (`glut undo`). Au-delà d’une compaction, seuls les fichiers peuvent revenir.
