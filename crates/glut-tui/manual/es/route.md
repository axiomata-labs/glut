Qué hosts sirven el modelo en OpenRouter (otros sirven cada modelo desde un solo host).

/route — el orden que Glut envía ahora, cada host con su motivo, precio y velocidad
/route prefer <hosts> — estos primero, en este orden (xiaomi deepinfra)
/route avoid <hosts> — estos nunca
/route quant <levels> — solo estas cuantizaciones (fp8 bf16)
/route strict on|off — solo los hosts preferidos, ningún otro de respaldo
/route off — las fijaciones de este modelo se van
/route rule on|off — la regla para todos los modelos
/route rule margin|uptime|speed|wait <n> — sus números

OpenRouter sirve un modelo desde muchos hosts, con cuantizaciones, precios y velocidades distintos. Glut lee los hosts de un modelo una vez al día y los ordena según su regla: precisión completa (bf16, fp16) antes que fp8 salvo que cueste más de un 40 % más, un host que no indica cuantización entre fp8 y fp4 — el del fabricante del modelo como completo —, fp4 solo si nadie más lo sirve; un host con menos de 95 % de disponibilidad, menos de 30 tokens por segundo o más de 10 segundos hasta el primer token va al final. El orden va con cada petición con respaldo permitido, así que ninguna petición falla por la regla. Las fijaciones de un modelo van antes que la regla. Todo se guarda en `~/.glut/providers.json`.
