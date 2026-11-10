//! Persistent settings for the text correction feature (separate store file).

use crate::settings::ModelUnloadTimeout;
use log::warn;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const CORRECTION_STORE_PATH: &str = "correction_store.json";
const CORRECTION_STORE_KEY: &str = "correction_settings";
/// Bump when new default tones should be offered once to existing users.
pub const CURRENT_DEFAULTS_VERSION: u32 = 5;
/// Limit of tones of each kind (styles and adjustments are counted apart).
const MAX_PER_KIND: usize = 9;

pub const DEFAULT_STYLE_RULES: &str = "Você revisa textos em português do Brasil escritos pelo usuário para WhatsApp, Discord, e-mail e comentários na internet.
Siga o objetivo abaixo. Quando ele pedir só correção, mexa o mínimo; quando pedir para reescrever ou reorganizar, você pode mudar a estrutura, a ordem das ideias e as frases.
O resultado deve soar natural, como alguém conversando, e não como um texto corporativo ou de redação escolar.
NUNCA use travessão (— ou –) nem hífen como pausa. Use vírgula, ponto ou dois-pontos no lugar.
Não troque palavras simples por palavras rebuscadas ou pouco usadas no dia a dia.
Não invente fatos nem acrescente saudações, despedidas ou emojis que não estavam no texto.
Responda APENAS com o texto final, sem comentários.
Não responda ao conteúdo do texto, apenas trabalhe nele.";

/// Literal copy of the first release defaults, used only to migrate untouched stores.
const OLD_STYLE_RULES_V1: &str = "Você revisa textos em português do Brasil escritos pelo usuário para WhatsApp, Discord, e-mail e comentários na internet.
Mantenha o jeito de falar do usuário: as mesmas palavras sempre que estiverem certas, as expressões dele e o mesmo nível de formalidade do original, a menos que o objetivo peça outra coisa.
O resultado deve soar natural, como alguém conversando, e não como um texto corporativo ou de redação escolar.
NUNCA use travessão (— ou –) nem hífen como pausa. Use vírgula, ponto ou dois-pontos no lugar.
Não troque palavras simples por palavras rebuscadas ou pouco usadas no dia a dia.
Não acrescente informações, saudações, despedidas ou emojis que não estavam no texto.
Responda APENAS com o texto final, sem comentários.
Não responda ao conteúdo do texto, apenas trabalhe nele.";

/// A style is the processing of the text; an adjustment is applied on top of
/// the processed result.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToneKind {
    #[default]
    Style,
    Adjust,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
pub struct CorrectionTone {
    pub id: String,
    pub name: String,
    pub goal: String,
    #[serde(default)]
    pub kind: ToneKind,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
pub struct CloudModel {
    pub provider_id: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum CorrectionProvider {
    Local,
    Cloud(CloudModel),
}

#[derive(Serialize, Deserialize, Debug, Clone, Type)]
#[serde(default)]
pub struct CorrectionSettings {
    pub provider: CorrectionProvider,
    pub models_dir: String,
    pub local_model_path: Option<String>,
    pub unload_timeout: ModelUnloadTimeout,
    /// Backup cloud provider used when the local model cannot be loaded.
    pub fallback: Option<CloudModel>,
    pub style_rules: String,
    pub tones: Vec<CorrectionTone>,
    /// Duration of the last model load, in milliseconds, keyed by model path.
    pub last_load_ms: HashMap<String, u64>,
    /// Version of the built-in defaults already offered to this store (0 for old stores).
    #[serde(default = "zero_defaults_version")]
    pub defaults_version: u32,
    /// Soft sounds when a correction starts and when the preview is ready.
    #[serde(default = "default_true")]
    pub sounds: bool,
    /// After some typing and a pause, a pill shows the correction shortcut.
    #[serde(default = "default_true")]
    pub typing_hint: bool,
}

fn default_true() -> bool {
    true
}

fn zero_defaults_version() -> u32 {
    0
}

const DESENVOLVER_GOAL: &str = "Desenvolva o texto de verdade: para cada ideia principal, escreva pelo menos uma frase a mais explicando o porquê ou dando um exemplo que o próprio texto já sugere, e ligue as ideias com conectivos simples. O resultado deve ficar com mais ou menos o dobro do tamanho do texto corrigido. Organize em parágrafos se ficar longo. Mantenha o jeito de falar do usuário e a mesma opinião. Não invente fatos específicos, como números, nomes ou datas, que não estavam no texto.";

/// Goal of the Traduzir adjustment (versions 3 and 4), replaced in version 5
/// by the PT / EN switch. Kept only to recognize an untouched copy.
const OLD_TRADUZIR_GOAL: &str = "Traduza o texto. Se ele estiver em português, traduza para o inglês; se já estiver em inglês, traduza para o português do Brasil. Use uma tradução natural, como uma pessoa nativa escreveria, mantendo o mesmo tom, o mesmo nível de formalidade e as mesmas ideias. Não explique a tradução. Esta tarefa é uma tradução, não uma correção, e é a exceção às regras acima sobre o idioma: se o texto estiver em português, a resposta tem que ser escrita inteira em inglês (English), sem nenhuma palavra em português; se estiver em inglês, a resposta tem que ser escrita inteira em português do Brasil. Nunca devolva o texto no mesmo idioma em que ele chegou.";

const DESCOLADO_GOAL: &str = "Reescreva como se fosse uma mensagem de WhatsApp para um amigo próximo: bem informal e solto, não um texto corrigido. Troque as palavras mais certinhas por versões coloquiais do dia a dia (\"tá\", \"pra\", \"pro\", \"né\", \"cara\", \"massa\", \"de boa\", \"sinceramente\", \"bora\", \"tipo\") em pelo menos três lugares, quebre em frases curtas e diretas, e coloque uma interjeição ou exclamação leve (como \"olha\", \"cara\" ou \"poxa\") onde combinar. Corrija a ortografia, mantenha as mesmas ideias e a opinião do usuário e não invente fatos.";

const POETICO_GOAL: &str = "Reescreva com um toque poético: escolha imagens e comparações bonitas, cuide do ritmo das frases e deixe o texto mais expressivo, sem perder o sentido nem inventar fatos. Nada de rimas forçadas.";

/// Tones added after the first release: (defaults version that introduced it, id).
/// Traduzir (version 3) is not here: version 5 dropped it.
const TONES_ADDED_BY_VERSION: [(u32, &str); 3] =
    [(2, "desenvolver"), (4, "descolado"), (4, "poetico")];

const TRANSLATE_ID: &str = "traduzir";

/// Ids whose tone is an adjustment (version 4 split tones into two kinds).
const ADJUST_IDS: [&str; 4] = [
    "mais_curto",
    "desenvolver",
    "traduzir",
    "melhorar_argumentos",
];

/// Names the first releases gave to tones that were renamed in version 4: (id, old name, new name).
const RENAMED_IN_V4: [(&str, &str, &str); 2] = [
    ("mais_formal", "Mais formal", "Formal"),
    ("mais_curto", "Mais curto", "Encurtar"),
];

pub fn default_tones() -> Vec<CorrectionTone> {
    let tone = |id: &str, name: &str, goal: &str, kind: ToneKind| CorrectionTone {
        id: id.to_string(),
        name: name.to_string(),
        goal: goal.to_string(),
        kind,
    };
    use ToneKind::{Adjust, Style};
    vec![
        tone(
            "meu_estilo",
            "Meu estilo",
            "Corrija ortografia, acentuação, concordância, pontuação e vírgulas, e deixe as frases coerentes e fáceis de ler. Mantenha as mesmas palavras sempre que estiverem certas, as expressões do usuário e o mesmo nível de formalidade. Não deixe o texto mais longo do que precisa.",
            Style,
        ),
        tone(
            "mais_formal",
            "Formal",
            "Reescreva com tom formal e educado, como num e-mail de trabalho: troque gírias e abreviações por palavras comuns e corretas, use frases completas e organize em parágrafos se precisar. Sem ficar engessado nem usar palavras difíceis.",
            Style,
        ),
        tone("descolado", "Descolado", DESCOLADO_GOAL, Style),
        tone("poetico", "Poético", POETICO_GOAL, Style),
        tone(
            "mais_curto",
            "Encurtar",
            "Reescreva bem mais curto: corte pelo menos metade do texto, junte as frases em poucas frases, tire repetições, rodeios e detalhes que não mudam o sentido, mantendo os fatos e a opinião do usuário.",
            Adjust,
        ),
        tone("desenvolver", "Desenvolver", DESENVOLVER_GOAL, Adjust),
        tone(
            "melhorar_argumentos",
            "Melhorar argumentos",
            "Reorganize o texto para convencer melhor: comece pela ideia principal, coloque os argumentos numa ordem lógica, junte ou separe frases quando ajudar, ligue as ideias com conectivos simples e termine com uma conclusão clara. Pode mudar bastante a estrutura, mas não invente fatos nem mude a opinião do usuário.",
            Adjust,
        ),
    ]
}

/// Literal copy of the first release tones (id, name, goal), used only to migrate untouched stores.
const OLD_TONE_GOALS_V1: [(&str, &str, &str); 4] = [
        ("meu_estilo", "Meu estilo", "Corrija ortografia, acentuação, concordância, pontuação e vírgulas, e deixe as frases coerentes e fáceis de ler. Não deixe o texto mais longo do que precisa."),
        ("mais_formal", "Mais formal", "Corrija o texto e deixe o tom mais formal, como num e-mail de trabalho, sem ficar engessado."),
        ("mais_curto", "Mais curto", "Corrija o texto e deixe ele mais curto e direto, sem perder nenhuma informação."),
        ("melhorar_argumentos", "Melhorar argumentos", "Corrija o texto e deixe os argumentos mais claros e convincentes, na ordem mais lógica, sem inventar fatos."),
];

#[cfg(test)]
fn old_tones_v1() -> Vec<CorrectionTone> {
    OLD_TONE_GOALS_V1
        .iter()
        .map(|(id, name, goal)| CorrectionTone {
            id: id.to_string(),
            name: name.to_string(),
            goal: goal.to_string(),
            kind: ToneKind::Style,
        })
        .collect()
}

/// Replaces the first release defaults with the current ones when the user
/// never edited them. Edited rules or tone goals are left untouched.
pub fn migrate_defaults(s: &mut CorrectionSettings) -> bool {
    let mut changed = false;
    if s.style_rules == OLD_STYLE_RULES_V1 {
        s.style_rules = DEFAULT_STYLE_RULES.to_string();
        changed = true;
    }
    let new_tones = default_tones();
    for tone in &mut s.tones {
        let is_old = OLD_TONE_GOALS_V1
            .iter()
            .any(|(id, _, goal)| tone.id == *id && tone.goal == *goal);
        if !is_old {
            continue;
        }
        if let Some(new) = new_tones.iter().find(|t| t.id == tone.id) {
            tone.goal = new.goal.clone();
            changed = true;
        }
    }
    if s.defaults_version < CURRENT_DEFAULTS_VERSION {
        let from = s.defaults_version;
        if from < 4 {
            for tone in &mut s.tones {
                if ADJUST_IDS.contains(&tone.id.as_str()) {
                    tone.kind = ToneKind::Adjust;
                }
                if let Some((_, _, new_name)) = RENAMED_IN_V4
                    .iter()
                    .find(|(id, old, _)| tone.id == *id && tone.name == *old)
                {
                    tone.name = new_name.to_string();
                }
            }
        }
        for (version, id) in TONES_ADDED_BY_VERSION {
            if from >= version || s.tones.iter().any(|t| t.id == id) {
                continue;
            }
            let Some(tone) = new_tones.iter().find(|t| t.id == id) else {
                continue;
            };
            let of_kind = s.tones.iter().filter(|t| t.kind == tone.kind).count();
            if of_kind >= MAX_PER_KIND {
                continue;
            }
            // New tones go after the last tone of their kind (adjustments at the end).
            let at = s
                .tones
                .iter()
                .rposition(|t| t.kind == tone.kind)
                .map_or(s.tones.len(), |i| i + 1);
            s.tones.insert(at, tone.clone());
        }
        if from < 5 {
            // The PT / EN switch replaces Traduzir; an edited copy stays as a
            // plain adjustment.
            s.tones
                .retain(|t| !(t.id == TRANSLATE_ID && t.goal == OLD_TRADUZIR_GOAL));
        }
        s.defaults_version = CURRENT_DEFAULTS_VERSION;
        changed = true;
    }
    changed
}

impl Default for CorrectionSettings {
    fn default() -> Self {
        Self {
            provider: CorrectionProvider::Local,
            models_dir: r"D:\IA\LLM\model".to_string(),
            local_model_path: None,
            unload_timeout: ModelUnloadTimeout::Min5,
            fallback: None,
            style_rules: DEFAULT_STYLE_RULES.to_string(),
            tones: default_tones(),
            last_load_ms: HashMap::new(),
            defaults_version: CURRENT_DEFAULTS_VERSION,
            sounds: true,
            typing_hint: true,
        }
    }
}

pub fn get_correction_settings(app: &AppHandle) -> CorrectionSettings {
    let store = app
        .store(crate::portable::store_path(CORRECTION_STORE_PATH))
        .expect("Failed to initialize correction store");
    match store.get(CORRECTION_STORE_KEY) {
        Some(value) => {
            let mut settings = parse_or_salvage(&value);
            if migrate_defaults(&mut settings) {
                write_correction_settings(app, &settings);
            }
            settings
        }
        None => CorrectionSettings::default(),
    }
}

/// Parses stored settings; if the whole value is invalid, keeps every field
/// that is individually valid and defaults only the broken ones.
fn parse_or_salvage(stored: &serde_json::Value) -> CorrectionSettings {
    match serde_json::from_value::<CorrectionSettings>(stored.clone()) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to parse correction settings ({e}); salvaging valid fields");
            salvage(stored)
        }
    }
}

fn salvage(stored: &serde_json::Value) -> CorrectionSettings {
    let Some(stored_map) = stored.as_object() else {
        warn!("Stored correction settings are not a JSON object; using defaults");
        return CorrectionSettings::default();
    };
    let mut merged =
        serde_json::to_value(CorrectionSettings::default()).expect("defaults serialize");
    if !stored_map.contains_key("defaults_version") {
        merged
            .as_object_mut()
            .expect("merged stays an object")
            .insert("defaults_version".into(), 0.into());
    }
    for (key, value) in stored_map {
        let map = merged.as_object_mut().expect("merged stays an object");
        let previous = map.insert(key.clone(), value.clone());
        if serde_json::from_value::<CorrectionSettings>(merged.clone()).is_err() {
            warn!("Dropping invalid correction settings field '{key}', keeping its default");
            let map = merged.as_object_mut().expect("merged stays an object");
            match previous {
                Some(previous) => map.insert(key.clone(), previous),
                None => map.remove(key),
            };
        }
    }
    serde_json::from_value(merged).unwrap_or_default()
}

pub fn write_correction_settings(app: &AppHandle, s: &CorrectionSettings) {
    let store = app
        .store(crate::portable::store_path(CORRECTION_STORE_PATH))
        .expect("Failed to initialize correction store");
    store.set(
        CORRECTION_STORE_KEY,
        serde_json::to_value(s).expect("correction settings serialize"),
    );
}

#[tauri::command]
#[specta::specta]
pub fn get_correction_settings_cmd(app: AppHandle) -> CorrectionSettings {
    get_correction_settings(&app)
}

#[tauri::command]
#[specta::specta]
pub fn set_correction_settings_cmd(
    app: AppHandle,
    settings: CorrectionSettings,
) -> Result<(), String> {
    write_correction_settings(&app, &settings);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted_by_id(mut tones: Vec<CorrectionTone>) -> Vec<CorrectionTone> {
        tones.sort_by(|a, b| a.id.cmp(&b.id));
        tones
    }

    fn names(tones: &[CorrectionTone]) -> Vec<&str> {
        tones.iter().map(|t| t.name.as_str()).collect()
    }

    fn tone(id: &str, name: &str, kind: ToneKind) -> CorrectionTone {
        CorrectionTone {
            id: id.into(),
            name: name.into(),
            goal: "g".into(),
            kind,
        }
    }

    #[test]
    fn default_tones_are_four_styles_then_three_adjustments() {
        let t = default_tones();
        assert_eq!(
            names(&t),
            [
                "Meu estilo",
                "Formal",
                "Descolado",
                "Poético",
                "Encurtar",
                "Desenvolver",
                "Melhorar argumentos"
            ]
        );
        let ids: Vec<_> = t.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "meu_estilo",
                "mais_formal",
                "descolado",
                "poetico",
                "mais_curto",
                "desenvolver",
                "melhorar_argumentos"
            ]
        );
        assert!(t[..4].iter().all(|t| t.kind == ToneKind::Style));
        assert!(t[4..].iter().all(|t| t.kind == ToneKind::Adjust));
    }

    #[test]
    fn new_style_goals_are_the_approved_texts() {
        let t = default_tones();
        assert!(t[2]
            .goal
            .starts_with("Reescreva como se fosse uma mensagem de WhatsApp"));
        assert!(t[3].goal.starts_with("Reescreva com um toque poético"));
        assert!(t[3].goal.ends_with("Nada de rimas forçadas."));
    }

    #[test]
    fn v3_store_gets_kinds_new_names_and_two_new_styles_once() {
        let mut old = default_tones();
        old.retain(|t| t.id != "descolado" && t.id != "poetico");
        for t in &mut old {
            t.kind = ToneKind::Style;
            if t.id == "mais_formal" {
                t.name = "Mais formal".into();
            }
            if t.id == "mais_curto" {
                t.name = "Mais curto".into();
            }
        }
        let mut s = CorrectionSettings {
            tones: old,
            defaults_version: 3,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.defaults_version, 5);
        let kind = |id: &str| s.tones.iter().find(|t| t.id == id).unwrap().kind;
        for id in ["mais_curto", "desenvolver", "melhorar_argumentos"] {
            assert_eq!(kind(id), ToneKind::Adjust, "{id}");
        }
        for id in ["meu_estilo", "mais_formal", "descolado", "poetico"] {
            assert_eq!(kind(id), ToneKind::Style, "{id}");
        }
        assert_eq!(
            names(&s.tones),
            [
                "Meu estilo",
                "Formal",
                "Descolado",
                "Poético",
                "Encurtar",
                "Desenvolver",
                "Melhorar argumentos"
            ]
        );
        // Idempotent, and a deleted new style does not come back.
        assert!(!migrate_defaults(&mut s));
        s.tones.retain(|t| t.id != "descolado");
        assert!(!migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 6);
    }

    #[test]
    fn migration_renames_only_untouched_default_names() {
        let mut s = CorrectionSettings {
            tones: vec![
                tone("mais_formal", "Super formal", ToneKind::Style),
                tone("mais_curto", "Mais curto", ToneKind::Style),
            ],
            defaults_version: 3,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        let name = |id: &str| s.tones.iter().find(|t| t.id == id).unwrap().name.clone();
        assert_eq!(name("mais_formal"), "Super formal");
        assert_eq!(name("mais_curto"), "Encurtar");
    }

    #[test]
    fn new_styles_go_right_after_the_existing_styles() {
        let mut s = CorrectionSettings {
            tones: vec![
                tone("meu_estilo", "Meu estilo", ToneKind::Style),
                tone("meu_custom", "Custom", ToneKind::Style),
                tone("mais_curto", "Mais curto", ToneKind::Style),
            ],
            defaults_version: 3,
            ..Default::default()
        };
        migrate_defaults(&mut s);
        let ids: Vec<_> = s.tones.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "meu_estilo",
                "meu_custom",
                "descolado",
                "poetico",
                "mais_curto"
            ]
        );
    }

    #[test]
    fn old_store_gets_the_new_tones_once() {
        let mut s = CorrectionSettings {
            tones: old_tones_v1(),
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 7);
        assert_eq!(s.defaults_version, 5);
        s.tones.pop();
        s.tones.pop();
        assert!(!migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 5);
    }

    #[test]
    fn limit_is_nine_per_kind() {
        // Nine styles: no room for the two new styles.
        let mut tones: Vec<_> = (0..9)
            .map(|i| tone(&format!("s{i}"), "x", ToneKind::Style))
            .collect();
        tones.push(tone("mais_curto", "Mais curto", ToneKind::Style));
        let mut s = CorrectionSettings {
            tones,
            defaults_version: 3,
            ..Default::default()
        };
        migrate_defaults(&mut s);
        let styles = s.tones.iter().filter(|t| t.kind == ToneKind::Style).count();
        assert_eq!(styles, 9);
        assert!(!s.tones.iter().any(|t| t.id == "descolado"));
        assert_eq!(s.defaults_version, 5);

        // Nine adjustments do not block the new styles.
        let mut tones: Vec<_> = (0..9)
            .map(|i| tone(&format!("a{i}"), "x", ToneKind::Adjust))
            .collect();
        tones.push(tone("meu_estilo", "Meu estilo", ToneKind::Style));
        let mut s = CorrectionSettings {
            tones,
            defaults_version: 3,
            ..Default::default()
        };
        migrate_defaults(&mut s);
        assert!(s.tones.iter().any(|t| t.id == "descolado"));
        assert!(s.tones.iter().any(|t| t.id == "poetico"));
        assert_eq!(s.tones.len(), 12);
    }

    #[test]
    fn old_store_without_kind_loads_as_style() {
        let s: CorrectionSettings = serde_json::from_value(serde_json::json!({
            "tones": [{"id": "x", "name": "Meu", "goal": "g"}]
        }))
        .unwrap();
        assert_eq!(s.tones[0].kind, ToneKind::Style);
        let salvaged = parse_or_salvage(&serde_json::json!({
            "unload_timeout": "nope",
            "tones": [{"id": "x", "name": "Meu", "goal": "g"}]
        }));
        assert_eq!(salvaged.tones[0].kind, ToneKind::Style);
        assert_eq!(
            serde_json::to_value(ToneKind::Adjust).unwrap(),
            serde_json::json!("adjust")
        );
    }

    #[test]
    fn store_without_version_field_reads_as_zero() {
        let s: CorrectionSettings = serde_json::from_str(r#"{"models_dir":"E:\\m"}"#).unwrap();
        assert_eq!(s.defaults_version, 0);
        // Salvage path (one invalid field) must also treat a missing version as 0.
        let salvaged = parse_or_salvage(&serde_json::json!({"unload_timeout": "nope"}));
        assert_eq!(salvaged.defaults_version, 0);
        assert_eq!(
            CorrectionSettings::default().defaults_version,
            CURRENT_DEFAULTS_VERSION
        );
    }

    #[test]
    fn default_rules_forbid_dashes_and_fancy_words() {
        assert!(DEFAULT_STYLE_RULES.contains("NUNCA use travessão"));
        assert!(DEFAULT_STYLE_RULES.contains("palavras rebuscadas"));
    }

    #[test]
    fn defaults_use_local_provider_and_user_models_dir() {
        let s = CorrectionSettings::default();
        assert_eq!(s.provider, CorrectionProvider::Local);
        assert_eq!(s.models_dir, r"D:\IA\LLM\model");
        assert_eq!(s.unload_timeout, ModelUnloadTimeout::Min5);
    }

    #[test]
    fn settings_missing_fields_fall_back_to_defaults() {
        let s: CorrectionSettings = serde_json::from_str(r#"{"models_dir":"E:\\m"}"#).unwrap();
        assert_eq!(s.models_dir, r"E:\m");
        assert_eq!(s.tones.len(), 7);
    }

    #[test]
    fn one_invalid_field_keeps_the_other_stored_fields() {
        let stored = serde_json::json!({
            "unload_timeout": "nope",
            "models_dir": "E:\\m",
            "tones": [{"id": "x", "name": "Meu", "goal": "g"}],
        });
        let s = parse_or_salvage(&stored);
        assert_eq!(s.unload_timeout, ModelUnloadTimeout::Min5);
        assert_eq!(s.models_dir, r"E:\m");
        assert_eq!(s.tones.len(), 1);
        assert_eq!(s.tones[0].name, "Meu");
    }

    #[test]
    fn migrates_untouched_old_defaults() {
        let mut s = CorrectionSettings {
            style_rules: OLD_STYLE_RULES_V1.into(),
            tones: old_tones_v1(),
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.style_rules, DEFAULT_STYLE_RULES);
        assert_eq!(sorted_by_id(s.tones), sorted_by_id(default_tones()));
    }

    #[test]
    fn keeps_user_edited_rules_and_tones() {
        let mut tones = old_tones_v1();
        tones[1].goal = "meu objetivo".into();
        let mut s = CorrectionSettings {
            style_rules: "minhas regras".into(),
            tones,
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.style_rules, "minhas regras");
        assert_eq!(s.tones[1].goal, "meu objetivo");
        let melhorar = s.tones.iter().find(|t| t.id == "melhorar_argumentos");
        assert_eq!(melhorar.unwrap().goal, default_tones().last().unwrap().goal);
    }

    #[test]
    fn old_goal_with_a_different_id_is_untouched() {
        let mut tones = old_tones_v1();
        tones[2].id = "outro".into();
        let goal = tones[2].goal.clone();
        let mut s = CorrectionSettings {
            style_rules: "minhas regras".into(),
            tones,
            ..Default::default()
        };
        migrate_defaults(&mut s);
        assert_eq!(s.tones[2].goal, goal);
    }

    #[test]
    fn second_migration_changes_nothing() {
        let mut s = CorrectionSettings {
            style_rules: OLD_STYLE_RULES_V1.into(),
            tones: old_tones_v1(),
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert!(!migrate_defaults(&mut s));
    }

    #[test]
    fn edited_rules_with_old_tones_only_changes_tones() {
        let mut s = CorrectionSettings {
            style_rules: "minhas regras".into(),
            tones: old_tones_v1(),
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.style_rules, "minhas regras");
        assert_eq!(sorted_by_id(s.tones), sorted_by_id(default_tones()));
    }

    #[test]
    fn old_rules_with_edited_tones_only_changes_rules() {
        let edited = vec![CorrectionTone {
            id: "meu_estilo".into(),
            name: "Meu estilo".into(),
            goal: "meu objetivo".into(),
            kind: ToneKind::Style,
        }];
        let mut s = CorrectionSettings {
            style_rules: OLD_STYLE_RULES_V1.into(),
            tones: edited.clone(),
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.style_rules, DEFAULT_STYLE_RULES);
        assert_eq!(s.tones[0], edited[0]);
    }

    #[test]
    fn new_defaults_keep_the_dash_rule_and_restructure_tone() {
        assert!(DEFAULT_STYLE_RULES.contains("NUNCA use travessão"));
        assert!(default_tones()[6].goal.contains("Reorganize"));
    }

    #[test]
    fn non_object_value_falls_back_to_defaults() {
        let s = parse_or_salvage(&serde_json::json!(42));
        assert_eq!(s.tones.len(), 7);
    }

    #[test]
    fn sounds_default_on() {
        assert!(CorrectionSettings::default().sounds);
        let s: CorrectionSettings = serde_json::from_str("{}").unwrap();
        assert!(s.sounds);
        let salvaged = parse_or_salvage(&serde_json::json!({"unload_timeout": "nope"}));
        assert!(salvaged.sounds);
        let off = parse_or_salvage(&serde_json::json!({"sounds": false, "unload_timeout": "nope"}));
        assert!(!off.sounds);
    }

    #[test]
    fn typing_hint_default_on() {
        assert!(CorrectionSettings::default().typing_hint);
        let s: CorrectionSettings = serde_json::from_str("{}").unwrap();
        assert!(s.typing_hint);
        let salvaged = parse_or_salvage(&serde_json::json!({"unload_timeout": "nope"}));
        assert!(salvaged.typing_hint);
        let off =
            parse_or_salvage(&serde_json::json!({"typing_hint": false, "unload_timeout": "nope"}));
        assert!(!off.typing_hint);
        let bad = parse_or_salvage(&serde_json::json!({"typing_hint": "yes", "sounds": false}));
        assert!(bad.typing_hint);
        assert!(!bad.sounds);
    }

    #[test]
    fn store_with_removed_button_fields_still_loads() {
        let v = serde_json::json!({
            "models_dir": r"E:\m",
            "floating_button": false,
            "button_excluded_apps": ["x.exe"],
        });
        assert_eq!(parse_or_salvage(&v).models_dir, r"E:\m");
        // Also through a salvage (another field is invalid).
        let v = serde_json::json!({
            "models_dir": r"E:\m",
            "floating_button": false,
            "button_excluded_apps": ["x.exe"],
            "unload_timeout": "nope",
        });
        assert_eq!(parse_or_salvage(&v).models_dir, r"E:\m");
    }

    /// A store as version 4 left it: the defaults with Traduzir among the adjustments.
    fn v4_store() -> CorrectionSettings {
        let mut tones = default_tones();
        let at = tones.iter().position(|t| t.id == "desenvolver").unwrap() + 1;
        tones.insert(
            at,
            CorrectionTone {
                id: "traduzir".into(),
                name: "Traduzir".into(),
                goal: OLD_TRADUZIR_GOAL.into(),
                kind: ToneKind::Adjust,
            },
        );
        CorrectionSettings {
            tones,
            defaults_version: 4,
            ..Default::default()
        }
    }

    #[test]
    fn v5_removes_the_untouched_traduzir_adjustment() {
        let mut s = v4_store();
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.defaults_version, 5);
        assert!(!s.tones.iter().any(|t| t.id == "traduzir"));
        assert_eq!(s.tones, default_tones());
        // Idempotent.
        assert!(!migrate_defaults(&mut s));
        assert_eq!(s.tones, default_tones());
    }

    #[test]
    fn v5_keeps_an_edited_traduzir_as_a_plain_adjustment() {
        let mut s = v4_store();
        let t = s.tones.iter_mut().find(|t| t.id == "traduzir").unwrap();
        t.goal = "Traduza para o espanhol.".into();
        let edited = t.clone();
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.defaults_version, 5);
        let kept = s.tones.iter().find(|t| t.id == "traduzir").unwrap();
        assert_eq!(kept, &edited);
        assert_eq!(kept.kind, ToneKind::Adjust);
        assert!(!migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 8);
    }

    #[test]
    fn v5_removes_the_default_goal_even_if_renamed() {
        let mut s = v4_store();
        s.tones
            .iter_mut()
            .find(|t| t.id == "traduzir")
            .unwrap()
            .name = "Inglês".into();
        assert!(migrate_defaults(&mut s));
        assert!(!s.tones.iter().any(|t| t.id == "traduzir"));
    }

    #[test]
    fn older_stores_never_get_traduzir_any_more() {
        let mut s = CorrectionSettings {
            tones: default_tones()
                .into_iter()
                .filter(|t| t.id != "descolado" && t.id != "poetico")
                .collect(),
            defaults_version: 2,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.defaults_version, 5);
        assert!(!s.tones.iter().any(|t| t.id == "traduzir"));
        assert_eq!(s.tones.len(), 7);
    }

    #[test]
    fn store_without_traduzir_at_v4_only_moves_the_version() {
        let mut s = CorrectionSettings {
            tones: default_tones(),
            defaults_version: 4,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.tones, default_tones());
        assert_eq!(s.defaults_version, 5);
    }
}
