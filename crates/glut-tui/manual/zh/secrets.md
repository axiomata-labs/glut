查看并设置本会话中对模型隐藏的密钥。

/secrets — 迄今隐藏的每个值，按编号、种类、长度和出现位置列出（绝不显示值本身）
/secrets allow <n> — 本会话中放行密钥 n
/secrets allow <n> always — 始终放行它（保存其指纹，绝不保存值）
/secrets add <name> <regex> — 隐藏你自己的格式，从下一个结果起生效
/secrets remove <name> — 删除你的一个格式
/secrets on|off — 开启或关闭遮蔽

工具回答中的密钥到达模型时是 ‹secret n: …›；Glut 只会把它的值放回已含有该值的文件中。你的消息看起来像含有密钥时，发出之前会先询问。模型会在你的下一条消息时得知更改。你的格式和选择保存在 `~/.glut/secrets.json` 中。

遮蔽只防范意外显示的密钥；沙箱和网络 Gate 仍是屏障。
