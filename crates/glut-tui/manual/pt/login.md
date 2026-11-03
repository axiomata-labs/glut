Entrar com uma assinatura em vez de uma chave — hoje o seu plano ChatGPT Plus ou Pro.

/login — as assinaturas, cada uma com seu estado; ⏎ entra ou sai
/login <provider> — entra nela na hora, como /login chatgpt
/login <provider> <address> — via SSH: o endereço em que o navegador terminou, colado de volta
/login openrouter — o OpenRouter dá ao Glut uma chave própria, guardada como uma digitada; limite-a ou apague-a em openrouter.ai/settings/keys

O endereço é mostrado primeiro; ⏎ o abre no seu navegador. O Glut guarda o login no Keychain (ou em `~/.glut/credentials.json`) e o renova sozinho; sair o revoga na OpenAI.

No plano uma chamada não custa dinheiro, mas valem os limites de uso do plano: quando um é atingido, o turno termina e o Glut aponta para Configurações do ChatGPT → Uso, onde você também limita o que cada app pode usar. No shell: glut providers login|logout <provider>.
