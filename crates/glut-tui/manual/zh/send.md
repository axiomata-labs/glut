向另一个 Pane 或后台代理发送一条消息，在那里如同你亲手输入的一样处理。

/send — 可发送对象的列表：此处的 Pane 和代理，然后是其他 Glut 窗口；⏎ 会把 /send <who> 放入输入框
/send <text> — 发给唯一的另一个 Pane
/send <n|name> <text> — 发给 Pane n 或该名字的代理（三个及以上时需要）
/send plan — 把此 Pane 的计划交给另一个 Pane 去构建
/send <n|name> plan — 交给指定的那一个

会话空闲时，消息立即开始；否则在其一轮之后开始。按键仍留在此 Pane。
