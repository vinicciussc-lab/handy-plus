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
pub const CURRENT_DEFAULTS_VERSION: u32 = 2;
const MAX_TONES: usize = 9;

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

#[derive(Serialize, Deserialize, Debug, Clone, Type, PartialEq)]
pub struct CorrectionTone {
    pub id: String,
    pub name: String,
    pub goal: String,
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
}

fn default_true() -> bool {
    true
}

fn zero_defaults_version() -> u32 {
    0
}

const DESENVOLVER_GOAL: &str = "Desenvolva o texto de verdade: para cada ideia principal, escreva pelo menos uma frase a mais explicando o porquê ou dando um exemplo que o próprio texto já sugere, e ligue as ideias com conectivos simples. O resultado deve ficar com mais ou menos o dobro do tamanho do texto corrigido. Organize em parágrafos se ficar longo. Mantenha o jeito de falar do usuário e a mesma opinião. Não invente fatos específicos, como números, nomes ou datas, que não estavam no texto.";

pub fn default_tones() -> Vec<CorrectionTone> {
    let tone = |id: &str, name: &str, goal: &str| CorrectionTone {
        id: id.to_string(),
        name: name.to_string(),
        goal: goal.to_string(),
    };
    vec![
        tone(
            "meu_estilo",
            "Meu estilo",
            "Corrija ortografia, acentuação, concordância, pontuação e vírgulas, e deixe as frases coerentes e fáceis de ler. Mantenha as mesmas palavras sempre que estiverem certas, as expressões do usuário e o mesmo nível de formalidade. Não deixe o texto mais longo do que precisa.",
        ),
        tone(
            "mais_formal",
            "Mais formal",
            "Reescreva com tom formal e educado, como num e-mail de trabalho: troque gírias e abreviações por palavras comuns e corretas, use frases completas e organize em parágrafos se precisar. Sem ficar engessado nem usar palavras difíceis.",
        ),
        tone(
            "mais_curto",
            "Mais curto",
            "Reescreva bem mais curto: corte pelo menos metade do texto, junte as frases em poucas frases, tire repetições, rodeios e detalhes que não mudam o sentido, mantendo os fatos e a opinião do usuário.",
        ),
        tone(
            "melhorar_argumentos",
            "Melhorar argumentos",
            "Reorganize o texto para convencer melhor: comece pela ideia principal, coloque os argumentos numa ordem lógica, junte ou separe frases quando ajudar, ligue as ideias com conectivos simples e termine com uma conclusão clara. Pode mudar bastante a estrutura, mas não invente fatos nem mude a opinião do usuário.",
        ),
        tone(
            "desenvolver",
            "Desenvolver",
            DESENVOLVER_GOAL,
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

fn old_tones_v1() -> Vec<CorrectionTone> {
    OLD_TONE_GOALS_V1
        .iter()
        .map(|(id, name, goal)| CorrectionTone {
            id: id.to_string(),
            name: name.to_string(),
            goal: goal.to_string(),
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
        if !s.tones.iter().any(|t| t.id == "desenvolver") && s.tones.len() < MAX_TONES {
            if let Some(tone) = new_tones.into_iter().find(|t| t.id == "desenvolver") {
                s.tones.push(tone);
            }
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

    #[test]
    fn default_tones_are_the_five_from_the_spec() {
        let names: Vec<_> = default_tones().into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            [
                "Meu estilo",
                "Mais formal",
                "Mais curto",
                "Melhorar argumentos",
                "Desenvolver"
            ]
        );
    }

    #[test]
    fn defaults_include_desenvolver_last() {
        let t = default_tones();
        assert_eq!(t.len(), 5);
        assert_eq!(
            (t[4].id.as_str(), t[4].name.as_str()),
            ("desenvolver", "Desenvolver")
        );
    }

    #[test]
    fn old_store_gets_desenvolver_once() {
        let mut s = CorrectionSettings {
            tones: default_tones()[..4].to_vec(),
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 5);
        assert_eq!(s.defaults_version, 2);
        s.tones.pop();
        assert!(!migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 4);
    }

    #[test]
    fn full_tone_list_is_not_extended() {
        let mut tones = default_tones()[..4].to_vec();
        while tones.len() < 9 {
            tones.push(CorrectionTone {
                id: format!("t{}", tones.len()),
                name: "x".into(),
                goal: "y".into(),
            });
        }
        let mut s = CorrectionSettings {
            tones,
            defaults_version: 0,
            ..Default::default()
        };
        migrate_defaults(&mut s);
        assert_eq!(s.tones.len(), 9);
        assert_eq!(s.defaults_version, 2);
    }

    #[test]
    fn existing_desenvolver_is_not_duplicated() {
        let mut s = CorrectionSettings {
            defaults_version: 0,
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.tones.len(), 5);
        assert_eq!(s.defaults_version, 2);
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
        assert_eq!(s.tones.len(), 5);
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
        assert_eq!(s.tones, default_tones());
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
        assert_eq!(s.tones[3].goal, default_tones()[3].goal);
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
        assert_eq!(s.tones, default_tones());
    }

    #[test]
    fn old_rules_with_edited_tones_only_changes_rules() {
        let edited = vec![CorrectionTone {
            id: "meu_estilo".into(),
            name: "Meu estilo".into(),
            goal: "meu objetivo".into(),
        }];
        let mut s = CorrectionSettings {
            style_rules: OLD_STYLE_RULES_V1.into(),
            tones: edited.clone(),
            ..Default::default()
        };
        assert!(migrate_defaults(&mut s));
        assert_eq!(s.style_rules, DEFAULT_STYLE_RULES);
        assert_eq!(s.tones, edited);
    }

    #[test]
    fn new_defaults_keep_the_dash_rule_and_restructure_tone() {
        assert!(DEFAULT_STYLE_RULES.contains("NUNCA use travessão"));
        assert!(default_tones()[3].goal.contains("Reorganize"));
    }

    #[test]
    fn non_object_value_falls_back_to_defaults() {
        let s = parse_or_salvage(&serde_json::json!(42));
        assert_eq!(s.tones.len(), 5);
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
}
