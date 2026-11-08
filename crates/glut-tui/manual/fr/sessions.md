Écrire aux autres fenêtres Glut de cette machine.

/sessions — leurs panes et agents : adresse, projet, modèle, mode, au travail ou en attente ; ⏎ met /send <address> dans la saisie
/sessions off — cette fenêtre ne prend plus leurs messages
/sessions on — et de nouveau

Un message du propriétaire là-bas entre aussitôt ; celui d'un modèle là-bas seulement s'il travaille dans le mode du pane qu'il atteint, sinon il attend au-dessus de la saisie : ⏎ le laisse entrer, Esc le jette. Aucun n'est ta parole au modèle d'ici. Seul Glut parle aux autres fenêtres — une commande du modèle ne les atteint jamais.

Les modèles se voient aussi : leur outil `who` liste chaque agent ici et dans les autres fenêtres — rôle, modèle, dossier, branche, occupé ou libre — et la ligne que chacun a posée sur ce qu'il fait, dans ses propres mots.

Un modèle peut demander quelque chose à un autre, ici ou dans une autre fenêtre : le tour que sa demande y lance lui répond — la dernière réponse de ce tour lui revient comme réponse, pendant qu'il continue autre chose. Un agent occupé la prend dès la fin de son tour ; celui qui ne peut pas le dit. Si personne ne répondra — tu as écarté la demande avec Esc, le pane a été fermé, la fenêtre s'est terminée — Glut le dit au demandeur.

Tu vois tout dans l'onglet Équipe (⌃X T) : chaque agent ici et dans les autres fenêtres — occupé, libre ou en attente de toi, sa branche et sa ligne — et ce qui est passé entre eux : messages, demandes, réponses, et qui est arrivé ou parti.

Là où deux agents se gênent, Glut le leur dit : un agent au travail dans le même arbre de travail et sur la même branche qu'un autre qui y travaille, ou un agent qui modifie un fichier qu'un autre a modifié dans le dernier quart d'heure, l'apprend une fois avant son prochain pas — et tu vois la note dans son flux et dans l'onglet Équipe.

Un agent que tu as lancé peut diriger : avec `start_helpers`, il lance ses propres assistants en arrière-plan — chacun avec un rôle, un modèle, sa première tâche, dans son dossier ou dans une copie du dépôt à lui — et leurs réponses lui reviennent. Un assistant a le mode et les règles « toujours » de son meneur, plus étroits selon son rôle, et ne lance aucun assistant. En mode build, tu dis oui une fois pour toute l'équipe ; en auto, ils démarrent aussitôt ; en ask et plan, le meneur ne fait que les proposer. Avec `take_work`, le meneur rapatrie le travail d'un assistant depuis sa copie — vérifié comme le gagnant de l'arène, non commité — et le ferme.
