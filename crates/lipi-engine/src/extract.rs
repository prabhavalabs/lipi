//! The extractor: detects the input format and dispatches to the right extractor.

use crate::config::{Event, ExtractConfig};
use crate::html::html_to_markdown;
use crate::ocr::{OcrEngine, lang_arg};
use crate::pdf;
use crate::text::{decode, text_to_markdown};
use anyhow::{Context, Result, bail};
use lipi_core::{Document, InputFormat, Method, Page, sniff};
use lipi_script::{Lang, ScriptShares, assess};
use lipi_sys::governor::Governor;
use lipi_sys::{Hardware, deps, paths};
use std::path::Path;
use std::time::Instant;

/// Extracts documents with a fixed configuration.
pub struct Extractor {
    cfg: ExtractConfig,
    ocr: Option<OcrEngine>,
    governor: Governor,
    notes: Vec<String>,
}

impl Extractor {
    /// Build an extractor, locating Tesseract, the pinned models and PDFium.
    pub fn new(cfg: ExtractConfig) -> Self {
        let hw = Hardware::probe();
        lipi_sys::governor::apply_priority(cfg.profile);
        let governor = Governor::new(cfg.profile, &hw);
        let mut notes = Vec::new();
        let ocr = deps::find_tesseract().map(|tesseract| {
            let tessdata = if deps::tessdata_ready() {
                Some(paths::tessdata_dir())
            } else {
                notes.push("lipi's pinned OCR models are not installed; using Tesseract's own models (run `lipi setup`)".into());
                None
            };
            OcrEngine { tesseract, tessdata, profile: cfg.profile }
        });
        if ocr.is_none() {
            notes.push(
                "Tesseract was not found: scanned pages and images cannot be read (run `lipi setup`)".into(),
            );
        }
        Extractor { cfg, ocr, governor, notes }
    }

    /// Setup notes (missing components) to show the user once.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Extract one file.
    pub fn extract_path(&self, path: &Path, events: &(dyn Fn(Event) + Sync)) -> Result<Document> {
        let t0 = Instant::now();
        let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let Some(format) = sniff(path, &bytes) else {
            bail!("{}: unsupported or unrecognised file format", path.display());
        };
        let mut doc = Document::new(path.display().to_string(), format);
        events(Event::DocumentStart { source: doc.source.clone(), pages: None });
        match format {
            InputFormat::Pdf => {
                let (pages, warnings) =
                    pdf::extract(&bytes, &self.cfg, self.ocr.as_ref(), &self.governor, events)?;
                doc.pages = pages;
                doc.warnings.extend(warnings);
            }
            InputFormat::Image => {
                doc.pages = self.extract_image(path, events)?;
            }
            InputFormat::Html => {
                let (md, title) = html_to_markdown(&decode(&bytes));
                if let Some(t) = title {
                    doc.metadata.insert("title".into(), t);
                }
                doc.pages.push(markup_page(lipi_script::normalize(&md), Method::Markup));
            }
            InputFormat::Markdown => {
                doc.pages.push(markup_page(lipi_script::normalize(&decode(&bytes)), Method::Markup));
            }
            InputFormat::Text => {
                doc.pages.push(markup_page(
                    lipi_script::normalize(&text_to_markdown(&decode(&bytes))),
                    Method::Markup,
                ));
            }
            InputFormat::Office => {
                let fmt = anydoc::Format::from_bytes(&bytes).or_else(|| anydoc::Format::from_path(path));
                let md = anydoc::to_markdown_bytes(&bytes, fmt).map_err(|e| anyhow::anyhow!("{e}"))?;
                doc.pages.push(markup_page(lipi_script::normalize(&md), Method::Office));
            }
        }
        // Markup inputs cannot be OCRed; warn when they carry legacy-font text.
        for p in doc.pages.iter_mut().filter(|p| matches!(p.method, Method::Markup | Method::Office)) {
            let h = assess(&p.markdown, self.cfg.langs.first().copied());
            if h.reasons.iter().any(|r| r == "latin_gibberish") {
                p.flags.push("legacy_font_text".into());
                doc.warnings.push("text appears to be typed in a legacy (non-Unicode) Sinhala/Tamil font; a font converter is needed".into());
            } else if h.reasons.iter().any(|r| r == "visual_order") {
                p.flags.push("visual_order".into());
            }
        }
        doc.seconds = t0.elapsed().as_secs_f32();
        Ok(doc)
    }

    fn extract_image(&self, path: &Path, events: &(dyn Fn(Event) + Sync)) -> Result<Vec<Page>> {
        let Some(engine) = &self.ocr else {
            bail!("{}: images need Tesseract; run `lipi setup`", path.display());
        };
        self.governor.admit(|h| events(Event::Hold(pdf::describe_hold(h))));
        let t0 = Instant::now();
        let scratch = tempfile::Builder::new().prefix("lipi-").tempdir()?;
        // Tesseract reads most formats itself; normalise anything else (e.g. WebP, GIF) to PNG.
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        let input = if matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "tif" | "tiff" | "bmp" | "pnm" | "pbm" | "pgm" | "ppm"
        ) {
            path.to_path_buf()
        } else {
            let p = scratch.path().join("input.png");
            image::open(path)?.to_luma8().save(&p)?;
            p
        };
        let mut flags = Vec::new();
        let langs: Vec<Lang> = if !self.cfg.langs.is_empty() {
            self.cfg.langs.clone()
        } else {
            match engine.probe_langs(&input, scratch.path()) {
                Ok(v) if !v.is_empty() => {
                    flags.push(format!("probed_langs:{}", lang_arg(&v)));
                    v
                }
                _ => vec![Lang::En],
            }
        };
        events(Event::OcrQueued { pages: 1 });
        let results = engine.recognize(&input, &langs, 3, None)?;
        events(Event::OcrPageDone);
        let secs = t0.elapsed().as_secs_f32();
        let n = results.len().max(1) as f32;
        Ok(results
            .into_iter()
            .map(|r| {
                let mut p = Page::new(
                    r.page,
                    Method::Ocr { engine: engine.engine_id(), langs: lang_arg(&langs), dpi: 0 },
                );
                p.markdown = lipi_script::normalize(&r.text);
                p.langs = ScriptShares::of(&p.markdown).languages(0.05, 20);
                p.confidence = r.confidence;
                p.flags = flags.clone();
                if r.confidence.is_some_and(|c| c < 70.0) {
                    p.flags.push("low_ocr_confidence".into());
                }
                p.seconds = secs / n;
                p
            })
            .collect())
    }
}

fn markup_page(markdown: String, method: Method) -> Page {
    let mut p = Page::new(1, method);
    p.langs = ScriptShares::of(&markdown).languages(0.05, 20);
    p.markdown = markdown;
    p
}
