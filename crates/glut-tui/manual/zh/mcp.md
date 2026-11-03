查看 MCP 服务器及各自的状态，登录托管的服务器，添加或移除服务器。

/mcp — 每个服务器：已连接及其工具、等待登录，或失败
/mcp login <name> — 登录托管的服务器；按 ⏎ 在浏览器中打开其地址
/mcp login <name> <address> — 通过 SSH：把浏览器最终停留的地址粘贴回来
/mcp logout <name> — 退出登录并忘记其 Token
/mcp add <name> <url> — 把托管的服务器添加到 `~/.glut/mcp.json` 并连接它
/mcp add <name> -- <command> [args] — 添加由 Glut 自己启动的服务器
/mcp remove <name> — 把它从 `~/.glut/mcp.json` 和会话中移除

如果服务器提供的登录方式 Glut 无法使用，则会要求输入 Token（隐藏输入）。项目自带的服务器由项目自己更改：/mcp remove 只移除你的。`glut mcp` 在会话之外执行同样的操作。

服务器在其工具运行期间可能会询问你：一个表单（⇥ ↑↓ 切换字段，空格勾选，⏎ 发送，Esc 关闭，⌃D 拒绝）或一个要打开的页面（o）。只有你来回答，模型永远不会；索要密码或密钥的表单会被拒绝，无人在场时所有询问都会被拒绝。
