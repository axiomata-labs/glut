Se Glut cerca una volta al giorno una versione più nuova e la mostra nel piè di pagina.

/update on — una volta al giorno, in background, Glut legge il numero dell'ultima versione da `https://glut-ai.com/latest.txt`; se è più nuova di questa, il piè di pagina la mostra con discrezione (`↑ 0.2.0`)
/update off — Glut non chiede nulla e il piè di pagina non mostra nulla

/update da solo dice se c'è un Glut più nuovo e il comando che aggiorna questo, secondo come è stato installato (`brew upgrade glut`, `cargo install glut-tui` o lo script di installazione). Glut non si aggiorna mai da solo e non chiede mai durante l'esecuzione di un piano, un giro di un ciclo, `glut run`, `glut acp` o l'arena. Conservato in `~/.glut/prefs.json`; `"updateCheck": false` in `~/.glut/settings.json` lo spegne dove /update non ha detto altro.
