把会话写出来，以便阅读或分享。

/export — Markdown，写入 `~/.glut/exports`
/export html — 单个 HTML 文件，样式内嵌，无脚本
/export jsonl — 消息，JSON 行格式
/export copy — 把 Markdown 放到剪贴板，而不是写成文件
/export full — 附上完整的工具结果和系统提示词
/export <format> <path> — 写入你自己指定的文件（`~/` 是你的主目录，相对路径位于工作文件夹中）

这些词可以任意顺序给出，路径放在最后。不加 full 时，导出的内容就是模型所见：较长的结果会被缩短。已有的文件绝不会被覆盖。Glut 隐藏的密钥依然隐藏，但你自己输入的内容仍可能含有密钥：分享之前请先读一遍导出内容。在 /resume 中，⌃E 导出列表中的一个会话。
