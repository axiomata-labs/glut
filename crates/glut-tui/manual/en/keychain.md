Keep the Keychain out of reach of the commands Glut runs.

/keychain — whether it is open, and the programs that may ask for it
/keychain add <program> — puts a program on the list (gh, or git and a sub-command: git push)
/keychain remove <program> — takes it off the list
/keychain open — opens it to every command, until this session ends
/keychain close — shuts it again

A key or a sign-in in the Keychain is reached past every file rule of the sandbox, so a command Glut runs finds the Keychain shut. A command of a listed program asks first — in every mode, `auto` too — and runs with it open once you allow it; an "always" makes it a rule of yours. With nobody there (a plan, `glut run`) only such a rule opens it. When a command fails on the shut Keychain, Glut says so under its output.

Allowing a command gives the Keychain to everything it starts as well: a git hook, a gh extension or a build script can then read any key in it.

Only your own rules give the Keychain: a project's `.glut/` allow rule lets a command run, never with it open. A chain gets it only when every part is listed or only reads. What `glut keychain open` keeps counts only here, where you sit — never in `glut run` or a host's run.

A rule of yours opens it unasked only when it names a program and its sub-command (`gh pr *`, `git push *`) — never a whole program (`gh *`). A 🔑 card shows the whole command, ↑↓ through it, and allows only once you have read it to its end.

The list is kept in `~/.glut/keychain.json`; `glut keychain open` keeps the Keychain open across sessions, `glut keychain close` shuts it for good.
