//! PDF extraction and the per-page router.
//!
//! For every page lipi chooses the cheapest method whose output can be trusted:
//!
//! 1. **Text layer** (pdf-inspector): Markdown with headings and tables, in milliseconds.
//! 2. The text layer is rejected when pdf-inspector marks the page as needing OCR, when the
//!    page uses a legacy Sinhala/Tamil font, or when lipi's script checks find Latin gibberish,
//!    visual-order vowel signs, foreign code points or replacement characters.
//! 3. **Legacy-font conversion**: a page typed in a legacy font lipi has a table for (FM
//!    Abhaya) is converted to Unicode token by token, converting only the words PDFium reports
//!    as drawn with that font (see [`crate::legacy`]). The converted page must pass the script
//!    checks; otherwise the page is OCRed.
//! 4. **Verification**: a Sinhala/Tamil text layer (or converted page) that passes the checks is
//!    still compared with OCR of one sample page, because mapping errors such as a lost
//!    rakaransaya (`ශී` for `ශ්‍රී`) keep the text well-formed. When the two disagree, every
//!    Sinhala/Tamil page of the same kind in the document is OCRed.
//! 5. **OCR** (Tesseract): the page is rendered with PDFium and recognised with the language
//!    packs for the scripts on the page.

use crate::config::{Event, ExtractConfig, OcrMode};
use crate::legacy::{FontWords, convert_markdown, font_words};
use crate::metrics::agreement;
use crate::ocr::{OcrEngine, lang_arg};
use crate::postcorrect;
use anyhow::{Context, Result};
use lipi_core::{Method, Page};
use lipi_script::correct::Corrector;
use lipi_script::fonts::{is_prior_ocr_font, is_unnamed_truetype, legacy_font};
use lipi_script::health::Verdict;
use lipi_script::legacy::{Converter, converter};
use lipi_script::repair::repair_visual_order;
use lipi_script::{Lang, ScriptShares, assess};
use lipi_sys::governor::{Governor, Hold};
use pdfium_render::prelude::*;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

/// Minimum order-insensitive agreement between a Sinhala/Tamil text layer and OCR for the text
/// layer to be trusted.
pub const VERIFY_MIN_AGREEMENT: f64 = 0.90;
/// Minimum agreement between a converted legacy-font page and OCR. Lower than
/// [`VERIFY_MIN_AGREEMENT`] because here the OCR side carries the errors: on 41 held-out FM
/// Abhaya gazette pages the converted text and Tesseract agreed at 0.82-1.00 (median 0.94), the
/// low end being tables that the two read in different orders (see `bench/README.md`).
pub const LEGACY_VERIFY_MIN_AGREEMENT: f64 = 0.80;
/// Indic letters a page needs before it is used for verification.
const VERIFY_MIN_LETTERS: usize = 200;
/// OCR confidence below which a page is flagged.
const LOW_CONFIDENCE: f32 = 70.0;
/// OCR confidence below which the language packs are re-probed and the page retried once.
const RETRY_CONFIDENCE: f32 = 55.0;

static PDFIUM: OnceLock<Result<Pdfium, String>> = OnceLock::new();

/// Load PDFium once per process from lipi's pinned install or `$LIPI_PDFIUM`.
pub fn pdfium() -> Result<&'static Pdfium, String> {
    PDFIUM
        .get_or_init(|| {
            let lib = lipi_sys::deps::pdfium_library()
                .ok_or_else(|| "PDFium is not installed; run `lipi setup`".to_string())?;
            Pdfium::bind_to_library(&lib)
                .map(Pdfium::new)
                .map_err(|e| format!("loading {}: {e}", lib.display()))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Font names used on each page (0-based index), read with lopdf.
fn page_fonts(bytes: &[u8]) -> Vec<Vec<String>> {
    let Ok(doc) = lopdf::Document::load_mem(bytes) else { return Vec::new() };
    doc.get_pages()
        .values()
        .map(|&id| {
            doc.get_page_fonts(id)
                .map(|fonts| {
                    fonts
                        .values()
                        .filter_map(|f| f.get(b"BaseFont").ok().and_then(|o| o.as_name().ok()))
                        .map(|n| String::from_utf8_lossy(n).to_string())
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}

fn has_latin_font(fonts: &[String]) -> bool {
    const LATIN: &[&str] = &[
        "times",
        "arial",
        "helvetica",
        "calibri",
        "cambria",
        "verdana",
        "georgia",
        "garamond",
        "minion",
        "courier",
        "tahoma",
        "book",
    ];
    fonts.iter().any(|f| {
        let n = f.to_ascii_lowercase();
        LATIN.iter().any(|l| n.contains(l))
    })
}

/// Selects the page plans one verification sample stands for.
type MethodFilter = fn(&Method) -> bool;

/// Router decision for one page.
#[derive(Debug, Clone)]
enum Plan {
    Text { markdown: String, method: Method, flags: Vec<String> },
    Ocr { langs: Vec<Lang>, flags: Vec<String> },
    Empty,
}

struct OcrJob {
    index: usize,
    image: PathBuf,
    langs: Vec<Lang>,
}

/// Extract a PDF into pages.
pub fn extract(
    bytes: &[u8],
    cfg: &ExtractConfig,
    ocr: Option<&OcrEngine>,
    corrector: Option<&Corrector>,
    governor: &Governor,
    events: &(dyn Fn(Event) + Sync),
) -> Result<(Vec<Page>, Vec<String>)> {
    let mut warnings = Vec::new();
    let started = Instant::now();
    let inspected = pdf_inspector::extract_pages_markdown_mem(bytes, None);
    let fonts = page_fonts(bytes);
    let page_count = match &inspected {
        Ok(r) => r.pages.len(),
        Err(e) => {
            warnings.push(format!("text layer unreadable ({e}); using OCR for every page"));
            pdfium()
                .ok()
                .and_then(|p| p.load_pdf_from_byte_slice(bytes, None).ok().map(|d| d.pages().len() as usize))
                .unwrap_or(0)
        }
    };

    // 1. Plan every page from its text layer. Pages typed in a convertible legacy font also get
    //    per-font word sets from PDFium so that only legacy-font tokens are converted. When lopdf
    //    cannot read the file (damaged cross-reference tables are common), PDFium's font names
    //    stand in for pages whose text layer reads as Latin.
    let mut fonts = fonts;
    fonts.resize(page_count, Vec::new());
    let raw_md = |i: usize| inspected.as_ref().map(|r| r.pages[i].markdown.as_str()).unwrap_or("");
    let converting = cfg.legacy_convert && cfg.ocr != OcrMode::Always;
    let probe: Vec<usize> = (0..page_count)
        .filter(|&i| {
            converting && fonts[i].is_empty() && {
                let s = ScriptShares::of(raw_md(i));
                let garbled = inspected
                    .as_ref()
                    .is_ok_and(|r| r.pages[i].ocr_reason.as_deref().is_some_and(|x| x.contains("garbled")));
                (s.latin >= 100 && s.shares().latin > 0.5) || garbled
            }
        })
        .collect();
    let mut words: HashMap<usize, FontWords> =
        if probe.is_empty() { HashMap::new() } else { font_words(bytes, &probe) };
    for (&i, w) in &words {
        fonts[i] = w.fonts.clone();
    }
    let converters: Vec<Option<&'static Converter>> = (0..page_count)
        .map(|i| {
            if !converting {
                return None;
            }
            fonts[i].iter().find_map(|f| legacy_font(f)).and_then(|l| converter(l.family))
        })
        .collect();
    let convertible: Vec<usize> =
        (0..page_count).filter(|&i| converters[i].is_some() && !words.contains_key(&i)).collect();
    if !convertible.is_empty() {
        let w = font_words(bytes, &convertible);
        if w.len() < convertible.len() {
            warnings.push(
                "font information unavailable for some legacy-font pages; every token converted".into(),
            );
        }
        words.extend(w);
    }
    let mut plans: Vec<Plan> = Vec::with_capacity(page_count);
    for (i, conv) in converters.iter().enumerate() {
        let page_fonts = &fonts[i];
        let (needs_ocr, reason) = match &inspected {
            Ok(r) => (r.pages[i].needs_ocr, r.pages[i].ocr_reason.clone()),
            Err(_) => (true, Some("unreadable".into())),
        };
        let legacy = conv.map(|c| (c, words.get(&i)));
        plans.push(plan_page(raw_md(i), needs_ocr, reason, page_fonts, legacy, cfg));
    }

    // 2. Verify Sinhala/Tamil text layers, and converted legacy-font pages, against OCR of the
    //    richest page of each kind.
    let mut verified_ocr: HashMap<usize, Page> = HashMap::new();
    if cfg.verify
        && cfg.ocr == OcrMode::Auto
        && let Some(engine) = ocr
    {
        let groups: [(MethodFilter, f64, &str); 2] = [
            (|m| matches!(m, Method::TextLayer), VERIFY_MIN_AGREEMENT, "text layer"),
            (
                |m| matches!(m, Method::LegacyConverted { .. }),
                LEGACY_VERIFY_MIN_AGREEMENT,
                "converted legacy-font text",
            ),
        ];
        for (is_kind, min_agreement, label) in groups {
            let candidate = plans
                .iter()
                .enumerate()
                .filter_map(|(i, p)| match p {
                    Plan::Text { markdown, method, .. } if is_kind(method) => {
                        let s = ScriptShares::of(markdown);
                        (s.si + s.ta >= VERIFY_MIN_LETTERS).then_some((i, s.si + s.ta))
                    }
                    _ => None,
                })
                .max_by_key(|&(_, n)| n)
                .map(|(i, _)| i);
            let Some(i) = candidate else { continue };
            let Plan::Text { markdown, .. } = &plans[i] else { unreachable!() };
            let langs = ScriptShares::of(markdown).languages(0.15, 20);
            let tmp = tempfile::Builder::new().prefix("lipi-").tempdir()?;
            match render_pages(bytes, &[i], cfg.dpi, tmp.path()) {
                Ok(images) => {
                    let page = ocr_page(
                        engine,
                        corrector,
                        governor,
                        &images[0].image,
                        i,
                        &langs,
                        cfg,
                        events,
                        vec![],
                    );
                    let score = agreement(markdown, &page.markdown);
                    if score < min_agreement {
                        events(Event::Info(format!(
                            "{label} disagrees with OCR (agreement {score:.2}); using OCR for Sinhala/Tamil pages"
                        )));
                        for p in plans.iter_mut() {
                            if let Plan::Text { markdown, method, .. } = p
                                && is_kind(method)
                            {
                                let s = ScriptShares::of(markdown);
                                if s.si + s.ta >= 50 {
                                    *p = Plan::Ocr {
                                        langs: s.languages(0.15, 20),
                                        flags: vec![format!("verify_failed:{score:.2}")],
                                    };
                                }
                            }
                        }
                        let mut page = page;
                        page.flags.insert(0, format!("verify_failed:{score:.2}"));
                        verified_ocr.insert(i, page);
                    } else {
                        for p in plans.iter_mut() {
                            if let Plan::Text { markdown, method, flags } = p
                                && is_kind(method)
                            {
                                let s = ScriptShares::of(markdown);
                                if s.si + s.ta >= 50 {
                                    flags.push(format!("verified:{score:.2}"));
                                }
                            }
                        }
                    }
                }
                Err(e) => warnings.push(format!("verification skipped: {e}")),
            }
        }
    }

    // 3. OCR the pages that need it.
    let ocr_indices: Vec<usize> = plans
        .iter()
        .enumerate()
        .filter(|(i, p)| matches!(p, Plan::Ocr { .. }) && !verified_ocr.contains_key(i))
        .map(|(i, _)| i)
        .collect();
    let mut ocr_pages: Vec<Option<Page>> = vec![None; page_count];
    for (i, p) in verified_ocr.drain() {
        ocr_pages[i] = Some(p);
    }
    if !ocr_indices.is_empty() {
        match (ocr, cfg.ocr) {
            (_, OcrMode::Never) => {}
            (None, _) => warnings.push(format!(
                "{} page(s) need OCR but Tesseract is not available; run `lipi setup`",
                ocr_indices.len()
            )),
            (Some(engine), _) => {
                let tmp = tempfile::Builder::new().prefix("lipi-").tempdir()?;
                match render_pages(bytes, &ocr_indices, cfg.dpi, tmp.path()) {
                    Ok(images) => {
                        events(Event::OcrQueued { pages: images.len() });
                        let jobs: Vec<OcrJob> = ocr_indices
                            .iter()
                            .zip(images)
                            .map(|(&index, r)| {
                                let langs = match &plans[index] {
                                    Plan::Ocr { langs, .. } if !langs.is_empty() => langs.clone(),
                                    _ => r.text_langs,
                                };
                                OcrJob { index, image: r.image, langs }
                            })
                            .collect();
                        let workers = cfg
                            .workers
                            .unwrap_or_else(|| governor.profile().workers(&lipi_sys::Hardware::probe()));
                        let pool = rayon::ThreadPoolBuilder::new().num_threads(workers.max(1)).build()?;
                        let done: Vec<(usize, Page)> = pool.install(|| {
                            jobs.par_iter()
                                .map(|j| {
                                    let flags = match &plans[j.index] {
                                        Plan::Ocr { flags, .. } => flags.clone(),
                                        _ => vec![],
                                    };
                                    let page = ocr_page(
                                        engine, corrector, governor, &j.image, j.index, &j.langs, cfg,
                                        events, flags,
                                    );
                                    events(Event::OcrPageDone);
                                    (j.index, page)
                                })
                                .collect()
                        });
                        for (i, p) in done {
                            ocr_pages[i] = Some(p);
                        }
                    }
                    Err(e) => warnings.push(format!("cannot render pages for OCR: {e}")),
                }
            }
        }
    }

    // 4. Assemble.
    let mut pages = Vec::with_capacity(page_count);
    for (i, plan) in plans.into_iter().enumerate() {
        let n = i as u32 + 1;
        let page = match (plan, ocr_pages[i].take()) {
            (_, Some(p)) => p,
            (Plan::Text { markdown, method, flags }, None) => {
                let mut p = Page::new(n, method);
                p.langs = ScriptShares::of(&markdown).languages(0.05, 20);
                p.markdown = markdown;
                p.flags = flags;
                p
            }
            (Plan::Ocr { flags, .. }, None) => {
                // OCR unavailable or disabled: keep whatever text layer there was, flagged.
                let md = inspected
                    .as_ref()
                    .ok()
                    .map(|r| lipi_script::normalize(&r.pages[i].markdown))
                    .unwrap_or_default();
                let reason = if cfg.ocr == OcrMode::Never { "ocr_disabled" } else { "ocr_unavailable" };
                let mut p = if md.trim().is_empty() {
                    Page::new(n, Method::Failed { reason: format!("{reason}: {}", flags.join(",")) })
                } else {
                    Page::new(n, Method::TextLayer)
                };
                p.flags = flags;
                p.flags.push(format!("unreliable_text_layer:{reason}"));
                p.langs = ScriptShares::of(&md).languages(0.05, 20);
                p.markdown = md;
                p
            }
            (Plan::Empty, None) => Page::new(n, Method::Empty),
        };
        pages.push(page);
    }
    let text_secs = started.elapsed().as_secs_f32();
    for p in pages.iter_mut().filter(|p| !matches!(p.method, Method::Ocr { .. })) {
        p.seconds = text_secs / page_count.max(1) as f32;
    }
    Ok((pages, warnings))
}

/// Decide how to extract one page from its raw pdf-inspector Markdown. `convert` carries the
/// converter for the page's legacy font and PDFium's per-font words (`None` when unavailable).
fn plan_page(
    raw_md: &str,
    needs_ocr: bool,
    reason: Option<String>,
    fonts: &[String],
    convert: Option<(&Converter, Option<&FontWords>)>,
    cfg: &ExtractConfig,
) -> Plan {
    let md = &lipi_script::normalize(raw_md);
    let legacy = fonts.iter().find_map(|f| legacy_font(f));
    let expected = cfg.langs.first().copied().or(legacy.map(|l| l.lang));
    let health = assess(md, expected);
    let mut flags = Vec::new();
    if fonts.iter().any(|f| is_prior_ocr_font(f)) {
        flags.push("prior_ocr_layer".to_string());
    }
    if fonts.iter().any(|f| is_unnamed_truetype(f)) {
        flags.push("type3_truetype_fonts".to_string());
    }
    let langs_for_ocr = || -> Vec<Lang> {
        if !cfg.langs.is_empty() {
            return cfg.langs.clone();
        }
        if let Some(l) = legacy {
            let mut v = vec![l.lang];
            if has_latin_font(fonts) {
                v.push(Lang::En);
            }
            return v;
        }
        if health.latin_gibberish < 0.5 {
            // Scripts seen in the (possibly broken) text layer, keeping minor scripts such as a
            // Sinhala masthead on a Tamil page.
            let v: Vec<Lang> = health
                .letters
                .languages(0.03, 50)
                .into_iter()
                .filter(|&l| health.letters.count(l) >= 30)
                .collect();
            if !v.is_empty() {
                return v;
            }
        }
        Vec::new() // decided by probing the rendered page
    };
    if cfg.ocr == OcrMode::Always {
        flags.push("ocr_forced".into());
        return Plan::Ocr { langs: langs_for_ocr(), flags };
    }
    // A legacy-font page is converted before pdf-inspector's verdict is honoured: its
    // "suspected garbled text" is exactly what a legacy font looks like.
    if let Some(l) = legacy
        && (health.shares.latin > 0.5 || md.trim().is_empty())
        && let Some((conv, words)) = convert
        && conv.family == l.family
    {
        // Convert before normalisation: a legacy code such as U+00AD is a glyph here. When
        // pdf-inspector withheld the text it suspected of being garbled, PDFium's text stands in
        // (plain lines, no headings or tables).
        let source = match words {
            Some(w) if raw_md.trim().is_empty() && !w.text.trim().is_empty() => {
                flags.push("legacy_text_source:pdfium".into());
                w.text.as_str()
            }
            _ => raw_md,
        };
        let (converted, stats) = convert_markdown(source, words, conv);
        let after = assess(&converted, Some(conv.lang));
        if after.is_healthy() {
            flags.push(format!("legacy_converted:{}", conv.family));
            if words.is_none() {
                flags.push("legacy_convert:no_font_info".into());
            }
            if stats.unmatched > 0 {
                flags.push(format!("legacy_unmatched_tokens:{}", stats.unmatched));
            }
            return Plan::Text {
                markdown: converted,
                method: Method::LegacyConverted { family: conv.family.to_string() },
                flags,
            };
        }
        flags.push(format!("legacy_convert_failed:{}", after.reasons.join("+")));
    }
    if needs_ocr {
        flags.push(format!("inspector:{}", reason.unwrap_or_else(|| "unreliable_text".into())));
        return Plan::Ocr { langs: langs_for_ocr(), flags };
    }
    if let Some(l) = legacy
        && health.shares.latin > 0.5
    {
        flags.push(format!("legacy_font:{}", l.family));
        return Plan::Ocr { langs: langs_for_ocr(), flags };
    }
    match health.verdict {
        Verdict::Broken => {
            if cfg.repair && health.reasons == ["visual_order"] {
                let r = repair_visual_order(md);
                if assess(&r.text, expected).is_healthy() {
                    flags.push("visual_order_repaired".into());
                    return Plan::Text {
                        markdown: r.text,
                        method: Method::TextLayerRepaired { moved: r.moved },
                        flags,
                    };
                }
            }
            flags.push(format!("text_layer:{}", health.reasons.join("+")));
            Plan::Ocr { langs: langs_for_ocr(), flags }
        }
        Verdict::Empty if md.trim().is_empty() => Plan::Empty,
        _ => Plan::Text { markdown: md.clone(), method: Method::TextLayer, flags },
    }
}

/// A rendered page and the scripts its (possibly broken) text layer contains.
struct Rendered {
    image: PathBuf,
    text_langs: Vec<Lang>,
}

/// Languages implied by a text layer even when its text is unusable: broken Sinhala/Tamil
/// mappings still use code points of the right script. Legacy-font (Latin) text gives nothing.
///
/// A Latin-only hint is ignored: a page that needs OCR and whose text layer reads as Latin may
/// well be a legacy or unmapped Sinhala/Tamil font, so such pages are probed instead.
fn script_hint(text: &str) -> Vec<Lang> {
    let h = assess(text, None);
    if h.latin_gibberish >= 0.5 {
        return Vec::new();
    }
    let langs: Vec<Lang> =
        h.letters.languages(0.03, 50).into_iter().filter(|&l| h.letters.count(l) >= 30).collect();
    if langs.iter().any(|l| l.is_indic()) { langs } else { Vec::new() }
}

/// Render pages (0-based indices) to greyscale PNGs.
fn render_pages(bytes: &[u8], indices: &[usize], dpi: u32, dir: &Path) -> Result<Vec<Rendered>> {
    let pdfium = pdfium().map_err(anyhow::Error::msg)?;
    let doc = pdfium.load_pdf_from_byte_slice(bytes, None).context("PDFium could not open the document")?;
    let mut out = Vec::with_capacity(indices.len());
    for &i in indices {
        let page = doc.pages().get(i as PdfPageIndex).with_context(|| format!("page {}", i + 1))?;
        let width_px = ((page.width().value / 72.0) * dpi as f32).round().clamp(200.0, 12000.0) as i32;
        let cfg = PdfRenderConfig::new().set_target_width(width_px).render_form_data(true);
        let img = page.render_with_config(&cfg)?.as_image()?.into_luma8();
        let path = dir.join(format!("page-{:05}.png", i + 1));
        img.save(&path)?;
        let text_langs = page.text().map(|t| script_hint(&t.all())).unwrap_or_default();
        out.push(Rendered { image: path, text_langs });
    }
    Ok(out)
}

/// OCR one rendered page, waiting for the governor first.
#[allow(clippy::too_many_arguments)]
fn ocr_page(
    engine: &OcrEngine,
    corrector: Option<&Corrector>,
    governor: &Governor,
    image: &Path,
    index: usize,
    langs: &[Lang],
    cfg: &ExtractConfig,
    events: &(dyn Fn(Event) + Sync),
    mut flags: Vec<String>,
) -> Page {
    let n = index as u32 + 1;
    let _slot = governor.admit(|h| events(Event::Hold(describe_hold(h))));
    let t0 = Instant::now();
    let langs: Vec<Lang> = if langs.is_empty() {
        let scratch = image.parent().unwrap_or(Path::new("."));
        match engine.probe_langs(image, scratch) {
            Ok(v) if !v.is_empty() => {
                flags.push(format!("probed_langs:{}", lang_arg(&v)));
                v
            }
            _ => vec![Lang::En],
        }
    } else {
        langs.to_vec()
    };
    let mut result = engine.recognize(image, &langs, 3, Some(cfg.dpi));
    let mut langs = langs;
    // Safety net: very low confidence usually means the wrong language packs. Probe the page and
    // retry once with what the probe finds, keeping whichever result is more confident.
    let conf_of = |r: &Result<Vec<crate::ocr::OcrPage>>| {
        r.as_ref().ok().and_then(|v| v.first()).and_then(|p| p.confidence).unwrap_or(0.0)
    };
    // An explicit --lang is never overridden.
    if cfg.langs.is_empty()
        && conf_of(&result) < RETRY_CONFIDENCE
        && !flags.iter().any(|f| f.starts_with("probed_langs"))
    {
        let scratch = image.parent().unwrap_or(Path::new("."));
        if let Ok(probed) = engine.probe_langs(image, scratch)
            && !probed.is_empty()
            && probed != langs
        {
            let retry = engine.recognize(image, &probed, 3, Some(cfg.dpi));
            if conf_of(&retry) > conf_of(&result) {
                flags.push(format!("retried_langs:{}->{}", lang_arg(&langs), lang_arg(&probed)));
                result = retry;
                langs = probed;
            }
        }
    }
    let mut page = match result {
        Ok(pages) => {
            let p = pages.into_iter().next();
            let mut page = Page::new(
                n,
                Method::Ocr { engine: engine.engine_id(), langs: lang_arg(&langs), dpi: cfg.dpi },
            );
            if let Some(p) = p {
                page.markdown = postcorrect::finish(&p, corrector, &mut flags);
                page.confidence = p.confidence;
                if p.confidence.is_some_and(|c| c < LOW_CONFIDENCE) {
                    flags.push("low_ocr_confidence".into());
                }
            }
            page
        }
        Err(e) => Page::new(n, Method::Failed { reason: e.to_string() }),
    };
    page.langs = ScriptShares::of(&page.markdown).languages(0.05, 20);
    page.flags = flags;
    page.seconds = t0.elapsed().as_secs_f32();
    page
}

/// Human-readable description of a governor hold.
pub fn describe_hold(h: &Hold) -> String {
    match h {
        Hold::LowMemory { available, required } => format!(
            "low memory ({} available, {} reserved for the system): pausing or running one page at a time",
            lipi_sys::hardware::human_bytes(*available),
            lipi_sys::hardware::human_bytes(*required)
        ),
        Hold::Pressure(p) => format!("{p:?} memory pressure: pausing or running one page at a time"),
        Hold::Load(l) => format!("system load {l:.1} is high: running one page at a time"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_only_text_layers_give_no_language_hint() {
        let en =
            "Parliament of the Democratic Socialist Republic of Sri Lanka and the Act shall come into force "
                .repeat(3);
        assert!(script_hint(&en).is_empty());
        let ta = "இலங்ைகச் சனநாயக ேசாசᾢசக் குᾊயரசு வர்த்தமானப் பத்திாிைக Gazette Extraordinary ".repeat(3);
        assert_eq!(script_hint(&ta), vec![Lang::Ta, Lang::En]);
    }

    fn cfg() -> ExtractConfig {
        ExtractConfig::default()
    }

    #[test]
    fn legacy_font_page_goes_to_ocr_with_its_language() {
        let fm = "Y%S ,xld m%cd;dka;%sl iudcjd§ ckrcfha w;s úfYI .eiÜ m;%h wxl 2256$28 – 2021 fkdjeïn¾ ui 30 jeks wÕyrejdod ".repeat(4);
        let fonts = vec!["DAPBCE+FMAbhayax".to_string(), "DAPBAA+TimesNewRomanPS-BoldMT".to_string()];
        match plan_page(&fm, false, None, &fonts, None, &cfg()) {
            Plan::Ocr { langs, flags } => {
                assert_eq!(langs, vec![Lang::Si, Lang::En]);
                assert!(flags.contains(&"legacy_font:fm-abhaya".to_string()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn legacy_font_page_is_converted_when_a_table_exists() {
        let fm = "Y%S ,xld m%cd;dka;%sl iudcjd§ ckrcfha w;s úfYI .eiÜ m;%h wxl 2256$28 – 2021 fkdjeïn¾ ui 30 jeks wÕyrejdod ".repeat(4);
        let fonts = vec!["DAPBCE+FMAbhayax".to_string(), "DAPBAA+TimesNewRomanPS-BoldMT".to_string()];
        let conv = converter("fm-abhaya").unwrap();
        let mut words = FontWords { legacy_chars: 100, other_chars: 0, ..Default::default() };
        words.legacy.extend(fm.split_whitespace().map(str::to_string));
        match plan_page(&fm, false, None, &fonts, Some((conv, Some(&words))), &cfg()) {
            Plan::Text { markdown, method: Method::LegacyConverted { family }, flags } => {
                assert_eq!(family, "fm-abhaya");
                assert!(
                    markdown.starts_with("ශ්\u{200D}රී ලංකා ප්\u{200D}රජාතාන්ත්\u{200D}රික සමාජවාදී ජනරජයේ"),
                    "{markdown}"
                );
                assert!(markdown.contains("2256/28 – 2021 නොවැම්බර් මස 30 වැනි අඟහරුවාදා"), "{markdown}");
                assert!(flags.contains(&"legacy_converted:fm-abhaya".to_string()));
            }
            other => panic!("{other:?}"),
        }
        // Switched off, or without a table for the family, the page goes to OCR as before.
        let off = ExtractConfig { legacy_convert: false, ..cfg() };
        assert!(matches!(plan_page(&fm, false, None, &fonts, None, &off), Plan::Ocr { .. }));
        assert!(matches!(
            plan_page(&fm, false, None, &["Bamini".to_string()], Some((conv, Some(&words))), &cfg()),
            Plan::Ocr { .. }
        ));
    }

    #[test]
    fn visual_order_page_is_repaired_when_asked() {
        let bad = "இலங்ைகச் சனநாயக ேசாசலிசக் குடியரசு பாராளுமன்றம் ".repeat(5);
        assert!(matches!(plan_page(&bad, false, None, &[], None, &cfg()), Plan::Ocr { .. }));
        let c = ExtractConfig { repair: true, ..cfg() };
        match plan_page(&bad, false, None, &[], None, &c) {
            Plan::Text { markdown, method: Method::TextLayerRepaired { moved }, .. } => {
                assert!(markdown.starts_with("இலங்கைச் சனநாயக சோசலிசக்"));
                assert_eq!(moved, 10);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn healthy_english_uses_the_text_layer() {
        let en = "The appellant contends that the learned judge erred in law and that the order should be set aside. ".repeat(3);
        assert!(matches!(
            plan_page(&en, false, None, &["Times-Roman".into()], None, &cfg()),
            Plan::Text { method: Method::TextLayer, .. }
        ));
        assert!(matches!(plan_page("", false, None, &[], None, &cfg()), Plan::Empty));
        assert!(matches!(plan_page(&en, true, Some("scanned".into()), &[], None, &cfg()), Plan::Ocr { .. }));
    }
}
