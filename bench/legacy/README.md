# Legacy-font mapping derivation

Tools that derive and evaluate the legacy-font to Unicode tables in `crates/lipi-script/src/legacy/`.
The tables are learned from real documents, not copied from other converters: the text layer of a
legacy-font page contains the font's own codes, OCR of the rendered page contains Unicode, and an
alignment model learns which code sequences produce which Unicode sequences.

Nothing under `bench/corpus/` is versioned. Only the resulting table (Rust source) and the numbers in
`bench/README.md` leave the work directory.

## Requirements

```bash
python3 -m venv .venv && .venv/bin/pip install -r bench/legacy/requirements.txt   # PyMuPDF, RapidFuzz
```

Tesseract 5 with the `sin`/`eng` models (lipi's pinned `tessdata_best` set after `lipi setup`), and a
list of PDF paths, one per line, relative to a root directory. Run Python with `-I`: the PDFs are
untrusted input.

## 1. Survey the fonts

```bash
.venv/bin/python -I bench/legacy/survey_fonts.py --root <raw-root> --list docs.txt --sample 200
```

Groups the base font names of every page into legacy families and prints documents and pages per
family, the raw font names and other non-Latin-looking font names.

## 2. Derive a mapping

```bash
W=bench/corpus/legacy/work
P=".venv/bin/python -I bench/legacy/derive.py"
$P select --root <raw-root> --list legacy_docs.txt --out $W --train-pages 90 --holdout-pages 40
$P pairs  --root <raw-root> --selection $W/train.jsonl   --out $W --tessdata <tessdata-dir>
$P pairs  --root <raw-root> --selection $W/holdout.jsonl --out $W --tessdata <tessdata-dir>
$P learn  --out $W                       # mapping_candidates.json + learn_report.txt
$P inspect --out $W 're'                 # word pairs containing a code sequence, for review
$P emit   $W/mapping_reviewed.json       # Rust table on stdout
```

- `select` picks pages with at least 600 glyphs in an FM font, at most two per document, and
  splits them by document into a training and a held-out set.
- `pairs` renders each page at 300 dpi (PyMuPDF), runs Tesseract `sin+eng` (TSV output,
  `OMP_THREAD_LIMIT=1`, `nice -n 19`), builds words from the text layer with the font of every
  glyph (PyMuPDF `rawdict`), and pairs text-layer words with OCR words by mutual best bounding-box
  overlap (IoU at least 0.4). Pairs, OCR text and OCR time are stored per page.
- `learn` keeps pairs of all-legacy-font words with OCR confidence at least 60 and IoU at least 0.5,
  turns the OCR word into the visual order a legacy font stores (NFD, pre-base signs moved before
  their consonant cluster) and runs eight Viterbi-EM iterations of a monotone alignment model. A
  code token is one to three codes; it emits zero to four Unicode units (a virama + ZWJ pair counts
  as one unit). Tokens are initialised from Dice co-occurrence between codes and units, longer
  tokens pay a length penalty (`--lam`), and a multi-code token is reported only when it occurs at
  least `--min-multi` times, its majority emission has share at least `--min-share` and differs from
  what its single codes would give. `learn_report.txt` lists every token with its emission counts and
  marks shares below 0.7 for review.
- `inspect` prints the word pairs containing a code sequence so that a reviewer who reads the script
  can settle the cases where OCR errors win the majority.
- `emit` prints a reviewed `{code: unicode}` JSON as a sorted Rust table.

### Review of the FM Abhaya table (2026-10-08)

Training data: 91 pages from 54 Sinhala gazettes (documents.gov.lk, nuuuwan mirror), 19,338 word
pairs. 161 candidate tokens came out of `learn`; 115 of 128 single codes had a majority share of
0.9 or more. The review changed:

- learned multi-code tokens that only reflected OCR errors were dropped (`.;` as ගන, `;a` as න,
  `da` as ා, `fj` as ව, `l+` as කු and similar): their single codes compose correctly;
- codes whose majority reading was an OCR confusion were corrected from the paired words: `±` is
  දැ (දැන්වීම, දැක්වෙන), not දු; `ø` is ද්‍ර (මුද්‍රණ), the learner had split the ර onto the
  following code; `ð` is ජි (සාමාජික); `Ó` is ථි (ආර්ථික); `Å` is ඛි; `Æ` is ලූ; `Ö` is චී;
  `×` is ඥා (ආඥාපනත); `ý` is ඡි; `wE` is ඈ;
- the quote codes `z`/`Z`/`zz`/`ZZ` and `—`/`˜` map to typographic quotes (OCR produced ASCII
  quotes);
- `re`/`rE` → රු/රූ were added as context forms (the learner showed ු as the second reading of
  `e`, all of it after ර);
- `Ta`/`TA` → ඕ, `T!` → ඖ and `` `v `` → ඬ were added by analogy with `ta` → ඒ and
  `` `. `` → ඟ, `` `o `` → ඳ; they did not occur in the corpus.

Two orthographic rules live in the converter rather than the table: a pre-base sign moves after the
consonant cluster that follows it, and a vowel sign typed before a rakaransaya or yansaya (`YS%`)
moves after the conjunct.

## 3. Evaluate on held-out pages

```bash
.venv/bin/python -I bench/legacy/eval.py --root <raw-root> --selection $W/holdout.jsonl \
    --out $W --lipi target/debug/lipi --ocr-from-pairs --show 3
```

Each held-out page is copied into a single-page PDF and extracted with `lipi` (conversion,
`--no-verify`). The converted text is compared with OCR of the same page: character error rate with
OCR as the reference, unordered word F1, and the character-bigram agreement used by lipi's
verification. Without `--ocr-from-pairs` the OCR side is a second `lipi` run with
`--no-legacy-convert`. Results go to `eval_results.json`; the summary is copied into
`bench/README.md`.
