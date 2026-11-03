Be told when a turn ends or Glut asks, while the window is not in front.

/notify on — a turn's end and every ask
/notify asks — only the asks
/notify off — nothing
/notify system — through macOS's own notifications (a click on one opens Script Editor, not the terminal)
/notify after <s> — a turn's end is told only once it ran this many seconds (10 by default, up to 3600)
/notify tiles on|off — the tiles at the window's top right

/notify alone shows what holds now. Glut uses the terminal's own notifications where it knows it has them, and the bell. A notification names Glut, the project, the pane and what happened, never the model's words. Kept in `~/.glut/prefs.json`, over `notify` in `~/.glut/settings.json`.
