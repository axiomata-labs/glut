Evitar que os dados do Glut em ~/.glut cresçam sem fim.

/clean — o que o Glut guarda, item por item, e o que pode sair segundo os seus prazos
/clean now — limpa já, em segundo plano
/clean keep — os prazos
/clean keep <kind> <days>d — define um: snapshots, journals, sessions, results ou lean

O que sai: os snapshots de uma execução mais antigos que o seu prazo (7 dias) — o depósito inteiro de uma pasta quando não resta nenhum, exista a pasta ou não; o diário de uma execução inacabada (7 dias); uma sessão não escrita há 90 dias, com o seu registo de eventos e os seus resultados longos; os resultados longos sem sessão após 30 dias; as imagens que nenhuma sessão guardada nomeia; as linhas de lean.jsonl com mais de 90 dias. As exportações são só contadas: são ficheiros seus.

Uma vez por dia o Glut limpa sozinho ao arrancar — aqui e em `glut run` — e diz o que liberou. Saídas de build e caches (uma pasta com um CACHEDIR.TAG, sob node_modules/ ou o target/ de um projeto Cargo) não têm snapshots.

Os prazos ficam em `~/.glut/clean.json`. Na linha de comandos: `glut clean`, `glut clean --dry-run`, `glut clean keep sessions 30d`; `glut clean --dry-run --json` escreve o relatório para um programa.
