Faz este pane assumir um papel: o prompt e as ferramentas de um agente nomeado, nunca o modelo dele.

/role — os papéis para escolher
/role <name> — assume esse papel daqui em diante
/role none — volta a ser um faz-tudo, com todas as ferramentas

O Glut traz reviewer, tester e docs; seus próprios papéis ficam em `~/.glut/agents`, os de um projeto em `.glut/agents` dele (lidos só numa pasta confiável). As ferramentas de um papel valem de imediato, o prompt dele a partir do próximo turno. Um papel que restringe as ferramentas precisa do Gate.
