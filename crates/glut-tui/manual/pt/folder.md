Dá a esta sessão pastas ao lado da de trabalho, para que o Glut trabalhe entre projetos.

/folder — as pastas ao lado da de trabalho
/folder add <path> — adiciona uma para esta sessão
/folder remove <path> — tira uma

As ferramentas de arquivos alcançam essa pasta pelo caminho completo, os comandos podem escrever nela dentro do sandbox, o Gate a julga como a pasta de trabalho e o undo devolve os arquivos dela também. Só você adiciona uma pasta, nunca um modelo, uma Skill ou um projeto. O Glut recusa a pasta pessoal, uma pasta que contém o projeto ou fica dentro dele, uma com segredos e duas pastas uma dentro da outra.

As pastas valem só para esta sessão; `glut run --folder <path>` dá uma a uma execução sem interface.
