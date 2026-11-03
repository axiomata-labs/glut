选择 Glut 的颜色。

/theme — 可选的主题
/theme <name> — glut（Glut 自己的），或 Axiomata 的某个主题（catppuccin-mocha、github-dark、…）

Glut 只给文字着色并保留终端的背景：浅色主题需要浅色终端。保存在 `~/.glut/prefs.json` 中；在由宿主应用打开的窗口中，该选择只对本会话有效。

每个主题都会给页脚的各部分、按类别区分的工具名称（读取、写入、命令、对外访问）、各个 Pane 和代理以及输入框上方的线条赋予各自柔和的颜色；需要你处理的内容依然醒目。阅读模式和 `NO_COLOR` 保持朴素。
