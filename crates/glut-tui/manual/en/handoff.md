Hand the work over to a later session of this folder.

/handoff write — the model writes what was done, how things stand and what comes next
/handoff read — takes the folder's handoff into this session

A handoff is kept in `~/.glut/handoffs`, one per folder. Once read it moves aside, so no later session reads it twice. Writing one is a paid call of its own; the conversation stays as it is.
