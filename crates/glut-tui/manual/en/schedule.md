Plans that run while Glut is closed: a task at a time of day or every so often.

/schedule — the Runs tab (⌃X R): every plan with its next time and its last result
/schedule list — the plans in the stream
/schedule add <name> [flags] <task> — makes a plan, or changes the one of that name
/schedule run <name> — runs it now, beside Glut
/schedule pause <name> — holds it until resume
/schedule resume <name> — it runs at its times again
/schedule remove <name> — the plan and its record go; its sessions stay

The flags are those of `glut schedule add`:
- `--at 07:00` a time of day, with `--days mo-fr` or `sa,su` (every day without)
- `--every 2h` an interval from 10m to 7d, instead of --at
- `--mode ask|build|plan|dont_ask|auto` what it may do without a rule; ask unless named
- `--allow <rule>`, `--deny <rule>` rules in Claude Code's form, such as `Bash(cargo test:*)`; repeatable
- `--folder <path>` a folder beside its own; repeatable
- `--max-usd 1`, `--max-minutes 30` the most a run may cost and take: 1 $ and 30 min unless named
- `--max-tokens 1M` on a subscription, the most tokens a run may use: 1M unless named
- `--replace` the rules and folders given take the place of the plan's instead of joining them
- `--cwd <path>`, `--provider <id>`, `--model <id>` its folder (this one otherwise), provider and model, fixed when it is made

The task is a message, or /skill <name> [what for]. launchd starts each run, only while you are logged in; a time missed asleep is made up once on waking. Nobody is there to answer: every ask is refused and noted, so allow what it needs. A run is a kept session of its own, told by a macOS notification. In the Runs tab: ⏎ opens a plan or a run's session, r runs it now, p pauses or resumes, d deletes it.

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" run the tests and report what fails
