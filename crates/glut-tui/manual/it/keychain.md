Tenere il portachiavi fuori dalla portata dei comandi che Glut esegue.

/keychain — se è aperto, e i programmi che possono chiederlo
/keychain add <program> — mette un programma nella lista (gh, oppure git e un sottocomando: git push)
/keychain remove <program> — lo toglie dalla lista
/keychain open — lo apre a ogni comando, fino alla fine di questa sessione
/keychain close — lo richiude

Una chiave o un accesso nel portachiavi si raggiunge oltre ogni regola sui file della sandbox, perciò un comando che Glut esegue trova il portachiavi chiuso. Un comando di un programma in lista chiede prima — in ogni modalità, anche in `auto` — e gira con il portachiavi aperto appena lo permetti; un «sempre» ne fa una tua regola. Quando non c'è nessuno (un piano, `glut run`) solo una tale regola lo apre. Quando un comando fallisce sul portachiavi chiuso, Glut lo dice sotto il suo output.

Permettere un comando dà il portachiavi anche a tutto ciò che avvia: un hook di git, un'estensione di gh o uno script di build può allora leggerne qualsiasi chiave.

Solo le tue regole danno il portachiavi: una regola di permesso nel `.glut/` di un progetto lascia eseguire un comando, mai con il portachiavi aperto. Una catena lo ottiene solo se ogni parte è nella lista o legge soltanto. Ciò che `glut keychain open` tiene aperto vale solo qui, dove sei tu — mai in `glut run` né nell'esecuzione di un host.

Una tua regola lo apre senza chiedere solo se nomina un programma e il suo sottocomando (`gh pr *`, `git push *`) — mai un programma intero (`gh *`). Una scheda 🔑 mostra tutto il comando, ↑↓ per scorrerlo, e consente solo dopo che l'hai letto fino in fondo.

La lista resta in `~/.glut/keychain.json`; `glut keychain open` tiene il portachiavi aperto da una sessione all'altra, `glut keychain close` lo chiude per sempre.
