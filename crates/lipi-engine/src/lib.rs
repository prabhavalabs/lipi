//! Extraction engine: format-specific extractors, the OCR driver and the page router.

pub mod config;
pub mod extract;
pub mod html;
pub mod legacy;
pub mod metrics;
pub mod ocr;
pub mod pdf;
pub mod postcorrect;
pub mod text;

pub use config::{Event, ExtractConfig, OcrMode};
pub use extract::Extractor;
