//! `lipi extract`.

use crate::ExtractArgs;
use anyhow::{Context, Result, bail};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use lipi_core::{Document, RenderOptions, render, sniff};
use lipi_engine::{Event, Extractor};
use std::io::{IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Collect input files: files as given, directories walked recursively for supported formats.
pub fn collect_inputs(inputs: &[PathBuf]) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut out = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut stack = vec![input.clone()];
            while let Some(dir) = stack.pop() {
                let mut entries: Vec<_> =
                    std::fs::read_dir(&dir)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                entries.sort();
                for p in entries {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    if name.starts_with('.') || name.contains(".gt.") {
                        continue;
                    }
                    if p.is_dir() {
                        stack.push(p);
                    } else if is_supported(&p) {
                        let rel = p.strip_prefix(input).unwrap_or(&p).to_path_buf();
                        out.push((p, rel));
                    }
                }
            }
        } else if input.is_file() {
            let rel = PathBuf::from(input.file_name().unwrap_or_default());
            out.push((input.clone(), rel));
        } else {
            bail!("{}: no such file or directory", input.display());
        }
    }
    Ok(out)
}

fn is_supported(p: &Path) -> bool {
    let mut head = [0u8; 4096];
    let n = std::fs::File::open(p).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    sniff(p, &head[..n]).is_some()
}

pub fn progress_bar(enabled: bool) -> ProgressBar {
    let pb = ProgressBar::with_draw_target(
        Some(0),
        if enabled { ProgressDrawTarget::stderr() } else { ProgressDrawTarget::hidden() },
    );
    pb.set_style(
        ProgressStyle::with_template("{spinner} OCR {pos}/{len} pages [{elapsed}] {wide_msg}")
            .expect("valid template"),
    );
    pb
}

/// Print a line above the progress bar when it is visible, or plainly on stderr otherwise.
pub fn say(pb: &ProgressBar, line: String) {
    if pb.is_hidden() {
        eprintln!("{line}");
    } else {
        pb.println(line);
    }
}

pub fn events_for(pb: &ProgressBar, quiet: bool) -> impl Fn(Event) + Sync + '_ {
    move |e| match e {
        Event::DocumentStart { source, .. } => pb.set_message(source),
        Event::OcrQueued { pages } => {
            pb.inc_length(pages as u64);
            pb.enable_steady_tick(std::time::Duration::from_millis(120));
        }
        Event::OcrPageDone => pb.inc(1),
        Event::Hold(m) if !quiet => say(pb, format!("  ⏸ {m}")),
        Event::Info(m) if !quiet => say(pb, format!("  ℹ {m}")),
        _ => {}
    }
}

/// One-line summary of a document for stderr.
pub fn summary(doc: &Document) -> String {
    let methods = doc.method_counts().iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(" · ");
    let langs = doc.langs().iter().map(|l| l.code()).collect::<Vec<_>>().join("+");
    let conf = doc.mean_ocr_confidence().map(|c| format!("  OCR conf {c:.0}")).unwrap_or_default();
    format!(
        "✓ {}  {} page(s)  {}  [{}]{}  {:.1}s",
        doc.source,
        doc.pages.len(),
        methods,
        if langs.is_empty() { "-" } else { &langs },
        conf,
        doc.seconds
    )
}

pub fn run(a: ExtractArgs) -> Result<ExitCode> {
    let cfg = a.opts.config().map_err(anyhow::Error::msg)?;
    let files = collect_inputs(&a.inputs)?;
    if files.is_empty() {
        bail!("no supported documents found");
    }
    let to_stdout = a.output.is_none() && files.len() == 1;
    if a.output.is_none() && files.len() > 1 {
        bail!("{} inputs: give an output directory with -o", files.len());
    }
    let extractor = Extractor::new(cfg);
    let interactive = std::io::stderr().is_terminal() && !a.quiet;
    if !a.quiet {
        for n in extractor.notes() {
            eprintln!("note: {n}");
        }
    }
    let pb = progress_bar(interactive);
    let events = events_for(&pb, a.quiet);
    let opts = RenderOptions { page_markers: a.page_markers };
    let mut failures = 0;
    for (path, rel) in &files {
        let doc = match extractor.extract_path(path, &events) {
            Ok(d) => d,
            Err(e) => {
                failures += 1;
                say(&pb, format!("✗ {}: {e:#}", path.display()));
                continue;
            }
        };
        let rendered = render(&doc, a.format, opts);
        if to_stdout {
            pb.finish_and_clear();
            std::io::stdout().write_all(rendered.as_bytes())?;
        } else {
            let out = a.output.as_ref().expect("checked above");
            let target = if files.len() == 1 && !out.is_dir() && out.extension().is_some() {
                out.clone()
            } else {
                out.join(rel).with_extension(a.format.extension())
            };
            if let Some(dir) = target.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&target, rendered).with_context(|| format!("writing {}", target.display()))?;
        }
        if !a.quiet {
            say(&pb, summary(&doc));
            for w in &doc.warnings {
                say(&pb, format!("  ⚠ {w}"));
            }
            let flagged: Vec<String> = doc
                .pages
                .iter()
                .filter(|p| !p.flags.is_empty())
                .take(6)
                .map(|p| format!("p{} {}", p.number, p.flags.join(",")))
                .collect();
            if !flagged.is_empty() {
                say(&pb, format!("  · {}", flagged.join("; ")));
            }
        }
    }
    pb.finish_and_clear();
    Ok(if failures == 0 { ExitCode::SUCCESS } else { ExitCode::from(2) })
}
