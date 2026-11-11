# Handy Plus

**Ditado por voz e corretor de texto com IA, rodando no seu próprio PC, em qualquer programa do Windows.**

O Handy Plus é uma versão pessoal do [Handy](https://github.com/cjpais/Handy), o app de ditado de código aberto do CJ Pais. Ele mantém tudo o que o Handy faz (você fala e o texto aparece onde está o cursor) e acrescenta um **corretor de texto**: você seleciona um texto em qualquer programa, aperta um atalho e recebe o texto corrigido no seu estilo, sem sair de onde está.

Tudo roda localmente, na sua placa de vídeo. Nenhum texto sai do seu computador, a menos que você escolha um provedor na nuvem.

> Projeto pessoal, sem fins comerciais e sem distribuição pública.

---

## Sumário

- [O que ele faz](#o-que-ele-faz)
- [Atalhos](#atalhos)
- [Modelos recomendados](#modelos-recomendados)
- [Requisitos](#requisitos)
- [Instalação](#instalação)
- [Configuração](#configuração)
- [Antivírus acusando vírus](#antivírus-acusando-vírus)
- [Compilar a partir do código](#compilar-a-partir-do-código)
- [Estrutura do projeto](#estrutura-do-projeto)
- [Créditos](#créditos)
- [Licença](#licença)

---

## O que ele faz

### Corretor de texto (novo no Handy Plus)

1. Selecione um texto em qualquer programa (WhatsApp, e-mail, Discord, YouTube, navegador, Word...). Se não selecionar nada, ele pega a caixa de texto inteira.
2. Aperte **Ctrl + \\** (a tecla ao lado do Z).
3. Um painel aparece com o texto original e as opções. Escolha um **estilo** (ou aperte Enter para "Meu estilo").
4. Confira o resultado e aperte **Enter** para substituir o texto, ou **Ctrl + C** para copiar.

O corretor:

- conserta gramática, vírgulas e coerência, deixando o texto com cara de gente falando;
- **nunca usa travessão** nem palavras rebuscadas;
- detecta se o texto está em português ou inglês e mantém o idioma.

**Estilos** (reescrevem o texto a partir do original):

| #   | Estilo     | O que faz                                         |
| --- | ---------- | ------------------------------------------------- |
| 1   | Meu estilo | Corrige mantendo o seu jeito de escrever          |
| 2   | Formal     | Deixa o texto mais sério, para trabalho e e-mails |
| 3   | Descolado  | Tom leve, de conversa entre amigos                |
| 4   | Poético    | Mais expressivo, com imagens e ritmo              |

**Ajustes** (agem sobre o resultado do estilo, um de cada vez):

| #   | Ajuste              | O que faz                           |
| --- | ------------------- | ----------------------------------- |
| 5   | Encurtar            | Diz o mesmo com menos palavras      |
| 6   | Desenvolver         | Estende o texto com mais detalhes   |
| 7   | Melhorar argumentos | Deixa o raciocínio mais convincente |

Estilos e ajustes são editáveis nas configurações: dá para mudar o nome, as instruções, a ordem e criar novos (até 9 de cada).

**Outros recursos do painel:**

- **Chave PT ⇄ EN:** traduz o resultado para o outro idioma (clique ou tecla **T**). Virar de volta mostra a versão anterior na hora.
- **Memória da sessão:** cada combinação gerada fica guardada enquanto o painel está aberto. Voltar a ela é instantâneo.
- **Variações:** clicar de novo na opção já escolhida gera uma versão diferente.
- **Original e Corrigido:** a tecla **Tab** alterna entre os dois.
- **Fechar:** Esc, clicar fora, começar a digitar ou apertar o atalho de novo.
- **Dica de atalho:** depois que você digita um pouco e pausa, uma pílula discreta lembra o atalho (dá para desligar).
- **Sons suaves** quando começa e quando fica pronto (dá para desligar).

### Pensado para quem joga

A placa de vídeo é dividida com os jogos, então o Handy Plus cuida dela:

- **Pré-carrega o modelo enquanto você digita**, para a primeira correção sair rápida.
- **Não carrega nada com jogo em tela cheia** e descarrega o modelo se um jogo precisar da memória de vídeo.
- **Confere a memória livre da placa** antes de carregar (via NVML, nas placas NVIDIA).
- **Descarrega o modelo** depois de um tempo parado (configurável).
- Se a placa estiver ocupada, pode usar um **provedor na nuvem como reserva** (opcional).

### Ditado por voz (herdado do Handy)

Aperte o atalho, fale, solte: o texto é transcrito no seu PC e colado onde está o cursor. Funciona com vários modelos de fala (Parakeet, Whisper, Moonshine, SenseVoice e outros) e tem pós-processamento opcional com IA.

---

## Atalhos

| Atalho                    | Ação                                                           |
| ------------------------- | -------------------------------------------------------------- |
| **Ctrl + \\**             | Corrigir o texto selecionado (ou a caixa inteira)              |
| **Ctrl + Shift + \\**     | Traduzir o texto selecionado (abre com a chave PT ⇄ EN virada) |
| **Ctrl + Espaço**         | Ditado por voz                                                 |
| **Ctrl + Shift + Espaço** | Ditado com pós-processamento por IA                            |

**Com o painel de correção aberto:**

| Tecla    | Ação                                                    |
| -------- | ------------------------------------------------------- |
| 1 a 9    | Escolher estilo ou ajuste                               |
| Enter    | Primeiro estilo (antes de escolher) ou aplicar (depois) |
| T        | Trocar o idioma (PT ⇄ EN)                               |
| Tab      | Alternar entre Corrigido e Original                     |
| Ctrl + C | Copiar o resultado                                      |
| Esc      | Fechar sem mudar nada                                   |

Todos os atalhos podem ser trocados nas configurações. "\\" é a tecla à esquerda do Z no teclado ABNT2.

---

## Modelos recomendados

O Handy Plus **não vem com os modelos de IA**: você baixa os arquivos `.gguf` e aponta a pasta nas configurações.

### Para a correção

| Modelo                                 | Arquivo                      | Tamanho | Observação                                                                   |
| -------------------------------------- | ---------------------------- | ------- | ---------------------------------------------------------------------------- |
| **Gemma 4 E4B Instruct** (recomendado) | `gemma-4-E4B-it-Q4_K_M.gguf` | ~5 GB   | Bom equilíbrio entre qualidade, velocidade e memória. Testado neste projeto. |

- Download: [unsloth/gemma-4-E4B-it-GGUF](https://huggingface.co/unsloth/gemma-4-E4B-it-GGUF) no Hugging Face (escolha a versão **Q4_K_M**).
- Qualquer modelo de chat em GGUF compatível com o llama.cpp funciona. Modelos maiores corrigem melhor, mas ocupam mais memória de vídeo e demoram mais para carregar.
- O "modo de raciocínio" do Gemma fica desligado de propósito, para a resposta sair rápida.

### Para o ditado

| Modelo                                | Observação                                                |
| ------------------------------------- | --------------------------------------------------------- |
| **Parakeet V3** (recomendado)         | Rápido e preciso, com bom português.                      |
| Whisper (Small, Medium, Turbo, Large) | Alternativas; os maiores são mais precisos e mais lentos. |

Os modelos de ditado são baixados pelo próprio app, na tela de modelos.

---

## Requisitos

### Para usar

| Item            | Mínimo                                                                                                                | Recomendado                                    |
| --------------- | --------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| Sistema         | Windows 10 ou 11, 64 bits                                                                                             | Windows 11                                     |
| Placa de vídeo  | Compatível com Vulkan e com memória livre para o modelo + ~2,5 GB de folga (cerca de 8 GB no total com o Gemma 4 E4B) | NVIDIA com 12 a 16 GB (ex.: RTX 5060 Ti 16 GB) |
| Memória RAM     | 16 GB                                                                                                                 | 32 GB                                          |
| Espaço em disco | ~6 GB para o modelo de correção + ~1 GB para o de ditado                                                              | SSD                                            |
| Microfone       | Só para o ditado                                                                                                      | —                                              |

Notas:

- Sem placa de vídeo compatível, o modelo pode rodar no processador, mas a correção fica bem mais lenta.
- A checagem de memória de vídeo e o modo jogo usam a NVML, então funcionam melhor em placas NVIDIA. Em outras placas o app corrige normalmente, só sem essa checagem.
- O primeiro carregamento do modelo leva de 8 a 20 segundos, dependendo do disco. Depois disso a correção sai em poucos segundos.

### Opcional: provedor na nuvem

Em vez do modelo local (ou como reserva quando a placa está ocupada), dá para usar um provedor online com a sua chave de API: Google Gemini, OpenAI, Anthropic (Claude), OpenRouter, Groq, Cerebras, Z.AI ou qualquer API compatível com OpenAI. Nesse caso o texto é enviado para o provedor escolhido.

---

## Instalação

1. Rode o instalador `Handy Plus_<versão>_x64-setup.exe`.
2. Baixe o modelo de correção (veja [Modelos recomendados](#modelos-recomendados)) e salve numa pasta, por exemplo `D:\IA\LLM\model`.
3. Abra o Handy Plus → **Correção** → aponte a pasta dos modelos e escolha o arquivo `.gguf`.
4. Na tela de modelos, baixe o **Parakeet V3** para o ditado.
5. Selecione um texto em qualquer programa e aperte **Ctrl + \\**.

Na primeira vez, o Handy Plus importa as configurações e os modelos do Handy original, se ele estiver instalado.

---

## Configuração

Na tela **Correção** você encontra:

- **Provedor:** modelo local ou nuvem, e um provedor de reserva opcional.
- **Pasta e modelo:** onde estão os arquivos `.gguf` e qual usar.
- **Descarregar depois de:** quanto tempo parado até liberar a memória de vídeo.
- **Regras de estilo:** as instruções gerais que valem para toda correção (por exemplo, "nunca use travessão").
- **Estilos e Ajustes:** duas listas editáveis com nome e instruções de cada opção.
- **Pré-carregar ao digitar**, **Dica de atalho** e **Sons:** cada um pode ser desligado.
- **Atalhos** de correção e de tradução.

---

## Antivírus acusando vírus

Alguns antivírus, como o Kaspersky, podem marcar o Handy Plus como **"Trojan.Generic"**. É um **falso positivo**. Ele vem do comportamento do programa, não de um vírus encontrado no arquivo. Para funcionar em qualquer programa, o Handy Plus:

- escuta o teclado para os atalhos;
- aperta Ctrl + C e Ctrl + V sozinho para ler e substituir o texto;
- usa a área de transferência;
- abre um processo auxiliar (`llama-server`) para rodar a IA.

Programas maliciosos fazem coisas parecidas, e como o Handy Plus não tem assinatura digital, o antivírus desconfia. Para resolver, adicione a pasta `C:\Users\<você>\AppData\Local\Handy Plus\` às **exclusões** do antivírus e marque o `Handy Plus.exe` como **aplicativo confiável** (sem monitorar a atividade).

---

## Compilar a partir do código

O passo a passo completo está em [BUILD.md](BUILD.md). Resumo para Windows:

```bat
:: 1. Dependências do frontend
bun install

:: 2. Motor de fala (uma vez)
scripts\dev-env.cmd scripts\build-transcribe.cmd

:: 3. llama-server, que roda o modelo de correção (uma vez)
powershell -ExecutionPolicy Bypass -File scripts\fetch-llama-server.ps1

:: 4. Rodar em modo de desenvolvimento
scripts\dev-env.cmd bun run tauri dev

:: 5. Gerar o instalador
scripts\dev-env.cmd bun run tauri build --bundles nsis
```

O instalador fica em `src-tauri\target\release\bundle\nsis\`.

---

## Estrutura do projeto

```
Handy-Plus/
├── src/                          Interface (React + TypeScript + Tailwind)
│   ├── components/settings/
│   │   └── correction/           Tela de configurações da correção
│   ├── overlay/
│   │   ├── correction/           Painel de correção
│   │   └── hint/                 Pílula com a dica de atalho
│   └── i18n/locales/             Traduções (pt e en revisados)
├── src-tauri/                    Núcleo (Rust + Tauri 2)
│   ├── src/correction/           Corretor de texto
│   │   ├── session.rs            Fluxo do painel: escolha, geração, aplicar
│   │   ├── selection.rs          Captura do texto selecionado
│   │   ├── local_engine.rs       Inicia e encerra o llama-server
│   │   ├── stream.rs             Recebe a resposta da IA aos poucos
│   │   ├── text.rs               Instruções, limpeza e detecção de idioma
│   │   ├── gpu_guard.rs          Modo jogo e memória de vídeo
│   │   ├── typing_hint.rs        Dica de atalho e pré-carregamento
│   │   ├── input_hooks.rs        Teclado e mouse durante a correção
│   │   ├── settings.rs           Configurações e migrações
│   │   └── import.rs             Importa dados do Handy original
│   └── resources/                Sons, ícones e o llama-server
├── scripts/                      Scripts de build para Windows
└── docs/handy-plus/              Design, plano e decisões do projeto
```

A documentação técnica do Handy Plus está em [`docs/handy-plus/`](docs/handy-plus/):

- [design-da-correcao.md](docs/handy-plus/design-da-correcao.md): o que foi pedido e como funciona;
- [plano-de-implementacao.md](docs/handy-plus/plano-de-implementacao.md): as tarefas, rodada por rodada;
- [decisoes.md](docs/handy-plus/decisoes.md): as decisões técnicas tomadas no caminho.

---

## Créditos

O Handy Plus só existe por causa do trabalho destes projetos e pessoas:

| Projeto                                                 | Autor                                                | Uso no Handy Plus                                              | Licença                  |
| ------------------------------------------------------- | ---------------------------------------------------- | -------------------------------------------------------------- | ------------------------ |
| [**Handy**](https://github.com/cjpais/Handy)            | [CJ Pais](https://github.com/cjpais) e colaboradores | Base inteira do app: ditado, interface, atalhos, configurações | MIT                      |
| [llama.cpp](https://github.com/ggml-org/llama.cpp)      | Georgi Gerganov e colaboradores                      | Roda o modelo de correção (`llama-server`)                     | MIT                      |
| [ggml](https://github.com/ggml-org/ggml)                | Georgi Gerganov e colaboradores                      | Base de cálculo da IA no ditado e na correção                  | MIT                      |
| [transcribe.cpp](https://github.com/handy-computer)     | Equipe do Handy                                      | Motor de ditado                                                | MIT                      |
| [Gemma 4](https://ai.google.dev/gemma)                  | Google DeepMind                                      | Modelo de correção recomendado                                 | Licença própria do Gemma |
| [Unsloth](https://huggingface.co/unsloth)               | Unsloth AI                                           | Versões GGUF quantizadas do Gemma                              | Conforme o modelo        |
| [Parakeet](https://huggingface.co/nvidia)               | NVIDIA                                               | Modelo de ditado recomendado                                   | CC-BY-4.0                |
| [Whisper](https://github.com/openai/whisper)            | OpenAI                                               | Modelos de ditado alternativos                                 | MIT                      |
| [Silero VAD](https://github.com/snakers4/silero-vad)    | Silero Team                                          | Detecta quando você está falando                               | MIT                      |
| [Tauri](https://tauri.app)                              | Tauri Programme                                      | Estrutura do app (Rust + web)                                  | MIT / Apache-2.0         |
| [whatlang](https://github.com/greyblake/whatlang-rs)    | Sergey Potapov                                       | Detecção de idioma (PT ou EN)                                  | MIT                      |
| [nvml-wrapper](https://github.com/Cldfire/nvml-wrapper) | Cldfire                                              | Leitura da memória de vídeo NVIDIA                             | MIT / Apache-2.0         |
| [enigo](https://github.com/enigo-rs/enigo)              | enigo-rs                                             | Simula Ctrl + C e Ctrl + V                                     | MIT                      |

Um agradecimento especial ao **CJ Pais**, criador do Handy, por publicar um app de ditado tão bem feito como código aberto. Se você gosta do Handy, apoie o projeto original em [handy.computer](https://handy.computer).

O Handy Plus foi desenvolvido com a ajuda do [Claude Code](https://claude.com/claude-code), da Anthropic.

---

## Licença

Distribuído sob a licença **MIT**, a mesma do Handy. Veja [LICENSE](LICENSE).

O aviso de copyright original do Handy é mantido, como a licença exige. Os modelos de IA (Gemma, Parakeet, Whisper) têm licenças próprias, que valem para quem os baixa e usa.
