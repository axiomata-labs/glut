查看模型的窗口有多满，或告诉 Glut 它有多大。

/context — 已用的 Token、窗口及其大小的来源，以及指南（`AGENTS.md`、`CLAUDE.md`）每一轮的开销
/context <tokens> — 设置此模型的窗口：65536、64k 或 1M

你设置的窗口会为此模型保存在 `~/.glut/providers.json` 中，并优先于提供商的列表。Glut 会在窗口将满之前不久自动压缩。
