//! The extraction result: a document made of pages, each with its Markdown and provenance.

use lipi_script::Lang;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::format::InputFormat;

/// How a page's text was obtained.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Method {
    /// The PDF text layer, used as is.
    TextLayer,
    /// The PDF text layer after deterministic visual-order repair.
    TextLayerRepaired {
        /// Vowel signs moved.
        moved: usize,
    },
    /// The PDF text layer typed in a legacy (non-Unicode) font, converted to Unicode.
    LegacyConverted {
        /// Legacy font family, e.g. `fm-abhaya`.
        family: String,
    },
    /// Optical character recognition of the rendered page or image.
    Ocr {
        /// Engine identifier, e.g. `tesseract-5.5.2`.
        engine: String,
        /// Language packs used, e.g. `sin+eng`.
        langs: String,
        /// Rendering resolution.
        dpi: u32,
    },
    /// Parsed from markup (HTML, Markdown, plain text).
    Markup,
    /// Converted from an office format (DOCX, PPTX, XLSX, ODT, RTF, EPUB, ...).
    Office,
    /// The page has no text.
    Empty,
    /// Extraction failed; the reason is recorded.
    Failed {
        /// Human-readable reason.
        reason: String,
    },
}

impl Method {
    /// Short label for summaries.
    pub fn label(&self) -> &'static str {
        match self {
            Method::TextLayer => "text-layer",
            Method::TextLayerRepaired { .. } => "text-layer-repaired",
            Method::LegacyConverted { .. } => "legacy-converted",
            Method::Ocr { .. } => "ocr",
            Method::Markup => "markup",
            Method::Office => "office",
            Method::Empty => "empty",
            Method::Failed { .. } => "failed",
        }
    }
}

/// One page (or the single logical page of a non-paginated input).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Page {
    /// 1-based page number.
    pub number: u32,
    /// How the text was obtained.
    pub method: Method,
    /// Page content as Markdown.
    pub markdown: String,
    /// Languages detected in the final text, largest share first.
    pub langs: Vec<Lang>,
    /// Mean OCR word confidence (0-100), when OCR was used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    /// Why the router chose the method, and quality warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub flags: Vec<String>,
    /// Wall-clock seconds spent on the page.
    pub seconds: f32,
}

impl Page {
    /// A page with the given number and method and no content yet.
    pub fn new(number: u32, method: Method) -> Self {
        Page {
            number,
            method,
            markdown: String::new(),
            langs: Vec::new(),
            confidence: None,
            flags: Vec::new(),
            seconds: 0.0,
        }
    }
}

/// The extraction result for one input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    /// Input path as given.
    pub source: String,
    /// Detected input format.
    pub format: InputFormat,
    /// Pages in order.
    pub pages: Vec<Page>,
    /// Document metadata (title, author, producer, ...).
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub metadata: BTreeMap<String, String>,
    /// Document-level warnings.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub warnings: Vec<String>,
    /// Wall-clock seconds for the whole document.
    pub seconds: f32,
    /// lipi version that produced the result.
    pub lipi_version: String,
}

impl Document {
    /// Empty document for a source.
    pub fn new(source: impl Into<String>, format: InputFormat) -> Self {
        Document {
            source: source.into(),
            format,
            pages: Vec::new(),
            metadata: BTreeMap::new(),
            warnings: Vec::new(),
            seconds: 0.0,
            lipi_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Languages across all pages, largest total first.
    pub fn langs(&self) -> Vec<Lang> {
        let all: String = self.pages.iter().map(|p| p.markdown.as_str()).collect::<Vec<_>>().join("\n");
        lipi_script::ScriptShares::of(&all).languages(0.05, 20)
    }

    /// Count of pages per method label.
    pub fn method_counts(&self) -> BTreeMap<&'static str, usize> {
        let mut m = BTreeMap::new();
        for p in &self.pages {
            *m.entry(p.method.label()).or_insert(0) += 1;
        }
        m
    }

    /// Mean OCR confidence over OCR pages.
    pub fn mean_ocr_confidence(&self) -> Option<f32> {
        let v: Vec<f32> = self.pages.iter().filter_map(|p| p.confidence).collect();
        if v.is_empty() { None } else { Some(v.iter().sum::<f32>() / v.len() as f32) }
    }
}
