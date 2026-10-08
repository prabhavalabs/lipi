//! Text-layer health checks.
//!
//! A PDF text layer can look like text and still be wrong. The failure modes seen in Sri Lankan
//! government documents are:
//!
//! * **Legacy fonts.** Sinhala/Tamil typed with FM Abhaya, DL Manel, Bamini and similar fonts map
//!   glyphs onto Latin code points, so the text layer reads as Latin gibberish
//!   (`Y%S ,xld` for `ශ්‍රී ලංකා`).
//! * **Visual order.** Pre-base vowel signs (Sinhala kombuva, Tamil ெ ே ை) are stored before
//!   the consonant they follow in logical order (`පළාෙත්` for `පළාතේ`, `இலங்ைக` for `இலங்கை`).
//! * **Wrong mappings.** Glyphs mapped to unrelated code points inside Indic words (`ɼ`, `ᾢ`).
//! * **Lost glyphs.** U+FFFD replacement characters where a font had no mapping.

use crate::script::{Lang, ScriptShares, Shares};
use serde::{Deserialize, Serialize};

/// Verdict on a piece of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Usable as is.
    Healthy,
    /// Too little text to judge.
    Empty,
    /// Not usable; OCR (or a converter) is needed.
    Broken,
}

/// Measurements and verdict for a piece of text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextHealth {
    /// Letter counts per script.
    pub letters: ScriptShares,
    /// Script shares.
    pub shares: Shares,
    /// Misplaced dependent vowel signs per Indic word.
    pub visual_order: f32,
    /// Share of Indic words containing a foreign code point.
    pub foreign: f32,
    /// Latin "gibberish" score (0 = plausible English, 1 = legacy-font text).
    pub latin_gibberish: f32,
    /// Share of U+FFFD replacement characters.
    pub replacement: f32,
    /// Verdict.
    pub verdict: Verdict,
    /// Machine-readable reasons for a non-healthy verdict.
    pub reasons: Vec<String>,
}

impl TextHealth {
    /// Whether the verdict is [`Verdict::Healthy`].
    pub fn is_healthy(&self) -> bool {
        self.verdict == Verdict::Healthy
    }
}

/// Thresholds for [`assess`].
#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    /// Minimum letters before any judgement is made.
    pub min_letters: usize,
    /// Maximum misplaced vowel signs per Indic word.
    pub max_visual_order: f32,
    /// Maximum share of Indic words with a foreign code point.
    pub max_foreign: f32,
    /// Latin gibberish score at or above which the text is rejected.
    pub max_latin_gibberish: f32,
    /// Maximum share of replacement characters.
    pub max_replacement: f32,
}

impl Default for Thresholds {
    fn default() -> Self {
        Thresholds {
            min_letters: 20,
            max_visual_order: 0.03,
            max_foreign: 0.02,
            max_latin_gibberish: 0.5,
            max_replacement: 0.01,
        }
    }
}

const SI_SIGNS: std::ops::RangeInclusive<u32> = 0x0DCA..=0x0DDF;
const SI_CONSONANTS: std::ops::RangeInclusive<u32> = 0x0D9A..=0x0DC6;
const TA_SIGNS: std::ops::RangeInclusive<u32> = 0x0BBE..=0x0BCD;
const TA_CONSONANTS: std::ops::RangeInclusive<u32> = 0x0B95..=0x0BB9;

fn is_si_sign(c: char) -> bool {
    SI_SIGNS.contains(&(c as u32)) || matches!(c, '\u{0DF2}' | '\u{0DF3}')
}

fn is_ta_sign(c: char) -> bool {
    TA_SIGNS.contains(&(c as u32)) || c == '\u{0BD7}'
}

fn is_indic(c: char) -> bool {
    crate::script::is_sinhala(c) || crate::script::is_tamil(c)
}

fn is_joiner(c: char) -> bool {
    c == '\u{200C}' || c == '\u{200D}'
}

/// Words that contain at least one Sinhala or Tamil character.
fn indic_words(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| c.is_whitespace()).filter(|w| w.chars().any(is_indic))
}

/// Misplaced dependent vowel signs per Indic word: a sign must follow a consonant of its script
/// (joiners are skipped). Returns `(errors, words)`.
pub fn visual_order_errors(text: &str) -> (usize, usize) {
    let (mut errors, mut words) = (0, 0);
    for w in indic_words(text) {
        words += 1;
        let mut prev: Option<char> = None;
        for c in w.chars() {
            let sign_of = if is_si_sign(c) {
                Some(SI_CONSONANTS)
            } else if is_ta_sign(c) {
                Some(TA_CONSONANTS)
            } else {
                None
            };
            if let Some(cons) = sign_of
                && !prev.is_some_and(|p| cons.contains(&(p as u32)))
            {
                errors += 1;
            }
            if !is_joiner(c) {
                prev = Some(c);
            }
        }
    }
    (errors, words)
}

/// Misplaced vowel signs per Indic word.
pub fn visual_order_ratio(text: &str) -> f32 {
    let (e, w) = visual_order_errors(text);
    if w == 0 { 0.0 } else { e as f32 / w as f32 }
}

fn is_allowed_in_indic_word(c: char) -> bool {
    is_indic(c)
        || is_joiner(c)
        || c.is_ascii_digit()
        || c.is_ascii_punctuation()
        || matches!(c, '\u{2018}'..='\u{201F}' | '\u{2013}' | '\u{2014}' | '\u{2026}' | '\u{00A0}')
}

/// Share of Indic words that contain a code point foreign to Sinhala/Tamil text.
pub fn foreign_ratio(text: &str) -> f32 {
    let (mut bad, mut words) = (0usize, 0usize);
    for w in indic_words(text) {
        words += 1;
        if w.chars().any(|c| !is_allowed_in_indic_word(c) && !c.is_ascii_alphabetic()) {
            bad += 1;
        }
    }
    if words == 0 { 0.0 } else { bad as f32 / words as f32 }
}

const EN_FUNCTION_WORDS: &[&str] = &[
    "the", "of", "and", "to", "in", "a", "is", "that", "for", "by", "be", "or", "as", "on", "with", "any",
    "shall", "this", "which", "under", "such", "are", "it", "from", "at", "an", "not", "was", "has", "have",
    "been", "other", "may", "no", "all", "his", "her", "its", "their",
];

/// How much a run of Latin text looks like legacy-font Sinhala/Tamil rather than English.
///
/// Legacy-font text has almost no English function words *and* is full of symbols and
/// Latin-1 letters inside words (`m%cd;dka;%sl`, `úfYI`, `wÕyrejdod`). Lists of names and
/// addresses also lack function words but are spelled with plain letters, so both signals must
/// be present. Markup tags and emphasis are ignored. Returns `0.0` for fewer than 30 Latin
/// tokens.
pub fn latin_gibberish_score(text: &str) -> f32 {
    let stripped = strip_markup(text);
    let tokens: Vec<&str> = stripped
        .split_whitespace()
        .filter(|t| t.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 2)
        .collect();
    if tokens.len() < 30 {
        return 0.0;
    }
    let function = tokens
        .iter()
        .filter(|t| {
            let w: String = t.chars().filter(|c| c.is_ascii_alphabetic()).collect::<String>().to_lowercase();
            EN_FUNCTION_WORDS.contains(&w.as_str())
        })
        .count() as f32
        / tokens.len() as f32;
    let weird = tokens.iter().filter(|t| is_weird_token(t)).count() as f32 / tokens.len() as f32;
    // English prose has a function-word share around 0.3-0.5; legacy-font text near zero.
    let lack_of_function_words = (1.0 - function / 0.15).clamp(0.0, 1.0);
    let symbols = (weird / 0.15).clamp(0.0, 1.0);
    lack_of_function_words * symbols
}

/// A token with a symbol between letters, or a Latin-1 letter or symbol that legacy fonts use
/// for Indic glyphs.
fn is_weird_token(t: &str) -> bool {
    let cs: Vec<char> = t.chars().collect();
    let inner_symbol = cs.windows(3).any(|w| {
        w[0].is_ascii_alphabetic()
            && matches!(
                w[1],
                '%' | ';'
                    | ','
                    | '\''
                    | '['
                    | ']'
                    | '^'
                    | '$'
                    | '|'
                    | '`'
                    | '~'
                    | '@'
                    | '&'
                    | '*'
                    | '='
                    | '+'
                    | '\\'
                    | '{'
                    | '}'
                    | '#'
            )
            && w[2].is_ascii_alphabetic()
    });
    inner_symbol || cs.iter().any(|&c| ('\u{00A1}'..='\u{00FF}').contains(&c))
}

/// Remove HTML-like tags and Markdown emphasis markers.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            '*' | '_' | '#' => out.push(' '),
            _ => out.push(c),
        }
    }
    out
}

/// Assess a piece of text. `expected` is the language the caller believes the text is in.
pub fn assess(text: &str, expected: Option<Lang>) -> TextHealth {
    assess_with(text, expected, &Thresholds::default())
}

/// [`assess`] with explicit thresholds.
pub fn assess_with(text: &str, expected: Option<Lang>, t: &Thresholds) -> TextHealth {
    let letters = ScriptShares::of(text);
    let shares = letters.shares();
    let indic = letters.si + letters.ta;
    let visual_order = if indic >= 50 { visual_order_ratio(text) } else { 0.0 };
    let foreign = if indic >= 50 { foreign_ratio(text) } else { 0.0 };
    let latin_gibberish = if letters.latin >= 100 { latin_gibberish_score(text) } else { 0.0 };
    let total_chars = text.chars().count().max(1);
    let replacement = text.chars().filter(|&c| c == '\u{FFFD}').count() as f32 / total_chars as f32;

    let mut reasons = Vec::new();
    if replacement > t.max_replacement {
        reasons.push("replacement_chars".to_string());
    }
    if visual_order > t.max_visual_order {
        reasons.push("visual_order".to_string());
    }
    if foreign > t.max_foreign {
        reasons.push("foreign_code_points".to_string());
    }
    if latin_gibberish >= t.max_latin_gibberish {
        reasons.push("latin_gibberish".to_string());
    }
    if let Some(lang) = expected.filter(|l| l.is_indic())
        && letters.letters() >= 100
        && letters.share(lang) < 0.2
        && shares.latin > 0.6
    {
        reasons.push("wrong_script".to_string());
    }
    let verdict = if !reasons.is_empty() {
        Verdict::Broken
    } else if letters.letters() < t.min_letters {
        Verdict::Empty
    } else {
        Verdict::Healthy
    };
    TextHealth { letters, shares, visual_order, foreign, latin_gibberish, replacement, verdict, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD_SI: &str = "බස්නාහිර පළාතේ ගම්පහ දිස්ත්‍රික්කයේ ඉඩම් හිමිකම් නිරවුල් කිරීම සම්බන්ධයෙනි ";
    const BAD_SI: &str = "බස්නාහිර පළාෙත් ගම්පහ දිස්තික්කෙ ෙක.මසා ෙදසැම්බර් ";

    #[test]
    fn visual_order_detects_kombuva_before_consonant() {
        assert_eq!(visual_order_ratio(&GOOD_SI.repeat(4)), 0.0);
        assert!(visual_order_ratio(&BAD_SI.repeat(4)) > 0.05);
        assert_eq!(visual_order_ratio("இலங்கைச் சனநாயக சோசலிசக் குடியரசின் பாராளுமன்றம்"), 0.0);
        assert!(visual_order_ratio("இலங்ைகச் சனநாயக ேசாசலிசக் குடியரசு") > 0.2);
    }

    #[test]
    fn foreign_code_points_in_tamil_words() {
        assert!(foreign_ratio("இலங்ைகச் சனநாயக ேசாசᾢசக் குᾊயரசு வர்த்தமானப்") > 0.2);
        assert_eq!(foreign_ratio("இலங்கைச் சனநாயக சோசலிசக் குடியரசு"), 0.0);
    }

    #[test]
    fn legacy_font_latin_is_gibberish_english_is_not() {
        let fm = "Y%S ,xld m%cd;dka;%sl iudcjd§ ckrcfha w;s úfYI .eiÜ m;%h wxl 2256$28 – 2021 fkdjeïn¾ ui 30 jeks wÕyrejdod ".repeat(4);
        let en = "The appellant contends that the learned judge erred in law and that the order of the court below should be set aside in terms of section 5 of the Act. ".repeat(4);
        assert!(latin_gibberish_score(&fm) > 0.7, "{}", latin_gibberish_score(&fm));
        assert!(latin_gibberish_score(&en) < 0.2, "{}", latin_gibberish_score(&en));
        assert!(assess(&fm, None).reasons.contains(&"latin_gibberish".to_string()));
        assert!(assess(&en, Some(Lang::En)).is_healthy());
        // Party lists have few function words but are not gibberish.
        let parties = "## (Deceased) <u>DEFENDANT-APPELLANT</u> D. A. Piyaseeli No. 29, New Road, Ambalangoda. <u>SUBSTITUTED DEFENDANT-APPELLANT</u> Iddamalgoda Dissanayakalage Winson Ranasinghe Magala South, Karandeniya. ".repeat(4);
        assert!(latin_gibberish_score(&parties) < 0.2, "{}", latin_gibberish_score(&parties));
    }

    #[test]
    fn verdicts() {
        assert!(assess(&GOOD_SI.repeat(4), Some(Lang::Si)).is_healthy());
        let bad = assess(&BAD_SI.repeat(4), Some(Lang::Si));
        assert_eq!(bad.verdict, Verdict::Broken);
        assert_eq!(bad.reasons, vec!["visual_order"]);
        assert_eq!(assess("abc", None).verdict, Verdict::Empty);
    }
}
