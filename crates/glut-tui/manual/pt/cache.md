Escolhe como uma sessão em repouso mantém o cache de prompt do provedor, para que o próximo turno comece aquecido.

/cache ping — o Glut lê o cache de novo a cada poucos minutos enquanto a sessão descansa, por até 30 minutos
/cache 1h — a Anthropic o mantém por uma hora (cada escrita custa o dobro); com outros provedores a leitura o mantém
/cache off — não mantido: uma pausa além dos poucos minutos do provedor começa fria

/cache sozinho mostra a escolha atual; ping é o padrão. Guardado em `~/.glut/prefs.json`.
