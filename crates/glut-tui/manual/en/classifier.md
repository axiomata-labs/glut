Let a second model judge the commands only the mode would ask you about.

/classifier — which model judges, or that none does
/classifier session — the session's own model judges (as long as you set none)
/classifier <provider> <model> — a model of its own, a quick one is enough (openrouter deepseek/deepseek-v4.1-flash)
/classifier off — none judges: you are asked each time

Glut knows many commands that only read (ls, grep, git status, …) and runs them in every mode. A loop or a pipeline of them (for f in *.md; do grep -c x "$f"; done) it cannot judge from its text alone, so it asks. The second model sees that command and its folder — never the conversation, a file or an output — and says READS or CHANGES. Only on READS does the command run as a listed reader, in the sandbox; anything else, or no clear answer within seconds, asks you as before, its word under the command. A rule of yours, a heavy command, the Keychain and the sandbox stay as they are; the same command again is not judged twice in a session.

A command it let run says so beside it. The choice is kept in `~/.glut/classifier.json` and counts from the next turn; `glut run --classifier` sets it for one run.
