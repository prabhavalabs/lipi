//! Tesseract OCR driver.
//!
//! Tesseract runs as a separate, single-threaded process per page (see
//! [`lipi_sys::governor::worker_command`]), so one stuck page cannot take the extractor down and
//! the worker count alone determines CPU use. Results come back as TSV, which carries word
//! confidences and the block / paragraph / line structure used to rebuild paragraphs.

use anyhow::{Context, Result, bail};
use lipi_script::{Lang, ScriptShares};
use lipi_sys::Profile;
use lipi_sys::deps::Tesseract;
use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Per-page time limit for one OCR process.
const TIMEOUT: Duration = Duration::from_secs(300);

/// OCR output for one image page.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrPage {
    /// 1-based page number within the image (multi-page TIFF), 1 otherwise.
    pub page: u32,
    /// Recognised text as Markdown paragraphs.
    pub text: String,
    /// Mean word confidence (0-100).
    pub confidence: Option<f32>,
    /// Recognised words.
    pub words: usize,
    /// Every recognised word with its confidence, in reading order.
    pub word_confidences: Vec<(String, f32)>,
}

impl OcrPage {
    /// Lowest confidence of each distinct token, after [`lipi_script::normalize`].
    pub fn confidence_map(&self) -> HashMap<String, f32> {
        let mut m: HashMap<String, f32> = HashMap::new();
        for (w, c) in &self.word_confidences {
            let w = lipi_script::normalize(w);
            m.entry(w).and_modify(|e| *e = e.min(*c)).or_insert(*c);
        }
        m
    }
}

/// The OCR engine.
#[derive(Debug, Clone)]
pub struct OcrEngine {
    /// Tesseract executable.
    pub tesseract: Tesseract,
    /// Directory with lipi's pinned models; `None` uses Tesseract's own models.
    pub tessdata: Option<PathBuf>,
    /// Resource profile for worker processes.
    pub profile: Profile,
}

/// Tesseract language argument for a set of languages: Indic packs first, then English.
pub fn lang_arg(langs: &[Lang]) -> String {
    let mut v: Vec<Lang> = langs.to_vec();
    v.sort();
    v.dedup();
    if v.is_empty() {
        v.push(Lang::En);
    }
    v.iter().map(|l| l.tesseract()).collect::<Vec<_>>().join("+")
}

impl OcrEngine {
    /// Identifier recorded in page provenance.
    pub fn engine_id(&self) -> String {
        match &self.tessdata {
            Some(d) => format!(
                "tesseract-{}/{}",
                self.tesseract.version,
                d.file_name().and_then(|n| n.to_str()).unwrap_or("custom")
            ),
            None => format!("tesseract-{}/system", self.tesseract.version),
        }
    }

    /// Recognise an image file. `psm` is Tesseract's page segmentation mode.
    pub fn recognize(
        &self,
        image: &Path,
        langs: &[Lang],
        psm: u32,
        dpi: Option<u32>,
    ) -> Result<Vec<OcrPage>> {
        let mut cmd = lipi_sys::governor::worker_command(&self.tesseract.path, self.profile);
        cmd.arg(image).arg("stdout").arg("-l").arg(lang_arg(langs)).arg("--psm").arg(psm.to_string());
        if let Some(d) = dpi {
            cmd.arg("--dpi").arg(d.to_string());
        }
        if let Some(dir) = &self.tessdata {
            cmd.arg("--tessdata-dir").arg(dir);
        }
        // Request TSV through parameters rather than the `tsv` config file, which is not present
        // in lipi's model directory.
        cmd.args(["-c", "tessedit_create_tsv=1", "-c", "tessedit_create_txt=0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        let mut child = cmd.spawn().with_context(|| format!("starting {}", self.tesseract.path.display()))?;
        let mut stdout = child.stdout.take().expect("piped");
        let mut stderr = child.stderr.take().expect("piped");
        let out_reader = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = stdout.read_to_end(&mut b);
            b
        });
        let err_reader = std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = stderr.read_to_end(&mut b);
            b
        });
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if start.elapsed() > TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                bail!("tesseract timed out after {}s on {}", TIMEOUT.as_secs(), image.display());
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        let out = out_reader.join().unwrap_or_default();
        let err = err_reader.join().unwrap_or_default();
        if !status.success() {
            bail!("tesseract failed on {}: {}", image.display(), String::from_utf8_lossy(&err).trim());
        }
        let tsv = String::from_utf8_lossy(&out);
        if !tsv.starts_with("level\t") {
            bail!(
                "tesseract returned no TSV for {}: {}",
                image.display(),
                String::from_utf8_lossy(&err).trim()
            );
        }
        Ok(parse_tsv(&tsv))
    }

    /// Guess the scripts on an image by recognising three horizontal bands (top, middle,
    /// bottom) with all three language packs and measuring the script mix.
    ///
    /// Tesseract's own script detection labels Sinhala as Latin, so lipi measures the script mix
    /// of a short recognition pass instead. Several bands are used because government documents
    /// often carry a trilingual masthead above a single-language body.
    pub fn probe_langs(&self, image: &Path, scratch: &Path) -> Result<Vec<Lang>> {
        let img = image::open(image).with_context(|| format!("reading {}", image.display()))?;
        let (w, h) = (img.width(), img.height());
        let bands: Vec<image::DynamicImage> = if h > 900 {
            [(0.04, 0.16), (0.40, 0.14), (0.78, 0.14)]
                .iter()
                .map(|&(top, height)| {
                    img.crop_imm(w / 20, (h as f32 * top) as u32, w - w / 10, (h as f32 * height) as u32)
                })
                .collect()
        } else {
            vec![img]
        };
        let stem = image.file_stem().and_then(|s| s.to_str()).unwrap_or("page");
        let mut text = String::new();
        for (k, band) in bands.iter().enumerate() {
            let probe = scratch.join(format!("probe-{stem}-{k}.png"));
            band.to_luma8().save(&probe)?;
            let pages = self.recognize(&probe, &[Lang::Si, Lang::Ta, Lang::En], 6, None);
            let _ = std::fs::remove_file(&probe);
            for p in pages? {
                text.push_str(&p.text);
                text.push('\n');
            }
        }
        // A script must contribute 8% of the letters and at least 15 letters to be selected.
        let s = ScriptShares::of(&text);
        Ok(s.languages(0.08, 20).into_iter().filter(|&l| s.count(l) >= 15).collect())
    }
}

#[derive(Default)]
struct Para {
    lines: BTreeMap<u32, Vec<String>>,
}

/// Parse Tesseract TSV into pages of paragraphs.
pub fn parse_tsv(tsv: &str) -> Vec<OcrPage> {
    // page -> (block, par) -> lines
    let mut pages: BTreeMap<u32, BTreeMap<(u32, u32), Para>> = BTreeMap::new();
    let mut confs: BTreeMap<u32, Vec<(String, f32)>> = BTreeMap::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let text = cols[11].trim();
        if text.is_empty() {
            continue;
        }
        let num = |i: usize| cols[i].parse::<u32>().unwrap_or(0);
        let page = num(1).max(1);
        if let Ok(c) = cols[10].parse::<f32>()
            && c >= 0.0
        {
            confs.entry(page).or_default().push((text.to_string(), c));
        }
        pages
            .entry(page)
            .or_default()
            .entry((num(2), num(3)))
            .or_default()
            .lines
            .entry(num(4))
            .or_default()
            .push(text.to_string());
    }
    pages
        .into_iter()
        .map(|(page, paras)| {
            let text = paras
                .values()
                .map(|p| join_lines(p.lines.values().map(|w| w.join(" ")).collect()))
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            let word_confidences = confs.remove(&page).unwrap_or_default();
            let words = word_confidences.len();
            let confidence =
                (words > 0).then(|| word_confidences.iter().map(|(_, c)| *c).sum::<f32>() / words as f32);
            OcrPage { page, text, confidence, words, word_confidences }
        })
        .collect()
}

/// Join the lines of a paragraph, removing end-of-line hyphenation of Latin words.
fn join_lines(lines: Vec<String>) -> String {
    let mut out = String::new();
    for l in lines {
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        if out.is_empty() {
            out.push_str(l);
        } else if out.ends_with('-')
            && out.chars().rev().nth(1).is_some_and(|c| c.is_ascii_alphabetic())
            && l.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        {
            out.pop();
            out.push_str(l);
        } else {
            out.push(' ');
            out.push_str(l);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const TSV: &str =
        "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext
1\t1\t0\t0\t0\t0\t0\t0\t100\t100\t-1\t
5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t96.5\tThe
5\t1\t1\t1\t1\t2\t0\t0\t10\t10\t90.0\tjudg-
5\t1\t1\t1\t2\t1\t0\t0\t10\t10\t93.0\tment
5\t1\t1\t1\t2\t2\t0\t0\t10\t10\t95.0\twas
5\t1\t2\t1\t1\t1\t0\t0\t10\t10\t80.0\tශ්‍රී
5\t1\t2\t1\t1\t2\t0\t0\t10\t10\t70.0\tලංකා
";

    #[test]
    fn rebuilds_paragraphs_and_confidence() {
        let p = parse_tsv(TSV);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].text, "The judgment was\n\nශ්‍රී ලංකා");
        assert_eq!(p[0].words, 6);
        assert!((p[0].confidence.unwrap() - 87.416_67).abs() < 0.01);
        let m = p[0].confidence_map();
        assert_eq!(m.get("ලංකා"), Some(&70.0));
        assert_eq!(m.len(), 6);
    }

    #[test]
    fn language_argument_order() {
        assert_eq!(lang_arg(&[Lang::En, Lang::Si]), "sin+eng");
        assert_eq!(lang_arg(&[]), "eng");
        assert_eq!(lang_arg(&[Lang::Ta, Lang::Si, Lang::Ta]), "sin+tam");
    }
}
