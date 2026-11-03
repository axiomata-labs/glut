Troca para outro provedor de modelos.

/provider — os provedores que você configurou
/provider <id> — troca para esse, no modelo padrão dele
/provider key [id] — muda só a chave desse provedor (sem id, a do provedor padrão)

Os provedores ficam em `~/.glut/providers.json`, as chaves deles no Keychain (ou em `~/.glut/credentials.json`); `glut providers add <id>` adiciona um. A conversa continua, e a próxima inicialização começa com o provedor que você escolheu.

A chave é digitada oculta e testada na lista de modelos do provedor antes de substituir a antiga; se falhar, você escolhe se a mantém mesmo assim, senão a antiga fica. A sessão usa na hora uma chave guardada. Um provedor que entra com uma conta, não precisa de chave ou a lê de uma variável de ambiente avisa isso. No shell: `glut providers key <id>`.
