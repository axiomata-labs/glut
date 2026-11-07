Mostra os passos que mudaram arquivos e devolve os arquivos.

/steps — cada passo que mudou um arquivo; ⏎ num deles mostra o que mudou, ou devolve os arquivos ao que eram antes dele

Devolver os arquivos leva junto as suas edições desde então; a conversa fica. O Glut guarda um snapshot antes de cada passo que muda algo; o que os arquivos eram logo antes também é guardado, então `glut undo` o traz de volta. ⎋⎋ ou /rewind volta a conversa em vez disso.
