在后台启动一个代理：没有 Pane 的会话，通过名字联系。

/bg — 名为 agent（或 agent-2、…）的代理；在 git 仓库中，Glut 会询问它在哪里工作
/bg <name> — 该名字的代理（小写字母、数字和连字符）
/bg as <role> — 它扮演一个角色，并以该角色命名
/bg here — 它在此文件夹中工作
/bg tree [branch] — 它在一个新的 Worktree 中工作

/send <name> 给它工作，/peek <name> 读取它的流，/show <name> 把它调到前台，/close <name> 结束它。每个代理各自工作、各自计费：在一个窗口内启动第七个之前，Glut 会询问一次。⌃X B 启动一个并选择其角色，⌃X A 列出它们。

/bg docs-check as docs
