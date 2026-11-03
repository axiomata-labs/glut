See and set the keys hidden from the model in this session.

/secrets — every value hidden so far, by number, kind, length and where it was seen (never the value)
/secrets allow <n> — lets secret n through for this session
/secrets allow <n> always — lets it through always (its fingerprint is kept, never the value)
/secrets add <name> <regex> — hides a format of your own, from the next result on
/secrets remove <name> — drops a format of yours
/secrets on|off — masking on or off

A key in what a tool answers reaches the model as ‹secret n: …›; Glut puts its value back only into a file that already holds it. A message of yours that looks like it holds a key is asked about before it goes. The model hears of a change with your next message. Your formats and choices are kept in `~/.glut/secrets.json`.

Masking guards against a key shown by accident; the sandbox and the network Gate stay the barriers.
