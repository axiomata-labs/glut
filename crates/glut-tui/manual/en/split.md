Open a new session in a pane beside the main one.

/split — a new pane; in a git repository Glut asks where it works
/split here — it works in this folder, beside you
/split tree [branch] — it works in a new Worktree on a branch of its own
/split resume — it opens an earlier session, picked from the list
/split as <role> — it plays a role (/role lists them)

Up to six panes stand beside the main one, in two rows of three. ⌃P goes round the panes, ⌃X 2 splits too, ⌃X } and ⌃X { make the main pane wider or narrower. Two panes here change the same files; a Worktree keeps them apart until /close asks how it ends.

/split tree fix-login as reviewer
