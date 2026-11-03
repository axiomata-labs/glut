选择 Glut 用来打开文件和输入内容的编辑器。

/editor — 在你的系统上找到的编辑器
/editor <command> — 你自己的命令，`{file}` 和 `{line}` 放在需要的位置

没有 `{file}` 时，文件会添加在末尾。窗口式编辑器必须等到文件关闭后才返回（`code --wait`）。⌃X E 在其中编辑输入；/todo open 和 /memory open 也使用它。保存在 `~/.glut/prefs.json` 中。

/editor nvim +{line} {file}
