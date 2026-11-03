防止 Glut 在 ~/.glut 中的数据无限增长。

/clean — Glut 保存了什么（逐项），以及按你的期限可以清理的部分
/clean now — 立即在后台清理
/clean keep — 各项期限
/clean keep <kind> <days>d — 设置一项：snapshots、journals、sessions、results 或 lean

会清理的内容：超过期限（7 天）的运行快照 — 当一个文件夹的快照全部过期时，整个存储一并删除，无论文件夹是否还在；未完成运行的日志（7 天）；90 天未写入的会话，连同其事件日志和长结果；没有会话的长结果在 30 天后；没有任何保留会话引用的图片；lean.jsonl 中超过 90 天的行。导出只计数：那是你的文件。

Glut 每天启动时自动清理一次 — 在这里和 `glut run` 中 — 并告诉你释放了多少。构建输出和缓存（带 CACHEDIR.TAG 的文件夹、node_modules/ 下或 Cargo 项目的 target/ 下）完全不做快照。

期限保存在 `~/.glut/clean.json`。命令行：`glut clean`、`glut clean --dry-run`、`glut clean keep sessions 30d`；`glut clean --dry-run --json` 为程序输出报告。
