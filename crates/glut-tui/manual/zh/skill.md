启动一个 Skill：针对某类任务的指令，模型会加载并遵循它。

/skill — 可选的 Skill
/skill <name> [用途] — 在名字之后所写的内容上运行该 Skill

Glut 自带 make-agent、make-skill 和 make-hook，用来编写你自己的。你的放在 `~/.glut/skills` 中，项目的放在其 `.glut/skills` 中（仅在受信任的文件夹中读取）；同名的会遮蔽 Glut 自带的。

/skill make-hook 阻止在 main 上执行 git push
