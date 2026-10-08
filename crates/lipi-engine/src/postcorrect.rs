//! Post-correction of OCR text with the installed lexicon.
//!
//! The lexicon lives in lipi's data directory (`lexicon/<lang>.lex`, built with
//! `lipi lexicon build`). When it is missing, OCR text is only normalised.

use crate::ocr::OcrPage;
use lipi_script::correct::{Corrector, Lexicon};
use lipi_script::{Lang, ScriptShares};
use lipi_sys::paths;

/// Load the corrector for `lang` from the data directory.
///
/// `Ok(None)` when no lexicon is installed or the language has no rules; `Err` carries a note for
/// the user when a lexicon exists but cannot be read.
pub fn load(lang: Lang) -> Result<Option<Corrector>, String> {
    if lang != Lang::Si {
        return Ok(None);
    }
    let path = paths::lexicon_file(lang.code());
    if !path.is_file() {
        return Err(format!(
            "no Sinhala lexicon is installed, so OCR post-correction is off (build one with \
             `lipi lexicon build <text files>`; expected {})",
            path.display()
        ));
    }
    match Lexicon::load(&path) {
        Ok(l) if l.lang() == lang && !l.is_empty() => Ok(Some(Corrector::new(l))),
        Ok(_) => Err(format!("{} is not a {lang} lexicon; OCR post-correction is off", path.display())),
        Err(e) => Err(format!("cannot read {} ({e}); OCR post-correction is off", path.display())),
    }
}

/// Normalise the text of an OCR page and, when a corrector is given and the page carries
/// Sinhala, correct systematic confusions. Adds `corrected:N` to `flags` when N words changed.
pub fn finish(page: &OcrPage, corrector: Option<&Corrector>, flags: &mut Vec<String>) -> String {
    let text = lipi_script::normalize(&page.text);
    let Some(c) = corrector else {
        return text;
    };
    if ScriptShares::of(&text).count(c.lexicon().lang()) == 0 {
        return text;
    }
    let confidences = page.confidence_map();
    let (fixed, changed) = c.correct_text(&text, &|token| confidences.get(token).copied());
    if changed > 0 {
        flags.push(format!("corrected:{changed}"));
    }
    fixed
}

#[cfg(test)]
mod tests {
    use super::*;
    use lipi_script::correct::{Options, Rule};

    fn page(text: &str, confidences: &[(&str, f32)]) -> OcrPage {
        OcrPage {
            page: 1,
            text: text.to_string(),
            confidence: None,
            words: confidences.len(),
            word_confidences: confidences.iter().map(|(w, c)| (w.to_string(), *c)).collect(),
        }
    }

    fn corrector(gate: Option<f32>) -> Corrector {
        let mut lex = Lexicon::new(Lang::Si);
        for _ in 0..5 {
            lex.add_text("විජිත කිරීම");
        }
        Corrector::with_rules(
            lex,
            vec![Rule::new("ච", "ව", 0.5)],
            Options { confidence_gate: gate, ..Options::default() },
        )
    }

    #[test]
    fn corrects_sinhala_and_flags_the_page() {
        let c = corrector(None);
        let mut flags = vec![];
        let p = page("චිජිත කිරීම\u{200C}", &[("චිජිත", 80.0), ("කිරීම\u{200C}", 90.0)]);
        assert_eq!(finish(&p, Some(&c), &mut flags), "විජිත කිරීම");
        assert_eq!(flags, vec!["corrected:1".to_string()]);
    }

    #[test]
    fn confident_words_and_other_scripts_are_untouched() {
        let c = corrector(Some(90.0));
        let mut flags = vec![];
        let p = page("චිජිත", &[("චිජිත", 96.0)]);
        assert_eq!(finish(&p, Some(&c), &mut flags), "චිජිත");
        let p = page("இலங்கை", &[("இலங்கை", 50.0)]);
        assert_eq!(finish(&p, Some(&c), &mut flags), "இலங்கை");
        assert!(flags.is_empty());
        assert_eq!(finish(&page("චිජිත", &[]), None, &mut flags), "චිජිත");
    }
}
