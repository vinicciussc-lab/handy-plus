# Como compilar o Handy Plus

Guia para compilar o Handy Plus no **Windows 10/11 (64 bits)**, a partir do código.

O Handy original também compila no macOS e no Linux, e o código dessas plataformas continua no projeto. Mas o corretor de texto foi feito e testado só no Windows. Para outras plataformas, veja o [BUILD.md do Handy](https://github.com/cjpais/Handy/blob/main/BUILD.md).

---

## 1. Programas necessários

| Programa                                                                   | Para quê                     | Como instalar                                           |
| -------------------------------------------------------------------------- | ---------------------------- | ------------------------------------------------------- |
| [Rust](https://rustup.rs/) (estável)                                       | Núcleo do app                | `winget install Rustlang.Rustup`                        |
| [Bun](https://bun.sh/)                                                     | Interface e scripts          | `winget install Oven-sh.Bun`                            |
| Visual Studio 2022 Build Tools, com "Desenvolvimento para desktop com C++" | Compilador C/C++             | `winget install Microsoft.VisualStudio.2022.BuildTools` |
| [CMake](https://cmake.org/download/)                                       | Build do motor de fala       | `winget install Kitware.CMake`                          |
| [Vulkan SDK](https://vulkan.lunarg.com/sdk/home)                           | Aceleração na placa de vídeo | `winget install KhronosGroup.VulkanSDK`                 |

Depois de instalar, abra um terminal novo para as variáveis de ambiente (como `VULKAN_SDK`) valerem.

---

## 2. Preparar o projeto (uma vez)

Todos os comandos rodam na pasta do projeto, num **Prompt de Comando** (cmd).

```bat
:: Dependências da interface
bun install

:: Baixa as dependências do Rust (necessário para o passo seguinte)
cd src-tauri && cargo fetch && cd ..

:: Compila o motor de fala (transcribe.cpp) em .native\transcribe
scripts\dev-env.cmd scripts\build-transcribe.cmd

:: Baixa o llama-server (llama.cpp, versão Vulkan) em src-tauri\resources\llama-server
powershell -ExecutionPolicy Bypass -File scripts\fetch-llama-server.ps1

:: Modelo de detecção de voz usado pelo ditado
mkdir src-tauri\resources\models
curl -L -o src-tauri\resources\models\silero-vad-v6.2-F32.gguf https://huggingface.co/handy-computer/silero-vad-v6.2-gguf/resolve/main/silero-vad-v6.2-F32.gguf
```

### Para que serve o `scripts\dev-env.cmd`

Ele prepara o terminal antes de rodar o comando que vem depois dele:

- carrega o compilador do Visual Studio (64 bits);
- usa o Ninja no CMake (o gerador padrão do Visual Studio falha nesta máquina);
- coloca o Rust e o Bun no `PATH`;
- aponta o build para o motor de fala já compilado em `.native\transcribe`.

Por isso todo comando de build usa o formato `scripts\dev-env.cmd <comando>`.

---

## 3. Rodar em modo de desenvolvimento

```bat
scripts\dev-env.cmd bun run tauri dev
```

A primeira vez demora (10 a 15 minutos). As seguintes são bem mais rápidas.

> O modo de desenvolvimento também regenera `src/bindings.ts`, a ponte de tipos entre o Rust e a interface. Rode-o sempre que mudar um comando ou evento no Rust.

---

## 4. Gerar o instalador

```bat
scripts\dev-env.cmd bun run tauri build --bundles nsis
```

Arquivos gerados:

| Arquivo                 | Onde fica                                                                           |
| ----------------------- | ----------------------------------------------------------------------------------- |
| Instalador              | `src-tauri\target\release\bundle\nsis\Handy Plus_<versão>_x64-setup.exe`            |
| Programa (sem instalar) | `src-tauri\target\release\Handy Plus.exe`, que depende da pasta `resources` ao lado |

Feche o Handy Plus antes de compilar, inclusive pelo ícone da bandeja (botão direito → Sair). Se ele estiver aberto a partir da pasta `target\release`, o build falha com "o arquivo já está sendo usado por outro processo".

---

## 5. Testes e verificações

```bat
:: Testes do corretor (Rust)
scripts\dev-env.cmd cargo test --manifest-path src-tauri\Cargo.toml --lib correction::

:: Testes que usam o Gemma de verdade (precisam do modelo e da placa de vídeo)
scripts\dev-env.cmd cargo test --manifest-path src-tauri\Cargo.toml --lib correction:: -- --ignored

:: Lint da interface, traduções e build da interface
bun run lint
bun run check:translations
bun run build

:: Formatação do Rust
scripts\dev-env.cmd cargo fmt --manifest-path src-tauri\Cargo.toml
```

> Evite `bun run format` neste checkout: o Prettier troca as quebras de linha (CRLF para LF) de centenas de arquivos sem mudar o conteúdo. Formate só os arquivos que você mexeu, por exemplo `bun x prettier --write src/overlay/correction`.

---

## 6. Problemas comuns

### Erro no CMake do `transcribe-cpp-sys` (MSB1009 ou "ninja: loading build.ninja")

O build automático do motor de fala pelo cargo falha nesta máquina. Compile-o à parte com `scripts\dev-env.cmd scripts\build-transcribe.cmd` (passo 2) e sempre rode os builds pelo `dev-env.cmd`, que aponta para ele.

Se o `transcribe-cpp-sys` mudar de versão, atualize o caminho `SRC` em `scripts\build-transcribe.cmd` e rode o script de novo.

### Erros de caminho muito longo (`MSB3491`, `FTK1011`, `MSB6003`)

É o limite de 260 caracteres do Windows. Use uma pasta de build curta:

```bat
set CARGO_TARGET_DIR=C:\h
```

Os arquivos passam a sair em `C:\h\release\` em vez de `src-tauri\target\`.

### `bun` não encontrado

Rode os comandos pelo `scripts\dev-env.cmd`, que procura o Bun nas pastas de instalação do winget e do instalador oficial.

### Antivírus apaga o programa depois do build

Veja [Antivírus acusando vírus](README.md#antivírus-acusando-vírus) no README.
