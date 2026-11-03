在主 Pane 旁的一个 Pane 中打开新会话。

/split — 一个新 Pane；在 git 仓库中，Glut 会询问它在哪里工作
/split here — 它在此文件夹中工作，就在你身边
/split tree [branch] — 它在一个新的 Worktree 中、在自己的分支上工作
/split resume — 它打开一个早先的会话，从列表中选取
/split as <role> — 它扮演一个角色（/role 列出它们）

主 Pane 旁最多有六个 Pane，分成两行，每行三个。⌃P 在各 Pane 之间轮转，⌃X 2 也能拆分，⌃X } 和 ⌃X { 使主 Pane 变宽或变窄。这里的两个 Pane 会更改相同的文件；Worktree 使它们互不干扰，直到 /close 询问如何结束它。

/split tree fix-login as reviewer
