Inicia uma Skill: instruções para um tipo de tarefa, que o modelo carrega e segue.

/skill — as Skills para escolher
/skill <name> [para quê] — executa essa Skill sobre o que vem depois do nome dela

O Glut traz make-agent, make-skill e make-hook para você escrever as suas. As suas ficam em `~/.glut/skills`, as de um projeto em `.glut/skills` dele (lidas só numa pasta confiável); uma de mesmo nome esconde a do Glut.

/skill make-hook bloqueie git push na main
