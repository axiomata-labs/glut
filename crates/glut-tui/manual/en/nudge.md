Whether Glut asks the model to go on at the end of a turn, to keep to its task list and to write one when it works without one.

/nudge on — when a turn ends with items of the task list open, or with words that announce a next step ("next I'll…", a final colon), Glut sends the model one line of its own to go on or say why it stops; the stream shows it quietly. While the model works, a task list left unchanged brings Glut's reminder, and if that changes nothing, once a message of its own, and a run that has never written one brings its request for one, once a turn
/nudge off — a turn ends where the model ends it: no reminder of the task list, no request for one

The question at the end comes once per turn, never in plan mode, after a question to you, in a loop's round or a plan's run; the reminder and the request follow the same choice, but in a plan's run Glut always reminds. /nudge alone shows the choice now. Kept in `~/.glut/prefs.json`; `"nudge": false` in `~/.glut/settings.json` turns it off for every run.
