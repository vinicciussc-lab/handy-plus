# Decisões técnicas do Handy Plus

Decisões tomadas durante a implementação do corretor de texto, quando o plano não cobria um caso ou precisou ser ajustado. Cada uma diz o que foi decidido, por quê e o que custaria se estivesse errada.

Contexto: [design-da-correcao.md](design-da-correcao.md) e [plano-de-implementacao.md](plano-de-implementacao.md).

## Estrutura e build

1. **O vigia da placa recebe as dependências como parâmetros.** `spawn_watcher` recebe o motor, a flag de sessão ativa e o tempo de descarregar, em vez de ler o estado da sessão, que ainda não existia naquela etapa. _Se errado:_ uma mudança pequena de assinatura.
2. **A parte Rust do painel foi para a etapa da sessão.** A sessão precisava mostrar o painel para compilar. A etapa do painel ficou só com a interface. _Se errado:_ só muda a divisão das etapas.
3. **O clippy vale para o código mexido em cada etapa.** Os avisos que já existiam no Handy foram anotados, não corrigidos, para facilitar trazer atualizações do Handy original. _Se errado:_ um commit de limpeza depois.
4. **Não rodar o Handy sem modificações para testar.** Ele usaria as configurações reais do Handy do usuário. O primeiro teste aconteceu depois da troca de identidade do app. _Se errado:_ nenhum custo.
5. **O motor de fala é compilado fora do cargo** (`scripts\build-transcribe.cmd` + `TRANSCRIBE_DIR`). O CMake disparado pelo cargo falha nesta máquina. _Se errado:_ refazer o script ao atualizar o `transcribe-cpp-sys`.
6. **O `session.rs` grande foi aceito.** Dividir no meio do plano traria retrabalho. _Se errado:_ um commit de refatoração depois.

## Modelo e velocidade

6. **Pedido de aquecimento depois de carregar o modelo.** A primeira resposta levava 45 s. Um pedido mínimo logo após carregar, ainda na barra "Carregando modelo", deixa a primeira correção do usuário rápida. _Se errado:_ um pedido curto a mais por carregamento.
7. **Ordem dos sinais da sessão.** A flag de sessão ativa é ligada _antes_ de carregar o modelo, e o "último uso" é registrado _antes_ de desligar a flag. Assim o vigia da placa nunca descarrega o modelo no meio de uma correção. _Se errado:_ um recarregamento ocasional.
8. **Pré-carregar ao digitar sem brigar com jogos.** Depois que um jogo ou a falta de memória de vídeo descarrega o modelo, nenhum pré-carregamento acontece por 10 minutos. O pré-carregamento exige folga extra de 1 GiB para o servidor, e só descarrega um servidor que ele mesmo iniciou. Isso evita um ciclo de carregar e descarregar durante o jogo.

## Captura e colagem

8. **Devolver o foco à janela confirma o resultado.** Restaura janelas minimizadas e tenta de novo se preciso. Colar na janela errada é pior que mudar a assinatura planejada.
9. **Erro "Solte as teclas e tente de novo".** Se os modificadores continuam apertados depois de 600 ms, a captura não manda Ctrl + C. Ctrl + Shift + C abriria as ferramentas de desenvolvedor do navegador.
10. **Se a janela não voltar, a correção vai para a área de transferência**, com o aviso para colar com Ctrl + V. Nunca colar na janela errada, nunca perder a correção.
11. **A correção espera o ditado inteiro terminar** (gravando, transcrevendo, pós-processando, colando), e a captura roda sob a mesma trava da colagem do ditado. Garante que a área de transferência é sempre restaurada.
12. **Regras do botão flutuante em documentos** foram relaxadas fora do navegador. O botão flutuante foi removido depois, a pedido do usuário.
13. **O gancho global do mouse só fica ativo com o painel aberto.** Um gancho de mouse permanente atrasaria todo movimento do mouse, ruim para quem joga. O de teclado fica sempre ligado por causa da dica de atalho.

## Painel

12. **O estilo de sobreposição "nenhum" do ditado também escondia o painel.** Aceito por ora, porque o usuário usa o estilo "ao vivo". Pendente: o painel da correção deveria ignorar essa opção.
13. **Uma suspeita de "texto antigo piscando" foi descartada com evidência.** Toda saída da sessão esconde o painel, e limpar o painel ao abrir apagaria a primeira atualização da sessão nova.

## Estilos e ajustes

15. **"Encurtar" foi reescrito** para encurtar de verdade, com teste exigindo saída pelo menos 15% menor.
16. **"Desenvolver" foi reforçado**, com teste exigindo pelo menos 150% do tamanho de "Meu estilo".
17. **"Descolado" foi reforçado** porque saía quase igual a "Meu estilo". Ganhou um teste de que os estilos diferem entre si.
18. **"Descolado" aceito como está.** As regras gerais já proíbem o travessão. O modelo de 4B mantém cerca de 90% das palavras de qualquer jeito, e o usuário avalia no teste.
