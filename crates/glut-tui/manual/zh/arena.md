让同一任务同时在两到四个模型上运行并比较结果（竞技场）。

/arena — 列出所有 provider 的模型；空格键标记两到四个，⏎ 将它们放入输入框，再接着写任务
/arena <model>, <model>: <task> — 同样的操作写成一行（provider/model，或能对应唯一模型的名称）
/arena say <text> — 全部结束后向每个已回答的模型追加指示（测试台不可用）；时间、费用和 token 跨轮累计
/arena take <n> — 把模型 n 的改动应用到你的文件夹，不提交
/arena card — 再次显示对比卡片：各模型的数据和检查结果，下方是它的 diff（←/→ 切换模型，s 并排显示两个，c 检查，t 应用）
/arena check <command> — 针对此仓库的检查（cargo test、npm test 等），在每个模型的 worktree 中、在其沙箱里运行；保存后，会在下一次竞技场之后自动运行
/arena tasks — 列出 ~/.glut/arena/tasks 中基准测试的任务集；/arena tasks <set> 用上一个竞技场的模型运行一个任务集（或 /arena tasks <set>: <model>, <model>；在任务集后加 x3，每个任务连续运行三次（最多 x5），表格随后显示 2/3；加 --background 时模型作为后台智能体运行，不占 Pane，进度显示在 Agents 标签页），逐个任务运行，每个任务由各自的检查判定
/arena results — 每个任务集的结果（通过的任务数、Ø 耗时和费用）以及自由竞技场的胜利数
/arena blind on|off — 自由竞技场盲测（默认）：模型以随机顺序的字母显示，在你采用其中一个或结束竞技场后显示名称
/arena judge — 在你选择之后，启动竞技场的 Pane 的模型用两三句话点评各方案（不使用工具；选择前不可用，以免影响你的选择）
/arena end — 关闭竞技场的 Pane，删除它们的 worktree（会先询问）

每个模型在各自的 Pane 和 worktree 中工作，起点是你的文件夹当前的样子，包含未提交的文件和新文件（不含 .env 文件和链接），使用 auto：只有重量级命令才会询问。模型运行 30 分钟或花费 $2 后停止。云端模型同时运行，本地模型（Ollama、LM Studio）等其他模型结束后依次运行。全部结束后，各自的数据会显示在启动竞技场的 Pane 中，/arena card 会打开对比卡片：时间、费用、token、调用次数，以及各自改了什么。/arena take 会检查这些改动是否仍适用于你的文件夹；不适用则不会写入任何内容。

/arena sonnet, gpt-5: make the failing test pass
