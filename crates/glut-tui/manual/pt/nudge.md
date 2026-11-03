Se o Glut pede ao modelo que continue no fim de um turno, que se ate à lista de tarefas e que escreva uma quando trabalha sem nenhuma.

/nudge on — quando um turno termina com itens da lista de tarefas abertos, ou com palavras que anunciam um passo («agora vou…», dois-pontos no fim), o Glut envia ao modelo uma linha própria: continuar ou dizer por que para; o histórico mostra isso discretamente. Se a lista não muda enquanto ele trabalha, o Glut lembra, e se nada mudar, uma vez numa mensagem própria; se numa execução não escreveu nenhuma, pede uma, uma vez por turno
/nudge off — um turno termina onde o modelo o termina: sem lembrete da lista nem pedido de uma

A pergunta do fim vem uma vez por turno, nunca no modo plan, depois de uma pergunta a você, numa rodada de um loop ou na execução de um plano; o lembrete e o pedido seguem a mesma escolha, mas na execução de um plano o Glut lembra sempre. /nudge sozinho mostra a escolha atual. Guardado em `~/.glut/prefs.json`; `"nudge": false` em `~/.glut/settings.json` desliga para cada execução.
