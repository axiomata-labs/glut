在 Glut 关闭期间运行的计划：在某个时刻或每隔一段时间执行一项任务。

/schedule — 运行标签页（⌃X R）：每个计划及其下次时间和上次结果
/schedule list — 在流中列出计划
/schedule add <name> [flags] <task> — 创建计划，或修改同名的计划
/schedule run <name> — 立即运行它，与 Glut 并行
/schedule pause <name> — 暂停它，直到 resume
/schedule resume <name> — 它重新按时间运行
/schedule remove <name> — 计划及其记录一并删除；其会话保留

flags 即 `glut schedule add` 的标志：
- `--at 07:00` 一天中的某个时刻，可与 `--days mo-fr` 或 `sa,su` 搭配（不加则每天）
- `--every 2h` 从 10m 到 7d 的间隔，用于代替 --at
- `--mode ask|build|plan|dont_ask|auto` 没有规则时它可以不经询问做什么；除非指定，否则为 ask
- `--allow <rule>`、`--deny <rule>` Claude Code 格式的规则，例如 `Bash(cargo test:*)`；可重复
- `--folder <path>` 它自己旁边的文件夹；可重复
- `--max-usd 1`、`--max-minutes 30` 一次运行最多可花费和占用的量：除非指定，否则为 1 $ 和 30 分钟
- `--max-tokens 1M` 在订阅上一次运行最多可用的 token 数：除非指定，否则为 1M
- `--replace` 给出的规则和文件夹取代计划原有的，而不是并入其中
- `--cwd <path>`、`--provider <id>`、`--model <id>` 它的文件夹（否则为当前这个）、提供商和模型，创建时即固定

任务是一条消息，或 /skill <name> [用途]。launchd 启动每一次运行，且仅在你登录期间；睡眠中错过的时间会在唤醒时补跑一次。那里没有人可以回答：每一次询问都会被拒绝并记录，所以请允许它需要的一切。每次运行都是一个独立保存的会话，并通过 macOS 通知告知。在运行标签页中：⏎ 打开计划或某次运行的会话，r 立即运行，p 暂停或恢复，d 删除。

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" 运行测试并报告失败的项
