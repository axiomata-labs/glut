Run a task again and again in a background agent of its own, while Glut is open.

/loop <interval> [fresh] [as <role>] [notify] <task> — starts a loop; the first round at once
/loop — lists the loops: interval, rounds, the next round and the cost so far
/loop stop [name] — ends that loop, or the only one
/loop rounds [n] — the most rounds of a loop; alone it shows the limit now
/loop budget [usd] — the most a loop may cost; alone it shows the limit now
/loop tokens [n] — the most tokens a loop on a subscription may use (5M, 500k); alone it shows the limit now

Rounds never overlap: the interval is counted from a round's end, as 30s, 5m, 2h or 1h30m (from 10s to 24h). A round goes on with the conversation of the round before; with fresh each round starts a new one. The task is a message, or /skill <name> [what for], /compact or /init — no other command.

The agent is named loop (or after its role) and takes this pane's mode, the "always" you gave here, its folders beside and its role; as <role> may only name the role this pane plays. Its asks wait for you. A tile comes when a round asks or the loop ends; with notify after every round.

A loop ends after its rounds or once its rounds cost its budget (by default 50 rounds and 5 $, else `loop.rounds` and `loop.maxUsd` of `~/.glut/settings.json`; a change counts for loops started after it), when a round fails or you stop it (⎋ in its round), or when its model calls `end_loop` because the end your task named is reached. When the model's prices are unknown, only the rounds end it. Its agent stays in the Agents tab afterwards, to read or /close. x on its card in the Agents tab (⌃X A) ends the loop.

/loop 10m run the tests and fix what fails; stop when all pass
/loop 1h fresh as reviewer notify review the last commit
