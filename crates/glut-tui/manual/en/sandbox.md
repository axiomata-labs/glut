How the commands Glut runs are held, and whether they may write git folders.

/sandbox — the sandbox here (macOS: Seatbelt; Linux: bubblewrap and seccomp) and the git switch
/sandbox git read-only — no command writes a git folder: none commits from the sandbox
/sandbox git writable — commands may write git folders again, commits too
/sandbox required on — no command runs where there is no sandbox (a missing bubblewrap, a container without the right); off lets them run, held by the rules alone

The sandbox lets a command write only in your folders, read everything but your secrets and .env files, and reach the network only through Glut's proxy. On Linux it cannot refuse a name that is not there yet: what a command makes that macOS refuses — a new .envrc, a .git, a .git/commondir — goes to ~/.glut/quarantine the moment it appears, and once more after the command. A tool of yours that runs at that very moment could still meet it; with git folders read-only, none is made in them at all.

The switch counts in every session and window from the next command on, and is kept in ~/.glut/sandbox.json — never written by a command. glut sandbox show and glut sandbox git read-only|writable do the same outside Glut.
