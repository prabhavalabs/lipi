//! Unicode normalisation that is safe for Sinhala and Tamil.
//!
//! * NFC composition (also composes two-part vowel signs such as Sinhala ො and Tamil ொ).
//! * ZWJ (U+200D) is kept when it sits between Sinhala or Tamil characters: Sinhala needs it
//!   for yansaya, rakaransaya and repaya (`්‍ය`, `්‍ර`, `ර්‍`).
//! * ZWNJ (U+200C) right after a virama (Sinhala al-lakuna U+0DCA, Tamil pulli U+0BCD) is
//!   removed. It has no orthographic meaning there and Tesseract emits it after most
//!   word-final consonants.
//! * Stray joiners outside Indic context, zero-width spaces, BOMs, soft hyphens and control
//!   characters (other than newline and tab) are removed.
//! * Latin presentation ligatures are expanded.

use crate::script::{is_sinhala, is_tamil};
use unicode_normalization::UnicodeNormalization;

const ZWJ: char = '\u{200D}';
const ZWNJ: char = '\u{200C}';
const SI_VIRAMA: char = '\u{0DCA}';
const TA_VIRAMA: char = '\u{0BCD}';

fn is_indic(c: char) -> bool {
    is_sinhala(c) || is_tamil(c)
}

fn expand_ligature(c: char) -> Option<&'static str> {
    Some(match c {
        '\u{FB00}' => "ff",
        '\u{FB01}' => "fi",
        '\u{FB02}' => "fl",
        '\u{FB03}' => "ffi",
        '\u{FB04}' => "ffl",
        '\u{FB05}' | '\u{FB06}' => "st",
        _ => return None,
    })
}

/// Normalise extracted or recognised text. Newlines and Markdown structure are preserved.
pub fn normalize(text: &str) -> String {
    let composed: Vec<char> = text.nfc().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in composed.iter().enumerate() {
        let prev = if i > 0 { Some(composed[i - 1]) } else { None };
        let next = composed.get(i + 1).copied();
        match c {
            '\u{FEFF}' | '\u{200B}' | '\u{00AD}' | '\u{2060}' | '\u{180E}' => {}
            ZWNJ => {
                let after_virama = matches!(prev, Some(SI_VIRAMA) | Some(TA_VIRAMA));
                let indic_context = prev.is_some_and(is_indic) && next.is_some_and(is_indic);
                if !after_virama && indic_context {
                    out.push(c);
                }
            }
            ZWJ => {
                if prev.is_some_and(is_indic) && next.is_some_and(is_indic) {
                    out.push(c);
                }
            }
            '\r' => {
                if next != Some('\n') {
                    out.push('\n');
                }
            }
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => match expand_ligature(c) {
                Some(s) => out.push_str(s),
                None => out.push(c),
            },
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_sinhala_conjunct_joiners() {
        let s = "ශ්\u{200D}රී ලංකා ප්\u{200D}රජාතාන්ත්\u{200D}රික";
        assert_eq!(normalize(s), s);
    }

    #[test]
    fn drops_ocr_zwnj_after_virama() {
        assert_eq!(normalize("තීන්දුවක්\u{200C} මත"), "තීන්දුවක් මත");
        assert_eq!(normalize("ஆம்\u{200C} ஆண்டின்\u{200C}"), "ஆம் ஆண்டின்");
    }

    #[test]
    fn drops_stray_marks_and_expands_ligatures() {
        assert_eq!(normalize("a\u{200B}b\u{200D} c\u{FB01}\r\n"), "ab cfi\n");
    }

    #[test]
    fn composes_two_part_vowels() {
        // Sinhala kombuva + aela-pilla -> ො ; Tamil ெ + ா -> ொ
        assert_eq!(normalize("ක\u{0DD9}\u{0DCF}"), "කො");
        assert_eq!(normalize("க\u{0BC6}\u{0BBE}"), "கொ");
    }
}
