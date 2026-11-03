让此 Pane 扮演一个角色：一个具名代理的提示词和工具，而非其模型。

/role — 可选的角色
/role <name> — 从现在起扮演该角色
/role none — 再次成为全能角色，拥有所有工具

Glut 自带 reviewer、tester 和 docs；你自己的角色放在 `~/.glut/agents` 中，项目的放在其 `.glut/agents` 中（仅在受信任的文件夹中读取）。角色的工具立即生效，提示词从下一轮起生效。限制工具的角色需要 Gate。
