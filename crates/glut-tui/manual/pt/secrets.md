Mostra e define as chaves ocultas do modelo nesta sessão.

/secrets — cada valor oculto até agora, por número, tipo, tamanho e onde foi visto (nunca o valor)
/secrets allow <n> — deixa o segredo n passar nesta sessão
/secrets allow <n> always — deixa passar sempre (a impressão digital dele é guardada, nunca o valor)
/secrets add <name> <regex> — oculta um formato seu, a partir do próximo resultado
/secrets remove <name> — descarta um formato seu
/secrets on|off — liga ou desliga a ocultação

Uma chave no que uma ferramenta responde chega ao modelo como ‹secret n: …›; o Glut só devolve o valor dela a um arquivo que já a contém. Uma mensagem sua que parece conter uma chave gera uma pergunta antes de ser enviada. O modelo fica sabendo de uma mudança com a sua próxima mensagem. Seus formatos e escolhas são guardados em `~/.glut/secrets.json`.

A ocultação protege contra uma chave mostrada por acidente; o sandbox e o Gate de rede continuam sendo as barreiras.
