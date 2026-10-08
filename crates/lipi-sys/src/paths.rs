//! Where lipi keeps its downloaded models and libraries.
//!
//! The data directory is `$LIPI_HOME` when set, otherwise the platform data directory:
//! `~/Library/Application Support/org.prabhavalabs.lipi` (macOS), `$XDG_DATA_HOME/lipi` or
//! `~/.local/share/lipi` (Linux), `%APPDATA%\prabhavalabs\lipi\data` (Windows).

use crate::manifest::{PDFIUM_TAG, TESSDATA_SET};
use std::path::PathBuf;

/// Root data directory.
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("LIPI_HOME") {
        return PathBuf::from(p);
    }
    directories::ProjectDirs::from("org", "prabhavalabs", "lipi")
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".lipi"))
}

/// Directory holding the pinned Tesseract language models.
pub fn tessdata_dir() -> PathBuf {
    data_dir().join("tessdata").join(TESSDATA_SET)
}

/// Directory holding word lexicons for OCR post-correction (`lipi lexicon build`).
pub fn lexicon_dir() -> PathBuf {
    data_dir().join("lexicon")
}

/// Path of the lexicon for an ISO 639-1 language code, e.g. `si`.
pub fn lexicon_file(lang: &str) -> PathBuf {
    lexicon_dir().join(format!("{lang}.lex"))
}

/// Directory holding the pinned PDFium build.
pub fn pdfium_dir() -> PathBuf {
    data_dir().join("pdfium").join(PDFIUM_TAG.replace('/', "-"))
}

/// Path of the PDFium shared library inside [`pdfium_dir`].
pub fn pdfium_library() -> PathBuf {
    let dir = pdfium_dir();
    if cfg!(target_os = "windows") {
        dir.join("bin").join("pdfium.dll")
    } else if cfg!(target_os = "macos") {
        dir.join("lib").join("libpdfium.dylib")
    } else {
        dir.join("lib").join("libpdfium.so")
    }
}
