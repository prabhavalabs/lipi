//! Deterministic repair of pre-base vowel signs stored in visual order.
//!
//! PDF producers that place glyphs in visual order emit the pre-base vowel sign (Sinhala
//! kombuva `ෙ`, `ෛ`; Tamil `ெ`, `ே`, `ை`) *before* the consonant it belongs to. The
//! repair moves such a sign after the following consonant cluster and lets NFC compose two-part
//! vowels: `පළාෙත්` becomes `පළාතේ`, `இலங்ைக` becomes `இலங்கை`.
//!
//! The repair only touches signs that are misplaced (not preceded by a consonant) and followed
//! by a consonant, so already-correct text is returned unchanged.

use unicode_normalization::UnicodeNormalization;

const ZWJ: char = '\u{200D}';
const SI_VIRAMA: char = '\u{0DCA}';

fn is_si_consonant(c: char) -> bool {
    ('\u{0D9A}'..='\u{0DC6}').contains(&c)
}

fn is_ta_consonant(c: char) -> bool {
    ('\u{0B95}'..='\u{0BB9}').contains(&c)
}

fn is_consonant(c: char) -> bool {
    is_si_consonant(c) || is_ta_consonant(c)
}

fn is_prebase(c: char) -> bool {
    matches!(c, '\u{0DD9}' | '\u{0DDB}' | '\u{0BC6}' | '\u{0BC7}' | '\u{0BC8}')
}

/// Result of [`repair_visual_order`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repaired {
    /// Repaired, NFC-normalised text.
    pub text: String,
    /// Number of vowel signs moved.
    pub moved: usize,
}

/// Move misplaced pre-base vowel signs after the consonant cluster they belong to.
pub fn repair_visual_order(text: &str) -> Repaired {
    let cs: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(cs.len());
    let mut moved = 0;
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        let prev_is_consonant = out.last().is_some_and(|&p| is_consonant(p));
        let next_is_consonant = cs.get(i + 1).is_some_and(|&n| is_consonant(n));
        if is_prebase(c) && !prev_is_consonant && next_is_consonant {
            // Consonant cluster: C, or for Sinhala C (virama ZWJ C)* (yansaya, rakaransaya).
            let mut j = i + 1;
            let mut cluster = vec![cs[j]];
            j += 1;
            while cs.get(j) == Some(&SI_VIRAMA)
                && cs.get(j + 1) == Some(&ZWJ)
                && cs.get(j + 2).is_some_and(|&n| is_si_consonant(n))
            {
                cluster.extend_from_slice(&cs[j..j + 3]);
                j += 3;
            }
            out.extend(cluster);
            out.push(c);
            moved += 1;
            i = j;
            continue;
        }
        out.push(c);
        i += 1;
    }
    Repaired { text: out.into_iter().collect::<String>().nfc().collect(), moved }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health::visual_order_ratio;

    #[test]
    fn sinhala_kombuva_and_hal() {
        let r = repair_visual_order("පළාෙත්");
        assert_eq!(r.text, "පළාතේ");
        assert_eq!(r.moved, 1);
        assert_eq!(repair_visual_order("ෙදසැම්බර්").text, "දෙසැම්බර්");
    }

    #[test]
    fn sinhala_kombuva_with_aela_pilla() {
        assert_eq!(repair_visual_order("ෙකාළඹ").text, "කොළඹ");
    }

    #[test]
    fn sinhala_cluster_with_rakaransaya() {
        assert_eq!(repair_visual_order("ෙප්\u{200D}ර්").text, "ප්\u{200D}රේ");
    }

    #[test]
    fn tamil_prebase_signs() {
        assert_eq!(repair_visual_order("இலங்ைகச்").text, "இலங்கைச்");
        assert_eq!(repair_visual_order("ேசாசலிசக்").text, "சோசலிசக்");
        assert_eq!(repair_visual_order("ெகாழும்பு").text, "கொழும்பு");
    }

    #[test]
    fn correct_text_is_unchanged() {
        for s in ["බස්නාහිර පළාතේ ගම්පහ", "இலங்கைச் சனநாயக சோசலிசக் குடியரசு", "Sri Lanka"]
        {
            let r = repair_visual_order(s);
            assert_eq!(r.text, s);
            assert_eq!(r.moved, 0);
        }
    }

    #[test]
    fn repaired_text_passes_the_order_check() {
        let bad = "இலங்ைகச் சனநாயக ேசாசலிசக் குடியரசு ".repeat(3);
        assert!(visual_order_ratio(&bad) > 0.1);
        assert_eq!(visual_order_ratio(&repair_visual_order(&bad).text), 0.0);
    }
}
