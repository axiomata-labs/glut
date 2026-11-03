切换到另一个模型提供商。

/provider — 你已设置的提供商
/provider <id> — 切换到该提供商，使用其默认模型
/provider key [id] — 只更改该提供商的密钥（不带 id 时是默认提供商）

提供商保存在 `~/.glut/providers.json` 中，其密钥保存在钥匙串中（或 `~/.glut/credentials.json`）；`glut providers add <id>` 添加一个。对话继续，下次启动从你选择的提供商开始。

密钥以隐藏方式输入，并在替换旧密钥之前用该提供商的模型列表检验；若失败，由你决定是否仍然保留，否则旧密钥保持不变。会话会立即使用保留下来的密钥。使用账户登录、不需要密钥或从环境变量读取密钥的提供商会告知你。在 shell 中：`glut providers key <id>`。
