# Benchmarks

Recognition quality is the product. Every change to routing, OCR or normalisation is measured with
`lipi bench` on the same corpus, before and after.

## Running

```bash
lipi bench bench/corpus/si            # every <name>.<ext> with a sibling <name>.gt.txt
lipi bench bench/corpus --json results.json
```

`<name>_clean.png` and `<name>_scan.png` may share one `<name>.gt.txt`. Character error rate (CER)
and word error rate (WER) are computed after Unicode normalisation (NFC, OCR joiners removed) and
whitespace collapsing.

## Corpora

Corpora are not committed (see `.gitignore`). Two kinds are used:

1. **Synthetic pages.** `synth.py` renders Unicode text in chosen fonts, clean and degraded, with exact
   ground truth. This is useful for comparing engines and settings quickly.
2. **Real documents.** These are scans and PDFs with hand-checked transcriptions. Real documents decide
   defaults; synthetic pages alone can mislead (font choice moves Sinhala CER from under 1% to 9%).

```bash
python3 -m venv .venv && .venv/bin/pip install -r bench/requirements.txt
.venv/bin/python bench/synth.py --text sinhala.txt --lang si --font NotoSansSinhala.ttf --out bench/corpus/si
```

## Results

Measured on 2026-10-08 with Tesseract 5.5.2 and `tessdata_best` (`e12c65a`), on an Apple M4 with 2
workers. There are 24 synthetic A4 pages: 6 Sinhala and 6 Tamil texts, each clean and degraded.

| Language | Pages | Mean CER | Mean WER | Notes |
|---|---|---|---|---|
| Tamil | 12 | 0.58% | 4.39% | Consistent across fonts and degradation |
| Sinhala | 12 | 5.20% | 20.13% | 0.75–2.7% with Noto Sans Sinhala; 6–9% with Apple's Sinhala MN fonts, mostly ව/ච and ත/න confusions |

Model comparison on the same pages, measuring Tesseract alone:

| Model | Sinhala CER clean / degraded | Tamil CER clean / degraded |
|---|---|---|
| `tessdata` (standard) | 5.71% / 4.43% | 0.84% / 0.64% |
| `tessdata_best` single language | 5.63% / 4.47% | 0.51% / 0.64% |
| `tessdata_best` script model | 5.04% / 6.65% | 0.49% / 0.43% |
| `tessdata_best` with `+eng` | 5.93% / 5.02% | 0.29% / 0.47% |

## Legacy-font conversion

Measured on 2026-10-08 with the FM Abhaya converter (`lipi_script::legacy`), Tesseract 5.5.2 and
`tessdata_best` (`e12c65a`), one single-threaded OCR process, on an Apple M4. Method and tools are in
[legacy/README.md](legacy/README.md).

**Fonts in the corpus.** Of 200 sampled Sri Lankan legacy-font Sinhala/Tamil PDFs (government
gazettes, acts and bills), 173 use FM Abhaya or its bold face FM Ababld (803 of 1,106 pages), 13 use
Bamini (Tamil, 41 pages, usually next to an FM Abhaya masthead), 5 Kalaham (Tamil), 1 DL Anurada and
1 FM Malithi. Of 200 random Sinhala/Tamil PDFs, 92 contain FM Abhaya. FM Abhaya is therefore the
only family with a converter so far.

**Held-out pages.** 41 pages from 25 documents not used to derive the table, each with at least 600
FM glyphs. Every page was routed to the converter (`legacy_converted`), including three whose text
layer pdf-inspector withheld as garbled (converted from PDFium's text). The converted text is
compared with Tesseract `sin+eng` on the same page rendered at 300 dpi; OCR is the reference, so the
numbers include OCR's own errors (about 5% CER on this material).

| Measure (converted vs OCR, 41 pages) | Mean | Median | Min | Max |
|---|---|---|---|---|
| Character error rate | 10.6% | 6.4% | 0.2% | 73.9% |
| Unordered word F1 | 0.897 | 0.899 | 0.784 | 0.992 |
| Character-bigram agreement (verification score) | 0.937 | 0.940 | 0.822 | 0.996 |
| Conversion, seconds per page (pdf-inspector + PDFium fonts + conversion) | 0.34 | 0.32 | 0.10 | 0.62 |
| OCR, seconds per page (Tesseract alone) | 3.82 | 3.16 | 1.64 | 10.28 |

The CER maximum (73.9%) and the other values above 20% are table pages: pdf-inspector reads
exchange-rate and land-schedule tables row by row and Tesseract column by column, so the same words
appear in a different order (word F1 on that page is 0.90). Reading the differing words on three
pages by hand, the conversion side was correct where the two disagreed except for one typist error
preserved from the source (`Y%%S`); the OCR side had ව/ච and ත/න confusions, dropped conjuncts and
misread Latin reference numbers. Conversion is therefore at least as accurate as OCR on these pages
and about ten times faster.

The page verification threshold for converted pages (`LEGACY_VERIFY_MIN_AGREEMENT`, 0.80) sits just
below the lowest agreement observed.

Commands (paths are local; the corpus is not versioned):

```bash
P=".venv/bin/python -I bench/legacy/derive.py"; W=bench/corpus/legacy/work
$P select --root <raw-root> --list $W/../legacy_docs.txt --out $W --train-pages 90 --holdout-pages 40
$P pairs  --root <raw-root> --selection $W/train.jsonl   --out $W --tessdata <tessdata-dir>
$P pairs  --root <raw-root> --selection $W/holdout.jsonl --out $W --tessdata <tessdata-dir>
$P learn  --out $W
.venv/bin/python -I bench/legacy/eval.py --root <raw-root> --selection $W/holdout.jsonl \
    --out $W --lipi target/debug/lipi --ocr-from-pairs
```
