//! One-time installation of pinned assets and the Tesseract engine.
//!
//! Downloads stream to a `.part` file while hashing, are verified against the pinned SHA-256
//! and only then renamed into place. A file that is already present with the right checksum is
//! not downloaded again.

use crate::deps::which;
use crate::manifest::{Asset, TESSDATA, pdfium_asset};
use crate::paths;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};

/// SHA-256 of a file, lower-case hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut f = BufReader::new(File::open(path)?);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Download `asset` to `dest`, verifying its checksum. `progress(done, total)` is called as
/// bytes arrive. Returns `false` when the file was already present and valid.
pub fn fetch(asset: &Asset, dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<bool> {
    if dest.is_file() && sha256_file(dest)? == asset.sha256 {
        progress(asset.bytes, asset.bytes);
        return Ok(false);
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let part = dest.with_extension("part");
    let resp = ureq::get(asset.url).call().with_context(|| format!("downloading {}", asset.url))?;
    let mut reader = resp.into_body().into_reader();
    let mut out = File::create(&part)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, asset.bytes);
    }
    out.flush()?;
    drop(out);
    let got = hex::encode(h.finalize());
    if got != asset.sha256 {
        let _ = std::fs::remove_file(&part);
        bail!("checksum mismatch for {}: expected {}, got {got}", asset.name, asset.sha256);
    }
    std::fs::rename(&part, dest)?;
    Ok(true)
}

/// Install every pinned Tesseract model.
pub fn install_tessdata(mut progress: impl FnMut(&Asset, u64, u64)) -> Result<PathBuf> {
    let dir = paths::tessdata_dir();
    for a in TESSDATA {
        fetch(a, &dir.join(a.path), |d, t| progress(a, d, t))?;
    }
    Ok(dir)
}

/// Install the pinned PDFium build for this platform.
pub fn install_pdfium(mut progress: impl FnMut(&Asset, u64, u64)) -> Result<PathBuf> {
    let asset = pdfium_asset().context("no PDFium build is published for this platform")?;
    let dir = paths::pdfium_dir();
    let archive = dir.join(asset.path);
    fetch(&asset, &archive, |d, t| progress(&asset, d, t))?;
    let lib = paths::pdfium_library();
    if !lib.is_file() {
        let gz = flate2::read::GzDecoder::new(File::open(&archive)?);
        let mut tar = tar::Archive::new(gz);
        for entry in tar.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.to_path_buf();
            let keep = path.starts_with("lib")
                || path.starts_with("bin")
                || path.file_name().is_some_and(|n| n == "LICENSE");
            if keep && !path.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                entry.unpack_in(&dir)?;
            }
        }
    }
    if !lib.is_file() {
        bail!("PDFium archive did not contain {}", lib.display());
    }
    Ok(lib)
}

/// Commands that install the Tesseract engine with the platform's package manager, in order of
/// preference. Language models are not taken from the package manager; lipi installs its own
/// pinned models.
pub fn tesseract_install_plan() -> Option<Vec<String>> {
    let sudo = |cmd: &[&str]| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        #[cfg(unix)]
        if unsafe { libc::geteuid() } != 0 && which("sudo").is_some() {
            v.push("sudo".into());
        }
        v.extend(cmd.iter().map(|s| s.to_string()));
        v
    };
    if cfg!(target_os = "macos") {
        if which("brew").is_some() {
            return Some(vec!["brew".into(), "install".into(), "tesseract".into()]);
        }
        if which("port").is_some() {
            return Some(sudo(&["port", "install", "tesseract"]));
        }
        return None;
    }
    if cfg!(target_os = "windows") {
        if which("winget").is_some() {
            return Some(
                [
                    "winget",
                    "install",
                    "--id",
                    "UB-Mannheim.TesseractOCR",
                    "-e",
                    "--accept-source-agreements",
                    "--accept-package-agreements",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            );
        }
        if which("choco").is_some() {
            return Some(["choco", "install", "tesseract", "-y"].iter().map(|s| s.to_string()).collect());
        }
        if which("scoop").is_some() {
            return Some(["scoop", "install", "tesseract"].iter().map(|s| s.to_string()).collect());
        }
        return None;
    }
    for (pm, args) in [
        ("apt-get", &["apt-get", "install", "-y", "tesseract-ocr"][..]),
        ("dnf", &["dnf", "install", "-y", "tesseract"][..]),
        ("pacman", &["pacman", "-S", "--noconfirm", "tesseract"][..]),
        ("zypper", &["zypper", "install", "-y", "tesseract-ocr"][..]),
        ("apk", &["apk", "add", "tesseract-ocr"][..]),
    ] {
        if which(pm).is_some() {
            return Some(sudo(args));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_content() {
        let dir = tempfile_dir();
        let p = dir.join("x.txt");
        std::fs::write(&p, b"abc").unwrap();
        assert_eq!(
            sha256_file(&p).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("lipi-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_pinned_asset_has_a_checksum() {
        for a in TESSDATA.iter().chain(pdfium_asset().iter()) {
            assert_eq!(a.sha256.len(), 64, "{}", a.name);
            assert!(a.url.starts_with("https://"));
        }
    }
}
