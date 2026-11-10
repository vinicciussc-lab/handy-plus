# Handy Plus: correção de texto — Plano de implementação

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Adicionar ao fork pessoal do Handy um atalho que corrige o texto selecionado em qualquer programa do Windows, com prévia, troca de tom e IA local (llama.cpp) ou na nuvem.

**Architecture:** Todo o código novo fica no módulo `src-tauri/src/correction/` e em `src/overlay/correction/` + `src/components/settings/correction/`, mexendo o mínimo possível nos arquivos do Handy (para facilitar `git merge upstream/main`). A IA local roda num processo auxiliar `llama-server` (do llama.cpp, build Vulkan) aberto e fechado pelo Handy Plus, porque o Handy já carrega sua própria cópia do ggml via `transcribe-cpp`. O painel da correção reaproveita a janela da pílula (`recording_overlay`), que não recebe foco; por isso Enter, números e Esc são atalhos globais registrados só enquanto a correção está ativa.

**Tech Stack:** Tauri 2 (Rust + React/TypeScript, Bun), tauri-specta (bindings), tauri-plugin-store, reqwest (streaming SSE), enigo, crate `windows` 0.61, `nvml-wrapper`, llama.cpp `llama-server`.

**Spec:** `docs/superpowers/specs/2026-11-09-handy-plus-correcao-design.md`

## Global Constraints

- Plataforma alvo: Windows 11 x64, teclado ABNT2. Código específico de Windows sob `#[cfg(target_os = "windows")]`; nas outras plataformas o recurso pode simplesmente não existir.
- O ditado (bindings `transcribe`, `transcribe_with_post_process`, `cancel`, o `TranscriptionCoordinator` e o pós-processamento) não muda de comportamento.
- Nome do app: **Handy Plus**. Identificador: `com.handyplus.app`. Nunca publicado; manter o aviso MIT do Handy.
- Pasta de modelos padrão: `D:\IA\LLM\model` (configurável). Ignorar arquivos `mmproj-*`.
- Sampling local usado nos testes do spec: `temperature 1`, `top_k 64`, `top_p 0.95`, "pensar" sempre desligado.
- Tempo para descarregar o modelo local: padrão `ModelUnloadTimeout::Min5`.
- Textos novos da interface via i18next, com chaves em `en` e `pt`; `bun run check:translations` precisa passar (as outras línguas recebem o texto em inglês).
- O resultado nunca usa travessão quando o original não usava (spec seção 4).
- A área de transferência do usuário é sempre restaurada ao fim da correção, independente da configuração `clipboard_handling`.
- Correção devolve texto simples (sem formatação).
- Commits com prefixo convencional (`feat:`, `fix:`, `chore:`, `docs:`, `test:`), terminando com a linha `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Antes de cada commit: `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings` e `bun run lint`.

## Review Focus

1. **Seleção vazia ou só com espaços** → a pílula avisa "Nenhum texto selecionado" e some; nada é colado. (Teste na Task 7.)
2. **Texto muito longo** (acima de `MAX_CORRECTION_CHARS = 8000`) → aviso "Texto grande demais", sem chamar a IA. (Teste na Task 8.)
3. **Atalho de correção apertado durante um ditado ou com outra correção aberta** → ignorado, sem quebrar nenhuma das duas. (Teste na Task 8.)
4. **`llama-server` morre ou falha ao abrir no meio da geração** → painel mostra erro, o motor é marcado como descarregado e a próxima correção tenta abrir de novo. (Teste na Task 4.)
5. **Nenhum modelo escolhido ou pasta de modelos inexistente** → painel mostra "Escolha um modelo nas configurações", sem travar. (Teste na Task 8.)

---

### Task 1: Identidade do Handy Plus e importação das configurações do Handy

**Files:**
- Modify: `src-tauri/tauri.conf.json` (`productName`, `identifier`, `bundle.createUpdaterArtifacts`, remover `bundle.windows.signCommand`)
- Modify: `src-tauri/src/main.rs` (desligar o atualizador)
- Create: `src-tauri/src/correction/mod.rs`, `src-tauri/src/correction/import.rs`
- Modify: `src-tauri/src/lib.rs` (declarar `mod correction;` e chamar a importação no início do `setup`, antes de qualquer `get_settings`)

**Interfaces:**
- Produces: `correction::import::import_from_handy(src_dir: &Path, dst_dir: &Path) -> std::io::Result<ImportOutcome>` com `enum ImportOutcome { Imported, AlreadyHasSettings, NoHandyData }`.

- [ ] **Step 1: Instalar as ferramentas de build (pedir confirmação ao usuário antes de cada instalação)**

Faltam no PC: Rust (rustup), Bun, Visual Studio Build Tools 2022 com "Desktop development with C++", CMake e Vulkan SDK (ver `BUILD.md`). Já existem Git 2.56 e Node 24. Depois: `bun install` e baixar o VAD conforme `AGENTS.md` ("Model Setup").
Verificação: `bun run tauri dev` abre o Handy original sem erros (ainda com o nome antigo).

- [ ] **Step 2: Write the failing tests** em `src-tauri/src/correction/import.rs`

```rust
#[test] fn imports_settings_and_models_when_destination_is_empty()
// src tem settings_store.json e models/parakeet/x.bin; dst vazio
// => Imported; dst/settings_store.json igual ao original; dst/models/parakeet/x.bin existe
#[test] fn keeps_existing_destination_settings()
// dst já tem settings_store.json => AlreadyHasSettings; arquivo do dst não muda
#[test] fn does_nothing_without_handy_data()
// src não existe => NoHandyData; dst continua vazio
```
Usar `tempfile::tempdir()` (já é dev-dependency).

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::import`
Expected: FAIL (função não existe)

- [ ] **Step 4: Implement `import_from_handy`**

Copia `settings_store.json` e a pasta `models/` inteira (recursivo). Não copia histórico, logs nem `webview/`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test correction::import`
Expected: PASS

- [ ] **Step 6: Ligar a identidade nova**

- `tauri.conf.json`: `"productName": "Handy Plus"`, `"identifier": "com.handyplus.app"`, `"createUpdaterArtifacts": false`, remover `signCommand`.
- `main.rs`: antes de tudo, `std::env::set_var("HANDY_DISABLE_UPDATER", "1")` (o atualizador baixaria o Handy oficial por cima). 
- `lib.rs` `setup`: `import_from_handy(%APPDATA%\com.pais.handy, app_data_dir)` usando `crate::portable::app_data_dir`; logar o resultado. Pular no modo portátil.

- [ ] **Step 7: Verificar no app**

Run: `bun run tauri dev`
Expected: janela com nome "Handy Plus"; `%APPDATA%\com.handyplus.app\settings_store.json` existe; nas configurações aparecem Parakeet V3, idioma `pt` e atalho Ctrl + Espaço importados; ditar com Ctrl + Espaço funciona.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/src/main.rs src-tauri/src/lib.rs src-tauri/src/correction
git commit -m "feat: rename to Handy Plus and import settings from Handy on first run"
```

---

### Task 2: Configurações da correção, provedor Gemini e atalho

**Files:**
- Create: `src-tauri/src/correction/settings.rs`
- Modify: `src-tauri/src/settings.rs` (Gemini em `default_post_process_providers`; binding `correct_selection` em `get_default_settings`)
- Modify: `src-tauri/src/lib.rs` (registrar comandos em `collect_commands!`)

**Interfaces:**
- Produces:
```rust
#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
pub struct CorrectionTone { pub id: String, pub name: String, pub goal: String }

#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
pub struct CloudModel { pub provider_id: String, pub model: String }

#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CorrectionProvider { Local, Cloud(CloudModel) }

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
pub struct CorrectionSettings {
    pub provider: CorrectionProvider,             // default Local
    pub models_dir: String,                        // default r"D:\IA\LLM\model"
    pub local_model_path: Option<String>,          // default None
    pub unload_timeout: ModelUnloadTimeout,        // default Min5
    pub fallback: Option<CloudModel>,              // provedor reserva, default None
    pub style_rules: String,                       // default DEFAULT_STYLE_RULES
    pub tones: Vec<CorrectionTone>,                // default default_tones()
    pub last_load_ms: HashMap<String, u64>,        // tempo da última carga por caminho de modelo
}
pub const CORRECTION_STORE_PATH: &str = "correction_store.json";
pub fn get_correction_settings(app: &AppHandle) -> CorrectionSettings;
pub fn write_correction_settings(app: &AppHandle, s: &CorrectionSettings);
#[tauri::command] #[specta::specta] pub fn get_correction_settings_cmd(app: AppHandle) -> CorrectionSettings;
#[tauri::command] #[specta::specta] pub fn set_correction_settings_cmd(app: AppHandle, settings: CorrectionSettings) -> Result<(), String>;
```
- Binding id `correct_selection` (nome "Correct Selection") em `AppSettings.bindings`.

- [ ] **Step 1: Write the failing tests**

```rust
// correction/settings.rs
#[test] fn default_tones_are_the_four_from_the_spec() {
    let names: Vec<_> = default_tones().into_iter().map(|t| t.name).collect();
    assert_eq!(names, ["Meu estilo", "Mais formal", "Mais curto", "Melhorar argumentos"]);
}
#[test] fn default_rules_forbid_dashes_and_fancy_words() {
    assert!(DEFAULT_STYLE_RULES.contains("NUNCA use travessão"));
    assert!(DEFAULT_STYLE_RULES.contains("palavras rebuscadas"));
}
#[test] fn defaults_use_local_provider_and_user_models_dir() {
    let s = CorrectionSettings::default();
    assert_eq!(s.provider, CorrectionProvider::Local);
    assert_eq!(s.models_dir, r"D:\IA\LLM\model");
    assert_eq!(s.unload_timeout, ModelUnloadTimeout::Min5);
}
#[test] fn settings_missing_fields_fall_back_to_defaults() {
    let s: CorrectionSettings = serde_json::from_str(r#"{"models_dir":"E:\\m"}"#).unwrap();
    assert_eq!(s.models_dir, r"E:\m");
    assert_eq!(s.tones.len(), 4);
}
// settings.rs (tests existentes ficam como estão)
#[test] fn gemini_provider_is_available() {
    let p = default_post_process_providers().into_iter().find(|p| p.id == "gemini").unwrap();
    assert_eq!(p.base_url, "https://generativelanguage.googleapis.com/v1beta/openai");
    assert_eq!(p.api_format, PostProcessApiFormat::OpenAi);
}
#[test] fn correct_selection_binding_exists() {
    assert!(get_default_settings().bindings.contains_key("correct_selection"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::settings gemini_provider correct_selection_binding`
Expected: FAIL

- [ ] **Step 3: Implement**

`DEFAULT_STYLE_RULES` e os objetivos de `default_tones()` são copiados **literalmente** do spec, seção 6 (bloco "Regras iniciais" e tabela "Tons iniciais"). Ids dos tons: `meu_estilo`, `mais_formal`, `mais_curto`, `melhorar_argumentos`. Todos os campos com `#[serde(default)]`. Store separado via `tauri-plugin-store` em `crate::portable::store_path(CORRECTION_STORE_PATH)`.
Gemini: `id "gemini"`, `label "Gemini"`, `allow_base_url_edit false`, `supports_structured_output false`, `api_format OpenAi`; `ensure_post_process_defaults` já adiciona provedores novos a lojas antigas.
Atalho padrão: o nome da tecla que fica à esquerda do Z no ABNT2 (VK_OEM_102). Descobrir o nome aceito pelo `handy-keys` e pelo `tauri-plugin-global-shortcut` (`grep -rni "oem_102\|intlbackslash\|backslash"` nas fontes das duas crates em `~/.cargo/registry/src`). Usar `ctrl+<nome>` como `default_binding`; se nenhum nome mapear essa tecla, usar `ctrl+alt+c` e registrar a decisão no commit.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test`
Expected: PASS (todos, inclusive os do Handy)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/correction src-tauri/src/settings.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: add correction settings store, Gemini provider and correct_selection binding"
```

---

### Task 3: Montagem do prompt, limpeza da resposta e progresso

**Files:**
- Create: `src-tauri/src/correction/text.rs`
- Modify: `src-tauri/src/actions.rs` (tornar `strip_think_block` e `strip_invisible_chars` `pub(crate)`)

**Interfaces:**
- Produces:
```rust
pub const MAX_CORRECTION_CHARS: usize = 8000;
pub fn build_messages(rules: &str, goal: &str, text: &str) -> (String /*system*/, String /*user*/);
pub fn clean_output(original: &str, raw: &str) -> String;
pub fn generation_progress(generated_chars: usize, input_chars: usize) -> f32;
pub fn load_progress(elapsed_ms: u64, expected_ms: Option<u64>) -> Option<f32>;
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn system_prompt_is_rules_then_goal() {
    let (sys, user) = build_messages("R1\nR2", "Deixe formal.", "oi");
    assert_eq!(sys, "R1\nR2\n\nObjetivo: Deixe formal.");
    assert_eq!(user, "Texto:\n\noi");
}
#[test] fn removes_dash_pauses_when_original_had_none() {
    assert_eq!(clean_output("a b", "Oi — tudo bem?"), "Oi, tudo bem?");
    assert_eq!(clean_output("a b", "Oi – tudo bem?"), "Oi, tudo bem?");
}
#[test] fn keeps_dashes_the_user_typed() {
    assert_eq!(clean_output("x — y", "X — y."), "X — y.");
}
#[test] fn strips_think_block_and_invisible_chars_and_trims() {
    assert_eq!(clean_output("a", "<think>hm</think>\n Oi\u{200B} "), "Oi");
}
#[test] fn generation_progress_is_capped_until_done() {
    assert_eq!(generation_progress(0, 100), 0.0);
    assert!((generation_progress(50, 100) - 0.5).abs() < 1e-6);
    assert_eq!(generation_progress(150, 100), 0.95);
    assert_eq!(generation_progress(10, 0), 0.95);
}
#[test] fn load_progress_needs_a_previous_load() {
    assert_eq!(load_progress(1000, None), None);
    assert_eq!(load_progress(4500, Some(9000)), Some(0.5));
    assert_eq!(load_progress(20000, Some(9000)), Some(0.95));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::text`
Expected: FAIL

- [ ] **Step 3: Implement** em `correction/text.rs`

`clean_output`: `strip_think_block` → `strip_invisible_chars` → trim; se `original` não contém `—` nem `–`, trocar `\s*[—–]\s*` por `", "` (regex, crate já existe).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test correction::text`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/correction/text.rs src-tauri/src/correction/mod.rs src-tauri/src/actions.rs
git commit -m "feat: add correction prompt building, output cleanup and progress estimates"
```

---

### Task 4: Motor local (`llama-server` como processo auxiliar)

**Files:**
- Create: `scripts/fetch-llama-server.ps1`
- Modify: `.gitignore` (`src-tauri/resources/llama-server/`)
- Create: `src-tauri/src/correction/local_engine.rs`

**Interfaces:**
- Produces:
```rust
pub struct LocalEngine { /* Mutex<Option<Running>>; Running { child, port, model_path, gpu } */ }
pub enum EngineError { BinaryMissing, ModelMissing(String), StartFailed(String), Crashed }
impl LocalEngine {
    pub fn new(binary: PathBuf) -> Self;
    /// Abre (ou reaproveita) o servidor para `model`. `on_progress` recebe `load_progress(...)` a cada 200 ms.
    pub async fn ensure_loaded(&self, model: &Path, gpu: bool, expected_ms: Option<u64>,
        on_progress: impl Fn(Option<f32>) + Send) -> Result<(String /*base_url*/, u64 /*load_ms*/), EngineError>;
    pub fn unload(&self);
    pub fn is_loaded(&self) -> bool;
    pub fn touch(&self);                 // marca uso, reinicia o relógio de inatividade
    pub fn idle_for(&self) -> Option<Duration>;
}
pub fn server_args(model: &Path, port: u16, gpu: bool) -> Vec<String>;
```
O `LocalEngine` fica em `app.manage(Arc<LocalEngine>)` (feito na Task 8).

- [ ] **Step 1: Script de download**

`scripts/fetch-llama-server.ps1` baixa um release fixo do llama.cpp (`ggml-org/llama.cpp`, arquivo `llama-<tag>-bin-win-vulkan-x64.zip`) e extrai em `src-tauri/resources/llama-server/`. Escolher a tag mais recente que suporte Gemma 4 e fixá-la no script. Pedir confirmação ao usuário antes de rodar (download de ~50 MB do GitHub).
Verificação: `src-tauri/resources/llama-server/llama-server.exe --version` imprime a versão.

- [ ] **Step 2: Write the failing tests**

```rust
#[test] fn gpu_args_offload_all_layers_and_disable_thinking() {
    let a = server_args(Path::new(r"D:\m\g.gguf"), 8099, true);
    for want in ["-m", r"D:\m\g.gguf", "--host", "127.0.0.1", "--port", "8099", "-ngl", "999",
                 "--jinja", "--reasoning-budget", "0", "-c", "4096", "--no-webui"] {
        assert!(a.contains(&want.to_string()), "missing {want}");
    }
}
#[test] fn cpu_args_offload_nothing() {
    let a = server_args(Path::new("g.gguf"), 1, false);
    let i = a.iter().position(|x| x == "-ngl").unwrap();
    assert_eq!(a[i + 1], "0");
}
#[tokio::test] async fn missing_binary_is_reported() {
    let e = LocalEngine::new(PathBuf::from(r"C:\nao\existe.exe"));
    let r = e.ensure_loaded(Path::new("x.gguf"), true, None, |_| {}).await;
    assert!(matches!(r, Err(EngineError::BinaryMissing)));
    assert!(!e.is_loaded());
}
/// Manual: HANDY_PLUS_TEST_GGUF=caminho\modelo.gguf cargo test -- --ignored local_engine
#[tokio::test] #[ignore] async fn loads_real_model_and_recovers_after_kill() {
    // ensure_loaded => Ok; GET {base_url}/health == 200
    // matar o processo filho à força => is_loaded() vira false na próxima checagem
    // ensure_loaded de novo => Ok (Review Focus 4)
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::local_engine`
Expected: FAIL

- [ ] **Step 4: Implement `LocalEngine`**

Porta livre via `TcpListener::bind("127.0.0.1:0")`. Processo com `CREATE_NO_WINDOW` (`std::os::windows::process::CommandExt::creation_flags(0x08000000)`). Pronto quando `GET /health` responde 200 (503 enquanto carrega); timeout de 120 s → `StartFailed`. `is_loaded` também confere `child.try_wait()`; processo morto → limpa o estado. Matar o filho em `unload`, no `Drop` e no evento de saída do app (`RunEvent::Exit` em `lib.rs`, Task 8). Trocar de modelo ou de gpu/cpu = `unload` + abrir de novo.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri && cargo test correction::local_engine` e, com um GGUF pequeno em `HANDY_PLUS_TEST_GGUF`, `cargo test -- --ignored local_engine`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add scripts/fetch-llama-server.ps1 .gitignore src-tauri/src/correction/local_engine.rs src-tauri/src/correction/mod.rs
git commit -m "feat: manage a local llama-server process for text correction"
```

---

### Task 5: Cliente de chat com streaming

**Files:**
- Create: `src-tauri/src/correction/stream.rs`

**Interfaces:**
- Consumes: `PostProcessProvider`, `llm_client::send_chat_completion_with_schema` (Anthropic e fallback).
- Produces:
```rust
pub struct ChatTarget { pub base_url: String, pub api_key: String, pub model: String,
                        pub format: PostProcessApiFormat, pub local: bool }
pub async fn stream_chat(target: &ChatTarget, system: &str, user: &str,
    on_delta: impl FnMut(&str) + Send) -> Result<String, String>;
pub struct SseParser { /* buffer */ }
impl SseParser { pub fn feed(&mut self, chunk: &str) -> Vec<SseItem>; }
pub enum SseItem { Delta(String), Done }
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn parses_content_deltas_across_chunk_boundaries() {
    let mut p = SseParser::default();
    let mut out = p.feed("data: {\"choices\":[{\"delta\":{\"content\":\"Oi\"}}]}\n\ndata: {\"choi");
    out.extend(p.feed("ces\":[{\"delta\":{\"content\":\", mano\"}}]}\n\ndata: [DONE]\n\n"));
    assert_eq!(out, vec![SseItem::Delta("Oi".into()), SseItem::Delta(", mano".into()), SseItem::Done]);
}
#[test] fn ignores_reasoning_deltas_and_keepalives() {
    let mut p = SseParser::default();
    let out = p.feed(": ping\n\ndata: {\"choices\":[{\"delta\":{\"reasoning_content\":\"hm\"}}]}\n\n");
    assert!(out.is_empty());
}
#[test] fn local_request_body_disables_thinking_and_pins_sampling() {
    let b = request_body(&ChatTarget { local: true, ..test_target() }, "S", "U");
    assert_eq!(b["stream"], true);
    assert_eq!(b["chat_template_kwargs"]["enable_thinking"], false);
    assert_eq!(b["temperature"], 1.0); assert_eq!(b["top_k"], 64); assert_eq!(b["top_p"], 0.95);
}
#[test] fn cloud_request_body_has_no_local_only_fields() {
    let b = request_body(&ChatTarget { local: false, ..test_target() }, "S", "U");
    assert!(b.get("chat_template_kwargs").is_none() && b.get("top_k").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::stream`
Expected: FAIL

- [ ] **Step 3: Implement**

`fn request_body(target, system, user) -> serde_json::Value` (privada, testada acima; `test_target()` é um helper de teste com valores fixos). OpenAI: `POST {base_url}/chat/completions` com `reqwest` `bytes_stream()` → `SseParser`. Anthropic: sem streaming, chama `llm_client::send_chat_completion_with_schema(.., Some(system), None, true)` e entrega tudo num único `on_delta` (o painel mostra a barra de geração indeterminada nesse caso). Erro HTTP → `Err` com status e corpo curto.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test correction::stream`
Expected: PASS

- [ ] **Step 5: Teste real contra o motor local**

Com o servidor da Task 4 aberto num GGUF do Gemma 4 E4B, um teste `#[ignore]` manda os três textos da seção 11 do spec com `build_messages(DEFAULT_STYLE_RULES, objetivo de "Meu estilo", texto)`.
Expected: respostas parecidas com a tabela do spec, sem `—`, mantendo "a gente", "tá", "mano"; cada uma em menos de ~1 s com o modelo já carregado; nenhum texto de raciocínio. Se o "pensar" aparecer, ajustar `server_args`/`request_body` até sumir.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/correction/stream.rs src-tauri/src/correction/mod.rs
git commit -m "feat: stream chat completions for text correction"
```

---

### Task 6: Proteção da placa de vídeo

**Files:**
- Create: `src-tauri/src/correction/gpu_guard.rs`
- Modify: `src-tauri/Cargo.toml` (`nvml-wrapper` em `[target.'cfg(windows)'.dependencies]`; feature `Win32_UI_Shell` no crate `windows`)

**Interfaces:**
- Produces:
```rust
pub const LOAD_MARGIN_BYTES: u64 = 1_536 * 1024 * 1024;   // folga ao carregar
pub const IDLE_MIN_FREE_BYTES: u64 = 1024 * 1024 * 1024;  // abaixo disso, descarregar
pub fn free_vram_bytes() -> Option<u64>;                  // NVML; None se indisponível
pub fn fullscreen_app_active() -> bool;                   // SHQueryUserNotificationState
pub fn has_room(free: Option<u64>, model_bytes: u64) -> bool;
pub fn should_unload(loaded: bool, session_active: bool, fullscreen: bool, free: Option<u64>) -> bool;
pub fn spawn_watcher(app: AppHandle);                     // a cada 2 s
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn room_needs_model_plus_margin() {
    let gb = 1024u64.pow(3);
    assert!(has_room(Some(7 * gb), 5 * gb));
    assert!(!has_room(Some(6 * gb), 5 * gb));
    assert!(has_room(None, 5 * gb)); // sem NVML não bloqueia
}
#[test] fn unloads_idle_model_for_fullscreen_or_low_memory_only() {
    let gb = 1024u64.pow(3);
    assert!(should_unload(true, false, true, Some(8 * gb)));
    assert!(should_unload(true, false, false, Some(gb / 2)));
    assert!(!should_unload(true, true, true, Some(0)));     // correção em andamento
    assert!(!should_unload(false, false, true, Some(0)));   // nada carregado
    assert!(!should_unload(true, false, false, Some(8 * gb)));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::gpu_guard`
Expected: FAIL

- [ ] **Step 3: Implement**

`fullscreen_app_active`: `QUNS_BUSY`, `QUNS_RUNNING_D3D_FULL_SCREEN` ou `QUNS_PRESENTATION_MODE` → `true`. `spawn_watcher`: lê `Arc<LocalEngine>` e `CorrectionSessionState` (Task 8) do `app.state`, chama `engine.unload()` quando `should_unload` ou quando `engine.idle_for() >= unload_timeout.to_seconds()`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test correction::gpu_guard`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/correction/gpu_guard.rs src-tauri/src/correction/mod.rs
git commit -m "feat: keep the correction model off the GPU when games need it"
```

---

### Task 7: Copiar a seleção e colar o resultado

**Files:**
- Create: `src-tauri/src/correction/selection.rs`
- Modify: `src-tauri/src/input.rs` (`send_copy_ctrl_c`), `src-tauri/src/clipboard.rs` (`with_enigo` → `pub(crate)`; nova `paste_correction`)

**Interfaces:**
- Produces:
```rust
// input.rs
pub fn send_copy_ctrl_c(enigo: &mut Enigo, hold_ms: u64) -> Result<(), String>;
// clipboard.rs
pub fn paste_correction(text: String, app_handle: AppHandle) -> Result<(), String>;
pub(crate) fn correction_paste_method(m: PasteMethod) -> PasteMethod;
// correction/selection.rs
pub struct Captured { pub text: String, pub hwnd: isize }
pub enum CaptureError { NothingSelected, Clipboard(String) }
pub fn capture_selection(app: &AppHandle) -> Result<Captured, CaptureError>;
pub fn refocus(hwnd: isize);
pub fn is_blank_selection(text: &str) -> bool;
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn correction_keeps_clipboard_paste_methods_and_defaults_to_ctrl_v() {
    assert_eq!(correction_paste_method(PasteMethod::CtrlShiftV), PasteMethod::CtrlShiftV);
    assert_eq!(correction_paste_method(PasteMethod::ShiftInsert), PasteMethod::ShiftInsert);
    for m in [PasteMethod::Direct, PasteMethod::None, PasteMethod::ExternalScript] {
        assert_eq!(correction_paste_method(m), PasteMethod::CtrlV);
    }
}
#[test] fn whitespace_only_selection_counts_as_nothing() {   // Review Focus 1
    assert!(is_blank_selection("  \n\t "));
    assert!(!is_blank_selection(" oi "));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction_paste_method whitespace_only_selection`
Expected: FAIL

- [ ] **Step 3: Implement**

`capture_selection` (worker thread): guarda a janela em foco (`GetForegroundWindow`); espera até 600 ms as teclas Ctrl/Shift/Alt/Win serem soltas (`GetAsyncKeyState`); salva a área de transferência (texto, ou imagem se não houver texto, como em `paste_via_clipboard`); limpa; envia Ctrl + C via `with_enigo`; consulta a cada 50 ms por até 1000 ms; **restaura** o conteúdo salvo; texto vazio ou `is_blank_selection` → `NothingSelected`.
`paste_correction`: `PASTE_LOCK`, `paste_via_clipboard` com `correction_paste_method(settings.paste_method)` e os delays do usuário; sem espaço extra, sem auto-submit, sempre restaurando a área de transferência.
`refocus`: se a janela em foco não é `hwnd`, `SetForegroundWindow(hwnd)`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/input.rs src-tauri/src/clipboard.rs src-tauri/src/correction/selection.rs src-tauri/src/correction/mod.rs
git commit -m "feat: capture the selected text and paste corrections back"
```

---

### Task 8: Sessão de correção, atalhos temporários e evento para o painel

**Files:**
- Create: `src-tauri/src/correction/session.rs`
- Modify: `src-tauri/src/actions.rs` (entradas no `ACTION_MAP`), `src-tauri/src/lib.rs` (`manage` do engine e da sessão, `spawn_watcher`, evento em `collect_events!`, matar o engine em `RunEvent::Exit`)

**Interfaces:**
- Consumes: Tasks 2–7.
- Produces:
```rust
#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionPhase { Loading, Generating, Preview, NeedsFallback, Applied, Error, Hidden }
#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionErrorCode { NothingSelected, TextTooLong, NoModelChosen, ModelMissing, EngineFailed, CloudFailed }
#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackOption { Cloud, Cpu }
#[derive(Clone, Debug, Serialize, Deserialize, Type, tauri_specta::Event)]
pub struct CorrectionUpdateEvent {
    pub phase: CorrectionPhase, pub original: String, pub text: String,
    pub load_progress: Option<f32>, pub gen_progress: Option<f32>,
    pub tones: Vec<String>, pub tone_index: u32, pub model_label: String,
    pub elapsed_ms: Option<u64>, pub error: Option<CorrectionErrorCode>,
    pub fallback_options: Vec<FallbackOption>,
}
pub enum SessionInput { Escape, Enter, Digit(u8) }
pub enum SessionCommand { Cancel, Apply, Regenerate(usize), UseFallback(FallbackOption), Ignore }
pub fn decide(phase: &CorrectionPhase, input: SessionInput, tones_len: usize, fallback: &[FallbackOption]) -> SessionCommand;
pub fn should_start(dictation_recording: bool, session_active: bool) -> bool;
pub fn check_text(text: &str) -> Result<(), CorrectionErrorCode>;
pub struct CorrectionSessionState { /* AtomicBool active, Mutex<Option<JoinHandle>> etc. */ }
```
Bindings temporárias (ids): `correction_cancel` → `escape`; `correction_apply` → `enter`; `correction_tone_1` … `correction_tone_9` → `1` … `9`. Não aparecem na tela de atalhos.

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn escape_cancels_while_loading_generating_or_previewing() {
    for p in [CorrectionPhase::Loading, CorrectionPhase::Generating, CorrectionPhase::Preview] {
        assert_eq!(decide(&p, SessionInput::Escape, 4, &[]), SessionCommand::Cancel);
    }
}
#[test] fn preview_enter_applies_and_digits_pick_tones() {
    let p = CorrectionPhase::Preview;
    assert_eq!(decide(&p, SessionInput::Enter, 4, &[]), SessionCommand::Apply);
    assert_eq!(decide(&p, SessionInput::Digit(2), 4, &[]), SessionCommand::Regenerate(1));
    assert_eq!(decide(&p, SessionInput::Digit(5), 4, &[]), SessionCommand::Ignore);
}
#[test] fn fallback_digits_pick_options_and_enter_takes_the_first() {
    let opts = [FallbackOption::Cloud, FallbackOption::Cpu];
    let p = CorrectionPhase::NeedsFallback;
    assert_eq!(decide(&p, SessionInput::Digit(2), 4, &opts), SessionCommand::UseFallback(FallbackOption::Cpu));
    assert_eq!(decide(&p, SessionInput::Enter, 4, &opts), SessionCommand::UseFallback(FallbackOption::Cloud));
}
#[test] fn ignores_shortcut_during_dictation_or_another_correction() { // Review Focus 3
    assert!(should_start(false, false));
    assert!(!should_start(true, false));
    assert!(!should_start(false, true));
}
#[test] fn rejects_too_long_text() { // Review Focus 2
    assert_eq!(check_text(&"a".repeat(MAX_CORRECTION_CHARS + 1)), Err(CorrectionErrorCode::TextTooLong));
    assert_eq!(check_text("oi"), Ok(()));
}
```
`#[derive(Debug, PartialEq)]` em `SessionCommand`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::session`
Expected: FAIL

- [ ] **Step 3: Implement o fluxo**

`CorrectSelectionAction` no `ACTION_MAP` com chave `correct_selection`: `start` não faz nada; `stop` (tecla solta) chama `session::begin(app)` se `should_start(audio_manager.is_recording(), state.active)`. Ações `correction_*` chamam `session::input(app, SessionInput::…)`.
`begin` (numa task tokio):
1. `capture_selection` → erro vira painel `Error` (some após 3 s).
2. `check_text`; registrar `correction_cancel`.
3. Escolher alvo: `Cloud(CloudModel)` → `ChatTarget` com `base_url`/`api_format` do provedor em `settings.post_process_providers` e chave de `settings.post_process_api_keys`. `Local` → sem `local_model_path` ou arquivo inexistente → `NoModelChosen`/`ModelMissing` (Review Focus 5); se o engine não está carregado e `!has_room(free_vram_bytes(), tamanho do arquivo)` → fase `NeedsFallback` com `Cloud` (se `fallback` configurado) e `Cpu`, registrando `correction_apply` e dígitos.
4. `Loading`: `ensure_loaded` emitindo `load_progress`; salvar `load_ms` em `last_load_ms`. Falha → `EngineFailed` (Review Focus 4).
5. `Generating`: `stream_chat` com `build_messages(style_rules, tones[i].goal, original)`, acumulando texto e emitindo `gen_progress = generation_progress(...)` no máximo a cada 50 ms. Esc → abortar a task.
6. `Preview`: `clean_output`, `gen_progress = 1.0`, `elapsed_ms`; registrar `correction_apply` e `correction_tone_1..N` (N = `min(9, tones.len())`).
7. `Apply`: desregistrar temporárias, esconder o painel, `refocus(hwnd)`, `paste_correction`, fase `Applied` por 2 s. `Regenerate(i)` volta ao passo 5 a partir do **original**. `Cancel`: desregistrar, esconder.
Sempre: `engine.touch()` ao usar; ao terminar por qualquer caminho, desregistrar temporárias e `active = false`.
Overlay: chamar `overlay::show_correction_overlay(app)` (Task 9) ao entrar em `Loading`/`Generating`/`Preview`/`NeedsFallback`; `hide_recording_overlay` ao sair.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/correction src-tauri/src/actions.rs src-tauri/src/lib.rs src/bindings.ts
git commit -m "feat: run text correction sessions from the correct_selection shortcut"
```

---

### Task 9: Painel da correção na pílula

**Files:**
- Modify: `src-tauri/src/overlay.rs` (tamanho e função do estado `correction`)
- Create: `src/overlay/correction/CorrectionPanel.tsx`, `src/overlay/correction/CorrectionPanel.css`, `src/overlay/correction/diff.ts`, `src/overlay/correction/diff.test.ts`
- Modify: `src/overlay/RecordingOverlay.tsx` (renderizar o painel no estado `correction`), `src/i18n/locales/en/translation.json`, `src/i18n/locales/pt/translation.json`

**Interfaces:**
- Consumes: `events.correctionUpdateEvent` (bindings), estados da Task 8.
- Produces: `overlay::show_correction_overlay(app: &AppHandle)`; `const CORRECTION_WIDTH: f64 = 480.0; const CORRECTION_HEIGHT: f64 = 260.0;`; `diffWords(before: string, after: string): DiffPart[]` com `type DiffPart = { text: string; kind: "same" | "added" | "removed" }`.

- [ ] **Step 1: Write the failing tests**

```rust
// overlay.rs
#[test] fn correction_state_uses_the_large_panel() {
    assert_eq!(overlay_dimensions("correction"), (CORRECTION_WIDTH, CORRECTION_HEIGHT));
    assert_eq!(overlay_dimensions("recording"), (OVERLAY_WIDTH, OVERLAY_HEIGHT));
}
```
```ts
// src/overlay/correction/diff.test.ts  (bun test)
test("marks changed words", () => {
  expect(diffWords("oi mano blz", "Oi, mano, beleza")).toEqual([
    { text: "oi", kind: "removed" }, { text: "Oi,", kind: "added" }, { text: " ", kind: "same" },
    { text: "mano", kind: "removed" }, { text: "mano,", kind: "added" }, { text: " ", kind: "same" },
    { text: "blz", kind: "removed" }, { text: "beleza", kind: "added" },
  ]);
});
test("identical text has no changes", () => {
  expect(diffWords("tudo certo", "tudo certo").every((p) => p.kind === "same")).toBe(true);
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction_state_uses` e `bun test src/overlay/correction/diff.test.ts`
Expected: FAIL

- [ ] **Step 3: Implement**

`overlay.rs`: `overlay_dimensions("correction")` devolve o tamanho novo; o posicionamento do Windows passa a usar as dimensões do estado atual (hoje ele só distingue streaming via `WINDOWS_OVERLAY_IS_STREAMING`); `show_correction_overlay` = `show_overlay_state(app, "correction")`.
`diffWords`: tokens por `/(\s+)/` mantendo os espaços, LCS por token; dentro de um trecho alterado, as partes `removed` vêm antes das `added`.
`CorrectionPanel` segue o mockup aprovado (`.superpowers/brainstorm/618-1794248153/content/correcao-fluxo-v2.html`, copiado para a pasta do projeto): pílula com o tom e o modelo; acima dela o cartão com
- `Loading`: barra "Carregando modelo" (percentual quando `load_progress` existe, animação indeterminada quando não) e "Gerando texto" apagada;
- `Generating`: primeira barra verde com ✓, segunda com `gen_progress` e o texto chegando com cursor piscando;
- `Preview`: texto com `added` em verde e `removed` riscado em vermelho, lista numerada de tons à direita com o atual destacado, dicas `Enter aplicar · 1–N outro tom · Esc cancelar`, pílula "Pronto em X s";
- `NeedsFallback`: "Placa de vídeo cheia" com as opções numeradas (1 Nuvem, 2 Processador) e `Esc cancelar`;
- `Applied`: só a pílula "✓ Corrigido · Ctrl + Z desfaz";
- `Error`: só a pílula com a mensagem do código.
Textos (chaves em `overlay.correction.*`, `en` e `pt`): "Carregando modelo", "Gerando texto", "Pronto em {{s}} s", "Corrigido", "Ctrl + Z desfaz", "aplicar", "outro tom", "cancelar", "Placa de vídeo cheia", "Nuvem", "Processador", "Nenhum texto selecionado", "Texto grande demais", "Escolha um modelo nas configurações", "Modelo não encontrado", "O modelo local falhou", "A IA na nuvem falhou". Cores e fontes seguem o `RecordingOverlay.css`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri && cargo test` e `bun test src/overlay/correction/diff.test.ts` e `bun run build`
Expected: PASS / build sem erros

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/overlay.rs src/overlay src/i18n/locales/en/translation.json src/i18n/locales/pt/translation.json
git commit -m "feat: show correction progress and preview in the overlay"
```

---

### Task 10: Tela "Correção de texto" nas configurações

**Files:**
- Create: `src-tauri/src/correction/models.rs`
- Create: `src/components/settings/correction/CorrectionSettings.tsx`, `CorrectionProviderSection.tsx`, `CorrectionTonesEditor.tsx`, `index.ts`
- Modify: `src/components/settings/index.ts`, `src/components/Sidebar.tsx` (seção `correction`, ícone `SpellCheck` do lucide, sempre habilitada), `src-tauri/src/lib.rs` (comando), locales (`en`, `pt`; demais com o texto em inglês)

**Interfaces:**
- Produces:
```rust
#[derive(Serialize, Debug, Clone, Type, PartialEq)]
pub struct LocalModel { pub path: String, pub label: String, pub size_bytes: u64 }
pub fn find_local_models(dir: &Path) -> Vec<LocalModel>;
#[tauri::command] #[specta::specta] pub fn list_local_models(dir: String) -> Vec<LocalModel>;
```

- [ ] **Step 1: Write the failing tests**

```rust
#[test] fn finds_gguf_files_by_author_and_skips_projectors() {
    // dir/bartowski/Qwen-GGUF/qwen.Q4.gguf, dir/x/y/mmproj-y.gguf, dir/x/y/notes.txt
    let m = find_local_models(dir);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].label, "bartowski / qwen.Q4");
}
#[test] fn missing_dir_gives_empty_list() {
    assert!(find_local_models(Path::new(r"Z:\nao\existe")).is_empty());
}
```
Busca até 3 níveis, ordenado por `label`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri && cargo test correction::models`
Expected: FAIL

- [ ] **Step 3: Implement backend e tela**

A tela usa `commands.getCorrectionSettingsCmd`/`setCorrectionSettingsCmd`/`listLocalModels` e, para chaves de API, `changePostProcessApiKeySetting` (já existe). Seções, no visual de `PostProcessingSettings`:
1. Atalho: `<ShortcutInput shortcutId="correct_selection" />`.
2. Provedor: "Local" + cada provedor de `settings.post_process_providers` exceto `apple_intelligence`.
3. Se Local: pasta de modelos (texto + botão de escolher pasta via `tauri-plugin-dialog`), modelo (lista de `listLocalModels`, mostrando tamanho em GB), tempo para descarregar (mesmas opções do `ModelUnloadTimeout.tsx`).
4. Se nuvem: modelo (texto) e chave.
5. Provedor reserva (opcional): provedor + modelo.
6. "Regras do meu estilo": textarea.
7. Tons: lista com nome e objetivo editáveis, adicionar, apagar, subir/descer (máximo 9; o primeiro é o padrão do atalho).

- [ ] **Step 4: Run tests and checks**

Run: `cd src-tauri && cargo test`, `bun run lint`, `bun run check:translations`, `bun run build`
Expected: tudo PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/correction/models.rs src-tauri/src/lib.rs src/components src/i18n src/bindings.ts
git commit -m "feat: add the text correction settings page"
```

---

### Task 11: Modelo recomendado e verificação de ponta a ponta

**Files:**
- Modify: `docs/superpowers/specs/2026-11-09-handy-plus-correcao-design.md` (apenas se algo do spec mudou durante a construção)

- [ ] **Step 1: Baixar o Gemma 4 E4B em GGUF (pedir confirmação ao usuário com nome do arquivo, origem e tamanho)**

Destino: `D:\IA\LLM\model\<autor>\<repo>\<arquivo>.gguf`, quantização Q4_K_M, de um repositório GGUF oficial ou conhecido no Hugging Face. Escolher esse modelo na tela da Task 10.

- [ ] **Step 2: Rodar o app e conferir cada critério da seção 11 do spec**

Run: `bun run tauri dev`
Conferir e anotar no resumo final:
- Os três textos da tabela do spec, digitados no Bloco de Notas, no WhatsApp, no Discord e no navegador: resultado parecido, sem travessão, mantendo "a gente", "tá", "mano".
- Prévia em ~1 s com modelo carregado; na primeira vez a barra "Carregando modelo" aparece (e mostra percentual a partir da segunda carga).
- Números trocam o tom; Esc cancela; Enter aplica; Ctrl + Z desfaz no programa.
- Área de transferência: copiar uma frase, corrigir outro texto, colar → a frase original volta.
- Seleção vazia → aviso "Nenhum texto selecionado".
- Abrir um jogo em tela cheia com o modelo carregado → em até ~2 s o modelo sai da placa (`nvidia-smi`).
- Com a placa quase cheia, a correção oferece Nuvem/Processador e não trava nada.
- Ditado com Ctrl + Espaço igual a antes.

- [ ] **Step 3: Gerar o executável**

Run: `bun run tauri build --no-bundle` (se der erro de caminho longo: `$env:CARGO_TARGET_DIR = "C:\h"`)
Expected: `handy.exe` em `src-tauri/target/release/` abre como "Handy Plus", com `llama-server` ao lado em `resources/`.

- [ ] **Step 4: Arrumação (pedir confirmação ao usuário antes de apagar qualquer coisa)**

Oferecer remover `%LOCALAPPDATA%\Programs\WritingTools` (inclui a ponte) e o modelo `gemma4:e4b` do Ollama; lembrar o usuário de desligar a inicialização automática do Handy original.

- [ ] **Step 5: Commit**

```bash
git add -A docs
git commit -m "docs: record end-to-end verification of Handy Plus correction"
```

---

## Ajustes depois do primeiro teste do usuário (2026-11-10)

Feedback do usuário no teste da Task 11: (1) com texto longo é impossível ver o resto; (2) "Mais formal" e "Melhorar argumentos" mudam pouco, a intenção é mudar a estrutura e reorganizar as ideias; (3) a caixa é pequena e, com as marcações de mudança, fica difícil de ler; (4) quer ver a mensagem original; (5) quer ativar sem precisar selecionar (o botão flutuante é um projeto à parte, com design próprio; aqui só o atalho sem seleção).

### Task 12: Painel maior, rolagem por teclado e três visões

**Files:**
- Modify: `src-tauri/src/overlay.rs` (`CORRECTION_WIDTH`/`CORRECTION_HEIGHT`)
- Modify: `src-tauri/src/correction/session.rs` (novas bindings temporárias e campo `view`)
- Modify: `src/overlay/correction/CorrectionPanel.tsx`, `CorrectionPanel.css`, locales `en`/`pt` (+ inglês nas outras)

**Interfaces:**
- Produces:
```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CorrectionView { #[default] Corrected, Changes, Original }
impl CorrectionView { pub fn next(self) -> Self; } // Corrected -> Changes -> Original -> Corrected
// CorrectionUpdateEvent ganha: pub view: CorrectionView, pub scroll_nonce: u32, pub scroll_dir: i8 (-1 sobe, 1 desce, 0 nada)
// SessionInput ganha: Tab, ScrollUp, ScrollDown
// bindings temporárias novas: correction_view -> "tab", correction_scroll_up -> "up", correction_scroll_down -> "down"
//   (registradas em Preview e Generating; nomes de tecla conferidos no handy-keys e no global-hotkey)
```
- `decide`: `Tab` em Preview → `SessionCommand::NextView`; `ScrollUp`/`ScrollDown` em Generating ou Preview → `SessionCommand::Scroll(-1 | 1)`; em outras fases → `Ignore`.
- Tamanho: `CORRECTION_WIDTH = 640.0`, `CORRECTION_HEIGHT = 420.0`.

- [ ] **Step 1: Testes que falham**
```rust
#[test] fn view_cycles_corrected_changes_original() {
    assert_eq!(CorrectionView::Corrected.next(), CorrectionView::Changes);
    assert_eq!(CorrectionView::Changes.next(), CorrectionView::Original);
    assert_eq!(CorrectionView::Original.next(), CorrectionView::Corrected);
}
#[test] fn tab_switches_view_only_in_preview() {
    assert_eq!(decide(&CorrectionPhase::Preview, SessionInput::Tab, 4, &[]), SessionCommand::NextView);
    assert_eq!(decide(&CorrectionPhase::Generating, SessionInput::Tab, 4, &[]), SessionCommand::Ignore);
}
#[test] fn arrows_scroll_while_generating_or_previewing() {
    for p in [CorrectionPhase::Generating, CorrectionPhase::Preview] {
        assert_eq!(decide(&p, SessionInput::ScrollDown, 4, &[]), SessionCommand::Scroll(1));
        assert_eq!(decide(&p, SessionInput::ScrollUp, 4, &[]), SessionCommand::Scroll(-1));
    }
    assert_eq!(decide(&CorrectionPhase::Loading, SessionInput::ScrollDown, 4, &[]), SessionCommand::Ignore);
}
// overlay.rs: correction_state_uses_the_large_panel continua valendo com os valores novos
```
- [ ] **Step 2: Rodar e ver falhar.** `cargo test correction::session` → FAIL.
- [ ] **Step 3: Implementar.** Backend: `view` começa em `Corrected` a cada sessão e volta a `Corrected` a cada Regenerate; `Scroll(d)` incrementa `scroll_nonce` e emite com `scroll_dir = d`. Frontend: em Preview, `Corrected` mostra o texto corrigido limpo (sem marcações), `Changes` mostra o diff atual, `Original` mostra o texto original; um rótulo pequeno mostra a visão atual e a dica "Tab: Corrigido / Mudanças / Original". O cartão de texto ocupa a maior parte da janela, fonte 14 px, linha 1,5; rola ~3 linhas a cada `scroll_nonce` novo; durante Generating rola sozinho pro fim enquanto o usuário não usou as setas. Dicas: `Enter aplicar · Tab visão · ↑↓ rolar · 1–N tom · Esc cancelar` (pt sem travessão).
- [ ] **Step 4: Rodar e ver passar.** `cargo test`, `bun run lint`, `bun run check:translations`, `bun run build`.
- [ ] **Step 5: Commit.** `feat: bigger correction panel with keyboard scroll and three views`

### Task 13: Tons que reescrevem e reorganizam

**Files:**
- Modify: `src-tauri/src/correction/settings.rs`

**Interfaces:**
- `DEFAULT_STYLE_RULES` e `default_tones()` passam a ter os textos abaixo (literais).
- `pub fn migrate_old_defaults(s: &mut CorrectionSettings) -> bool` troca `style_rules` se for igual à regra antiga (`OLD_STYLE_RULES_V1`, cópia literal da atual) e troca o `goal` de cada tom cujo `id` e `goal` sejam iguais aos antigos (`OLD_TONE_GOALS_V1`); tons editados pelo usuário ficam intactos; devolve se mudou algo. Chamada em `get_correction_settings`, gravando de volta quando mudou.

Regras novas:
```
Você revisa textos em português do Brasil escritos pelo usuário para WhatsApp, Discord, e-mail e comentários na internet.
Siga o objetivo abaixo. Quando ele pedir só correção, mexa o mínimo; quando pedir para reescrever ou reorganizar, você pode mudar a estrutura, a ordem das ideias e as frases.
O resultado deve soar natural, como alguém conversando, e não como um texto corporativo ou de redação escolar.
NUNCA use travessão (— ou –) nem hífen como pausa. Use vírgula, ponto ou dois-pontos no lugar.
Não troque palavras simples por palavras rebuscadas ou pouco usadas no dia a dia.
Não invente fatos nem acrescente saudações, despedidas ou emojis que não estavam no texto.
Responda APENAS com o texto final, sem comentários.
Não responda ao conteúdo do texto, apenas trabalhe nele.
```

| Tom | Objetivo novo |
|---|---|
| Meu estilo | Corrija ortografia, acentuação, concordância, pontuação e vírgulas, e deixe as frases coerentes e fáceis de ler. Mantenha as mesmas palavras sempre que estiverem certas, as expressões do usuário e o mesmo nível de formalidade. Não deixe o texto mais longo do que precisa. |
| Mais formal | Reescreva com tom formal e educado, como num e-mail de trabalho: troque gírias e abreviações por palavras comuns e corretas, use frases completas e organize em parágrafos se precisar. Sem ficar engessado nem usar palavras difíceis. |
| Mais curto | Reescreva de forma mais curta e direta, cortando repetições e rodeios, sem perder nenhuma informação importante. |
| Melhorar argumentos | Reorganize o texto para convencer melhor: comece pela ideia principal, coloque os argumentos numa ordem lógica, junte ou separe frases quando ajudar, ligue as ideias com conectivos simples e termine com uma conclusão clara. Pode mudar bastante a estrutura, mas não invente fatos nem mude a opinião do usuário. |

- [ ] **Step 1: Testes que falham**
```rust
#[test] fn migrates_untouched_old_defaults() {
    let mut s = CorrectionSettings { style_rules: OLD_STYLE_RULES_V1.into(), tones: old_tones_v1(), ..Default::default() };
    assert!(migrate_old_defaults(&mut s));
    assert_eq!(s.style_rules, DEFAULT_STYLE_RULES);
    assert_eq!(s.tones, default_tones());
}
#[test] fn keeps_user_edited_rules_and_tones() {
    let mut tones = old_tones_v1(); tones[1].goal = "meu objetivo".into();
    let mut s = CorrectionSettings { style_rules: "minhas regras".into(), tones, ..Default::default() };
    migrate_old_defaults(&mut s);
    assert_eq!(s.style_rules, "minhas regras");
    assert_eq!(s.tones[1].goal, "meu objetivo");
    assert_eq!(s.tones[3].goal, default_tones()[3].goal);
}
#[test] fn new_defaults_keep_the_dash_rule_and_restructure_tone() {
    assert!(DEFAULT_STYLE_RULES.contains("NUNCA use travessão"));
    assert!(default_tones()[3].goal.contains("Reorganize"));
}
```
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.**
- [ ] **Step 4: Teste real (ignorado) com o Gemma** (`HANDY_PLUS_TEST_GGUF`): um texto de opinião desorganizado de ~5 frases (no estilo do usuário, informal e sem vírgulas) passado pelos 4 tons; anotar no relatório as 4 saídas. Esperado: "Mais formal" troca gírias e completa frases; "Melhorar argumentos" muda visivelmente a ordem e a estrutura; nenhum travessão.
- [ ] **Step 5: Commit.** `feat: let formal and argument tones restructure the text`

### Task 14: Atalho sem seleção pega o texto da caixa inteira

**Files:**
- Modify: `src-tauri/src/correction/selection.rs`, `src-tauri/src/input.rs`

**Interfaces:**
- `pub fn send_select_all_ctrl_a(enigo: &mut Enigo, hold_ms: u64) -> Result<(), String>` em `input.rs`.
- `capture_selection`: quando a primeira cópia não traz texto, envia Ctrl+A, espera ~60 ms e tenta copiar de novo (mesma lógica de salvar/restaurar, tudo sob `PASTE_LOCK`); `Captured` ganha `pub whole_field: bool`. Se ainda vier vazio → `NothingSelected`.
- `fn after_first_copy(first: &str) -> CaptureStep` puro, com `enum CaptureStep { Done, TrySelectAll }`.

- [ ] **Step 1: Teste que falha**
```rust
#[test] fn empty_first_copy_tries_select_all() {
    assert_eq!(after_first_copy(""), CaptureStep::TrySelectAll);
    assert_eq!(after_first_copy("  \n"), CaptureStep::TrySelectAll);
    assert_eq!(after_first_copy("oi"), CaptureStep::Done);
}
```
- [ ] **Step 2 a 4:** falhar, implementar, passar (`cargo test`).
- [ ] **Step 5: Commit.** `feat: correct the whole text box when nothing is selected`

---

## Segunda rodada de ajustes (2026-11-10): Desenvolver, som e botão flutuante

Spec: seção 14 do design. O usuário aprovou o botão flutuante; o tom "Desenvolver" e o som são escolhas do controlador avisadas ao usuário.

### Task 15: Tom "Desenvolver"

**Files:**
- Modify: `src-tauri/src/correction/settings.rs`, `src-tauri/src/correction/stream.rs` (teste real ignorado)

**Interfaces:**
- `CorrectionSettings` ganha `pub defaults_version: u32` (`#[serde(default)]`, 0 para lojas antigas; `Default` = `CURRENT_DEFAULTS_VERSION = 2`).
- `default_tones()` passa a ter 5 tons; o quinto é `CorrectionTone { id: "desenvolver", name: "Desenvolver", goal: <texto abaixo> }`.
- `migrate_old_defaults` (renomear para `migrate_defaults`, mantendo o comportamento V1) passa também a: se `defaults_version < 2`, acrescentar "Desenvolver" no fim quando não existir tom com id `desenvolver` e houver menos de 9 tons; depois `defaults_version = 2`. Se o usuário apagar o tom depois, ele não volta.

Objetivo do tom:
```
Desenvolva o texto: explique melhor cada ideia, acrescente detalhes, exemplos ou justificativas que deixem o raciocínio completo, e organize em parágrafos se ficar longo. Mantenha o jeito de falar do usuário e a mesma opinião. Não invente fatos específicos, como números, nomes ou datas, que não estavam no texto.
```

- [ ] **Step 1: Testes que falham**
```rust
#[test] fn defaults_include_desenvolver_last() {
    let t = default_tones();
    assert_eq!(t.len(), 5);
    assert_eq!((t[4].id.as_str(), t[4].name.as_str()), ("desenvolver", "Desenvolver"));
}
#[test] fn old_store_gets_desenvolver_once() {
    let mut s = CorrectionSettings { tones: default_tones()[..4].to_vec(), defaults_version: 0, ..Default::default() };
    assert!(migrate_defaults(&mut s));
    assert_eq!(s.tones.len(), 5);
    assert_eq!(s.defaults_version, 2);
    s.tones.pop();                       // usuário apagou
    assert!(!migrate_defaults(&mut s));  // não volta
    assert_eq!(s.tones.len(), 4);
}
#[test] fn full_tone_list_is_not_extended() {
    let mut tones = default_tones()[..4].to_vec();
    while tones.len() < 9 { tones.push(CorrectionTone { id: format!("t{}", tones.len()), name: "x".into(), goal: "y".into() }); }
    let mut s = CorrectionSettings { tones, defaults_version: 0, ..Default::default() };
    migrate_defaults(&mut s);
    assert_eq!(s.tones.len(), 9);
    assert_eq!(s.defaults_version, 2);
}
```
- [ ] **Step 2: Rodar e ver falhar.**
- [ ] **Step 3: Implementar.** Ajustar os testes de migração existentes que comparam com `default_tones()` se precisarem.
- [ ] **Step 4: Teste real (ignorado) com o Gemma:** acrescentar "Desenvolver" ao teste de tons; afirmar que a saída tem pelo menos 130% do tamanho do original e não tem travessão; gravar a saída no arquivo de saída dos tons.
- [ ] **Step 5: Commit.** `feat: add a Desenvolver tone that expands the text`

### Task 16: Som discreto ao começar e quando a prévia fica pronta

**Files:**
- Create: `src-tauri/resources/correction_start.wav`, `src-tauri/resources/correction_ready.wav` (gerados por script, versionados), `scripts/make-correction-sounds.py`
- Modify: `src-tauri/src/audio_feedback.rs` (helper `pub(crate)`), `src-tauri/src/correction/settings.rs`, `src-tauri/src/correction/session.rs`, `src/components/settings/correction/CorrectionSettings.tsx`, locales

**Interfaces:**
- `CorrectionSettings` ganha `pub sounds: bool` (`#[serde(default = "default_true")]`, padrão `true`).
- `audio_feedback::play_resource_sound(app: &AppHandle, file: &str, volume_scale: f32)` — toca um arquivo de `resources/` no dispositivo de saída escolhido, volume = `audio_feedback_volume * volume_scale`, sem depender da chave `audio_feedback` do ditado; assíncrono.
- `pub const CORRECTION_SOUND_VOLUME: f32 = 0.6;`
- Sessão: toca `correction_start.wav` uma vez por sessão ao entrar em Loading ou Generating (o que vier primeiro) e `correction_ready.wav` a cada vez que entra em Preview, só se `sounds` estiver ligado. Decisão pura e testada: `fn sound_for_transition(prev: &CorrectionPhase, next: &CorrectionPhase, start_played: bool) -> Option<CorrectionSound>` com `enum CorrectionSound { Start, Ready }`.
- Sons: script Python sem dependências (módulos `wave`, `math`, `struct`) gera WAV 44,1 kHz mono 16 bits: start = dois tons senoidais curtos e suaves subindo (ex.: 660 Hz → 880 Hz, ~90 ms cada, envelope com ataque e fade de ~15 ms, pico −18 dBFS); ready = um tom único 880 Hz ~140 ms com fade longo, pico −20 dBFS. Nada estridente.
- Tela: chave "Som ao corrigir" na seção da correção.

- [ ] **Step 1: Teste que falha**
```rust
#[test] fn sounds_play_on_start_once_and_on_each_preview() {
    use CorrectionPhase::*;
    assert_eq!(sound_for_transition(&Hidden, &Loading, false), Some(CorrectionSound::Start));
    assert_eq!(sound_for_transition(&Loading, &Generating, true), None);
    assert_eq!(sound_for_transition(&Hidden, &Generating, false), Some(CorrectionSound::Start));
    assert_eq!(sound_for_transition(&Generating, &Preview, true), Some(CorrectionSound::Ready));
    assert_eq!(sound_for_transition(&Preview, &Generating, true), None);
    assert_eq!(sound_for_transition(&Generating, &Error, true), None);
}
#[test] fn sounds_default_on() { assert!(CorrectionSettings::default().sounds); }
```
- [ ] **Step 2–4:** falhar, implementar, passar (`cargo test`, `bun run lint`, `bun run check:translations`, `bun run build`).
- [ ] **Step 5: Commit.** `feat: play soft sounds when a correction starts and is ready`

### Task 17: Detectar a caixa de texto em foco (UI Automation)

**Files:**
- Create: `src-tauri/src/correction/text_focus.rs`
- Modify: `src-tauri/Cargo.toml` (features `Win32_UI_Accessibility`, `Win32_System_Com`, `Win32_System_Ole`/`Variant` se precisar), `src-tauri/src/correction/settings.rs`

**Interfaces:**
```rust
#[derive(Clone, Debug, PartialEq)]
pub struct FieldInfo {
    pub hwnd: isize, pub pid: u32, pub process_name: String,   // ex. "Discord.exe"
    pub rect: (i32, i32, i32, i32),                              // left, top, right, bottom em px físicos
    pub editable: bool, pub password: bool, pub has_text: bool,
}
pub fn focused_field() -> Option<FieldInfo>;                     // Windows: IUIAutomation::GetFocusedElement; outros: None
pub fn should_show_button(field: Option<&FieldInfo>, enabled: bool, session_active: bool,
    fullscreen: bool, excluded: &[String], own_pid: u32) -> bool;
pub fn button_position(rect: (i32, i32, i32, i32), size_px: i32, inset_px: i32) -> (i32, i32);
// CorrectionSettings ganha: pub floating_button: bool (padrão true), pub button_excluded_apps: Vec<String> (padrão vazio)
```
- `focused_field`: COM inicializado na thread (MTA); `editable` = tipo Edit ou Document, habilitado e não somente leitura (ValuePattern `IsReadOnly` quando houver); `password` = `CurrentIsPassword`; `has_text` = valor não vazio (ValuePattern) ou, para Document, `TextPattern.DocumentRange.GetText(1)` não vazio; `rect` = `CurrentBoundingRectangle`; nome do processo via PID. Qualquer erro → `None`. Nunca bloquear mais que ~50 ms (sem chamadas que esperem a janela alvo).
- `should_show_button`: só `true` se `enabled`, sem sessão ativa, sem tela cheia, campo existe, `editable && !password && has_text`, PID diferente do próprio app e `process_name` fora de `excluded` (comparação sem diferenciar maiúsculas).
- `button_position`: canto inferior direito por dentro da caixa, `x = right - size - inset`, `y = bottom - size - inset`; caixas menores que o botão + 2×inset → posição à direita da caixa, alinhada embaixo (`x = right + inset`).

- [ ] **Step 1: Testes que falham**
```rust
fn f() -> FieldInfo { FieldInfo { hwnd: 1, pid: 10, process_name: "notepad.exe".into(),
    rect: (100, 100, 500, 200), editable: true, password: false, has_text: true } }
#[test] fn shows_for_editable_field_with_text() { assert!(should_show_button(Some(&f()), true, false, false, &[], 99)); }
#[test] fn hides_for_every_blocker() {
    assert!(!should_show_button(None, true, false, false, &[], 99));
    assert!(!should_show_button(Some(&f()), false, false, false, &[], 99));
    assert!(!should_show_button(Some(&f()), true, true, false, &[], 99));
    assert!(!should_show_button(Some(&f()), true, false, true, &[], 99));
    assert!(!should_show_button(Some(&FieldInfo { password: true, ..f() }), true, false, false, &[], 99));
    assert!(!should_show_button(Some(&FieldInfo { has_text: false, ..f() }), true, false, false, &[], 99));
    assert!(!should_show_button(Some(&FieldInfo { editable: false, ..f() }), true, false, false, &[], 99));
    assert!(!should_show_button(Some(&f()), true, false, false, &["NOTEPAD.EXE".into()], 99));
    assert!(!should_show_button(Some(&f()), true, false, false, &[], 10));
}
#[test] fn button_sits_inside_bottom_right_or_beside_small_fields() {
    assert_eq!(button_position((100, 100, 500, 200), 28, 6), (466, 166));
    assert_eq!(button_position((100, 100, 130, 120), 28, 6), (136, 92));
}
#[test] fn floating_button_defaults_on_with_no_exclusions() {
    let s = CorrectionSettings::default();
    assert!(s.floating_button);
    assert!(s.button_excluded_apps.is_empty());
}
/// Manual: abre o Bloco de Notas pelo usuário, não por automação. Só imprime focused_field() a cada 500 ms por 10 s.
#[test] #[ignore] fn print_focused_field() { /* ... */ }
```
(No segundo caso de `button_position`: caixa de 30×20 < 28+12 → `x = 130 + 6 = 136`, `y = 120 - 28 = 92`.)
- [ ] **Step 2–4:** falhar, implementar, passar.
- [ ] **Step 5: Commit.** `feat: detect the focused text box for the floating button`

### Task 18: Janela do botão flutuante e clique que corrige

**Files:**
- Create: `src/correction-button/index.html`, `main.tsx`, `CorrectionButton.tsx`, `CorrectionButton.css`; `src-tauri/src/correction/button.rs`
- Modify: `vite.config.ts` (entrada nova), `src-tauri/src/lib.rs` (criar janela, comando, iniciar o observador), `src-tauri/src/correction/session.rs` (`begin` aceita origem botão), `src/components/settings/correction/CorrectionSettings.tsx` (chave + lista de exclusões), locales

**Interfaces:**
```rust
pub const BUTTON_SIZE: f64 = 28.0;            // px lógicos
pub fn create_button_window(app: &AppHandle);  // label "correction_button": transparente, sem decoração, sempre no topo,
                                               // fora da barra de tarefas, focusable(false), accept_first_mouse(true), oculta
pub fn spawn_button_watcher(app: AppHandle, session_active: Arc<AtomicBool>); // thread a cada 250 ms: focused_field + should_show_button
                                               // -> mostra/move (button_position, convertendo px físicos com o fator de escala do monitor) ou esconde;
                                               // só emite mudanças; lembra o FieldInfo atual
#[tauri::command] #[specta::specta] pub fn correction_button_clicked(app: AppHandle);
                                               // esconde o botão e chama session::begin_from_button(app, hwnd do campo)
```
- `begin_from_button`: igual ao `begin` do atalho (mesmos bloqueios: ditado em andamento, sessão ativa), mas antes da captura garante que a janela alvo está em primeiro plano (`refocus(hwnd)`) e força o caminho do Ctrl+A (corrige a caixa inteira mesmo que haja uma seleção parcial). Escolha minha: a caixa inteira, porque o botão representa a caixa.
- Frontend do botão: círculo de 28 px com o ícone do Handy (ou um ícone simples de "✓ texto"), sombra leve, hover mais claro, `onClick` → `commands.correctionButtonClicked()`; sem texto visível (tooltip i18n "Corrigir texto").
- Tela da correção: chave "Botão flutuante nas caixas de texto" e lista editável "Não mostrar o botão nestes programas" (nomes de executável; adicionar/remover).
- O observador para de mostrar o botão enquanto `session_active`, e esconde ao fechar o app.

- [ ] **Step 1: Testes que falham** (Rust): `begin_from_button` usa os mesmos bloqueios de `should_start` (teste puro de uma função `button_click_allowed(dictation_busy, session_active) -> bool`); conversão de posição física→lógica `to_logical((x, y), scale) -> (f64, f64)` com escala 1.25.
- [ ] **Step 2–4:** falhar, implementar, passar (`cargo test`, `bun run lint`, `bun run check:translations`, `bun run build`; bindings.ts regenerado com uma execução de debug).
- [ ] **Step 5: Commit.** `feat: floating button that corrects the focused text box`

---

## Terceira rodada (2026-11-10): sem botão flutuante, Traduzir, painel com mouse, fechar por engano, dica de atalho

Spec: seção 15 do design.

### Task 19: Remover o botão flutuante

**Files:** remover `src-tauri/src/correction/button.rs`, `src-tauri/src/correction/text_focus.rs`, `src/correction-button/`; desfazer a entrada do vite, o `correction_button` em `capabilities/default.json`, a criação da janela/observador/saída em `lib.rs`, o comando `correction_button_clicked`, `begin_from_button`/`Origin::Button`/`CaptureMode::WholeField` em `session.rs`/`selection.rs` (o atalho continua: seleção primeiro, Ctrl+A se vazio), os campos `floating_button`/`button_excluded_apps` de `CorrectionSettings` (lojas antigas que ainda tenham esses campos devem continuar carregando: o serde ignora campos desconhecidos — confirmar com um teste), a seção da tela de configurações e as chaves de tradução usadas só por ela (em todas as línguas), e as features do crate `windows` que só o botão usava.

- [ ] **Step 1: Teste**
```rust
#[test] fn store_with_removed_button_fields_still_loads() {
    let v = serde_json::json!({"models_dir": "E:\\m", "floating_button": false, "button_excluded_apps": ["x.exe"]});
    let s = parse_or_salvage(v);  // ou a função pública de leitura usada hoje
    assert_eq!(s.models_dir, r"E:\m");
}
```
- [ ] **Step 2–4:** remover, `cargo test`, `bun run lint`, `bun run check:translations`, `bun run build` (bindings.ts regenerado por uma execução de debug).
- [ ] **Step 5: Commit.** `refactor: drop the floating button that most apps could not support`

### Task 20: Tom "Traduzir"

**Files:** `src-tauri/src/correction/settings.rs`, `src-tauri/src/correction/stream.rs` (teste real ignorado)

- `CURRENT_DEFAULTS_VERSION = 3`; `default_tones()` ganha no fim `CorrectionTone { id: "traduzir", name: "Traduzir", goal: <abaixo> }`; `migrate_defaults`: se `defaults_version < 3`, acrescenta "Traduzir" quando não houver tom com id `traduzir` e houver menos de 9 tons.

Objetivo:
```
Traduza o texto. Se ele estiver em português, traduza para o inglês; se já estiver em inglês, traduza para o português do Brasil. Use uma tradução natural, como uma pessoa nativa escreveria, mantendo o mesmo tom, o mesmo nível de formalidade e as mesmas ideias. Não explique a tradução.
```
- [ ] **Step 1: Testes** — `defaults_include_traduzir_last` (6 tons, último `traduzir`); `v2_store_gets_traduzir_once` (como o teste do Desenvolver); `full_tone_list_is_not_extended` continua valendo.
- [ ] **Step 4b: Teste real (ignorado) com o Gemma:** os textos do spec seção 11 pelo tom Traduzir saem em inglês (sem palavras como "você", "não", "muito"; contém palavras inglesas comuns); um texto em inglês sai em português. Sem travessão (a regra global de travessão vale também em inglês).
- [ ] **Step 5: Commit.** `feat: add a Traduzir tone for Portuguese and English`

### Task 21: Painel mais largo, mouse e atalhos de fechar/copiar

**Files:** `src-tauri/src/overlay.rs`, `src-tauri/src/correction/session.rs`, `src-tauri/src/lib.rs`, `src/overlay/correction/*`, locales

**Interfaces:**
```rust
// overlay.rs: CORRECTION_WIDTH = 760.0, CORRECTION_HEIGHT = 460.0
// SessionInput ganha: CopyKey (Ctrl+C), Shortcut (o atalho de correção apertado de novo), Ui(UiAction)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum UiAction { Apply, Copy, Cancel, Tone(u8), View(CorrectionView) }
#[tauri::command] #[specta::specta] pub fn correction_ui_action(app: AppHandle, action: UiAction);
// SessionCommand ganha: Copy, SetView(CorrectionView)
// binding temporária nova: correction_copy -> "ctrl+c" (só em Preview)
```
- `decide`: em Preview, `CopyKey`/`Ui(Copy)` → `Copy`; `Ui(Apply)` → `Apply`; `Ui(Cancel)` → `Cancel`; `Ui(Tone(n))` → `Regenerate(n-1)` se válido; `Ui(View(v))` → `SetView(v)`. Em Loading/Generating/Preview/NeedsFallback, `Shortcut` e `Ui(Cancel)` → `Cancel`. Em NeedsFallback, `Ui(Tone(n))` escolhe a opção n (igual ao dígito).
- `Copy`: põe o texto corrigido na área de transferência (de propósito, como no caso Copied), fecha o painel e mostra a pílula "Correção copiada" por 2 s.
- O atalho de correção apertado com uma sessão aberta deixa de ser ignorado e vira `SessionInput::Shortcut`.
- Frontend: rodapé do painel com botões Aplicar (destaque), Copiar e Cancelar (só em Preview; Cancelar também em Loading/Generating); a lista de tons e as abas de visão são clicáveis; rolagem com a rodinha no cartão de texto (`overflow-y: auto`, a janela aceita o mouse com `accept_first_mouse`); `onMouseDown` com `preventDefault` em todos os controles para nunca pegar foco. Margem interna do cartão 20 px, entre blocos 12 px, texto 15 px.
- [ ] **Step 1: Testes**
```rust
#[test] fn ui_actions_map_to_commands_in_preview() {
    let p = CorrectionPhase::Preview;
    assert_eq!(decide(&p, SessionInput::Ui(UiAction::Apply), 6, &[]), SessionCommand::Apply);
    assert_eq!(decide(&p, SessionInput::Ui(UiAction::Copy), 6, &[]), SessionCommand::Copy);
    assert_eq!(decide(&p, SessionInput::CopyKey, 6, &[]), SessionCommand::Copy);
    assert_eq!(decide(&p, SessionInput::Ui(UiAction::Tone(3)), 6, &[]), SessionCommand::Regenerate(2));
    assert_eq!(decide(&p, SessionInput::Ui(UiAction::Tone(9)), 6, &[]), SessionCommand::Ignore);
    assert_eq!(decide(&p, SessionInput::Ui(UiAction::View(CorrectionView::Original)), 6, &[]), SessionCommand::SetView(CorrectionView::Original));
}
#[test] fn shortcut_again_or_cancel_button_closes_any_open_phase() {
    for p in [CorrectionPhase::Loading, CorrectionPhase::Generating, CorrectionPhase::Preview, CorrectionPhase::NeedsFallback] {
        assert_eq!(decide(&p, SessionInput::Shortcut, 6, &[FallbackOption::Cpu]), SessionCommand::Cancel);
        assert_eq!(decide(&p, SessionInput::Ui(UiAction::Cancel), 6, &[FallbackOption::Cpu]), SessionCommand::Cancel);
    }
}
#[test] fn copy_key_only_bound_in_preview() { /* binding set por fase: correction_copy só em Preview */ }
```
- [ ] **Step 2–4:** falhar, implementar, passar (`cargo test`, lint, check:translations, build).
- [ ] **Step 5: Commit.** `feat: mouse controls, copy and close-again for the correction panel`

### Task 22: Fechar ao clicar fora ou voltar a digitar

**Files:** Create `src-tauri/src/correction/input_hooks.rs`; Modify `session.rs`, `lib.rs`

**Interfaces:**
```rust
pub enum HookEvent { MouseDown { x: i32, y: i32 }, TypingKey }   // TypingKey: tecla de texto não injetada
pub fn start_hooks(on_event: impl Fn(HookEvent) + Send + Sync + 'static); // thread própria com loop de mensagens,
                                                                         // WH_MOUSE_LL + WH_KEYBOARD_LL; Windows only
pub fn is_typing_key(vk: u32, injected: bool, ctrl_or_alt_down: bool) -> bool; // letras, números, espaço, pontuação, Backspace;
                                                                              // falso para injetadas, modificadores, F-keys,
                                                                              // setas, Tab, Enter, Esc, e com Ctrl/Alt
pub fn click_outside(x: i32, y: i32, panel: (i32, i32, i32, i32)) -> bool;
```
- Ganchos sempre instalados (servem também à Task 23), mas a sessão só reage enquanto aberta: em Loading/Generating/Preview/NeedsFallback, `MouseDown` fora do retângulo físico atual da janela `recording_overlay` → `Cancel`; `TypingKey` → `Cancel` (a tecla segue para o programa). As teclas que o painel usa (dígitos, Tab, setas, Enter, Esc, Ctrl+C) continuam com as bindings temporárias e não contam como digitação enquanto registradas. Eventos injetados pelo próprio app (Ctrl+C/Ctrl+A/Ctrl+V do enigo) são ignorados (`LLKHF_INJECTED`/`LLMHF_INJECTED`).
- O callback do gancho nunca bloqueia: só envia para um canal e retorna `CallNextHookEx` imediatamente.
- [ ] **Step 1: Testes** para `is_typing_key` (tabela: 'A', '1', espaço, Backspace → true; injetada, Ctrl+A, Shift sozinho, F5, seta, Enter, Esc, Tab → false) e `click_outside` (dentro, borda, fora).
- [ ] **Step 2–4:** falhar, implementar, passar.
- [ ] **Step 5: Commit.** `feat: close the correction panel on outside clicks or typing`

### Task 23: Dica de atalho ao digitar

**Files:** Create `src-tauri/src/correction/typing_hint.rs`; Modify `settings.rs` (`typing_hint: bool`, padrão true), `overlay.rs` (estado `"hint"`, ~340 × 44), `session.rs`/`lib.rs`, `src/overlay/` (pílula da dica, clicável), tela de configurações (chave "Mostrar dica do atalho ao digitar"), locales

**Interfaces:**
```rust
pub const HINT_MIN_KEYS: u32 = 12;
pub const HINT_IDLE_MS: u64 = 1500;
pub const HINT_SHOW_MS: u64 = 4000;
pub struct HintTracker { /* contagem por hwnd em primeiro plano, janelas que já viram a dica, último toque */ }
impl HintTracker {
    pub fn on_typing_key(&mut self, hwnd: isize, now_ms: u64);
    /// Chamado a cada ~250 ms; devolve Some(hwnd) quando a dica deve aparecer agora.
    pub fn poll(&mut self, now_ms: u64, blocked: bool) -> Option<isize>;
}
```
- Regras: conta só `TypingKey` (Task 22) na janela em primeiro plano; trocar de janela zera a contagem; quando `contagem >= HINT_MIN_KEYS` e passaram `HINT_IDLE_MS` sem teclas e `blocked == false` e a janela ainda não viu a dica → mostra e marca a janela. `blocked` = dica desligada, tela cheia, ditado em andamento, correção aberta ou janela do próprio Handy Plus.
- A dica: pílula no lugar da pílula do Handy (estado `"hint"`), texto `"<atalho> para melhorar o texto"` com o atalho atual formatado (ex.: "Ctrl + \"), some após `HINT_SHOW_MS` ou na próxima `TypingKey`; clique → mesmo caminho do atalho (`session::begin`), com o foco ainda no programa do usuário (a janela da pílula não pega foco). Não toca som.
- [ ] **Step 1: Testes** do `HintTracker`: não mostra antes de 12 teclas; não mostra antes de 1,5 s parado; mostra uma vez por janela; trocar de janela zera; `blocked` impede e não marca a janela.
- [ ] **Step 2–4:** falhar, implementar, passar (`cargo test`, lint, check:translations, build).
- [ ] **Step 5: Commit.** `feat: show a typing hint with the correction shortcut`

---

## Quarta rodada (2026-11-10): Aplicar, estilos e ajustes, atalho de tradução

Spec: seção 16 do design.

### Task 24: Aplicar cola com Ctrl + V e registra o resultado

**Files:** `src-tauri/src/clipboard.rs`, `src-tauri/src/correction/session.rs`
- `correction_paste_method(_: PasteMethod) -> PasteMethod` passa a devolver sempre `PasteMethod::CtrlV` (a correção é texto puro). Atualizar o teste existente para a regra nova.
- `Session::apply` registra `info!` com o resultado: "pasted", "refocus failed, left on clipboard" ou o erro (sem o texto).
- [ ] Teste: `correction_always_pastes_with_ctrl_v` cobre todas as variantes de `PasteMethod`.
- [ ] Commit: `fix: paste corrections with Ctrl+V so every app accepts them`

### Task 25: Estilos e ajustes nas configurações (modelo de dados e migração)

**Files:** `src-tauri/src/correction/settings.rs` (+ teste real ignorado em `stream.rs`)
- `CorrectionTone` ganha `kind: ToneKind` (`#[serde(default)]`, padrão `Style`); `enum ToneKind { Style, Adjust }` (snake_case).
- `default_tones()`: estilos `meu_estilo` "Meu estilo", `mais_formal` "Formal", `descolado` "Descolado", `poetico` "Poético"; ajustes `mais_curto` "Encurtar", `desenvolver` "Desenvolver", `traduzir` "Traduzir", `melhorar_argumentos` "Melhorar argumentos" (ids antigos mantidos para a migração reconhecer).
- Objetivos novos:
  - Descolado: `Reescreva com um tom descontraído e leve, como numa conversa entre amigos: pode usar gírias comuns e expressões do dia a dia, sem exagerar e sem ficar forçado. Mantenha as ideias e a opinião do usuário.`
  - Poético: `Reescreva com um toque poético: escolha imagens e comparações bonitas, cuide do ritmo das frases e deixe o texto mais expressivo, sem perder o sentido nem inventar fatos. Nada de rimas forçadas.`
  - Os ajustes recebem o texto já processado: os objetivos de Encurtar, Desenvolver, Traduzir e Melhorar argumentos continuam iguais aos atuais.
- `CURRENT_DEFAULTS_VERSION = 4`; migração `< 4`: dá `kind` aos ids conhecidos (`mais_curto`, `desenvolver`, `traduzir`, `melhorar_argumentos` → Adjust; os outros → Style); renomeia "Mais formal"→"Formal" e "Mais curto"→"Encurtar" só se o nome ainda for o padrão antigo; acrescenta `descolado` e `poetico` (logo após os estilos existentes) se ausentes e se houver espaço (limite 9 por tipo).
- Correção de bug: o limite de 9 passa a ser por tipo (`MAX_PER_KIND = 9`).
- [ ] Testes: padrões (4 estilos, 4 ajustes, ordem); migração v3→v4 (tipos, renomeia só nomes padrão, acrescenta os dois estilos uma vez, não ressuscita apagados, idempotente); loja antiga sem `kind` carrega como Style.
- [ ] Teste real (ignorado) com o Gemma: Descolado e Poético num texto do spec; sem travessão; Poético diferente do original de forma visível.
- [ ] Commit: `feat: split tones into styles and adjustments`

### Task 26: Sessão espera a escolha, ajustes encadeados e atalho de tradução

**Files:** `src-tauri/src/correction/session.rs`, `src-tauri/src/settings.rs` (binding `translate_selection`), `src-tauri/src/actions.rs`, `src-tauri/src/lib.rs`
- Nova fase `CorrectionPhase::Choosing` (painel aberto com o original e as opções; nada gerado). O atalho de correção abre em Choosing e dispara o carregamento do modelo local em segundo plano (sem gerar); se ainda estiver carregando quando o usuário escolher, mostra Loading normalmente.
- Lista numerada única: `options = estilos ++ ajustes` (números 1..9 na ordem; acima de 9 só clique). `CorrectionUpdateEvent` passa a levar `styles: Vec<String>`, `adjusts: Vec<String>`, `current_style: Option<u32>`, `applied_adjusts: Vec<u32>` (no lugar de `tones`/`tone_index`).
- `decide`: em Choosing, Digit/Ui(Tone) de um estilo → `RunStyle(i)`; de um ajuste → `RunAdjust(i)` (sobre o original); Enter → `RunStyle(0)`; Esc/Shortcut/Dismiss/Cancel → Cancel. Em Preview: estilo → `RunStyle(i)` (do original, zera ajustes); ajuste → `RunAdjust(i)` (sobre o texto atual); Enter → Apply; resto como hoje.
- Geração de ajuste: `build_messages(rules, goal_do_ajuste, texto_atual)`.
- Novo binding `translate_selection` (nome "Translate Selection"), padrão `ctrl+shift+section` no Windows; ação abre a sessão já rodando o ajuste `traduzir` sobre o texto capturado (se o ajuste não existir, usa o objetivo padrão de Traduzir).
- [ ] Testes de `decide` para Choosing e Preview (estilos, ajustes, Enter, números fora do intervalo), do mapeamento número→opção com mais de 9 itens, e do binding padrão de tradução (parse pelo handy-keys).
- [ ] Commit: `feat: let the user pick a style first and chain adjustments`

### Task 27: Painel e configurações para estilos e ajustes

**Files:** `src/overlay/correction/*`, `src/components/settings/correction/*`, locales
- Painel em Choosing: o texto original no cartão, dica "Escolha um estilo (Enter = Meu estilo)", duas colunas ou dois grupos: **Estilo** (1–4) e **Ajustes** (5–8), clicáveis; barra de carregamento discreta se o modelo ainda estiver carregando.
- Em Preview: os mesmos dois grupos, com o estilo atual destacado e os ajustes aplicados marcados com ✓ na ordem; rodapé Aplicar / Copiar / Cancelar como hoje.
- Configurações: duas listas editáveis (Estilos, Ajustes), cada uma com adicionar/apagar/subir/descer, nome + objetivo; o campo de atalho de tradução (`ShortcutInput shortcutId="translate_selection"`).
- i18n en + pt (sem travessão), inglês nas outras línguas.
- [ ] `bun run lint`, `bun run check:translations`, `bun run build`; bindings regenerados.
- [ ] Commit: `feat: style and adjustment controls in the panel and settings`

---

## Quinta rodada (2026-11-10)

Spec: seção 17 do design.

### Task 28: Ajustes exclusivos, memória das respostas, variação e sem a visão Mudanças

**Files:** `src-tauri/src/correction/session.rs`, `src/overlay/correction/*`, locales
- `CorrectionView` passa a ter só `Corrected` e `Original` (`next()` alterna); remover a visão Changes e o `diff` do painel (manter `diff.ts` só se ainda usado; senão remover com o teste).
- Estado da sessão: `current_style: Option<usize>`, `current_adjust: Option<usize>` (no lugar de `applied_adjusts: Vec`), `lang` (Task 29 usa; aqui o idioma é sempre o original), e um cache `HashMap<ResultKey, Vec<String>>` com `ResultKey { style: Option<usize>, adjust: Option<usize>, lang: Lang }` e um índice de variação atual por chave.
- Regras (função pura `plan_choice(state, choice) -> Plan`, testada):
  - Escolher estilo S: se S já é o atual e não há ajuste → `NewVariation`; senão o alvo vira (S, nenhum ajuste) → `Show(cached)` se existir, senão `Generate(from original)`.
  - Escolher ajuste A: se A já é o atual → `NewVariation`; senão o alvo vira (estilo atual, A) → cache ou `Generate(from resultado do estilo atual, ou do original se não houver estilo)`.
  - `NewVariation` gera de novo a mesma chave a partir da mesma base e acrescenta à lista; mostra a nova.
- `CorrectionUpdateEvent`: `current_adjust: Option<u32>` no lugar de `applied_adjusts`; o painel marca só um ajuste.
- [ ] Testes de `plan_choice` (troca de ajuste não acumula; voltar a uma combinação usa o cache; repetir a opção atual pede variação; estilo novo zera o ajuste; ajuste sem estilo usa o original).
- [ ] Commit: `feat: exclusive adjustments, cached results and variations`

### Task 29: Chave de idioma PT ⇄ EN

**Files:** `src-tauri/src/correction/{session,settings,text}.rs`, `src/overlay/correction/*`, locales
- `enum Lang { Pt, En }`; `fn detect_lang(text: &str) -> Lang` com o crate `whatlang` (já é dependência): inglês → En, qualquer outro → Pt.
- Sessão começa com `lang = detect_lang(original)`; `SessionInput::ToggleLang` (tecla `t` como binding temporária em Choosing/Preview, e `UiAction::ToggleLang`) troca o idioma alvo: o texto mostrado passa a ser a chave atual com o novo idioma — do cache, ou gerado traduzindo o texto da chave no idioma original com o objetivo de tradução (`TRANSLATE_GOAL_TO_EN` / `TRANSLATE_GOAL_TO_PT`, textos fixos no código, naturais, mantendo tom; o de inglês diz explicitamente que a resposta é toda em inglês e que é exceção à regra de português).
- `CorrectionUpdateEvent` ganha `lang: Lang` e `source_lang: Lang`; o painel mostra a chave (PT / EN) com o lado ativo destacado, clicável.
- Migração `CURRENT_DEFAULTS_VERSION = 5`: remove o tom `traduzir` se ainda tiver o objetivo padrão; se foi editado, vira ajuste comum. `default_tones()` deixa de ter Traduzir.
- O atalho `translate_selection` abre a sessão já com o idioma virado para o oposto do detectado e gera a tradução do original (sem estilo).
- [ ] Testes: `detect_lang` (frases pt e en do spec), migração v4→v5 (remove padrão, mantém editado, idempotente), `decide` para ToggleLang em Choosing/Preview e ignorado nas outras fases.
- [ ] Teste real (ignorado) com o Gemma: as frases do spec traduzidas pelos objetivos novos, sem travessão.
- [ ] Commit: `feat: language switch instead of the Traduzir adjustment`

### Task 30: Captura espera a área de transferência assentar

**Files:** `src-tauri/src/correction/selection.rs`
- Depois que o texto aparece, continuar consultando a sequência da área de transferência a cada 25 ms e só ler quando ela ficar parada por `SETTLE_MS = 150` (respeitando o limite total atual). Função pura `settled(last_change_ms, now_ms) -> bool` testada.
- `info!("Captured {} chars (whole_field={})")` sem conteúdo.
- [ ] Commit: `fix: wait for the clipboard to settle before reading the selection`

### Task 31: Pré-carregar o modelo ao digitar

**Files:** `src-tauri/src/correction/{typing_hint,session,settings}.rs`, tela de configurações, locales
- `CorrectionSettings.preload_on_typing: bool` (padrão true, default por campo).
- O rastreador da dica (Task 23) ganha um segundo gatilho: ao atingir `HINT_MIN_KEYS` teclas numa janela (sem esperar a pausa), se `preload_on_typing`, provedor local com modelo escolhido, motor não carregado, sem tela cheia, sem ditado, sem sessão aberta e `has_room(free_vram, tamanho do modelo)`, dispara `ensure_loaded` + aquecimento em segundo plano (uma vez por janela; nunca dois ao mesmo tempo; reaproveita o carregamento se a sessão abrir no meio). O vigia da placa e o tempo de descarregar continuam valendo.
- Função pura `should_preload(...) -> bool` testada.
- Chave "Pré-carregar o modelo quando eu começar a digitar" na tela da correção.
- [ ] Commit: `feat: preload the correction model while the user types`
