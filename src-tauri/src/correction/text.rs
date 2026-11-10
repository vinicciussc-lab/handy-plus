//! Pure helpers for text correction: prompt assembly, output cleanup and
//! progress estimates. No I/O, so they are unit-tested directly.

use crate::actions::{strip_invisible_chars, strip_think_block};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Longest selection sent to the correction model, in characters.
pub const MAX_CORRECTION_CHARS: usize = 8000;

/// Progress is held here until the work is actually finished.
const PROGRESS_CAP: f32 = 0.95;

/// An em dash or en dash with its surrounding spaces.
static DASH_PAUSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*[—–]\s*").expect("valid regex"));

/// The language of a text on the panel's PT / EN switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    #[default]
    Pt,
    En,
}

impl Lang {
    pub fn other(self) -> Self {
        match self {
            Self::Pt => Self::En,
            Self::En => Self::Pt,
        }
    }
}

/// The language of the selected text: English, or Portuguese for anything
/// else (the app is made for Portuguese).
pub fn detect_lang(text: &str) -> Lang {
    use whatlang::{Detector, Lang as W};
    // Only the two languages of the switch: short texts are guessed far
    // better between two than among seventy.
    let detector = Detector::with_allowlist(vec![W::Eng, W::Por]);
    match detector.detect_lang(text) {
        Some(W::Eng) => Lang::En,
        _ => Lang::Pt,
    }
}

/// Goal of a translation into English. It overrides the rules' "Portuguese
/// only" in so many words, or small models answer in Portuguese.
pub const TRANSLATE_GOAL_TO_EN: &str = "Traduza o texto para o inglês. Escreva um inglês natural, como uma pessoa nativa escreveria a mesma mensagem, mantendo o mesmo tom, o mesmo nível de formalidade, o mesmo jeito de falar e as mesmas ideias. Esta tarefa é uma tradução, não uma correção, e é a exceção às regras acima sobre o português: a resposta tem que ser escrita inteira em inglês (English), sem nenhuma palavra em português. Não use travessão. Não explique a tradução.";

/// Goal of a translation into Portuguese.
pub const TRANSLATE_GOAL_TO_PT: &str = "Traduza o texto para o português do Brasil. Escreva um português natural, como um brasileiro escreveria a mesma mensagem, mantendo o mesmo tom, o mesmo nível de formalidade, o mesmo jeito de falar e as mesmas ideias. Esta tarefa é uma tradução, não uma correção: a resposta tem que ser escrita inteira em português do Brasil, sem nenhuma frase em inglês. Não use travessão. Não explique a tradução.";

/// The goal that translates a text into `to`.
pub fn translate_goal(to: Lang) -> &'static str {
    match to {
        Lang::En => TRANSLATE_GOAL_TO_EN,
        Lang::Pt => TRANSLATE_GOAL_TO_PT,
    }
}

/// Builds `(system, user)` messages: the rules followed by the goal, and the
/// text to correct.
pub fn build_messages(rules: &str, goal: &str, text: &str) -> (String, String) {
    (
        format!("{rules}\n\nObjetivo: {goal}"),
        format!("Texto:\n\n{text}"),
    )
}

/// Cleans a model reply: strips a reasoning block and invisible characters,
/// then trims. Unless the original text used dashes, dash pauses become
/// commas, since models tend to add them.
pub fn clean_output(original: &str, raw: &str) -> String {
    let cleaned = strip_invisible_chars(strip_think_block(raw));
    let cleaned = cleaned.trim();
    if original.contains('—') || original.contains('–') {
        cleaned.to_string()
    } else {
        DASH_PAUSE.replace_all(cleaned, ", ").into_owned()
    }
}

/// Progress while generating, estimated from characters produced against the
/// input length. Capped at 0.95 until the generation really finishes.
pub fn generation_progress(generated_chars: usize, input_chars: usize) -> f32 {
    if input_chars == 0 {
        return PROGRESS_CAP;
    }
    (generated_chars as f32 / input_chars as f32).min(PROGRESS_CAP)
}

/// Progress while loading the model, estimated from the time the previous load
/// took. `None` when there is no previous load to compare with.
pub fn load_progress(elapsed_ms: u64, expected_ms: Option<u64>) -> Option<f32> {
    let expected_ms = expected_ms?;
    if expected_ms == 0 {
        return Some(PROGRESS_CAP);
    }
    Some((elapsed_ms as f32 / expected_ms as f32).min(PROGRESS_CAP))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_PT: [&str; 3] = [
        "oi mano blz? entao eu vi aquele video q vc mandou ontem e achei mt massa, mas acho q o cara errou umas coisa la no final sobre o processador, depois agente conversa melhor sobre isso",
        "Bom dia, gostaria de saber se é possível remarcar a reunião de amanhã pois eu tive um imprevisto, e não vou conseguir chegar no horário combinado, se puder ser na quinta seria ótimo",
        "cara esse jogo ta mt bom mas a otimização ta horrivel, minha placa fica em 100% e mesmo assim cai pra 40 fps nas cidade",
    ];

    #[test]
    fn detects_portuguese_and_english() {
        for text in SPEC_PT {
            assert_eq!(detect_lang(text), Lang::Pt, "{text}");
        }
        for text in [
            "I think the game is worth it even with the bugs, but the price is too high for what it offers, so I would wait for a sale.",
            "hey, can we move tomorrow's meeting to thursday? something came up",
            "thanks for the video you sent yesterday",
        ] {
            assert_eq!(detect_lang(text), Lang::En, "{text}");
        }
    }

    #[test]
    fn any_other_language_counts_as_portuguese() {
        assert_eq!(
            detect_lang("Hola, ¿cómo estás? Mañana vamos a la playa con mis amigos."),
            Lang::Pt
        );
        assert_eq!(detect_lang(""), Lang::Pt);
        assert_eq!(detect_lang("123"), Lang::Pt);
    }

    #[test]
    fn lang_flips() {
        assert_eq!(Lang::Pt.other(), Lang::En);
        assert_eq!(Lang::En.other(), Lang::Pt);
    }

    #[test]
    fn translation_goals_are_fixed_and_dash_free() {
        let en = translate_goal(Lang::En);
        let pt = translate_goal(Lang::Pt);
        assert_eq!(en, TRANSLATE_GOAL_TO_EN);
        assert_eq!(pt, TRANSLATE_GOAL_TO_PT);
        for goal in [en, pt] {
            assert!(!goal.contains('—') && !goal.contains('–'), "{goal}");
            assert!(goal.contains("mesmo tom"), "{goal}");
        }
        // The English one overrides the Portuguese rule, in so many words.
        assert!(en.contains("inteira em inglês"));
        assert!(en.contains("exceção"));
        assert!(pt.contains("português do Brasil"));
    }

    #[test]
    fn system_prompt_is_rules_then_goal() {
        let (sys, user) = build_messages("R1\nR2", "Deixe formal.", "oi");
        assert_eq!(sys, "R1\nR2\n\nObjetivo: Deixe formal.");
        assert_eq!(user, "Texto:\n\noi");
    }

    #[test]
    fn removes_dash_pauses_when_original_had_none() {
        assert_eq!(clean_output("a b", "Oi — tudo bem?"), "Oi, tudo bem?");
        assert_eq!(clean_output("a b", "Oi – tudo bem?"), "Oi, tudo bem?");
    }

    #[test]
    fn keeps_dashes_the_user_typed() {
        assert_eq!(clean_output("x — y", "X — y."), "X — y.");
    }

    #[test]
    fn strips_think_block_and_invisible_chars_and_trims() {
        assert_eq!(clean_output("a", "<think>hm</think>\n Oi\u{200B} "), "Oi");
    }

    #[test]
    fn generation_progress_is_capped_until_done() {
        assert_eq!(generation_progress(0, 100), 0.0);
        assert!((generation_progress(50, 100) - 0.5).abs() < 1e-6);
        assert_eq!(generation_progress(150, 100), 0.95);
        assert_eq!(generation_progress(10, 0), 0.95);
    }

    #[test]
    fn load_progress_needs_a_previous_load() {
        assert_eq!(load_progress(1000, None), None);
        assert_eq!(load_progress(4500, Some(9000)), Some(0.5));
        assert_eq!(load_progress(20000, Some(9000)), Some(0.95));
    }
}
