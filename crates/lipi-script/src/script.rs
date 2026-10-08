//! Letter counts per script and the languages they imply.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A language lipi knows how to recognise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    /// Sinhala (ISO 639-1 `si`).
    Si,
    /// Tamil (ISO 639-1 `ta`).
    Ta,
    /// English, standing for any Latin-script text (ISO 639-1 `en`).
    En,
}

impl Lang {
    /// All languages in priority order.
    pub const ALL: [Lang; 3] = [Lang::Si, Lang::Ta, Lang::En];

    /// ISO 639-1 code.
    pub fn code(self) -> &'static str {
        match self {
            Lang::Si => "si",
            Lang::Ta => "ta",
            Lang::En => "en",
        }
    }

    /// Tesseract language-pack name.
    pub fn tesseract(self) -> &'static str {
        match self {
            Lang::Si => "sin",
            Lang::Ta => "tam",
            Lang::En => "eng",
        }
    }

    /// Parse an ISO 639-1 / 639-2 code or English name.
    pub fn parse(s: &str) -> Option<Lang> {
        match s.trim().to_ascii_lowercase().as_str() {
            "si" | "sin" | "sinhala" | "sinhalese" => Some(Lang::Si),
            "ta" | "tam" | "tamil" => Some(Lang::Ta),
            "en" | "eng" | "english" | "latin" => Some(Lang::En),
            _ => None,
        }
    }

    /// Whether the language uses an Indic (Brahmic) script.
    pub fn is_indic(self) -> bool {
        matches!(self, Lang::Si | Lang::Ta)
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Sinhala block U+0D80–U+0DFF.
pub fn is_sinhala(c: char) -> bool {
    ('\u{0D80}'..='\u{0DFF}').contains(&c)
}

/// Tamil block U+0B80–U+0BFF.
pub fn is_tamil(c: char) -> bool {
    ('\u{0B80}'..='\u{0BFF}').contains(&c)
}

/// ASCII Latin letter.
pub fn is_latin(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Letter counts per script.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptShares {
    /// Sinhala code points.
    pub si: usize,
    /// Tamil code points.
    pub ta: usize,
    /// ASCII Latin letters.
    pub latin: usize,
}

/// Fractions of the counted letters, each in `0.0..=1.0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Shares {
    /// Share of Sinhala.
    pub si: f32,
    /// Share of Tamil.
    pub ta: f32,
    /// Share of Latin.
    pub latin: f32,
}

impl ScriptShares {
    /// Count letters of each script in `text`.
    pub fn of(text: &str) -> Self {
        let mut s = ScriptShares::default();
        for c in text.chars() {
            if is_sinhala(c) {
                s.si += 1;
            } else if is_tamil(c) {
                s.ta += 1;
            } else if is_latin(c) {
                s.latin += 1;
            }
        }
        s
    }

    /// Total counted letters.
    pub fn letters(&self) -> usize {
        self.si + self.ta + self.latin
    }

    /// Count for one language.
    pub fn count(&self, lang: Lang) -> usize {
        match lang {
            Lang::Si => self.si,
            Lang::Ta => self.ta,
            Lang::En => self.latin,
        }
    }

    /// Fractions of the counted letters.
    pub fn shares(&self) -> Shares {
        let n = self.letters().max(1) as f32;
        Shares { si: self.si as f32 / n, ta: self.ta as f32 / n, latin: self.latin as f32 / n }
    }

    /// Share of one language.
    pub fn share(&self, lang: Lang) -> f32 {
        self.count(lang) as f32 / self.letters().max(1) as f32
    }

    /// Languages whose share is at least `min_share`, largest first. Empty below `min_letters`.
    pub fn languages(&self, min_share: f32, min_letters: usize) -> Vec<Lang> {
        if self.letters() < min_letters {
            return Vec::new();
        }
        let mut langs: Vec<Lang> = Lang::ALL.into_iter().filter(|&l| self.share(l) >= min_share).collect();
        langs.sort_by_key(|l| std::cmp::Reverse(self.count(*l)));
        langs
    }

    /// The dominant language, if any script has letters.
    pub fn dominant(&self) -> Option<Lang> {
        if self.letters() == 0 {
            return None;
        }
        Lang::ALL.into_iter().max_by_key(|&l| self.count(l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_languages() {
        let s = ScriptShares::of("ශ්‍රී ලංකා Sri Lanka இலங்கை");
        assert!(s.si > 0 && s.ta > 0 && s.latin == 8);
        let langs = s.languages(0.1, 5);
        assert_eq!(langs.len(), 3);
        assert_eq!(ScriptShares::of("abc").languages(0.1, 20), Vec::<Lang>::new());
    }

    #[test]
    fn parse_codes() {
        assert_eq!(Lang::parse("SIN"), Some(Lang::Si));
        assert_eq!(Lang::parse("tamil"), Some(Lang::Ta));
        assert_eq!(Lang::parse("xx"), None);
        assert_eq!(Lang::Ta.tesseract(), "tam");
    }
}
