Which hosts serve the model on OpenRouter (others serve each model from one host).

/route — the order Glut sends now, each host with why, its price and speed
/route prefer <hosts> — these first, in this order (xiaomi deepinfra)
/route avoid <hosts> — these never
/route quant <levels> — only these quantizations (fp8 bf16)
/route strict on|off — only the preferred hosts, no other as a fallback
/route off — this model's pins go
/route rule on|off — the rule for every model
/route rule margin|uptime|speed|wait <n> — its numbers

OpenRouter serves one model from many hosts, at different quantizations, prices and speeds. Glut reads a model's hosts once a day and ranks them by its rule: full precision (bf16, fp16) before fp8 unless it costs more than 40 % more, a host that names no quantization between fp8 and fp4 — the model's maker's own as full —, fp4 only where nothing else serves; a host under 95 % uptime, under 30 tokens a second or over 10 seconds to the first token goes to the end. The order goes with every request with fallbacks allowed, so a request never fails for the rule. A model's pins come before the rule. Everything is kept in `~/.glut/providers.json`.
