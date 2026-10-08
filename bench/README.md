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
