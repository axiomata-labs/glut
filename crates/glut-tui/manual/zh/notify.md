窗口不在前台时，在一轮结束或 Glut 询问时得到通知。

/notify on — 一轮的结束和每一次询问
/notify asks — 仅询问
/notify off — 什么都不通知
/notify system — 通过 macOS 自带的通知（点击通知会打开“脚本编辑器”，而不是终端）
/notify after <s> — 只有一轮运行超过这么多秒后，才通知其结束（默认 10，最多 3600）
/notify tiles on|off — 窗口右上角的磁贴

单独的 /notify 显示当前生效的设置。在知道终端支持自带通知的地方，Glut 使用它们，以及铃声。通知会写明 Glut、项目、Pane 和发生了什么，但绝不包含模型的话。保存在 `~/.glut/prefs.json` 中，优先于 `~/.glut/settings.json` 中的 `notify`。
