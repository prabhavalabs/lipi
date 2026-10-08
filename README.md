# lipi

**lipi** (ලිපි · லிபி, "script, writing") is an offline document extraction toolchain. It turns PDFs, scanned pages, images, office documents, HTML and plain text into clean Markdown, with first-class accuracy for **Sinhala** and **Tamil** as well as English.

- **Local-first.** It never calls a cloud API, needs no API key, and sends no telemetry. After a one-time setup it runs fully offline.
- **Accuracy-first for low-resource scripts.** lipi checks every page's text layer before trusting it:
  - It detects legacy (non-Unicode) fonts, glyph mapping errors and vowel signs stored in visual order.
  - It converts text typed in the FM Abhaya legacy fonts to Unicode deterministically, keeping the
    headings and tables of the text layer, and OCRs only what it cannot convert.
  - It verifies Sinhala and Tamil text against OCR.
  - It chooses OCR languages from the scripts actually on the page.
- **Hardware-aware.** It detects CPU cores, memory and GPU, recommends a profile, warns when the machine cannot run one, and throttles itself so the machine stays responsive.
- **Fast.** The core is written in Rust. Python is used only for optional tooling and future model-based engines.

> Status: early development (0.1). See the [roadmap](docs/architecture/README.md#roadmap).

## Quick start

```bash
cargo install --path crates/lipi-cli     # build and install the `lipi` binary (Rust 1.88+)
lipi doctor                              # inspect hardware and dependencies
lipi setup                               # install the OCR engine, language models and PDF renderer
lipi extract report.pdf                  # Markdown to stdout
lipi extract scans/ -o out/ --format json
```

`lipi setup` lists every action and asks before doing anything. It:
1. installs Tesseract with the system package manager (Homebrew, apt, dnf, pacman, zypper, apk, winget, Chocolatey or Scoop);
2. downloads the pinned `tessdata_best` models for English, Sinhala and Tamil (38 MB);
3. downloads a PDFium build (3.5 MB).

Every download is verified by SHA-256.

## Usage

```
lipi extract <INPUT>... [-o OUT] [-f md|json|txt] [options]
```

| Option | Meaning |
|---|---|
| `-f, --format` | `md` (default), `json` (per-page method, languages, OCR confidence, flags) or `txt` |
| `-l, --lang si,ta,en` | Expected languages. Detected per page when omitted |
| `--ocr auto\|always\|never` | When to OCR PDF pages (default `auto`) |
| `--no-verify` | Skip checking Sinhala/Tamil text layers against OCR |
| `--repair` | Repair visual-order vowel signs instead of OCRing those pages |
| `--no-legacy-convert` | Do not convert FM Abhaya text layers to Unicode; OCR those pages instead |
| `--dpi 300` | Rendering resolution for OCR |
| `-p, --profile gentle\|balanced\|max` | Resource profile (default `balanced`) |
| `-w, --workers N` | Override the number of OCR workers |
| `--page-markers` | Insert `<!-- page N -->` between pages |

Supported inputs:
- PDF;
- PNG, JPEG, TIFF (multi-page), BMP, WebP and GIF;
- DOC/DOCX, ODT, PPT/PPTX, XLS/XLSX, ODS/ODP, RTF, EPUB and CSV;
- HTML, Markdown and plain text in any common encoding.

Formats are detected from content, not from file extensions.

**Profiles** (on an Apple M4 with 4 performance and 6 efficiency cores):

| Profile | OCR workers | Behaviour |
|---|---|---|
| `gentle` | 2 | Lowest priority, macOS background QoS (efficiency cores), waits when memory pressure appears |
| `balanced` | 3 | Performance cores minus one, reduced priority |
| `max` | 10 | Every physical core; waits only on critical memory pressure |

Environment variables:
- `LIPI_HOME`: data directory.
- `LIPI_TESSERACT`: path of the Tesseract executable.
- `LIPI_PDFIUM`: path of the PDFium library.

## Accuracy

`lipi bench <dir>` reports CER and WER for every document that has a `<name>.gt.txt`. On 24 synthetic A4 pages (clean and degraded, several fonts), the mean CER is **0.58% for Tamil** and **5.2% for Sinhala**. The Sinhala figure ranges from 0.03–2.7% with Noto Sans Sinhala to 6–9% with Apple's Sinhala MN fonts. Details and model comparisons are in [bench/README.md](bench/README.md).

On 41 held-out gazette pages typed in FM Abhaya, the legacy-font converter agrees with OCR of the same
pages at a word F1 of 0.88 while taking about 0.3 s per page against 3–4 s for OCR; the differences
are mostly OCR errors (see [bench/README.md](bench/README.md#legacy-font-conversion)).

The way lipi decides between a text layer, conversion and OCR, and why Sinhala and Tamil text layers
are verified, is described in [docs/architecture](docs/architecture/README.md).

## Repository layout

```
crates/lipi-core     Document model, input format detection, Markdown / JSON / text renderers
crates/lipi-script   Sinhala / Tamil / Latin script analysis, text-layer health checks,
                     visual-order repair, Unicode normalisation, legacy-font detection and
                     the FM Abhaya converter
crates/lipi-engine   Extractors (PDF, image, HTML, text, office), OCR driver, page router,
                     metrics
crates/lipi-sys      Hardware probe, profiles, resource governor, dependency installer
crates/lipi-cli      The `lipi` command-line interface
bench/               Benchmark method, results, the synthetic page generator and the
                     legacy-font mapping derivation tools (Python)
docs/architecture/   Architecture diagrams (D2) and design notes
```

Not versioned: model weights, benchmark corpora and extraction output (see `.gitignore`).

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

See [CONTRIBUTING.md](CONTRIBUTING.md) for the branching model, commit conventions and pull-request process.

## Acknowledgements

PDF text-layer extraction and office-format conversion build on [pdf-inspector](https://github.com/firecrawl/pdf-inspector) and [anydoc](https://github.com/firecrawl/anydoc) by Firecrawl (MIT). OCR uses [Tesseract](https://github.com/tesseract-ocr/tesseract) and the [tessdata_best](https://github.com/tesseract-ocr/tessdata_best) models (Apache-2.0). Page rendering uses [PDFium](https://pdfium.googlesource.com/pdfium/) builds from [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries).

## Licence

Apache License 2.0. See [LICENSE](LICENSE).
