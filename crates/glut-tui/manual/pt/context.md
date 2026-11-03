Mostra o quanto a janela do modelo está cheia, ou diz ao Glut o tamanho dela.

/context — os tokens usados, a janela e de onde vem o tamanho dela, e o que os guias (`AGENTS.md`, `CLAUDE.md`) custam a cada turno
/context <tokens> — define a janela deste modelo: 65536, 64k ou 1M

Uma janela que você define é guardada em `~/.glut/providers.json` para este modelo e vale antes da lista do provedor. O Glut compacta sozinho pouco antes de a janela encher.
