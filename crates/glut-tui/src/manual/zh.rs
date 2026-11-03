//! The commands' manual pages in Chinese (T60 help box): what `/help` shows when a command is opened.

/// Every command's page, by its name, in `commands::COMMANDS`' order.
pub const PAGES: [(&str, &str); 67] = [
    (
        "clear",
        "在此文件夹中开始一个新的空会话。\n\
         \n\
         /clear — 对话、计划的任务列表、费用和计量都从零开始\n\
         \n\
         你离开的会话会保留：/resume 可再次打开它。你的模型、模式和设置保持不变。",
    ),
    (
        "resume",
        "打开此文件夹中的早先会话并继续。\n\
         \n\
         /resume — 此文件夹已保存会话的列表，最新的在前；分支列在其来源会话之下\n\
         \n\
         在列表中：输入即可筛选，⏎ 打开一个，⌫ 删除一个（先询问；其分支仍作为独立会话保留），⌃E 把一个会话导出为 Markdown，写入 `~/.glut/exports`。在另一个 Pane 中打开的会话，\
         请到那里去找（⌃P）。\n\
         \n\
         /search 可按会话中说过的内容查找会话。",
    ),
    (
        "search",
        "按词查找已保存的会话：/resume 的列表，按对话中的文本筛选。\n\
         \n\
         /search <text> — 对话中含有该文本的、此文件夹的会话\n\
         \n\
         在列表中，⌃A 在此文件夹与所有项目之间切换。⏎ 打开此文件夹的会话；其他项目的会话会显示其文件夹 — 在那里启动 Glut 即可继续。",
    ),
    (
        "btw",
        "顺便问一个不打扰对话的问题：答案显示在输入框上方的窗口中，绝不会成为会话的一部分。\n\
         \n\
         /btw <question> — 询问此 Pane 的会话，即使它的一轮仍在进行\n\
         /btw <name> <question> — 按编号或名字询问另一个 Pane 或后台代理\n\
         /btw — 重新打开上一个答案\n\
         \n\
         模型根据对话的当前状态作答，且不运行任何工具：如果它需要查找什么，请改为以消息的形式提问。其费用计入会话的费用。⎋ 关闭窗口，PgUp PgDn 滚动。\n\
         \n\
         /btw reviewer 你进展到哪了？",
    ),
    (
        "projects",
        "在另一个项目中继续：Glut 工作过的文件夹之一，或按路径指定任意文件夹。\n\
         \n\
         /projects — 列表：★ 已固定，● 当前这个；⏎ 打开一个，p 固定或取消固定，x 隐藏一个，直到再次在其中工作\n\
         /projects <path> — 打开该文件夹（⇥ 补全文件夹名）\n\
         \n\
         当一轮或某个代理仍在工作时，Glut 会先询问，再停止它们并切换。会话和此窗口的 Pane 会保留。在由宿主应用打开的窗口中，文件夹由宿主选择。",
    ),
    (
        "folder",
        "给本会话添加工作文件夹旁的文件夹，使 Glut 能跨项目工作。\n\
         \n\
         /folder — 工作文件夹旁的文件夹\n\
         /folder add <path> — 为本会话添加一个\n\
         /folder remove <path> — 移除一个\n\
         \n\
         文件工具通过完整路径访问这样的文件夹，命令可在沙箱内写入其中，Gate 像对待工作文件夹一样判定它，撤销也会恢复其中的文件。只有你能添加文件夹，模型、Skill 或项目都不能。Glut 拒绝主目录、\
         包含该项目或位于其内部的文件夹、含有密钥的文件夹，以及互相嵌套的两个文件夹。\n\
         \n\
         这些文件夹只对本会话有效；`glut run --folder <path>` 可把一个文件夹交给无界面运行。",
    ),
    (
        "compact",
        "总结到目前为止的对话，使其占用模型窗口更少的空间。\n\
         \n\
         /compact — 模型写一份总结，取代较早的消息\n\
         \n\
         系统提示词、你的最新消息和完整的步骤会保留；总结所取代的内容保存在会话的结果存储中，绝不丢弃。Glut 也会在窗口将满之前不久自动压缩（`~/.glut/settings.json` 中的 `compact`）。\
         /context 显示窗口有多满。",
    ),
    (
        "handoff",
        "把工作交接给此文件夹的后续会话。\n\
         \n\
         /handoff write — 模型写下已完成的工作、现状以及接下来要做的事\n\
         /handoff read — 把此文件夹的交接内容读入本会话\n\
         \n\
         交接内容保存在 `~/.glut/handoffs` 中，每个文件夹一份。读取后它会被移开，因此之后的会话不会读到两次。写一份交接是一次单独计费的调用；对话保持原样。",
    ),
    (
        "export",
        "把会话写出来，以便阅读或分享。\n\
         \n\
         /export — Markdown，写入 `~/.glut/exports`\n\
         /export html — 单个 HTML 文件，样式内嵌，无脚本\n\
         /export jsonl — 消息，JSON 行格式\n\
         /export copy — 把 Markdown 放到剪贴板，而不是写成文件\n\
         /export full — 附上完整的工具结果和系统提示词\n\
         /export <format> <path> — 写入你自己指定的文件（`~/` 是你的主目录，相对路径位于工作文件夹中）\n\
         \n\
         这些词可以任意顺序给出，路径放在最后。不加 full 时，导出的内容就是模型所见：较长的结果会被缩短。已有的文件绝不会被覆盖。Glut 隐藏的密钥依然隐藏，但你自己输入的内容仍可能含有密钥：分享之前请先读一遍导出内容。\
         在 /resume 中，⌃E 导出列表中的一个会话。",
    ),
    (
        "quit",
        "结束 Glut，连同所有 Pane 和后台代理。\n\
         \n\
         /quit — 当另一个 Pane 或代理仍在工作时，Glut 会先提示：再次 /quit 则连它们一并结束\n\
         \n\
         ⌃C ⌃C 效果相同，Glut 空闲时在空行上按 ⌃D 也一样。会话会保留供 /resume 使用。",
    ),
    ("exit", "结束 Glut，同 /quit。"),
    (
        "model",
        "选择本会话所使用的模型。\n\
         \n\
         /model — 当前所用提供商的模型，来自其保存的列表\n\
         /model <id> — 按 id 切换到该模型\n\
         \n\
         对话以新模型继续，下次启动也从它开始。另一个提供商的模型要在 /provider 之后选择。`glut models refresh` 更新这些列表。",
    ),
    (
        "provider",
        "切换到另一个模型提供商。\n\
         \n\
         /provider — 你已设置的提供商\n\
         /provider <id> — 切换到该提供商，使用其默认模型\n\
         /provider key [id] — 只更改该提供商的密钥（不带 id 时是默认提供商）\n\
         \n\
         提供商保存在 `~/.glut/providers.json` 中，其密钥保存在钥匙串中（或 `~/.glut/credentials.json`）；`glut providers add <id>` 添加一个。\
         对话继续，下次启动从你选择的提供商开始。\n\
         \n\
         密钥以隐藏方式输入，并在替换旧密钥之前用该提供商的模型列表检验；若失败，由你决定是否仍然保留，否则旧密钥保持不变。会话会立即使用保留下来的密钥。\
         使用账户登录、不需要密钥或从环境变量读取密钥的提供商会告知你。在 shell 中：`glut providers key <id>`。\
         ",
    ),
    (
        "login",
        "用订阅代替密钥登录 — 目前是你的 ChatGPT Plus 或 Pro 套餐。\n\
         \n\
         /login — 订阅列表及各自状态；⏎ 登录或退出\n\
         /login <provider> — 立即登录该订阅，例如 /login chatgpt\n\
         /login <provider> <address> — 通过 SSH：把浏览器最终停留的地址粘贴回来\n\
         /login openrouter — OpenRouter 给 Glut 一个专用密钥，像手动输入的一样保存；可在 openrouter.ai/settings/keys 限额或删除\n\
         \n\
         先显示地址；⏎ 在你的浏览器中打开它。Glut 把登录保存在钥匙串（或 `~/.glut/credentials.json`）中并自动续期；退出登录会在 OpenAI 撤销它。\n\
         \n\
         使用套餐时每次调用不花钱，但适用套餐的使用上限：达到上限时本轮结束，Glut 会指向 ChatGPT 设置 → 用量，你也可以在那里限制每个应用的用量。在 shell 中：glut providers \
         login|logout <provider>。",
    ),
    (
        "effort",
        "设置模型回答之前思考的深度。\n\
         \n\
         /effort — 可选的级别\n\
         /effort <level> — none、minimal、low、medium、high、xhigh 或 max\n\
         /effort default — 由提供商决定\n\
         \n\
         更高的投入通常意味着在困难的工作上答案更好，但更慢、费用更高。并非每个模型都支持每个级别。计量（⌃N）显示当前使用的级别。",
    ),
    (
        "route",
        "OpenRouter 上由哪些主机提供模型（其他提供方每个模型只由一个主机提供）。\n\
         \n\
         /route — Glut 当前发送的顺序，每个主机附原因、价格和速度\n\
         /route prefer <hosts> — 优先这些，按此顺序（xiaomi deepinfra）\n\
         /route avoid <hosts> — 从不使用这些\n\
         /route quant <levels> — 只用这些量化（fp8 bf16）\n\
         /route strict on|off — 只用优先主机，不以其他主机兜底\n\
         /route off — 去掉此模型的固定设置\n\
         /route rule on|off — 所有模型的规则\n\
         /route rule margin|uptime|speed|wait <n> — 规则的数值\n\
         \n\
         OpenRouter 用许多主机提供同一个模型，量化、价格和速度各不相同。Glut \
         每天读取一次模型的主机并按规则排序：全精度（bf16、fp16）先于 fp8，除非贵出 40% 以上；未标明量化的主机排在 \
         fp8 与 fp4 之间（模型制作方自己的主机按全精度算）；fp4 只在没有别的主机时使用；可用率低于 95%、每秒少于 30 \
         个 token 或首个 token 超过 10 \
         秒的主机排到最后。顺序随每次请求发送并允许兜底，所以请求不会因规则而失败。模型的固定设置优先于规则。一切保存\
         在 `~/.glut/providers.json`。",
    ),
    (
        "context",
        "查看模型的窗口有多满，或告诉 Glut 它有多大。\n\
         \n\
         /context — 已用的 Token、窗口及其大小的来源，以及指南（`AGENTS.md`、`CLAUDE.md`）每一轮的开销\n\
         /context <tokens> — 设置此模型的窗口：65536、64k 或 1M\n\
         \n\
         你设置的窗口会为此模型保存在 `~/.glut/providers.json` 中，并优先于提供商的列表。Glut 会在窗口将满之前不久自动压缩。",
    ),
    (
        "thinking",
        "选择模型的思考在流中如何显示。\n\
         \n\
         /thinking off — 隐藏\n\
         /thinking folded — 每个想法一行；点击可展开一个\n\
         /thinking open — 完整显示每个想法\n\
         \n\
         单独的 /thinking 会折叠它们。⌃T 一次性展开或折叠所有想法。你的选择保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "panel",
        "在计划标签页上显示侧边栏，或隐藏它。\n\
         \n\
         /panel — 带有各标签页的侧边栏：计划、Gate、计量、代理、待办、运行和按键\n\
         \n\
         ⌃B 效果相同，⌃X ⇥ 把按键交给侧边栏（←→ 切换标签页，↑↓ 在其内移动，⎋ 返回）。侧边栏需要至少 100 列宽的窗口。是否显示会保留到下次启动。",
    ),
    (
        "meter",
        "查看会话的用量：上下文、Token 和费用。\n\
         \n\
         /meter — 侧边栏的计量标签页\n\
         \n\
         它显示窗口有多满以及 Glut 何时压缩，会话输入、缓存和输出的 Token 及其价格，每一轮和每个工具，以及 Glut 的精简输出节省了多少。⌃N 也能打开它。",
    ),
    (
        "agents",
        "查看 Glut 在本会话中启动的子代理。\n\
         \n\
         /agents — 子代理列表，每个列在启动它的那个之下；⏎ 打开其自己的流\n\
         \n\
         点击子代理的磁贴也能打开它。⌃X A 打开代理标签页：所有子代理和后台代理，实时显示 — ⏎ 打开一个，x 停止它。",
    ),
    (
        "plan",
        "完整阅读 Glut 从计划模式发来的计划，附任务列表。\n\
         \n\
         /plan — 本会话的最新计划\n\
         \n\
         在计划模式（⇧⇥）下，Glut 只阅读和规划而不更改任何内容，然后发出它的计划，由你批准或退回。/send plan 把它交给另一个 Pane 去构建。",
    ),
    (
        "scrollback",
        "把对话放进终端自身的回滚区，用终端自己的工具滚动、搜索和复制。\n\
         \n\
         /scrollback — 打印对话；⏎ 或 ⌃C 回到 Glut\n\
         \n\
         F3 效果相同。",
    ),
    (
        "split",
        "在主 Pane 旁的一个 Pane 中打开新会话。\n\
         \n\
         /split — 一个新 Pane；在 git 仓库中，Glut 会询问它在哪里工作\n\
         /split here — 它在此文件夹中工作，就在你身边\n\
         /split tree [branch] — 它在一个新的 Worktree 中、在自己的分支上工作\n\
         /split resume — 它打开一个早先的会话，从列表中选取\n\
         /split as <role> — 它扮演一个角色（/role 列出它们）\n\
         \n\
         主 Pane 旁最多有六个 Pane，分成两行，每行三个。⌃P 在各 Pane 之间轮转，⌃X 2 也能拆分，⌃X } 和 ⌃X { 使主 Pane 变宽或变窄。这里的两个 Pane 会更改相同的文件；\
         Worktree 使它们互不干扰，直到 /close 询问如何结束它。\n\
         \n\
         /split tree fix-login as reviewer",
    ),
    (
        "arena",
        "让同一任务同时在两到四个模型上运行并比较结果（竞技场）。\n\n/arena — 列出所有 provider 的模型；空格键标记两到四个，⏎ 将它们放入输入框，再接着写任务\n/arena <model>, <model>: <task> — 同样的操作写成一行（provider/model，或能对应唯一模型的名称）\n/arena take <n> — 把模型 n 的改动应用到你的文件夹，不提交\n/arena card — 再次显示对比卡片：各模型的数据和检查结果，下方是它的 diff（←/→ 切换模型，s 并排显示两个，c 检查，t 应用）\n/arena check <command> — 针对此仓库的检查（cargo test、npm test 等），在每个模型的 worktree 中、在其沙箱里运行；保存后，会在下一次竞技场之后自动运行\n/arena tasks — 列出 ~/.glut/arena/tasks 中基准测试的任务集；/arena tasks <set> 用上一个竞技场的模型运行一个任务集（或 /arena tasks <set>: <model>, <model>；在任务集后加 x3，每个任务连续运行三次（最多 x5），表格随后显示 2/3），逐个任务运行，每个任务由各自的检查判定\n/arena results — 每个任务集的结果（通过的任务数、Ø 耗时和费用）以及自由竞技场的胜利数\n/arena blind on|off — 自由竞技场盲测（默认）：模型以随机顺序的字母显示，在你采用其中一个或结束竞技场后显示名称\n/arena end — 关闭竞技场的 Pane，删除它们的 worktree（会先询问）\n\n每个模型在各自的 Pane 和 worktree 中工作，起点是你的文件夹当前的样子，包含未提交的文件和新文件（不含 .env 文件和链接），使用 auto：只有重量级命令才会询问。模型运行 30 分钟或花费 $2 后停止。云端模型同时运行，本地模型（Ollama、LM Studio）等其他模型结束后依次运行。全部结束后，各自的数据会显示在启动竞技场的 Pane 中，/arena card 会打开对比卡片：时间、费用、token、调用次数，以及各自改了什么。/arena take 会检查这些改动是否仍适用于你的文件夹；不适用则不会写入任何内容。\n\n/arena sonnet, gpt-5: make the failing test pass",
    ),
    (
        "swap",
        "让主 Pane 与另一个 Pane 互换位置。\n\
         \n\
         /swap — 此 Pane 成为主 Pane\n\
         /swap <n> — Pane n 成为主 Pane",
    ),
    (
        "close",
        "关闭一个 Pane，或结束一个后台代理。\n\
         \n\
         /close — 关闭此 Pane（主 Pane 除外）；当它正在工作时，再次 /close 会停止它并关闭\n\
         /close <name> — 结束该后台代理，其一轮被停止\n\
         /close <n> — 在当前位置按编号、名称或角色关闭 Pane n；你仍留在自己的 Pane\n\
         单独 /close，若可关闭的不止此 Pane — 显示列表，此 Pane 在最前：⏎ 关闭所选项\n\
         \n\
         会话保留供 /resume 使用。位于 Worktree 中的 Pane 会询问如何结束该 \
         Worktree：保留它及其分支，接管它（合并到你当前所在的分支），或丢弃它。⌃X 0 也能关闭此 Pane。",
    ),
    (
        "send",
        "向另一个 Pane 或后台代理发送一条消息，在那里如同你亲手输入的一样处理。\n\
         \n\
         /send — 可发送对象的列表：此处的 Pane 和代理，然后是其他 Glut 窗口；⏎ 会把 /send <who> 放入输入框\n\
         /send <text> — 发给唯一的另一个 Pane\n\
         /send <n|name> <text> — 发给 Pane n 或该名字的代理（三个及以上时需要）\n\
         /send plan — 把此 Pane 的计划交给另一个 Pane 去构建\n\
         /send <n|name> plan — 交给指定的那一个\n\
         \n\
         会话空闲时，消息立即开始；否则在其一轮之后开始。按键仍留在此 Pane。",
    ),
    (
        "role",
        "让此 Pane 扮演一个角色：一个具名代理的提示词和工具，而非其模型。\n\
         \n\
         /role — 可选的角色\n\
         /role <name> — 从现在起扮演该角色\n\
         /role none — 再次成为全能角色，拥有所有工具\n\
         \n\
         Glut 自带 reviewer、tester 和 docs；你自己的角色放在 `~/.glut/agents` 中，项目的放在其 `.glut/agents` 中（仅在受信任的文件夹中读取）。角色的工具立即生效，\
         提示词从下一轮起生效。限制工具的角色需要 Gate。",
    ),
    (
        "bg",
        "在后台启动一个代理：没有 Pane 的会话，通过名字联系。\n\
         \n\
         /bg — 名为 agent（或 agent-2、…）的代理；在 git 仓库中，Glut 会询问它在哪里工作\n\
         /bg <name> — 该名字的代理（小写字母、数字和连字符）\n\
         /bg as <role> — 它扮演一个角色，并以该角色命名\n\
         /bg here — 它在此文件夹中工作\n\
         /bg tree [branch] — 它在一个新的 Worktree 中工作\n\
         \n\
         /send <name> 给它工作，/peek <name> 读取它的流，/show <name> 把它调到前台，/close <name> 结束它。每个代理各自工作、各自计费：在一个窗口内启动第七个之前，Glut \
         会询问一次。⌃X B 启动一个并选择其角色，⌃X A 列出它们。\n\
         \n\
         /bg docs-check as docs",
    ),
    (
        "show",
        "把一个后台代理调到前台，成为一个 Pane。\n\
         \n\
         /show <name> — 该代理成为网格中的最后一个 Pane\n\
         \n\
         窗口没有空位时，先 /hide 或 /close 一个 Pane。⌃X A 列出后台的代理。",
    ),
    (
        "hide",
        "把此 Pane 送到后台，并保留其记忆。\n\
         \n\
         /hide — 该 Pane 离开屏幕，作为后台代理继续工作\n\
         \n\
         它保留一个可用于联系的名字（其角色，或 pane-<n>）：/show <name> 把它带回来，/send <name> 和 /peek <name> 可联系它。主 Pane 和最后一个 Pane 会保留。⌃X H \
         效果相同。",
    ),
    (
        "peek",
        "在此 Pane 之上阅读后台代理的流，而不把它调到前台。\n\
         \n\
         /peek <name> — 它的流，实时，仅供阅读\n\
         /peek — 再次关闭它\n\
         \n\
         ⎋ 也能关闭；/show <name> 把代理调到前台。",
    ),
    (
        "loop",
        "在 Glut 打开期间，让一个专属的后台代理反复执行一项任务。\n\
         \n\
         /loop <interval> [fresh] [as <role>] [notify] <task> — 启动一个循环；第一轮立即开始\n\
         /loop — 列出各循环：间隔、轮数、下一轮以及目前的费用\n\
         /loop stop [name] — 结束该循环，或唯一的那个\n\
         /loop rounds [n] — 循环的最多轮数；单独使用则显示当前的限制\n\
         /loop budget [usd] — 循环最多可花费的金额；单独使用则显示当前的限制\n\
         /loop tokens [n] — 订阅上的循环最多可用的 token 数 (5M, 500k)；单独使用则显示当前的限制\n\
         \n\
         各轮绝不重叠：间隔从一轮结束时算起，写作 30s、5m、2h 或 1h30m（从 10s 到 24h）。一轮会接续上一轮的对话；使用 fresh 时，每一轮都开始新的对话。任务是一条消息，或 /skill \
         <name> [用途]、/compact 或 /init — 不能是其他命令。\n\
         \n\
         该代理名为 loop（或以其角色命名），并沿用此 Pane 的模式、你在这里给出的“始终”、其旁的文件夹和其角色；as <role> 只能指定此 Pane 正在扮演的角色。它的询问会等你回答。\
         当某一轮需要询问或循环结束时，会出现一个磁贴；使用 notify 时，则每一轮之后都出现。\n\
         \n\
         循环在达到最多轮数后结束，或在各轮的花费达到其预算时结束（默认 50 轮和 5 $，否则取 `~/.glut/settings.json` 的 `loop.rounds` 和 `loop.maxUsd`；\
         修改只对之后启动的循环有效），或在某一轮失败、或你停止它（在它的那一轮中按 ⎋）时结束，或在其模型因你的任务所指定的终点已达到而调用 `end_loop` 时结束。模型的价格未知时，只有轮数会结束它。\
         之后它的代理仍留在代理标签页中，可阅读或 /close。在代理标签页（⌃X A）中，其卡片上的 x 结束该循环。\n\
         \n\
         /loop 10m 运行测试并修复失败项；全部通过时停止\n\
         /loop 1h fresh as reviewer notify 审查最近一次提交",
    ),
    (
        "schedule",
        "在 Glut 关闭期间运行的计划：在某个时刻或每隔一段时间执行一项任务。\n\
         \n\
         /schedule — 运行标签页（⌃X R）：每个计划及其下次时间和上次结果\n\
         /schedule list — 在流中列出计划\n\
         /schedule add <name> [flags] <task> — 创建计划，或修改同名的计划\n\
         /schedule run <name> — 立即运行它，与 Glut 并行\n\
         /schedule pause <name> — 暂停它，直到 resume\n\
         /schedule resume <name> — 它重新按时间运行\n\
         /schedule remove <name> — 计划及其记录一并删除；其会话保留\n\
         \n\
         flags 即 `glut schedule add` 的标志：\n\
         - `--at 07:00` 一天中的某个时刻，可与 `--days mo-fr` 或 `sa,su` 搭配（不加则每天）\n\
         - `--every 2h` 从 10m 到 7d 的间隔，用于代替 --at\n\
         - `--mode ask|build|plan|dont_ask|auto` 没有规则时它可以不经询问做什么；除非指定，否则为 ask\n\
         - `--allow <rule>`、`--deny <rule>` Claude Code 格式的规则，例如 `Bash(cargo test:*)`；可重复\n\
         - `--folder <path>` 它自己旁边的文件夹；可重复\n\
         - `--max-usd 1`、`--max-minutes 30` 一次运行最多可花费和占用的量：除非指定，否则为 1 $ 和 30 分钟\n\
         - `--max-tokens 1M` 在订阅上一次运行最多可用的 token 数：除非指定，否则为 1M\n\
         - `--replace` 给出的规则和文件夹取代计划原有的，而不是并入其中\n\
         - `--cwd <path>`、`--provider <id>`、`--model <id>` 它的文件夹（否则为当前这个）、提供商和模型，创建时即固定\n\
         \n\
         任务是一条消息，或 /skill <name> [用途]。launchd 启动每一次运行，且仅在你登录期间；睡眠中错过的时间会在唤醒时补跑一次。那里没有人可以回答：每一次询问都会被拒绝并记录，所以请允许它需要的一切。\
         每次运行都是一个独立保存的会话，并通过 macOS 通知告知。在运行标签页中：⏎ 打开计划或某次运行的会话，r 立即运行，p 暂停或恢复，d 删除。\n\
         \n\
         /schedule add tests --at 07:00 --days mo-fr --allow \"Bash(cargo test:*)\" 运行测试并报告失败的项",
    ),
    (
        "sessions",
        "给本机其他 Glut 窗口写信。\n\n/sessions — 它们的 Pane 和代理：地址、项目、模型、模式、工作中或等待中；⏎ 会把 \
        /send <address> 放入输入框\n/sessions off — 此窗口不再接收它们的消息\n/sessions on — \
        恢复接收\n\n那边所有者的消息会立即进入；那边模型的消息只有在与目标 Pane \
        同一模式时才立即进入，否则在输入框上方等待：⏎ 放行，Esc 丢弃。两者都不是你对这里模型的指示。只有 Glut \
        与其他窗口通信 — 模型的命令永远无法触及它们。",
    ),
    (
        "name",
        "为此窗口在本机 Glut 窗口中命名。\n\n/name — 此窗口当前的名称\n/name <new> — 新名称：字母、数字、- _ 和 \
        .\n\n其他窗口通过名称找到此窗口，通过 name/2 或 name/reviewer 找到某个 Pane。你起的名称随会话保存，/resume \
        时恢复。",
    ),
    (
        "diff",
        "逐个 hunk 审阅上一轮改动了什么。\n\
         \n\
         /diff — 上一个改动了文件的那一轮的更改\n\
         /diff session — 本会话的所有更改\n\
         \n\
         在审阅中：↑↓ 切换 hunk，←→ 切换文件，a 接受，r 驳回（该 hunk 会被还原，Glut 会在你的下一条消息时读到原因），c 评论（随你的下一条消息发出），v 把文件标为已审阅，s 在本轮与会话之间切换，\
         ⎋ 关闭。⌃R 也能打开它；/review 是同一条命令。",
    ),
    (
        "steps",
        "查看改动过文件的步骤，并把文件恢复回去。\n\
         \n\
         /steps — 每个改动过文件的步骤；在某一步上按 ⏎ 显示它改了什么，或把文件恢复到它之前的状态\n\
         \n\
         恢复文件时，你在那之后所做的编辑也会一并还原；对话保持不变。Glut 在每个会更改文件的步骤之前保留一份快照；文件刚才的状态也会保留，所以 `glut undo` 能把它找回来。⎋⎋ 则是回退对话。",
    ),
    (
        "todo",
        "把项目的待办事项保存在其 `TODO.md` 中，这是你和 Glut 共同使用的清单。\n\
         \n\
         /todo — 待办标签页：空格勾选一项，a 添加一项，o 打开编辑器\n\
         /todo <text> — 在清单末尾添加一项，Glut 工作时也可以\n\
         /todo open — 在你的编辑器中打开清单\n\
         /todo clean — 删除已完成的项，会先询问\n\
         \n\
         文件的其余部分绝不会被改动。⌃Y 显示或隐藏该标签页。",
    ),
    (
        "memory",
        "查看项目记忆保存的内容：Glut 为之后的会话记下的事实。\n\
         \n\
         /memory — 带描述的事实；⏎ 完整显示一条\n\
         /memory open — 在你的编辑器中打开索引（`MEMORY.md`）\n\
         \n\
         记忆位于仓库之外：在 `~/.glut/memory/<project>/` 中，或在 Claude Code 于该仓库中工作的情况下使用 Claude Code 的记忆。Glut 用它的 `memory` 工具写入；\
         在这里读取不会询问模型。",
    ),
    (
        "skill",
        "启动一个 Skill：针对某类任务的指令，模型会加载并遵循它。\n\
         \n\
         /skill — 可选的 Skill\n\
         /skill <name> [用途] — 在名字之后所写的内容上运行该 Skill\n\
         \n\
         Glut 自带 make-agent、make-skill 和 make-hook，用来编写你自己的。你的放在 `~/.glut/skills` 中，项目的放在其 `.glut/skills` \
         中（仅在受信任的文件夹中读取）；同名的会遮蔽 Glut 自带的。\n\
         \n\
         /skill make-hook 阻止在 main 上执行 git push",
    ),
    (
        "init",
        "为此文件夹写一份简短的 `AGENTS.md`，或检查并精简已有的指南。\n\
         \n\
         /init — 没有指南时 Glut 写 `AGENTS.md`；有指南时检查它并保持简短\n\
         /init --migrate — 再次询问是否把 `CLAUDE.md` 移入 `AGENTS.md`\n\
         \n\
         只有 `CLAUDE.md` 时，Glut 会询问一次是否把它移入 `AGENTS.md`（`CLAUDE.md` 随后导入它，所有 harness 共用一份指南）；回答否则会被记住。指南每一轮都会随带发送：\
         /context 显示它们的开销。",
    ),
    (
        "gate",
        "查看 Gate 据以判定的规则，并收回一个“始终”。\n\
         \n\
         /gate — 当前生效的规则：你在本会话中的“始终”，然后是 `~/.glut/settings.json` 和项目的 `.glut/` 中的规则；在某个“始终”上按 ⏎ 将其收回，Glut 下次会再次询问\n\
         \n\
         ⌃G 打开 Gate 标签页：每一次判定实时显示，包括访问过的主机和拒绝。⇧⇥ 切换模式。",
    ),
    (
        "mcp",
        "查看 MCP 服务器及各自的状态，登录托管的服务器，添加或移除服务器。\n\
         \n\
         /mcp — 每个服务器：已连接及其工具、等待登录，或失败\n\
         /mcp login <name> — 登录托管的服务器；按 ⏎ 在浏览器中打开其地址\n\
         /mcp login <name> <address> — 通过 SSH：把浏览器最终停留的地址粘贴回来\n\
         /mcp logout <name> — 退出登录并忘记其 Token\n\
         /mcp add <name> <url> — 把托管的服务器添加到 `~/.glut/mcp.json` 并连接它\n\
         /mcp add <name> -- <command> [args] — 添加由 Glut 自己启动的服务器\n\
         /mcp remove <name> — 把它从 `~/.glut/mcp.json` 和会话中移除\n\
         \n\
         如果服务器提供的登录方式 Glut 无法使用，则会要求输入 Token（隐藏输入）。项目自带的服务器由项目自己更改：/mcp \
         remove 只移除你的。`glut mcp` 在会话之外执行同样的操作。\n\
         \n\
         服务器在其工具运行期间可能会询问你：一个表单（⇥ ↑↓ 切换字段，空格勾选，⏎ 发送，Esc 关闭，⌃D \
         拒绝）或一个要打开的页面（o）。只有你来回答，模型永远不会；索要密码或密钥的表单会被拒绝，无人在场时所有询问都\
         会被拒绝。",
    ),
    (
        "images",
        "选择图片在流中的绘制方式。\n\
         \n\
         /images inline — 在终端能显示的地方显示真实图片\n\
         /images blocks — 真彩色半块字符\n\
         /images off — 只显示图片所在的那一行\n\
         \n\
         单独的 /images 显示当前的选择。默认情况下，Glut 在终端支持时内嵌绘制，其他情况下用块字符。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "links",
        "选择链接在流中的显示方式。\n\
         \n\
         /links clickable — 链接文字即是链接，前提是终端支持 OSC 8\n\
         /links plain — URL 写在文字之后\n\
         \n\
         单独的 /links 显示当前的选择。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "notify",
        "窗口不在前台时，在一轮结束或 Glut 询问时得到通知。\n\
         \n\
         /notify on — 一轮的结束和每一次询问\n\
         /notify asks — 仅询问\n\
         /notify off — 什么都不通知\n\
         /notify system — 通过 macOS 自带的通知（点击通知会打开“脚本编辑器”，而不是终端）\n\
         /notify after <s> — 只有一轮运行超过这么多秒后，才通知其结束（默认 10，最多 3600）\n\
         /notify tiles on|off — 窗口右上角的磁贴\n\
         \n\
         单独的 /notify 显示当前生效的设置。在知道终端支持自带通知的地方，Glut 使用它们，以及铃声。通知会写明 Glut、项目、Pane 和发生了什么，但绝不包含模型的话。保存在 \
         `~/.glut/prefs.json` 中，优先于 `~/.glut/settings.json` 中的 `notify`。",
    ),
    (
        "secrets",
        "查看并设置本会话中对模型隐藏的密钥。\n\
         \n\
         /secrets — 迄今隐藏的每个值，按编号、种类、长度和出现位置列出（绝不显示值本身）\n\
         /secrets allow <n> — 本会话中放行密钥 n\n\
         /secrets allow <n> always — 始终放行它（保存其指纹，绝不保存值）\n\
         /secrets add <name> <regex> — 隐藏你自己的格式，从下一个结果起生效\n\
         /secrets remove <name> — 删除你的一个格式\n\
         /secrets on|off — 开启或关闭遮蔽\n\
         \n\
         工具回答中的密钥到达模型时是 ‹secret n: …›；Glut 只会把它的值放回已含有该值的文件中。你的消息看起来像含有密钥时，发出之前会先询问。模型会在你的下一条消息时得知更改。你的格式和选择保存在 \
         `~/.glut/secrets.json` 中。\n\
         \n\
         遮蔽只防范意外显示的密钥；沙箱和网络 Gate 仍是屏障。",
    ),
    (
        "keychain",
        "让钥匙串远离 Glut 运行的命令。\n\
         \n\
         /keychain — 它是否开放，以及可以请求它的程序\n\
         /keychain add <program> — 把一个程序加入列表（gh，或 git 加子命令：git push）\n\
         /keychain remove <program> — 把它从列表中移除\n\
         /keychain open — 对所有命令开放，直到本会话结束\n\
         /keychain close — 再次关闭它\n\
         \n\
         钥匙串中的密钥或登录可以绕过沙盒的每条文件规则被读取，所以 Glut \
         运行的命令看到的钥匙串是关闭的。列表中程序的命令会先询问 — 在每种模式下，包括 `auto` — \
         你允许后便在钥匙串开放的情况下运行；“总是”会把它变成你的一条规则。没有人在场时（计划、`glut \
         run`），只有这样的规则才能打开它。当命令因钥匙串关闭而失败时，Glut 会在其输出下方说明。\n\
         \n\
         允许一个命令，也就把钥匙串交给了它启动的一切：git Hook、gh 扩展或构建脚本都可以读取其中的任何密钥。\n\
         \n\
         只有你自己的规则能放行钥匙串：项目 `.glut/` \
         中的允许规则可以让命令运行，但绝不会打开钥匙串。命令链只有在每个部分都在列表中或只读取时才能获得它。`glut \
         keychain open` 保持的打开状态只在这里（你在场时）有效 — 在 `glut run` 或宿主的运行中从不生效。\n\
         \n\
         你的规则只有在指明程序及其子命令时（`gh pr *`、`git push *`）才会不经询问打开它 — 从不针对整个程序（`gh \
         *`）。🔑 卡片显示完整命令，用 ↑↓ 浏览，读到末尾后才能允许。\n\
         \n\
         列表保存在 `~/.glut/keychain.json`；`glut keychain open` 让钥匙串跨会话保持开放，`glut keychain close` \
         永久关闭它。",
    ),
    (
        "sandbox",
        "Glut 运行的命令如何被约束，以及它们能否写入 Git 文件夹。\n\n/sandbox — 这里的沙箱（macOS：Seatbelt；Linux：bubblewrap 和 seccomp）和 Git \
         开关\n/sandbox git read-only — 没有命令写入 Git 文件夹：没有命令能从沙箱提交\n/sandbox git writable — 命令可以重新写入 Git \
         文件夹，也可以提交\n\
         /sandbox required on — 没有沙箱的地方不会运行任何命令（缺少 bubblewrap、没有权限的容器）；off \
         让它们只受规则约束地运行\n\n沙箱只让命令写入你的文件夹，读取除你的机密和 .env 文件之外的一切，并且只能通过 Glut 的代理访问网络。在 Linux \
         上，它无法拒绝一个尚不存在的名字：命令创建的、macOS 会拒绝的东西 — 新的 .envrc、.git、.git/commondir — 一出现就会被移到 \
         ~/.glut/quarantine，命令结束后还会再检查一次。恰好在那一刻运行的你的工具仍可能遇到它；Git \
         文件夹只读时，其中根本不会产生这类东西。\n\n这个开关从下一个命令起在每个会话和窗口中生效，保存在 ~/.glut/sandbox.json 中 — 从不由命令写入。glut sandbox show 和 \
         glut sandbox git read-only|writable 在 Glut 之外做同样的事。",
    ),
    (
        "classifier",
        "让第二个模型判断那些仅由模式询问你的命令。\n\
         \n\
         /classifier — 由哪个模型判断，或者不判断\n\
         /classifier session — 由会话的模型判断（只要你未设置其他模型）\n\
         /classifier <provider> <model> — 单独的模型，快速的即可（openrouter deepseek/deepseek-v4.1-flash）\n\
         /classifier off — 不判断：每次都询问你\n\
         \n\
         Glut 认识许多只读取的命令（ls、grep、git status 等），并在任何模式下运行它们。由它们组成的循环或管道（for f in *.md; do grep -c x \"$f\"; done）无法仅凭文本判断，所以会询问。第二个模型只看到该命令及其文件夹 — 从不看到对话、文件或输出 — 并回答 READS 或 CHANGES。只有 READS 时，命令才会像已知的读取命令一样在沙箱中运行；其他情况，或几秒内没有明确答案，则照常询问你，并在命令下方附上它的意见。你的规则、重型命令、钥匙串和沙箱保持不变；同一命令在一个会话中不会被判断两次。\n\
         \n\
         它放行的命令会在旁边注明。选择保存在 `~/.glut/classifier.json`，从下一轮起生效；`glut run --classifier` 可为一次运行设置。",
    ),
    (
        "ide",
        "Glut 旁边的编辑器：装了 glut.nvim 插件的 Neovim、VS Code（以及 Cursor、Windsurf），或装有 Glut 自己插件的 JetBrains IDE。\n\
         \n\
         /ide — Glut 连着哪个编辑器，哪些合适\n\
         /ide on — 连接（有多个合适时会询问）\n\
         /ide off — 在本次会话中断开\n\
         \n\
         连接后，你的每条消息都会带上当前文件和选区（下方显示 \
         ⧉），只要没有读取规则拦下该文件；模型可以询问打开了哪些文件、语言服务器怎么说；Glut \
         按编辑器所持的样子读取缓冲区（包括未保存的），关于改动的 gate 询问也会在编辑器中以差异显示 — \
         以先到的回答为准。启动时若有合适的编辑器，Glut 会自动连接：启动 Glut 的那个，否则是文件夹包含 Glut \
         文件夹的那个。页脚的“⧉”表示已连接。",
    ),
    (
        "clean",
        "防止 Glut 在 ~/.glut 中的数据无限增长。\n\
         \n\
         /clean — Glut 保存了什么（逐项），以及按你的期限可以清理的部分\n\
         /clean now — 立即在后台清理\n\
         /clean keep — 各项期限\n\
         /clean keep <kind> <days>d — 设置一项：snapshots、journals、sessions、results 或 lean\n\
         \n\
         会清理的内容：超过期限（7 天）的运行快照 — \
         当一个文件夹的快照全部过期时，整个存储一并删除，无论文件夹是否还在；未完成运行的日志（7 天）；90 \
         天未写入的会话，连同其事件日志和长结果；没有会话的长结果在 30 天后；没有任何保留会话引用的图片；lean.jsonl \
         中超过 90 天的行。导出只计数：那是你的文件。\n\
         \n\
         Glut 每天启动时自动清理一次 — 在这里和 `glut run` 中 — 并告诉你释放了多少。构建输出和缓存（带 CACHEDIR.TAG \
         的文件夹、node_modules/ 下或 Cargo 项目的 target/ 下）完全不做快照。\n\
         \n\
         期限保存在 `~/.glut/clean.json`。命令行：`glut clean`、`glut clean --dry-run`、`glut clean keep sessions \
         30d`；`glut clean --dry-run --json` 为程序输出报告。",
    ),
    (
        "motion",
        "选择屏幕的动态程度。\n\
         \n\
         /motion full — Glimm 呼吸，火花升起，火焰闪动\n\
         /motion calm — 一切保持静止；只有发生变化时才绘制一帧\n\
         /motion off — 同 calm\n\
         \n\
         单独的 /motion 显示当前的选择。无论你如何选择，macOS 的“减弱动态效果”和阅读器模式都会让屏幕保持静止。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "rule",
        "本会话中模型每个回合都遵守的规则。\n\
         \n\
         /rule <text> — 保留它，例如“每一步都更新任务清单”\n\
         /rule — 当前规则，带编号\n\
         /rule remove <n> — 撤回第 n 条\n\
         \n\
         规则在指南之后随每个回合的指示一起发送，直到你撤回；/clear 会保留它们，新会话从空开始。最多 20 条，每条 500 字符。若要每个会话都生效，请写入 `~/.glut/AGENTS.md` 或项目指南。",
    ),
    (
        "maxsteps",
        "一个回合最多走多少步。\n\
         \n\
         /maxsteps — 当前的上限\n\
         /maxsteps <n> — 回合在 n 步后停下，1 到 1000\n\
         \n\
         一步就是一次模型调用。到达上限时回合停下并说明；在空输入上按 ⏎ 会发送“Go on where you stopped.”，工作继续。预算和时间限制仍是真正的刹车。保存在 \
         `~/.glut/prefs.json`；`~/.glut/settings.json` 中的 `maxTurns` 对每次运行设置，`glut run --max-turns` 对一次运行设置。",
    ),
    (
        "nudge",
        "Glut 是否让模型在回合结束时继续、在工作时记住它的任务清单，并在没有清单时写一份。\n\
         \n\
         /nudge on — 当一个回合在任务清单仍有未完成项、或以宣布下一步的话（“接下来…”、结尾的冒号）结束时，Glut 给模型发一行自己的话：继续，或说明为何停下；记录中会低调显示。工作时清单没有变化，Glut \
         会提醒它，仍无变化时再用一条单独的消息要求一次；一次运行里从没写过清单，Glut 会请它写一份（每回合一次）\n\
         /nudge off — 回合在模型结束的地方结束：不提醒清单，也不请求写一份\n\
         \n\
         结尾的提问每回合一次，绝不在 plan 模式、向你提问之后、循环的一轮或计划的运行中；提醒与请求跟随同一个选择，但在计划的运行中 Glut 总会提醒。单独 /nudge 显示当前选择。保存在 \
         `~/.glut/prefs.json`；`~/.glut/settings.json` 中的 `\"nudge\": false` 对每次运行关闭它。",
    ),
    (
        "lsp",
        "语言服务器是否帮助模型。\n\
         \n\
         /lsp on — 编辑之后，文件的错误随结果一起返回，模型可以使用 lsp 工具：符号在哪里定义、在哪些地方使用、它的类型和文档，以及在整个项目中重命名\n\
         /lsp off — 不运行任何服务器：它们的进程立即结束，编辑后不再报错，也没有 lsp 工具\n\
         /lsp — 当前的选择，以及本会话的每个服务器：就绪、正在为项目建立索引，或本会话中已关闭\n\
         \n\
         Glut 只启动从不运行项目代码的服务器（不带构建脚本和过程宏的 rust-analyzer、pyright，以及 TOML、YAML、JSON、CSS、HTML 和 Markdown \
         的服务器），只在没有网络的沙盒中，并且只在 Gate 允许其命令行时启动。服务器在其语言的第一次编辑或提问时启动，并在会话期间一直保留；rust-analyzer 为大型项目建立索引需要几秒钟，在此期间 lsp \
         会说明，模型改用 grep。重命名是一次更改：对所有文件只询问一次，要么全部要么都不，之前先拍快照。保存在 `~/.glut/prefs.json`；`~/.glut/settings.json` 中的 \
         `\"lsp\": false` 对所有运行关闭，`glut run --no-lsp` 只对一次运行关闭。",
    ),
    (
        "reader",
        "为读屏软件设置 Glut。\n\
         \n\
         /reader on — 按键用文字表示，页脚简洁，没有 Glimm，边框朴素，没有任何动态，每次状态变化时输出一行文字\n\
         /reader off — 通常的屏幕\n\
         \n\
         单独的 /reader 显示当前的选择。保存在 `~/.glut/prefs.json` 中；`glut --reader` 使其在一次运行中保持开启。",
    ),
    (
        "language",
        "选择 Glut 对你说的语言。\n\
         \n\
         /language — 可选的语言\n\
         /language <code> — en、de、fr、it、es、zh、ja 或 pt\n\
         \n\
         未做选择时，Glut 使用你系统的语言。无论你在这里如何选择，模型都用你书写的语言作答。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "theme",
        "选择 Glut 的颜色。\n\
         \n\
         /theme — 可选的主题\n\
         /theme <name> — glut（Glut 自己的），或 Axiomata 的某个主题（catppuccin-mocha、github-dark、…）\n\
         \n\
         Glut 只给文字着色并保留终端的背景：浅色主题需要浅色终端。保存在 `~/.glut/prefs.json` \
         中；在由宿主应用打开的窗口中，该选择只对本会话有效。\n\
         \n\
         每个主题都会给页脚的各部分、按类别区分的工具名称（读取、写入、命令、对外访问）、各个 Pane \
         和代理以及输入框上方的线条赋予各自柔和的颜色；需要你处理的内容依然醒目。阅读模式和 `NO_COLOR` 保持朴素。",
    ),
    (
        "editor",
        "选择 Glut 用来打开文件和输入内容的编辑器。\n\
         \n\
         /editor — 在你的系统上找到的编辑器\n\
         /editor <command> — 你自己的命令，`{file}` 和 `{line}` 放在需要的位置\n\
         \n\
         没有 `{file}` 时，文件会添加在末尾。窗口式编辑器必须等到文件关闭后才返回（`code --wait`）。⌃X E 在其中编辑输入；/todo open 和 /memory open 也使用它。保存在 \
         `~/.glut/prefs.json` 中。\n\
         \n\
         /editor nvim +{line} {file}",
    ),
    (
        "cache",
        "选择空闲会话如何保持提供商的提示词缓存，使下一轮从热缓存开始。\n\
         \n\
         /cache ping — 会话空闲期间，Glut 每隔几分钟再次读取缓存，最多 30 分钟\n\
         /cache 1h — Anthropic 保留一小时（每次写入费用翻倍）；其他提供商则靠读取来保持\n\
         /cache off — 不保持：停顿超过提供商的几分钟后会冷启动\n\
         \n\
         单独的 /cache 显示当前的选择；ping 是默认值。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "footer",
        "选择页脚如何连接它的各部分。\n\
         \n\
         /footer powerline — Powerline 的箭头\n\
         /footer plain — 普通方块\n\
         \n\
         单独的 /footer 显示当前的选择及其原因。未做选择时，Glut 根据终端推测它是否绘制 Powerline 的字形。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "vim",
        "以 vi 的方式编辑输入。\n\
         \n\
         /vim on — ⎋ 进入普通模式（移动、操作符、计数、u 和 ⌃R），i a I A o O 回到插入模式\n\
         /vim off — 通常的编辑键\n\
         \n\
         单独的 /vim 会将其开启。普通模式下的 ⎋ 会停止 Glut。保存在 `~/.glut/prefs.json` 中。",
    ),
    (
        "help",
        "每个命令和每个按键，放在专属的框中。\n\
         \n\
         /help — 命令各占一行；输入即可筛选，↑↓ 选择，⏎ 打开其中一个的完整页面\n\
         /help <command> — 立即显示该命令的页面，如 /help loop\n\
         \n\
         在页面上 ↑↓ 和 PgUp PgDn 滚动，⎋ 回到列表，并从那里关闭此框。最后一项按主题列出所有按键、!cmd 和 vi 模式；⌃K 把按键保留在侧边栏。",
    ),
];
