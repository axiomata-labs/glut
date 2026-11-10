Glut 是否每天检查一次新版本并在底栏显示。

/update on — Glut 每天在后台从 `https://glut-ai.com/latest.txt` 读取一次最新版本号；若比当前版本新，底栏会低调地显示（`↑ 0.1.2`）
/update off — Glut 不检查，底栏也不显示

单独输入 /update 会告诉你是否有新版 Glut，并按安装方式给出更新命令（`brew upgrade glut`、`cargo install glut-tui` 或安装脚本）。Glut 从不自行更新，也从不在计划运行、循环的一轮、`glut run`、`glut acp` 或竞技场中检查。保存在 `~/.glut/prefs.json`；在 `~/.glut/settings.json` 中写 `"updateCheck": false` 可关闭（除非 /update 另有设置）。
