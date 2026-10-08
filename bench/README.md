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

## Sinhala post-correction

Measured on 2026-10-08 with the same engine and models, one OCR worker. The corrector
(`lipi_script::correct`) is on by default when a lexicon is installed; `--no-correct` gives the baseline.

### Data

1,088 Sinhala cabinet-decision texts (clean Unicode, about 172k words) were split **by document** with
`bench/postcorrect/split.py` (SHA-1 of the path, 80 / 10 / 10):

| Set | Documents | Words | Use |
|---|---|---|---|
| lexicon | 880 | 138k | `lipi lexicon build` → 8,002 distinct words, 206 KB |
| learning | 102 | 16.5k | 48 synthetic pages (8 texts × 3 fonts × clean/degraded) to learn confusion rules and tune thresholds |
| held-out | 106 | 17.4k | 36 synthetic pages (6 texts × 3 fonts × clean/degraded) for the numbers below |

No document is in two sets; the held-out pages were not looked at until the thresholds were fixed. The
corpus itself is not in the repository.

### Rules and thresholds

`bench/postcorrect/learn.py` aligned the OCR output of the learning pages with the ground truth
(words, then characters) and ranked the differing sequences. The committed table
(`crates/lipi-script/src/data/si_confusions.tsv`, 58 rules) keeps rules seen at least 4 times. The
first ten, as `ocr → truth (count, probability that the source was wrong)`:

| Rule | Count | Probability |
|---|---|---|
| ත → න | 918 | 0.22 |
| ච → ව | 424 | 0.67 |
| න → ත | 346 | 0.11 |
| ී → ි | 74 | 0.04 |
| ම → ම් | 56 | 0.02 |
| ඳු → ඳ | 52 | 0.60 |
| ූ → ු | 34 | 0.19 |
| ෙ → ේ | 33 | 0.04 |
| නූ → තු | 28 | 0.82 |
| එ → ඒ | 27 | 0.11 |

On the learning pages 36% of words were wrong with Sinhala MN, 37% with Sinhala Sangam MN and 0.6% with
Noto Sans Sinhala (clean pages). Tesseract's word confidence separates the two groups only weakly
(median 90 for wrong words, 96 for correct ones).

`bench/postcorrect/simulate.py` replayed the corrector over the cached learning-set OCR for a grid of
thresholds. The defaults in `Options` are: at most 2 rule applications; candidate score
(lexicon frequency × rule probabilities) at least 0.05; best score at least 3× the runner-up;
words with OCR confidence ≥ 97 are left alone; words shorter than 3 code points are left alone. On the
learning set this fixed 1,694 of 2,373 wrong out-of-lexicon words and changed none of the 284 correct
out-of-lexicon words; without the minimum score, 12 correct words were damaged (a rare word one unlikely
rule away from a common one), while a confidence gate at 95 would have forfeited 57 fixes for the same
protection.

### Results on the held-out pages

```bash
OMP_THREAD_LIMIT=1 nice -n 19 lipi bench corpus/heldout --no-correct -l si -w 1 -p max --json before.json
OMP_THREAD_LIMIT=1 nice -n 19 lipi bench corpus/heldout              -l si -w 1 -p max --json after.json
python3 -I bench/postcorrect/compare.py before.json after.json
```

| Font | Variant | Pages | CER before | CER after | WER before | WER after |
|---|---|---|---|---|---|---|
| Sinhala MN | clean | 6 | 9.15% | 4.85% | 36.47% | 12.76% |
| Sinhala MN | degraded | 6 | 7.80% | 3.89% | 32.12% | 10.26% |
| Sinhala Sangam MN | clean | 6 | 10.58% | 6.88% | 35.06% | 15.32% |
| Sinhala Sangam MN | degraded | 6 | 5.89% | 3.96% | 20.90% | 9.62% |
| Noto Sans Sinhala | clean | 6 | 2.37% | 2.36% | 3.59% | 3.53% |
| Noto Sans Sinhala | degraded | 6 | 2.36% | 2.28% | 3.72% | 3.21% |
| **all** | | 36 | **6.36%** | **4.04%** | **21.98%** | **9.11%** |

No page had a higher CER after correction. One of the six texts is hard for every font (10–17% CER
even with Noto, before and after), which keeps the means high; on the other five texts the MN fonts
end between 1.0% and 4.9% CER.

Tamil (12 synthetic pages in `$TMPDIR/lipi-synth`, `-l ta`): 0.58% CER and 4.39% WER both with and
without `--no-correct`; identical per page, and no page carries a `corrected` flag.

The `max` profile was used with `-w 1` because the machine reported memory-pressure level 2 during the
run and the `gentle`/`balanced` governors wait at that level; with one worker, `OMP_THREAD_LIMIT=1` and
`nice -n 19` the footprint is one low-priority single-threaded Tesseract process.

### Limitations

- The lexicon is small and single-domain (cabinet decisions). Coverage of held-out words of the same
  domain is about 97%; on other domains more correct words will be out of the lexicon, and only the
  minimum score and the confidence gate protect them. A larger, general lexicon is the next step.
- Rules and thresholds were learned on synthetic renderings of three fonts; real scans (other fonts,
  bleed-through, skew) still need a hand-checked benchmark.
- Correction is word by word; it has no context, so a confusion that yields another valid word
  (ත/න in short words) is left as it is.
