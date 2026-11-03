Passer le travail à une session ultérieure de ce dossier.

/handoff write — le modèle écrit ce qui a été fait, où en sont les choses et ce qui vient ensuite
/handoff read — reprend le handoff du dossier dans cette session

Un handoff est gardé dans `~/.glut/handoffs`, un par dossier. Une fois lu, il est mis de côté, si bien qu’aucune session ultérieure ne le lit deux fois. En écrire un est un appel payant à part ; la conversation reste telle qu’elle est.
