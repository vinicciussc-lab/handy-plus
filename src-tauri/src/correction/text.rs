//! Pure helpers for text correction: prompt assembly, output cleanup and
//! progress estimates. No I/O, so they are unit-tested directly.

use crate::actions::{strip_invisible_chars, strip_think_block};
use once_cell::sync::Lazy;
use regex::Regex;

/// Longest selection sent to the correction model, in characters.
#[allow(dead_code)]
pub const MAX_CORRECTION_CHARS: usize = 8000;

/// Progress is held here until the work is actually finished.
const PROGRESS_CAP: f32 = 0.95;

/// An em dash or en dash with its surrounding spaces.
static DASH_PAUSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s*[—–]\s*").expect("valid regex"));

/// Builds `(system, user)` messages: the rules followed by the goal, and the
/// text to correct.
#[allow(dead_code)]
pub fn build_messages(rules: &str, goal: &str, text: &str) -> (String, String) {
    (
        format!("{rules}\n\nObjetivo: {goal}"),
        format!("Texto:\n\n{text}"),
    )
}

/// Cleans a model reply: strips a reasoning block and invisible characters,
/// then trims. Unless the original text used dashes, dash pauses become
/// commas, since models tend to add them.
#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn generation_progress(generated_chars: usize, input_chars: usize) -> f32 {
    if input_chars == 0 {
        return PROGRESS_CAP;
    }
    (generated_chars as f32 / input_chars as f32).min(PROGRESS_CAP)
}

/// Progress while loading the model, estimated from the time the previous load
/// took. `None` when there is no previous load to compare with.
#[allow(dead_code)]
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
