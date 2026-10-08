//! `lipi bench`: accuracy against ground truth.

use crate::EngineArgs;
use crate::extract::{collect_inputs, events_for, progress_bar, say};
use anyhow::{Result, bail};
use lipi_core::{OutputFormat, RenderOptions, render};
use lipi_engine::Extractor;
use lipi_engine::metrics::{cer, wer};
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Serialize)]
struct Row {
    file: String,
    lang: String,
    cer: f64,
    wer: f64,
    seconds: f32,
    methods: String,
}

fn ground_truth(path: &Path) -> Option<PathBuf> {
    let stem = path.file_stem()?.to_str()?;
    let dir = path.parent()?;
    for cand in [format!("{stem}.gt.txt"), format!("{stem}.gt.md")] {
        let p = dir.join(&cand);
        if p.is_file() {
            return Some(p);
        }
    }
    // `name_variant.png` may share `name.gt.txt` (e.g. clean and degraded renderings).
    let base = stem.rsplit_once('_').map(|(b, _)| b)?;
    let p = dir.join(format!("{base}.gt.txt"));
    p.is_file().then_some(p)
}

pub fn run(dir: &Path, json: Option<&Path>, opts: &EngineArgs) -> Result<ExitCode> {
    let files: Vec<(PathBuf, PathBuf)> = collect_inputs(&[dir.to_path_buf()])?
        .into_iter()
        .filter(|(p, _)| ground_truth(p).is_some())
        .collect();
    if files.is_empty() {
        bail!("no documents with ground truth (`<name>.gt.txt`) in {}", dir.display());
    }
    let extractor = Extractor::new(opts.config().map_err(anyhow::Error::msg)?);
    let pb = progress_bar(std::io::stderr().is_terminal());
    let events = events_for(&pb, false);
    let mut rows = Vec::new();
    for (path, rel) in &files {
        let gt = std::fs::read_to_string(ground_truth(path).expect("filtered"))?;
        let doc = match extractor.extract_path(path, &events) {
            Ok(d) => d,
            Err(e) => {
                say(&pb, format!("✗ {}: {e:#}", path.display()));
                continue;
            }
        };
        let hyp = render(&doc, OutputFormat::Text, RenderOptions::default());
        let lang =
            lipi_script::ScriptShares::of(&gt).dominant().map(|l| l.code().to_string()).unwrap_or_default();
        let methods = doc.method_counts().keys().copied().collect::<Vec<_>>().join("+");
        rows.push(Row {
            file: rel.display().to_string(),
            lang,
            cer: cer(&gt, &hyp),
            wer: wer(&gt, &hyp),
            seconds: doc.seconds,
            methods,
        });
    }
    pb.finish_and_clear();
    println!("{:<40} {:>4} {:>8} {:>8} {:>7}  method", "file", "lang", "CER %", "WER %", "sec");
    for r in &rows {
        println!(
            "{:<40} {:>4} {:>8.2} {:>8.2} {:>7.1}  {}",
            r.file,
            r.lang,
            r.cer * 100.0,
            r.wer * 100.0,
            r.seconds,
            r.methods
        );
    }
    let mut by_lang: BTreeMap<&str, Vec<&Row>> = BTreeMap::new();
    for r in &rows {
        by_lang.entry(r.lang.as_str()).or_default().push(r);
    }
    println!();
    for (lang, rs) in &by_lang {
        let n = rs.len() as f64;
        println!(
            "{lang:<4} n={:<3} mean CER {:6.2}%  mean WER {:6.2}%  {:.1} s/file",
            rs.len(),
            rs.iter().map(|r| r.cer).sum::<f64>() / n * 100.0,
            rs.iter().map(|r| r.wer).sum::<f64>() / n * 100.0,
            rs.iter().map(|r| r.seconds as f64).sum::<f64>() / n
        );
    }
    if let Some(j) = json {
        std::fs::write(j, serde_json::to_string_pretty(&rows)?)?;
    }
    Ok(ExitCode::SUCCESS)
}
