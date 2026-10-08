//! Extraction settings and progress events.

use lipi_script::Lang;
use lipi_sys::Profile;
use std::str::FromStr;

/// When to use OCR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OcrMode {
    /// Use the text layer when it passes the checks; OCR otherwise.
    #[default]
    Auto,
    /// OCR every PDF page, ignoring the text layer.
    Always,
    /// Never OCR; pages without a usable text layer are reported as failed.
    Never,
}

impl FromStr for OcrMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Ok(OcrMode::Auto),
            "always" | "force" => Ok(OcrMode::Always),
            "never" | "off" => Ok(OcrMode::Never),
            other => Err(format!("unknown OCR mode `{other}` (use auto, always or never)")),
        }
    }
}

/// Extraction settings.
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    /// Languages the caller expects. Empty means detect.
    pub langs: Vec<Lang>,
    /// When to use OCR.
    pub ocr: OcrMode,
    /// Cross-check Sinhala/Tamil text layers against OCR of a sample page.
    pub verify: bool,
    /// Repair visual-order vowel signs instead of OCRing when that is the only defect.
    pub repair: bool,
    /// Rendering resolution for OCR.
    pub dpi: u32,
    /// Resource profile.
    pub profile: Profile,
    /// Override the profile's worker count.
    pub workers: Option<usize>,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        ExtractConfig {
            langs: Vec::new(),
            ocr: OcrMode::Auto,
            verify: true,
            repair: false,
            dpi: 300,
            profile: Profile::Balanced,
            workers: None,
        }
    }
}

/// Progress events reported while extracting.
#[derive(Debug, Clone)]
pub enum Event {
    /// A document started; `pages` is known for paginated inputs.
    DocumentStart {
        /// Source path.
        source: String,
        /// Page count, when known.
        pages: Option<u32>,
    },
    /// OCR jobs were queued.
    OcrQueued {
        /// Number of pages queued for OCR.
        pages: usize,
    },
    /// One OCR page finished.
    OcrPageDone,
    /// The governor is holding new work back.
    Hold(String),
    /// Informational message.
    Info(String),
}
