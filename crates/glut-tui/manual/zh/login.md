用订阅代替密钥登录 — 目前是你的 ChatGPT Plus 或 Pro 套餐。

/login — 订阅列表及各自状态；⏎ 登录或退出
/login <provider> — 立即登录该订阅，例如 /login chatgpt
/login <provider> <address> — 通过 SSH：把浏览器最终停留的地址粘贴回来
/login openrouter — OpenRouter 给 Glut 一个专用密钥，像手动输入的一样保存；可在 openrouter.ai/settings/keys 限额或删除

先显示地址；⏎ 在你的浏览器中打开它。Glut 把登录保存在钥匙串（或 `~/.glut/credentials.json`）中并自动续期；退出登录会在 OpenAI 撤销它。

使用套餐时每次调用不花钱，但适用套餐的使用上限：达到上限时本轮结束，Glut 会指向 ChatGPT 设置 → 用量，你也可以在那里限制每个应用的用量。在 shell 中：glut providers login|logout <provider>。
