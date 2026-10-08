//! `lipi`: offline document extraction with first-class Sinhala and Tamil support.

mod bench;
mod doctor;
mod extract;
mod setup;

use clap::{Args, Parser, Subcommand};
use lipi_core::OutputFormat;
use lipi_engine::{ExtractConfig, OcrMode};
use lipi_script::Lang;
use lipi_sys::Profile;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "lipi", version, about = "Offline document extraction with first-class Sinhala and Tamil support", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Extract text and tables from documents into Markdown (default), JSON or plain text.
    Extract(ExtractArgs),
    /// Inspect this machine's hardware and lipi's dependencies, and recommend a profile.
    Doctor {
        /// Print the report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Install the OCR engine, the pinned language models and the PDF renderer.
    Setup {
        /// Do not ask for confirmation.
        #[arg(short, long)]
        yes: bool,
        /// Only install language models and the PDF renderer; leave the OCR engine alone.
        #[arg(long)]
        skip_engine: bool,
    },
    /// Measure accuracy (CER/WER) against ground truth: every `<name>.<ext>` with a sibling
    /// `<name>.gt.txt` is extracted and compared.
    Bench {
        /// Directory with documents and ground-truth files.
        dir: PathBuf,
        /// Write per-file results as JSON to this path.
        #[arg(long)]
        json: Option<PathBuf>,
        #[command(flatten)]
        opts: EngineArgs,
    },
}

#[derive(Args)]
struct ExtractArgs {
    /// Files or directories to extract (directories are searched recursively).
    #[arg(required = true)]
    inputs: Vec<PathBuf>,
    /// Output file (one input) or directory (several inputs). Defaults to stdout for one input.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Output format: md, json or txt.
    #[arg(short, long, default_value = "md")]
    format: OutputFormat,
    /// Insert `<!-- page N -->` markers between pages (Markdown).
    #[arg(long)]
    page_markers: bool,
    /// Do not print the per-document summary on stderr.
    #[arg(short, long)]
    quiet: bool,
    #[command(flatten)]
    opts: EngineArgs,
}

#[derive(Args, Clone)]
struct EngineArgs {
    /// Expected languages, comma-separated (si, ta, en). Detected when omitted.
    #[arg(short, long, value_delimiter = ',')]
    lang: Vec<String>,
    /// When to OCR PDF pages: auto, always or never.
    #[arg(long, default_value = "auto")]
    ocr: OcrMode,
    /// Do not cross-check Sinhala/Tamil text layers against OCR.
    #[arg(long)]
    no_verify: bool,
    /// Repair visual-order vowel signs in text layers instead of OCRing those pages.
    #[arg(long)]
    repair: bool,
    /// Do not convert legacy-font (FM Abhaya) text layers; OCR those pages instead.
    #[arg(long)]
    no_legacy_convert: bool,
    /// Rendering resolution for OCR.
    #[arg(long, default_value_t = 300)]
    dpi: u32,
    /// Resource profile: gentle (background), balanced (default) or max.
    #[arg(short, long, default_value = "balanced")]
    profile: Profile,
    /// Number of OCR workers (overrides the profile).
    #[arg(short, long)]
    workers: Option<usize>,
}

impl EngineArgs {
    fn config(&self) -> Result<ExtractConfig, String> {
        let mut langs = Vec::new();
        for l in &self.lang {
            langs.push(Lang::parse(l).ok_or_else(|| format!("unknown language `{l}` (use si, ta or en)"))?);
        }
        Ok(ExtractConfig {
            langs,
            ocr: self.ocr,
            verify: !self.no_verify,
            repair: self.repair,
            legacy_convert: !self.no_legacy_convert,
            dpi: self.dpi.clamp(72, 600),
            profile: self.profile,
            workers: self.workers,
        })
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Extract(a) => extract::run(a),
        Command::Doctor { json } => doctor::run(json),
        Command::Setup { yes, skip_engine } => setup::run(yes, skip_engine),
        Command::Bench { dir, json, opts } => bench::run(&dir, json.as_deref(), &opts),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
