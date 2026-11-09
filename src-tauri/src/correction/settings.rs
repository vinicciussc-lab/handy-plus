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

pub const DEFAULT_STYLE_RULES: &str = "Você revisa textos em português do Brasil escritos pelo usuário para WhatsApp, Discord, e-mail e comentários na internet.
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
}

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
            "Corrija ortografia, acentuação, concordância, pontuação e vírgulas, e deixe as frases coerentes e fáceis de ler. Não deixe o texto mais longo do que precisa.",
        ),
        tone(
            "mais_formal",
            "Mais formal",
            "Corrija o texto e deixe o tom mais formal, como num e-mail de trabalho, sem ficar engessado.",
        ),
        tone(
            "mais_curto",
            "Mais curto",
            "Corrija o texto e deixe ele mais curto e direto, sem perder nenhuma informação.",
        ),
        tone(
            "melhorar_argumentos",
            "Melhorar argumentos",
            "Corrija o texto e deixe os argumentos mais claros e convincentes, na ordem mais lógica, sem inventar fatos.",
        ),
    ]
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
        }
    }
}

pub fn get_correction_settings(app: &AppHandle) -> CorrectionSettings {
    let store = app
        .store(crate::portable::store_path(CORRECTION_STORE_PATH))
        .expect("Failed to initialize correction store");
    match store.get(CORRECTION_STORE_KEY) {
        Some(value) => parse_or_salvage(&value),
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
    fn default_tones_are_the_four_from_the_spec() {
        let names: Vec<_> = default_tones().into_iter().map(|t| t.name).collect();
        assert_eq!(
            names,
            ["Meu estilo", "Mais formal", "Mais curto", "Melhorar argumentos"]
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
        assert_eq!(s.tones.len(), 4);
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
    fn non_object_value_falls_back_to_defaults() {
        let s = parse_or_salvage(&serde_json::json!(42));
        assert_eq!(s.tones.len(), 4);
    }
}
