Run the same task on two to four models at once and compare them (the arena).

/arena — the models of every provider; Space marks two to four, ⏎ puts them into the input to add the task
/arena <model>, <model>: <task> — the same in one line (provider/model, or a name that fits one model)
/arena say <text> — a follow-up to every model that answered, once all are over (not on the bench); time, cost and tokens add up over the rounds
/arena take <n> — model n's changes into your folder, not committed
/arena card — the comparison card again: each model's numbers and check, its diff below (←/→ another model, s two side by side, c check, t take)
/arena check <command> — the check for this repository (cargo test, npm test …), run in every model's worktree, in its sandbox; kept, it runs by itself after the next arena
/arena tasks — the test bench's sets in ~/.glut/arena/tasks; /arena tasks <set> runs one on the last arena's models (or /arena tasks <set>: <model>, <model>; with x3 after the set, each task three times in a row (up to x5), the grid then reads 2/3; with --background its models run as agents in the background, no pane each, their progress in the Agents tab), task after task, each judged by its own check
/arena results — every set's results (tasks passed, mean time and cost) and the free arena's wins
/arena blind on|off — the free arena blind (the default): its models go by letters in a drawn order, their names told once you take one or end the arena
/arena judge — after your pick, the model of the pane the arena started from comments on the solutions in two or three sentences (no tools; never before, so it cannot colour your choice)
/arena end — the arena's panes closed, their worktrees removed (asked first)

Each model works in a pane and a Worktree of its own that starts from your folder as it is, uncommitted and new files included (no .env file, no link), in auto: only heavy commands ask. A model stops after 30 minutes or $2. Cloud models run at once, local ones (Ollama, LM Studio) one after another once the others are done. When all are over, their numbers stand in the pane the arena started from, and /arena card opens the comparison card: time, cost, tokens, calls and what each changed. /arena take checks that the changes still fit your folder; nothing is written when they do not.

/arena sonnet, gpt-5: make the failing test pass
