//! Recognition of legacy (non-Unicode) Sinhala and Tamil font families from PDF font names.
//!
//! Before Unicode was widely used, Sinhala and Tamil were typed with fonts that draw Indic
//! glyphs at Latin code points. A PDF that embeds such a font has a text layer that reads as
//! Latin, so the font name is a strong signal that the page needs conversion or OCR.

use crate::script::Lang;

/// A legacy font family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyFont {
    /// Family identifier, e.g. `fm-abhaya`.
    pub family: &'static str,
    /// Script the font draws.
    pub lang: Lang,
}

/// (prefix of the normalised font name, family, language). Matched against the font name in
/// lower case with the subset tag (`ABCDEF+`) and spaces, hyphens and underscores removed.
const FAMILIES: &[(&str, &str, Lang)] = &[
    // Sinhala: FM (Fonts of Malith) family, DL family and other common legacy faces.
    ("fmabhaya", "fm-abhaya", Lang::Si),
    ("fmabaya", "fm-abhaya", Lang::Si),
    ("fmabab", "fm-abhaya", Lang::Si),
    ("fmbindumathi", "fm-bindumathi", Lang::Si),
    ("fmmalithi", "fm-malithi", Lang::Si),
    ("fmderana", "fm-derana", Lang::Si),
    ("fmemanee", "fm-emanee", Lang::Si),
    ("fmganganee", "fm-ganganee", Lang::Si),
    ("fmsamantha", "fm-samantha", Lang::Si),
    ("fmarjun", "fm-arjunn", Lang::Si),
    ("fmbasuru", "fm-basuru", Lang::Si),
    ("fmgemunu", "fm-gemunu", Lang::Si),
    ("fmrashmi", "fm-rashmi", Lang::Si),
    ("fmsandhyanee", "fm-sandhyanee", Lang::Si),
    ("fmpodiyan", "fm-podiyan", Lang::Si),
    ("fmmadhura", "fm-madhura", Lang::Si),
    ("fmsamanthax", "fm-samantha", Lang::Si),
    ("dlmanel", "dl-manel", Lang::Si),
    ("dlparas", "dl-paras", Lang::Si),
    ("dlaraliya", "dl-araliya", Lang::Si),
    ("dlhimaya", "dl-himaya", Lang::Si),
    ("dlsarasavi", "dl-sarasavi", Lang::Si),
    ("dlyasarasi", "dl-yasarasi", Lang::Si),
    ("kaputa", "kaputa", Lang::Si),
    ("thibus", "thibus", Lang::Si),
    ("amalee", "amalee", Lang::Si),
    ("sandaya", "sandaya", Lang::Si),
    ("wijaya", "wijaya", Lang::Si),
    ("isiwara", "isi", Lang::Si),
    ("kandy", "kandy", Lang::Si),
    // Tamil.
    ("bamini", "bamini", Lang::Ta),
    ("baamini", "bamini", Lang::Ta),
    ("vanavil", "vanavil", Lang::Ta),
    ("kalaham", "kalaham", Lang::Ta),
    ("amudham", "amudham", Lang::Ta),
    ("kalaimakal", "kalaimakal", Lang::Ta),
    ("shreetam", "shree-tam", Lang::Ta),
    ("elango", "elango", Lang::Ta),
    ("mylai", "mylai", Lang::Ta),
    ("tamtam", "tam", Lang::Ta),
    ("tscu", "tscii", Lang::Ta),
    ("tscii", "tscii", Lang::Ta),
];

fn normalise_name(name: &str) -> String {
    let base = match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.chars().all(|c| c.is_ascii_uppercase()) => rest,
        _ => name,
    };
    base.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

/// Identify a legacy Sinhala or Tamil font from a PDF `/BaseFont` name.
pub fn legacy_font(name: &str) -> Option<LegacyFont> {
    let n = normalise_name(name);
    for &(prefix, family, lang) in FAMILIES {
        if n.starts_with(prefix) {
            return Some(LegacyFont { family, lang });
        }
    }
    // "TAM-", "TAB-" families (Tamil Nadu government encodings) keep the hyphen in the raw name.
    let raw = name.split_once('+').map(|(_, r)| r).unwrap_or(name).to_ascii_uppercase();
    if raw.starts_with("TAM-") || raw.starts_with("TAB-") {
        return Some(LegacyFont { family: "tam-tab", lang: Lang::Ta });
    }
    None
}

/// Whether a font name indicates a text layer produced by an earlier OCR pass
/// (an invisible font laid over a scanned image).
pub fn is_prior_ocr_font(name: &str) -> bool {
    let n = normalise_name(name);
    n.contains("glyphlessfont") || n.contains("hiddenhorzocr")
}

/// Whether the font is a Type 3 font converted from TrueType by an old Windows PostScript driver
/// (`MSTT31c4e8`). Such fonts often carry legacy Sinhala/Tamil glyphs with no usable mapping.
pub fn is_unnamed_truetype(name: &str) -> bool {
    normalise_name(name).starts_with("mstt31")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_subset_prefixed_names() {
        assert_eq!(legacy_font("DAPBCE+FMAbhayax").unwrap().family, "fm-abhaya");
        assert_eq!(legacy_font("DAPBBC+FMAbabldBold").unwrap().lang, Lang::Si);
        assert_eq!(legacy_font("DL-Manel-bold").unwrap().family, "dl-manel");
        assert_eq!(legacy_font("Bamini").unwrap().lang, Lang::Ta);
        assert_eq!(legacy_font("ABCDEF+TAM-Kavi").unwrap().family, "tam-tab");
        assert!(legacy_font("PMGAGJ+IskoolaPota").is_none());
        assert!(legacy_font("TimesNewRomanPSMT").is_none());
    }

    #[test]
    fn prior_ocr_and_type3_names() {
        assert!(is_prior_ocr_font("HiddenHorzOCR"));
        assert!(is_prior_ocr_font("AAAAAA+GlyphLessFont"));
        assert!(is_unnamed_truetype("BBPNOK+MSTT31c4e8"));
        assert!(!is_unnamed_truetype("Arial"));
    }
}
