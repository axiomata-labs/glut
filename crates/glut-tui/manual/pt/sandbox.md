Como são contidos os comandos que o Glut executa, e se podem escrever nas pastas git.

/sandbox — a sandbox aqui (macOS: Seatbelt; Linux: bubblewrap e seccomp) e o interruptor do git
/sandbox git read-only — nenhum comando escreve numa pasta git: nenhum faz commit a partir da sandbox
/sandbox git writable — os comandos podem voltar a escrever nas pastas git, também fazer commit
/sandbox required on — nenhum comando corre onde não há sandbox (falta o bubblewrap, um contentor sem o direito); off deixa-os correr, contidos só pelas regras

A sandbox deixa um comando escrever só nas tuas pastas, ler tudo menos os teus segredos e os ficheiros .env, e chegar à rede só pelo proxy do Glut. Em Linux não consegue recusar um nome que ainda não existe: o que um comando cria e o macOS recusa — um .envrc novo, um .git, um .git/commondir — vai para ~/.glut/quarantine assim que aparece, e mais uma vez depois do comando. Uma ferramenta tua que corra precisamente nesse momento ainda o poderia encontrar; com as pastas git só de leitura, não se cria nenhum nelas.

O interruptor vale em cada sessão e janela a partir do próximo comando, e fica em ~/.glut/sandbox.json — nunca escrito por um comando. glut sandbox show e glut sandbox git read-only|writable fazem o mesmo fora do Glut.
