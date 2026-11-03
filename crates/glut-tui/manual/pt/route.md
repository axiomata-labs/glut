Que hosts servem o modelo no OpenRouter (os outros servem cada modelo a partir de um só host).

/route — a ordem que o Glut envia agora, cada host com o motivo, o preço e a velocidade
/route prefer <hosts> — estes primeiro, por esta ordem (xiaomi deepinfra)
/route avoid <hosts> — estes nunca
/route quant <levels> — só estas quantizações (fp8 bf16)
/route strict on|off — só os hosts preferidos, nenhum outro como recurso
/route off — as fixações deste modelo vão-se
/route rule on|off — a regra para todos os modelos
/route rule margin|uptime|speed|wait <n> — os seus números

O OpenRouter serve um modelo a partir de muitos hosts, com quantizações, preços e velocidades diferentes. O Glut lê os hosts de um modelo uma vez por dia e ordena-os pela sua regra: precisão total (bf16, fp16) antes de fp8 salvo se custar mais de 40 % a mais, um host que não indica a quantização entre fp8 e fp4 — o do fabricante do modelo como total —, fp4 só se mais ninguém o servir; um host abaixo de 95 % de disponibilidade, abaixo de 30 tokens por segundo ou acima de 10 segundos até ao primeiro token vai para o fim. A ordem vai com cada pedido, com recurso permitido, para que nenhum pedido falhe por causa da regra. As fixações de um modelo vêm antes da regra. Tudo fica em `~/.glut/providers.json`.
