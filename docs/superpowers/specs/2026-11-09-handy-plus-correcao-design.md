# Handy Plus: correção de texto com IA

Data: 2026-11-09
Status: rascunho, aguardando aprovação

## 1. Intenção

O usuário escreve muito em português no Windows: WhatsApp, Discord, e-mail, comentários no YouTube e, às vezes, em jogos. Hoje, para refinar um texto, ele copia, abre o Gemini ou o Claude, cola, pede a correção e copia de volta. Isso é lento e chato. O celular Samsung dele faz isso direto no teclado, e ele quer o mesmo no PC.

Ele já usa o **Handy** (ditado por voz, open source, licença MIT) e gosta muito dele. A ideia é criar uma **versão pessoal do Handy, chamada Handy Plus**, com um recurso a mais: selecionar um texto digitado em qualquer programa, apertar um atalho e receber uma versão corrigida no estilo dele.

Antes disso, ele testou o Writing Tools (outro programa open source que faz algo parecido). O motivo de não ficar com ele: não dava pra saber quando estava processando, a interface não agradou e a customização (escolher modelo, provedor, prompts) era escondida ou limitada. O Handy Plus precisa ser o oposto: estado sempre visível e tudo configurável, com o visual do Handy.

O Handy Plus **nunca vai ser publicado**. É para uso pessoal. Com o tempo, o usuário quer ir moldando uma interface e um estilo próprios em cima dele.

## 2. Objetivos

1. Corrigir texto selecionado em qualquer programa do Windows com um atalho global.
2. O resultado soa como o usuário escrevendo, só que certinho: corrige ortografia, acentuação, concordância, vírgulas e coerência, mantendo as palavras e o jeito de falar dele.
3. O usuário sempre sabe em que etapa está (carregando modelo, gerando texto, pronto).
4. Ele confere o resultado antes de aplicar, vê o que mudou e pode trocar de tom rapidamente.
5. Modelo e provedor de IA escolhidos nas configurações, como o Handy já faz com os modelos de voz.
6. Funciona com IA local (padrão) e, opcionalmente, com IA na nuvem.

## 3. Não-objetivos

- **Mexer no ditado.** Tudo que o Handy já faz com voz (Parakeet V3, atalhos, pós-processamento, histórico) continua exatamente igual.
- Publicar, distribuir ou enviar o recurso para o Handy oficial.
- Depender do Ollama ou de qualquer outro programa externo rodando em segundo plano para a IA local.
- Salvar as correções no histórico do Handy (pode vir depois; fica fora desta versão).
- Redesenhar a interface do Handy. O visual próprio é um projeto futuro e separado.

## 4. Antiobjetivos (o que tornaria isso um fracasso mesmo funcionando)

- **Texto com cara de IA.** Usar travessão (— ou –) no lugar de vírgula, ou trocar palavras simples por palavras rebuscadas ou formais demais. O usuário acha isso artificial.
- **Travar a tela ou o jogo** por causa da IA ocupando a placa de vídeo.
- **Ficar no escuro:** apertar o atalho e não saber se algo está acontecendo.
- Perder o que estava na área de transferência do usuário.
- Ser lento a ponto de valer mais a pena abrir o Gemini.
- Quebrar ou piorar o ditado.

## 5. Como o usuário usa

Interface no estilo do Handy, usando a mesma "pílula" que aparece embaixo da tela durante o ditado. Fluxo aprovado pelo usuário a partir de mockups:

1. **Seleciona o texto e aperta o atalho de correção.** A correção começa na hora com o tom padrão, **"Meu estilo"**, sem perguntar nada.
2. **Aparece um painel acima da pílula com duas barras de progresso:**
   - **"Carregando modelo"**: só aparece quando o modelo local não está carregado na placa. Se já estiver, a etapa é pulada.
   - **"Gerando texto"**: enche conforme a correção é gerada. Como a correção costuma ter quase o mesmo tamanho do original, o progresso é estimado pelo tamanho do texto selecionado. O texto corrigido vai aparecendo aos poucos dentro do painel.
3. **Prévia:** quando termina, o painel mostra o texto corrigido com as mudanças destacadas e, ao lado, a **lista de tons numerada** (por exemplo: 1 Meu estilo, 2 Mais formal, 3 Mais curto).
   - **Enter** aplica.
   - **Número** refaz a correção a partir do texto original, no tom escolhido.
   - **Esc** cancela e não muda nada. Esc também interrompe uma geração em andamento.
4. **Aplicado:** o texto selecionado é substituído no programa onde o usuário estava, e a pílula mostra "Corrigido" com o lembrete de que **Ctrl + Z** desfaz (o desfazer é o do próprio programa onde o texto foi colado).

Os mockups aprovados estão em `.superpowers/brainstorm/618-1794248153/content/correcao-fluxo-v2.html` (pasta ignorada pelo git).

## 6. Tons (prompts)

O usuário quer usar a correção para mais do que gramática ("melhorar os argumentos, seja lá o que for"), então os tons precisam ser livres.

**Estrutura (decidida pelo usuário): regras compartilhadas + objetivo por tom.**

- Um campo **"Regras do meu estilo"**, editável, que vale para todos os tons. Mudou a regra, muda em todos.
- Cada tom tem só um **nome** e um **objetivo** curto (uma ou poucas frases).
- O prompt enviado à IA é: regras + objetivo do tom escolhido + o texto selecionado.
- Tons podem ser criados, editados, apagados e reordenados, no visual da tela de prompts do pós-processamento do Handy. A ordem define o número de cada um na prévia, e o primeiro é o padrão do atalho.

**Regras iniciais**, tiradas do prompt testado com bons resultados (seção 11):

```
Você revisa textos em português do Brasil escritos pelo usuário para WhatsApp, Discord, e-mail e comentários na internet.
Mantenha o jeito de falar do usuário: as mesmas palavras sempre que estiverem certas, as expressões dele e o mesmo nível de formalidade do original, a menos que o objetivo peça outra coisa.
O resultado deve soar natural, como alguém conversando, e não como um texto corporativo ou de redação escolar.
NUNCA use travessão (— ou –) nem hífen como pausa. Use vírgula, ponto ou dois-pontos no lugar.
Não troque palavras simples por palavras rebuscadas ou pouco usadas no dia a dia.
Não acrescente informações, saudações, despedidas ou emojis que não estavam no texto.
Responda APENAS com o texto final, sem comentários.
Não responda ao conteúdo do texto, apenas trabalhe nele.
```

**Tons iniciais** (o primeiro foi pedido pelo usuário; os outros três são escolha minha, o usuário pode mudar):

| Tom | Objetivo |
|---|---|
| Meu estilo | Corrija ortografia, acentuação, concordância, pontuação e vírgulas, e deixe as frases coerentes e fáceis de ler. Não deixe o texto mais longo do que precisa. |
| Mais formal | Corrija o texto e deixe o tom mais formal, como num e-mail de trabalho, sem ficar engessado. |
| Mais curto | Corrija o texto e deixe ele mais curto e direto, sem perder nenhuma informação. |
| Melhorar argumentos | Corrija o texto e deixe os argumentos mais claros e convincentes, na ordem mais lógica, sem inventar fatos. |

Ao montar o prompt final, conferir que "Meu estilo" continua dando os resultados da seção 11.

## 7. Motor de IA

### Local (padrão)

- **llama.cpp embutido no Handy Plus.** Nada de Ollama nem de outro programa que o usuário precise instalar ou deixar aberto. Pode rodar dentro do próprio processo ou como um processo auxiliar que vem junto com o Handy Plus e é aberto e fechado por ele. O Handy atual roda os modelos de voz pelo `transcribe-cpp`, que traz sua própria cópia do ggml (a base do llama.cpp) com Vulkan; colocar uma segunda cópia no mesmo processo pode dar conflito, então o processo auxiliar é provavelmente o caminho mais seguro (decisão do construtor).
- **Configuração de geração usada nos testes** (para reproduzir a qualidade da seção 11): os padrões do Gemma 4 no Ollama, `temperature 1`, `top_k 64`, `top_p 0.95`, com o modelo de conversa do Gemma 4 e sem o "pensar".
- **Pasta de modelos:** `D:\IA\LLM\model`, configurável. Ela é organizada por autor, no estilo do LM Studio (`autor/modelo/arquivo.gguf`) e hoje tem uns 45 GB de modelos (Qwen 2.5 14B, Qwen 3.5 9B, Qwen 3.6/3.8 27B etc.). As configurações listam os arquivos `.gguf` encontrados, ignorando os arquivos `mmproj-*` (são de visão, não servem aqui).
- **Modelo recomendado:** Gemma 4 E4B (cerca de 7,5 bilhões de parâmetros, Q4_K_M, uns 5 GB na placa). Nos testes, foi o melhor em manter o jeito do usuário. Ele ainda não existe em GGUF na pasta: precisa ser baixado para `D:\IA\LLM\model`, com confirmação do usuário antes. Os modelos de 14B e 27B que ele já tem funcionam, mas são pesados demais para corrigir texto rápido.
- **"Pensar" desligado.** O Gemma 4 vem com o modo de raciocínio ligado por padrão, o que levou cada correção de menos de 1 segundo para 6 a 16 segundos sem melhorar o resultado. A correção precisa sempre rodar com esse modo desligado, para qualquer modelo que tenha ele.
- **Carrega quando usa, libera depois.** O modelo é carregado na primeira correção e descarregado depois de um tempo sem uso, aproveitando a mesma ideia da opção de descarregar modelo que o Handy já tem para voz (o usuário usa 5 minutos).
- **Proteção da placa de vídeo.** O objetivo é nunca causar travamento por memória estourada.
  - **Antes de carregar:** conferir se há memória livre suficiente na placa para o modelo, com folga. Se não houver (por exemplo, um jogo ocupando a memória), **não carregar** e avisar no painel, oferecendo o provedor reserva na nuvem (se configurado), o processador, ou cancelar.
  - **Enquanto o modelo está parado na placa (decidido pelo usuário):** ficar de olho e **descarregar na hora** se um jogo ou outro programa em tela cheia abrir, ou se a memória livre da placa ficar apertada. A próxima correção recarrega o modelo (uns 9 segundos, com a barra de progresso).
  - O tamanho da folga e como detectar tela cheia ficam com o construtor.

### Nuvem (opcional)

- Reaproveitar a lista de provedores e o jeito de guardar chaves do pós-processamento do Handy (Anthropic, OpenAI, OpenRouter, Groq, personalizado etc.). O Handy do usuário não tem nenhuma chave salva hoje, então não há nada a importar.
- **Provedor reserva** (escolha minha): uma configuração opcional de provedor e modelo na nuvem, usada só quando o local não pode carregar por falta de memória na placa. Se não estiver configurado, o painel oferece apenas o processador ou cancelar.
- **Adicionar o Gemini** como provedor (escolha minha). O usuário usa o Gemini, e ele tem um endereço compatível com o formato da OpenAI.
- O provedor "personalizado" continua permitindo apontar para o Ollama ou o LM Studio, se o usuário quiser um dia.
- A escolha de provedor e modelo da correção é **separada** da do pós-processamento do ditado, já que o ditado continua sem IA.

## 8. Configurações

Uma seção nova "Correção de texto" nas configurações do Handy Plus, com o visual das outras seções:

- Atalho da correção. **Padrão: Ctrl + \\** (a tecla ao lado do Z no teclado ABNT2), que o usuário escolheu quando testou o Writing Tools. Escolha minha manter esse.
- Provedor: Local ou um dos provedores na nuvem.
- Se local: pasta de modelos, modelo escolhido, tempo para descarregar.
- Se nuvem: modelo e chave (reaproveitando os campos do Handy).
- Provedor reserva (opcional).
- Regras do meu estilo e lista de tons (seção 6).

## 9. Restrições e ambiente

- **Windows 11**, teclado ABNT2, interface do Handy em português (o recurso novo precisa ter os textos em português).
- **Placa:** RTX 5060 Ti com 16 GB. O Parakeet V3 é pequeno (0,6 bilhão de parâmetros), então deve sobrar bastante espaço na placa para o modelo de correção. Não está confirmado se ele roda no processador ou na placa (o Handy atual usa Vulkan no Windows); o construtor deve medir.
- **Configurações atuais do Handy do usuário:** Parakeet V3, idioma `pt`, atalho de ditado Ctrl + Espaço, colar com Ctrl + Shift + V, pós-processamento desligado, descarregar modelo de voz após 5 min. Ficam em `%APPDATA%\com.pais.handy\settings_store.json`.
- **Handy Plus substitui o Handy no dia a dia** (escolha minha): os dois abertos ao mesmo tempo brigariam pelo atalho Ctrl + Espaço. O Handy Plus usa um identificador de app próprio (configurações separadas, o Handy original continua instalado e intacto) e, na primeira vez que abrir, importa as configurações do Handy original para o ditado continuar igual. Desligar a inicialização automática do Handy original fica com o usuário.
- **Texto colado sem formatação** (escolha minha): a correção devolve texto simples, igual ao que o usuário já faz ao colar com Ctrl + Shift + V. Links, negrito ou menções selecionados num e-mail ou no Discord podem perder a formatação. O foco é texto de conversa, então isso é aceitável nesta versão.
- **Base:** fork pessoal de `github.com/cjpais/Handy` (Tauri v2, Rust em `src-tauri/`, frontend TypeScript em `src/`, gerenciado com Bun). Partes relevantes: `llm_client.rs`, `settings.rs` (campos `post_process_*`), `shortcut/`, `overlay.rs`, `clipboard.rs`, `paste_tx/`, `src/components/settings/PostProcessingSettings*`.
- **Licença:** MIT. Manter o aviso de copyright do Handy. O nome e o logo do Handy não são livres, mas como o projeto é pessoal e nunca vai ser publicado, o nome "Handy Plus" é aceitável.
- **Atualizações:** manter o repositório oficial como remoto `upstream` para trazer novidades do Handy de vez em quando. Concentrar o código novo em arquivos próprios, mexendo o mínimo possível nos arquivos do Handy, para facilitar essas atualizações.
- **Onde fica:** `D:\IA\Handy-Plus` (escolha minha, seguindo os outros projetos do usuário em `D:\IA\`).
- **Ferramentas que faltam no PC** para compilar (instalar com confirmação do usuário): Rust, Bun, Visual Studio Build Tools com C++, CMake e Vulkan SDK. Já instalados: Git e Node.

## 10. O que fica para o construtor

- Como copiar o texto selecionado (o Writing Tools simula Ctrl + C e lê a área de transferência, com espera de até 2 segundos) e como devolver o texto (reaproveitar o método de colar do Handy). A área de transferência do usuário precisa ser restaurada no final.
- O que fazer quando não há texto selecionado (sugestão: a pílula avisa "Nenhum texto selecionado" e some).
- Como o painel recebe as teclas (Enter, números, Esc) sem bagunçar o foco: depois de aplicar ou cancelar, o foco precisa voltar para a janela onde o usuário estava, para o texto ser colado no lugar certo.
- Comportamento em jogos em tela cheia exclusiva, onde o painel pode não aparecer por cima. O mínimo é não travar nada; um comportamento mais simples nesse caso é aceitável.
- Como mostrar as mudanças na prévia (por palavra, cores, se mostra o que foi removido), desde que fique claro o que mudou.
- Biblioteca de llama.cpp para Rust e como medir a memória livre da placa.
- Limite de tamanho de texto, se precisar.
- Qual arquivo GGUF exato do Gemma 4 E4B baixar.

## 11. Como saber que deu certo

Testes feitos nesta conversa com o Gemma 4 E4B, o prompt "Meu estilo" e o "pensar" desligado, na placa do usuário:

| Antes | Depois |
|---|---|
| oi mano blz? entao eu vi aquele video q vc mandou ontem e achei mt massa, mas acho q o cara errou umas coisa la no final sobre o processador, depois agente conversa melhor sobre isso | Oi, mano, beleza? Então, eu vi aquele vídeo que você mandou ontem e achei muito massa, mas acho que o cara errou umas coisas lá no final sobre o processador. Depois a gente conversa melhor sobre isso. |
| Bom dia, gostaria de saber se é possível remarcar a reunião de amanhã pois eu tive um imprevisto, e não vou conseguir chegar no horário combinado, se puder ser na quinta seria ótimo | Bom dia, gostaria de saber se é possível remarcar a reunião de amanhã, pois eu tive um imprevisto e não vou conseguir chegar no horário combinado. Se puder ser na quinta, seria ótimo. |
| cara esse jogo ta mt bom mas a otimização ta horrivel, minha placa fica em 100% e mesmo assim cai pra 40 fps nas cidade | Cara, esse jogo tá muito bom, mas a otimização tá horrível. Minha placa fica em 100% e mesmo assim cai pra 40 fps nas cidades. |

Tempos: cerca de 0,3 a 0,5 segundo com o modelo já carregado e uns 9 segundos na primeira vez (carregando o modelo). Para comparação, o Qwen 2.5 7B foi tão rápido, mas ficou mais formal (trocou "a gente" por "nós") e deixou "Mt" com maiúscula.

O Handy Plus está pronto quando:
- Os três textos acima saem com qualidade parecida, sem travessão, mantendo "a gente", "tá" e "mano".
- Com o modelo carregado, a prévia aparece em cerca de 1 segundo. Na primeira vez, a barra de "Carregando modelo" mostra o progresso.
- Funciona em WhatsApp, Discord, navegador e e-mail.
- Com um jogo ocupando quase toda a memória da placa, a correção não trava nada: ela avisa e oferece outra saída.
- O ditado continua funcionando exatamente como antes.
- A área de transferência do usuário volta a ter o que tinha antes.

## 12. Decisões que não vieram do usuário

Para o usuário conferir. Todas foram tomadas por mim durante a escrita e podem ser mudadas:

1. Atalho padrão Ctrl + \\.
2. Tons iniciais Mais formal, Mais curto e Melhorar argumentos, com os objetivos da seção 6.
3. Gemini adicionado como provedor na nuvem.
4. Provedor reserva opcional para quando a placa estiver cheia.
5. Handy Plus substitui o Handy no dia a dia, com configurações próprias e importação das do Handy na primeira vez.
6. Correção devolve texto sem formatação.
7. Projeto em `D:\IA\Handy-Plus`.
8. Correções não vão para o histórico nesta versão.

## 13. Arrumação depois

Quando o Handy Plus estiver funcionando, o Writing Tools e a ponte criados durante os testes podem ser removidos: `%LOCALAPPDATA%\Programs\WritingTools` (inclui a ponte em `ponte\`). O modelo `gemma4:e4b` no Ollama também pode ser apagado, se o usuário não usar o Ollama para outra coisa.

## 14. Adições depois dos testes do usuário (2026-11-10)

Aprovadas pelo usuário em conversa, depois de testar a primeira versão.

### Painel e tons (já construídos)
- Painel maior (640 × 420), setas ↑↓ para rolar, Tab alterna Corrigido / Mudanças / Original (Corrigido limpo é o padrão).
- Regras compartilhadas passam a permitir reescrever quando o objetivo pede; "Mais formal", "Mais curto" e "Melhorar argumentos" mudam o texto de forma visível. Tons salvos com o texto antigo padrão são atualizados sozinhos; tons editados pelo usuário nunca.
- Atalho sem seleção: se nada estiver selecionado, pega o texto inteiro da caixa (Ctrl+A).

### Tom "Desenvolver"
O usuário pediu uma forma de "estender mais o texto". Escolha minha (avisada ao usuário, que não corrigiu): um quinto tom, **Desenvolver**, que deixa o texto mais longo explicando melhor as ideias, com detalhes, exemplos ou justificativas, mantendo o jeito de falar e a opinião do usuário e sem inventar fatos específicos. Entra para quem já usa o app, a menos que a lista já tenha 9 tons.

### Som discreto
O usuário ouve um "tique-tique" durante a geração mesmo com o som desligado: é o ruído elétrico da placa de vídeo (coil whine), não um som do app. Ele pediu um efeito sonoro suave. Escolha minha: um som curto e suave quando a correção começa e outro quando a prévia fica pronta; sem som contínuo. Usa o volume dos sons do Handy (um pouco mais baixo), com uma opção própria de ligar e desligar na tela da correção (ligada por padrão).

### Botão flutuante
Aprovado pelo usuário com esta descrição:
- Ao clicar numa caixa de texto de um programa comum (navegador, Bloco de Notas, Office, WhatsApp, Discord), aparece um botãozinho redondo do Handy Plus no canto inferior direito da caixa, **só quando a caixa já tem texto**.
- Um clique corrige o texto inteiro da caixa com o painel de sempre (mesmo fluxo do atalho sem seleção). Clicar no botão não tira o foco da caixa.
- Some quando o foco sai da caixa e enquanto uma correção está aberta.
- Nunca aparece em jogos nem em programas em tela cheia (o mesmo detector da placa de vídeo), nem em campos de senha, nem nas janelas do próprio Handy Plus.
- Nas configurações da correção: chave para ligar e desligar (ligado por padrão) e uma lista de programas (nome do executável, por exemplo `Discord.exe`) onde ele nunca aparece.
- Limite conhecido: depende da acessibilidade do Windows (UI Automation); em alguns apps pode não aparecer ou aparecer fora do lugar. O atalho continua funcionando em qualquer lugar.

## 15. Terceira rodada (2026-11-10): sem botão flutuante, dica de atalho, tradução e painel mais fácil

Aprovado pelo usuário depois de testar o botão flutuante.

- **Botão flutuante descartado.** No teste ele só apareceu na pesquisa do Google; WhatsApp, Discord e YouTube não informam a caixa de texto pela acessibilidade do Windows. O código do botão e da detecção da caixa é removido.
- **Dica de atalho no lugar do botão.** Não depende de saber onde está a caixa: quando o usuário digita algumas palavras em qualquer programa (pelo menos ~12 teclas de texto) e para por ~1,5 s, aparece por ~4 s uma pílula discreta embaixo no centro (onde fica a pílula do Handy) com "<atalho> para melhorar o texto". Some ao voltar a digitar. Clicar nela faz a correção (mesmo caminho do atalho). No máximo uma vez por janela. Nunca em tela cheia ou jogo, durante um ditado ou com uma correção aberta, nem nas janelas do Handy Plus. Chave para desligar (ligada por padrão). O app conta teclas, nunca guarda o que foi digitado.
- **Tom "Traduzir"** (sexto tom): português → inglês natural mantendo o tom; se o texto já estiver em inglês, inglês → português. Outros idiomas: o usuário cria tons próprios.
- **Erros previstos no painel:** clicar fora do painel fecha (igual ao Esc); voltar a digitar fecha e a digitação segue normal; apertar o atalho de novo fecha; Ctrl + C copia a correção e fecha (sem colar); Enter continua aplicando e Ctrl + Z desfaz.
- **Mouse no painel:** botões Aplicar, Copiar e Cancelar; clicar num tom refaz nesse tom; clicar em Corrigido / Mudanças / Original troca a visão; rodinha rola o texto. Editar o texto à mão dentro do painel fica para depois (exigiria o painel tomar o foco do teclado).
- **Painel mais largo e com respiro:** ~760 × 460, com mais margem interna.
