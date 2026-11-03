How many steps a turn takes at most.

/maxsteps — the limit now
/maxsteps <n> — a turn stops after n steps, from 1 to 1000

A step is one call of the model. At the limit the turn stops and says so; ⏎ on the empty input sends "Go on where you stopped." and the work goes on. Budgets and the time limit stay the real brakes. Kept in `~/.glut/prefs.json`; `maxTurns` in `~/.glut/settings.json` sets it for every run, `glut run --max-turns` for one.
