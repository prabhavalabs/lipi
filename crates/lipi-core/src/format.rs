//! Input format detection from content, with the file extension as a fallback.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Detected input format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputFormat {
    /// PDF.
    Pdf,
    /// Raster image (PNG, JPEG, TIFF, BMP, WebP, GIF).
    Image,
    /// HTML or XHTML.
    Html,
    /// Markdown.
    Markdown,
    /// Plain text.
    Text,
    /// Office or e-book container (DOC, DOCX, ODT, PPT/PPTX, XLS/XLSX, RTF, EPUB, CSV, ...).
    Office,
}

const OFFICE_EXT: &[&str] = &[
    "doc", "docx", "docm", "odt", "ppt", "pptx", "pptm", "pps", "ppsx", "odp", "xls", "xlsx", "xlsm", "xlsb",
    "ods", "rtf", "epub", "csv",
];
const IMAGE_EXT: &[&str] =
    &["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp", "gif", "jp2", "pnm", "pbm", "pgm", "ppm"];

fn ext(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Detect the format of `bytes` (the start of the file is enough) read from `path`.
pub fn sniff(path: &Path, bytes: &[u8]) -> Option<InputFormat> {
    let e = ext(path);
    let head = &bytes[..bytes.len().min(4096)];
    // Binary signatures first.
    if head.starts_with(b"%PDF") || head.windows(5).take(1024).any(|w| w == b"%PDF-") {
        return Some(InputFormat::Pdf);
    }
    let image = head.starts_with(b"\x89PNG")
        || head.starts_with(b"\xFF\xD8\xFF")
        || head.starts_with(b"II*\0")
        || head.starts_with(b"MM\0*")
        || head.starts_with(b"BM") && IMAGE_EXT.contains(&e.as_str())
        || head.starts_with(b"GIF8")
        || (head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP")
        || head.starts_with(b"\0\0\0\x0CjP  ");
    if image {
        return Some(InputFormat::Image);
    }
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"\xD0\xCF\x11\xE0") || head.starts_with(b"{\\rtf")
    {
        return Some(InputFormat::Office);
    }
    if OFFICE_EXT.contains(&e.as_str()) {
        return Some(InputFormat::Office);
    }
    // Text-like content.
    let text = String::from_utf8_lossy(head);
    let lower = text.trim_start_matches('\u{FEFF}').trim_start().to_ascii_lowercase();
    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<?xml") && lower.contains("<html")
        || matches!(e.as_str(), "html" | "htm" | "xhtml")
        || (lower.contains("<body") && lower.contains("</"))
    {
        return Some(InputFormat::Html);
    }
    if matches!(e.as_str(), "md" | "markdown" | "mdx") {
        return Some(InputFormat::Markdown);
    }
    if IMAGE_EXT.contains(&e.as_str()) {
        return Some(InputFormat::Image);
    }
    if e == "pdf" {
        return Some(InputFormat::Pdf);
    }
    let printable = head.iter().filter(|&&b| b >= 0x20 || matches!(b, b'\n' | b'\r' | b'\t')).count();
    if head.is_empty() || printable * 100 / head.len().max(1) >= 95 {
        return Some(InputFormat::Text);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_win_over_extensions() {
        assert_eq!(sniff(Path::new("a.txt"), b"%PDF-1.7\n"), Some(InputFormat::Pdf));
        assert_eq!(sniff(Path::new("a.txt"), b"<!DOCTYPE html><html>"), Some(InputFormat::Html));
        assert_eq!(sniff(Path::new("x"), b"\x89PNG\r\n\x1a\n"), Some(InputFormat::Image));
        assert_eq!(sniff(Path::new("x.docx"), b"PK\x03\x04"), Some(InputFormat::Office));
        assert_eq!(sniff(Path::new("notes.md"), "# Title\nශ්‍රී".as_bytes()), Some(InputFormat::Markdown));
        assert_eq!(sniff(Path::new("a.txt"), "இலங்கை".as_bytes()), Some(InputFormat::Text));
        assert_eq!(sniff(Path::new("a.bin"), &[0u8, 1, 2, 3, 4, 5, 6, 7]), None);
    }
}
