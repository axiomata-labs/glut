一个回合最多走多少步。

/maxsteps — 当前的上限
/maxsteps <n> — 回合在 n 步后停下，1 到 1000

一步就是一次模型调用。到达上限时回合停下并说明；在空输入上按 ⏎ 会发送“Go on where you stopped.”，工作继续。预算和时间限制仍是真正的刹车。保存在 `~/.glut/prefs.json`；`~/.glut/settings.json` 中的 `maxTurns` 对每次运行设置，`glut run --max-turns` 对一次运行设置。
