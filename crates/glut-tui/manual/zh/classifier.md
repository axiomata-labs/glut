让第二个模型判断那些仅由模式询问你的命令。

/classifier — 由哪个模型判断，或者不判断
/classifier session — 由会话的模型判断（只要你未设置其他模型）
/classifier <provider> <model> — 单独的模型，快速的即可（openrouter deepseek/deepseek-v4.1-flash）
/classifier off — 不判断：每次都询问你

Glut 认识许多只读取的命令（ls、grep、git status 等），并在任何模式下运行它们。由它们组成的循环或管道（for f in *.md; do grep -c x "$f"; done）无法仅凭文本判断，所以会询问。第二个模型只看到该命令及其文件夹 — 从不看到对话、文件或输出 — 并回答 READS 或 CHANGES。只有 READS 时，命令才会像已知的读取命令一样在沙箱中运行；其他情况，或几秒内没有明确答案，则照常询问你，并在命令下方附上它的意见。你的规则、重型命令、钥匙串和沙箱保持不变；同一命令在一个会话中不会被判断两次。

它放行的命令会在旁边注明。选择保存在 `~/.glut/classifier.json`，从下一轮起生效；`glut run --classifier` 可为一次运行设置。
