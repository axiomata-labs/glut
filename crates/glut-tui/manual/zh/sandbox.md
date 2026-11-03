Glut 运行的命令如何被约束，以及它们能否写入 Git 文件夹。

/sandbox — 这里的沙箱（macOS：Seatbelt；Linux：bubblewrap 和 seccomp）和 Git 开关
/sandbox git read-only — 没有命令写入 Git 文件夹：没有命令能从沙箱提交
/sandbox git writable — 命令可以重新写入 Git 文件夹，也可以提交
/sandbox required on — 没有沙箱的地方不会运行任何命令（缺少 bubblewrap、没有权限的容器）；off 让它们只受规则约束地运行

沙箱只让命令写入你的文件夹，读取除你的机密和 .env 文件之外的一切，并且只能通过 Glut 的代理访问网络。在 Linux 上，它无法拒绝一个尚不存在的名字：命令创建的、macOS 会拒绝的东西 — 新的 .envrc、.git、.git/commondir — 一出现就会被移到 ~/.glut/quarantine，命令结束后还会再检查一次。恰好在那一刻运行的你的工具仍可能遇到它；Git 文件夹只读时，其中根本不会产生这类东西。

这个开关从下一个命令起在每个会话和窗口中生效，保存在 ~/.glut/sandbox.json 中 — 从不由命令写入。glut sandbox show 和 glut sandbox git read-only|writable 在 Glut 之外做同样的事。
