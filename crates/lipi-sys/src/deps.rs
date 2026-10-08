//! Discovery of the external components lipi uses: the Tesseract engine, the pinned language
//! models and the PDFium renderer.

use crate::manifest::{Asset, TESSDATA};
use crate::paths;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The Tesseract executable.
#[derive(Debug, Clone, Serialize)]
pub struct Tesseract {
    /// Path to the executable.
    pub path: PathBuf,
    /// Version string, e.g. `5.5.2`.
    pub version: String,
}

impl Tesseract {
    /// Major version number.
    pub fn major(&self) -> u32 {
        self.version.split('.').next().and_then(|v| v.parse().ok()).unwrap_or(0)
    }
}

fn exe_name(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

/// Find a program on `PATH`.
pub fn which(name: &str) -> Option<PathBuf> {
    let exe = exe_name(name);
    std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(&exe)).find(|p| p.is_file()))
}

/// Locate Tesseract: `$LIPI_TESSERACT`, then `PATH`, then the usual install locations.
pub fn find_tesseract() -> Option<Tesseract> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(p) = std::env::var_os("LIPI_TESSERACT") {
        candidates.push(PathBuf::from(p));
    }
    if let Some(p) = which("tesseract") {
        candidates.push(p);
    }
    for p in [
        "/opt/homebrew/bin/tesseract",
        "/usr/local/bin/tesseract",
        "/usr/bin/tesseract",
        r"C:\Program Files\Tesseract-OCR\tesseract.exe",
        r"C:\Program Files (x86)\Tesseract-OCR\tesseract.exe",
    ] {
        candidates.push(PathBuf::from(p));
    }
    candidates.into_iter().filter(|p| p.is_file()).find_map(|path| {
        let out = Command::new(&path).arg("--version").output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
        let version = text
            .lines()
            .find_map(|l| l.trim().strip_prefix("tesseract "))
            .map(|v| v.trim_start_matches('v').split_whitespace().next().unwrap_or("").to_string())?;
        Some(Tesseract { path, version })
    })
}

/// State of one pinned file.
#[derive(Debug, Clone, Serialize)]
pub struct FileState {
    /// Asset name.
    pub name: &'static str,
    /// Expected location.
    pub path: PathBuf,
    /// Present with the expected size.
    pub present: bool,
}

fn file_state(asset: &Asset, dir: &Path) -> FileState {
    let path = dir.join(asset.path);
    let present = std::fs::metadata(&path).map(|m| m.len() == asset.bytes).unwrap_or(false);
    FileState { name: asset.name, path, present }
}

/// State of the pinned Tesseract models.
pub fn tessdata_state() -> Vec<FileState> {
    let dir = paths::tessdata_dir();
    TESSDATA.iter().map(|a| file_state(a, &dir)).collect()
}

/// Whether every pinned Tesseract model is installed.
pub fn tessdata_ready() -> bool {
    tessdata_state().iter().all(|s| s.present)
}

/// Location of the PDFium library: `$LIPI_PDFIUM` or the pinned install.
pub fn pdfium_library() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("LIPI_PDFIUM") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let p = paths::pdfium_library();
    p.is_file().then_some(p)
}
