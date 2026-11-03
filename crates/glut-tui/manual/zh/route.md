OpenRouter 上由哪些主机提供模型（其他提供方每个模型只由一个主机提供）。

/route — Glut 当前发送的顺序，每个主机附原因、价格和速度
/route prefer <hosts> — 优先这些，按此顺序（xiaomi deepinfra）
/route avoid <hosts> — 从不使用这些
/route quant <levels> — 只用这些量化（fp8 bf16）
/route strict on|off — 只用优先主机，不以其他主机兜底
/route off — 去掉此模型的固定设置
/route rule on|off — 所有模型的规则
/route rule margin|uptime|speed|wait <n> — 规则的数值

OpenRouter 用许多主机提供同一个模型，量化、价格和速度各不相同。Glut 每天读取一次模型的主机并按规则排序：全精度（bf16、fp16）先于 fp8，除非贵出 40% 以上；未标明量化的主机排在 fp8 与 fp4 之间（模型制作方自己的主机按全精度算）；fp4 只在没有别的主机时使用；可用率低于 95%、每秒少于 30 个 token 或首个 token 超过 10 秒的主机排到最后。顺序随每次请求发送并允许兜底，所以请求不会因规则而失败。模型的固定设置优先于规则。一切保存在 `~/.glut/providers.json`。
