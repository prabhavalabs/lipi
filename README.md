# lipi

**lipi** (ලිපි · லிபி, "script, writing") is an offline document extraction toolchain. It turns PDFs, scanned pages, images, office documents, HTML and plain text into clean Markdown, with first-class accuracy for **Sinhala** and **Tamil** as well as English.

- **Local-first.** It never calls a cloud API, needs no API key, and sends no telemetry. After a one-time setup it runs fully offline.
- **Accuracy-first for low-resource scripts.** lipi checks every page's text layer before trusting it:
  - It detects legacy (non-Unicode) fonts, glyph mapping errors and vowel signs stored in visual order.
  - It verifies Sinhala and Tamil text against OCR.
  - It chooses OCR languages from the scripts actually on the page.
- **Hardware-aware.** It detects CPU cores, memory and GPU, recommends a profile, warns when the machine cannot run one, and throttles itself so the machine stays responsive.
- **Fast.** The core is written in Rust. Python is used only for optional model-based engines.

> Status: early development. See [the roadmap](#roadmap).

## Quick start

```bash
cargo install --path crates/lipi-cli     # build and install the `lipi` binary
lipi doctor                              # inspect hardware and dependencies
lipi setup                               # install OCR engine, language models and PDF renderer
lipi extract report.pdf                  # Markdown to stdout
lipi extract scans/ -o out/ --format json
```

## Repository layout

```
crates/lipi-core     Document model, input format detection, Markdown / JSON / text renderers
crates/lipi-script   Sinhala / Tamil / Latin script analysis, text-layer health checks,
                     visual-order repair, Unicode normalisation, legacy-font detection
crates/lipi-engine   Extractors (PDF, image, HTML, text, office), OCR driver, page router,
                     resource-governed worker pool
crates/lipi-sys      Hardware probe, profiles, dependency installer, pinned downloads
crates/lipi-cli      The `lipi` command-line interface
docs/                Architecture and design notes
```

Not versioned: model weights, benchmark corpora and extraction output (see `.gitignore`).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the branching model, commit conventions and pull-request process.

## Acknowledgements

PDF text-layer extraction and office-format conversion build on [pdf-inspector](https://github.com/firecrawl/pdf-inspector) and [anydoc](https://github.com/firecrawl/anydoc) by Firecrawl (MIT). OCR uses [Tesseract](https://github.com/tesseract-ocr/tesseract) (Apache-2.0). Page rendering uses [PDFium](https://pdfium.googlesource.com/pdfium/) builds from [pdfium-binaries](https://github.com/bblanchon/pdfium-binaries).

## Licence

Apache License 2.0. See [LICENSE](LICENSE).
