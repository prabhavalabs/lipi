//! `lipi lexicon build`: word-frequency lexicons for OCR post-correction.

use anyhow::{Context, Result, bail};
use lipi_script::Lang;
use lipi_script::correct::Lexicon;
use lipi_sys::paths;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Text files among `inputs`; directories are searched recursively for `.txt` and `.md` files.
fn text_files(inputs: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut stack = vec![input.clone()];
            while let Some(dir) = stack.pop() {
                let mut entries: Vec<PathBuf> =
                    std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                entries.sort();
                for p in entries {
                    if p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('.')) {
                        continue;
                    }
                    if p.is_dir() {
                        stack.push(p);
                    } else if p
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| matches!(e, "txt" | "md"))
                    {
                        out.push(p);
                    }
                }
            }
        } else if input.is_file() {
            out.push(input.clone());
        } else {
            bail!("{}: no such file or directory", input.display());
        }
    }
    Ok(out)
}

pub fn build(inputs: &[PathBuf], lang: &str, min_count: u32, output: Option<&Path>) -> Result<ExitCode> {
    let lang =
        Lang::parse(lang).ok_or_else(|| anyhow::anyhow!("unknown language `{lang}` (use si, ta or en)"))?;
    let files = text_files(inputs)?;
    if files.is_empty() {
        bail!("no text files (.txt, .md) found");
    }
    let mut lexicon = Lexicon::new(lang);
    for f in &files {
        let bytes = std::fs::read(f).with_context(|| format!("reading {}", f.display()))?;
        lexicon.add_text(&String::from_utf8_lossy(&bytes));
    }
    if lexicon.is_empty() {
        bail!("no {lang} words found in {} file(s)", files.len());
    }
    lexicon.prune(min_count.max(1));
    let out = output.map(Path::to_path_buf).unwrap_or_else(|| paths::lexicon_file(lang.code()));
    lexicon.save(&out).with_context(|| format!("writing {}", out.display()))?;
    eprintln!(
        "{} file(s), {} words, {} distinct (count ≥ {}) → {}",
        files.len(),
        lexicon.tokens(),
        lexicon.len(),
        min_count.max(1),
        out.display()
    );
    Ok(ExitCode::SUCCESS)
}
