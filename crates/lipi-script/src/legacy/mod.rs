//! Converters for legacy (non-Unicode) Sinhala fonts.
//!
//! A legacy font draws Sinhala glyphs at Latin code points, so a PDF text layer typed with it
//! reads as `Y%S ,xld` for ශ්‍රී ලංකා. The glyph codes are stored in *visual* order: a
//! pre-base vowel sign (kombuva ෙ, kombu deka ෛ) comes before the consonant it belongs to,
//! and the two-part vowels ො ෝ ේ ෞ are stored as kombuva + consonant + ා / ් / ෟ.
//!
//! Conversion therefore has three steps:
//!
//! 1. longest-match replacement of code sequences with Unicode (multi-code ligatures such as
//!    `Y%S` → ශ්‍රී and context forms such as `re` → රු matter);
//! 2. moving each pre-base sign after the consonant cluster that follows it, and a vowel sign
//!    typed before a rakaransaya or yansaya (`YS%` for ශ්‍රී) after the conjunct;
//! 3. [`crate::normalize`], whose NFC step composes the two-part vowels.
//!
//! Only the FM Abhaya family ([`fm`]) has a table so far. Tables are derived from aligned
//! text-layer / OCR word pairs of real documents (see `bench/legacy/` in the repository), not
//! copied from other converters.

pub mod fm;

use crate::normalize::normalize;
use crate::script::Lang;
use std::collections::HashMap;
use std::sync::OnceLock;

const ZWJ: char = '\u{200D}';
const VIRAMA: char = '\u{0DCA}';

fn is_si_consonant(c: char) -> bool {
    ('\u{0D9A}'..='\u{0DC6}').contains(&c)
}

fn is_prebase(c: char) -> bool {
    matches!(c, '\u{0DD9}' | '\u{0DDB}')
}

/// Dependent vowel signs other than the virama (U+0DCF–U+0DDF, U+0DF2, U+0DF3).
fn is_vowel_sign(c: char) -> bool {
    ('\u{0DCF}'..='\u{0DDF}').contains(&c) || matches!(c, '\u{0DF2}' | '\u{0DF3}')
}

/// A legacy-font to Unicode converter built from a code-sequence table.
#[derive(Debug)]
pub struct Converter {
    /// Family identifier, e.g. `fm-abhaya`.
    pub family: &'static str,
    /// Script the font draws.
    pub lang: Lang,
    index: HashMap<&'static str, &'static str>,
    max_len: usize,
}

impl Converter {
    /// Build a converter from `(code sequence, Unicode)` pairs.
    pub fn new(family: &'static str, lang: Lang, table: &'static [(&'static str, &'static str)]) -> Self {
        let index: HashMap<&'static str, &'static str> = table.iter().copied().collect();
        let max_len = table.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(1);
        Converter { family, lang, index, max_len }
    }

    /// Number of code sequences in the table.
    pub fn len(&self) -> usize {
        self.index.len()
    }

    /// Whether the table is empty.
    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }

    /// Whether `c` is a code the table maps (digits and most punctuation are not).
    pub fn maps(&self, c: char) -> bool {
        let mut buf = [0u8; 4];
        self.index.contains_key(c.encode_utf8(&mut buf) as &str)
    }

    /// Convert legacy-font text to Unicode. Codes outside the table (digits, spaces, unknown
    /// symbols) pass through unchanged.
    pub fn convert(&self, text: &str) -> String {
        let cs: Vec<char> = text.chars().collect();
        let mut out: Vec<char> = Vec::with_capacity(cs.len() * 2);
        let mut key = String::with_capacity(16);
        let mut i = 0;
        while i < cs.len() {
            let mut matched = false;
            for len in (1..=self.max_len.min(cs.len() - i)).rev() {
                key.clear();
                key.extend(&cs[i..i + len]);
                if let Some(v) = self.index.get(key.as_str()) {
                    out.extend(v.chars());
                    i += len;
                    matched = true;
                    break;
                }
            }
            if !matched {
                out.push(cs[i]);
                i += 1;
            }
        }
        reorder_prebase(&mut out);
        reorder_signs_before_conjuncts(&mut out);
        normalize(&out.into_iter().collect::<String>())
    }
}

/// Move a vowel sign that precedes a conjunct part (`් ZWJ consonant`, i.e. rakaransaya or
/// yansaya) after it: the glyphs ි and ්‍ර sit above and below the same consonant, so typists
/// enter them in either order, while Unicode wants the conjunct first (`ක්‍රි`, not `කි්‍ර`).
pub fn reorder_signs_before_conjuncts(v: &mut Vec<char>) {
    let mut i = 0;
    while i + 3 < v.len() {
        if is_vowel_sign(v[i]) && v[i + 1] == VIRAMA && v[i + 2] == ZWJ && is_si_consonant(v[i + 3]) {
            let sign = v.remove(i);
            v.insert(i + 3, sign);
            i += 4;
            continue;
        }
        i += 1;
    }
}

/// Move every pre-base vowel sign after the consonant cluster that follows it. The cluster is
/// a consonant optionally followed by conjunct parts (`්‍ර`, `්‍ය`, touching letters written as
/// virama + ZWJ + consonant). A sign that is not followed by a consonant stays where it is.
pub fn reorder_prebase(v: &mut Vec<char>) {
    let mut i = 0;
    while i < v.len() {
        if is_prebase(v[i]) && v.get(i + 1).is_some_and(|&c| is_si_consonant(c)) {
            let mut j = i + 2;
            while v.get(j) == Some(&VIRAMA)
                && v.get(j + 1) == Some(&ZWJ)
                && v.get(j + 2).is_some_and(|&c| is_si_consonant(c))
            {
                j += 3;
            }
            let sign = v.remove(i);
            v.insert(j - 1, sign);
            i = j;
            continue;
        }
        i += 1;
    }
}

static FM: OnceLock<Converter> = OnceLock::new();

/// The converter for a legacy font family identifier (see [`crate::fonts::legacy_font`]), if
/// lipi has a table for it.
pub fn converter(family: &str) -> Option<&'static Converter> {
    match family {
        fm::FAMILY => Some(FM.get_or_init(|| Converter::new(fm::FAMILY, Lang::Si, fm::TABLE))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A small table in the style of the FM one, for testing the mechanics independently.
    const T: &[(&str, &str)] = &[
        ("l", "ක"),
        ("f", "\u{0DD9}"),
        ("ff", "\u{0DDB}"),
        ("d", "ා"),
        ("a", "\u{0DCA}"),
        ("m", "ප"),
        ("%", "\u{0DCA}\u{200D}ර"),
        ("g", "ට"),
        ("j", "ව"),
        ("o", "ද"),
        ("H", "\u{0DCA}\u{200D}ය"),
        ("r", "ර"),
        ("e", "ැ"),
        ("re", "රු"),
        ("$", "/"),
    ];

    fn conv() -> Converter {
        Converter::new("test", Lang::Si, T)
    }

    #[test]
    fn two_part_vowels_compose_after_reordering() {
        let c = conv();
        assert_eq!(c.convert("fldg"), "කොට");
        assert_eq!(c.convert("fla"), "කේ");
        assert_eq!(c.convert("flda"), "කෝ");
        assert_eq!(c.convert("ffjoH"), "වෛද්‍ය");
    }

    #[test]
    fn prebase_sign_moves_past_conjuncts() {
        assert_eq!(conv().convert("fm%"), "ප්‍රෙ");
        assert_eq!(conv().convert("fm%d"), "ප්‍රො");
    }

    #[test]
    fn longest_match_wins_and_unknown_codes_pass_through() {
        let c = conv();
        assert_eq!(c.convert("re"), "රු");
        assert_eq!(c.convert("le"), "කැ");
        assert_eq!(c.convert("12$3 x"), "12/3 x");
        assert!(c.maps('l') && !c.maps('1'));
    }

    #[test]
    fn vowel_sign_typed_before_a_conjunct_moves_after_it() {
        let c = conv();
        // ක + ි + ්‍ර typed as `ls%` must give ක්‍රි, the same as `l%s`.
        let t: &[(&str, &str)] = &[("l", "ක"), ("s", "ි"), ("%", "\u{0DCA}\u{200D}ර"), ("S", "ී"), ("Y", "ශ")];
        let c2 = Converter::new("t", Lang::Si, t);
        assert_eq!(c2.convert("ls%"), c2.convert("l%s"));
        assert_eq!(c2.convert("YS%"), "ශ්\u{200D}රී");
        assert_eq!(c.convert("fm%"), "ප්\u{200D}රෙ");
    }

    #[test]
    fn sign_without_following_consonant_is_left_alone() {
        let mut v: Vec<char> = "f d".chars().collect();
        reorder_prebase(&mut v);
        assert_eq!(v.into_iter().collect::<String>(), "f d");
        let mut v = vec!['\u{0DD9}', ' ', 'ක'];
        reorder_prebase(&mut v);
        assert_eq!(v, vec!['\u{0DD9}', ' ', 'ක']);
    }

    #[test]
    fn fm_table_is_available_and_well_formed() {
        let c = converter("fm-abhaya").expect("FM table");
        assert!(c.len() > 80);
        assert!(converter("bamini").is_none());
        let mut seen = std::collections::HashSet::new();
        for (k, v) in fm::TABLE {
            assert!(!k.is_empty() && !v.is_empty(), "{k:?} -> {v:?}");
            assert!(seen.insert(*k), "duplicate code {k:?}");
        }
    }
}
