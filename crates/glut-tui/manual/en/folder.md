Give this session folders beside the working one, so Glut can work across projects.

/folder — the folders beside the working one
/folder add <path> — adds one for this session
/folder remove <path> — takes one away

The file tools reach such a folder by its full path, commands may write there inside the sandbox, the Gate judges it like the working folder and undo puts its files back too. Only you add a folder, never a model, a Skill or a project. Glut refuses the home folder, a folder that holds the project or lies inside it, one with secrets, and two folders inside each other.

The folders hold for this session only; `glut run --folder <path>` gives one to a headless run.
