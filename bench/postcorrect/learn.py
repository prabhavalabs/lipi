#!/usr/bin/env python3
"""Learn OCR confusion rules for Sinhala from synthetic pages with ground truth.

For every `<name>_clean.png` / `<name>_scan.png` with a sibling `<name>.gt.txt` the page is recognised
with Tesseract (same settings as lipi: `sin`, page segmentation mode 3, TSV output), the recognised
words are aligned with the ground truth, and each differing word pair is aligned character by
character. Runs of differing characters become candidate rules `ocr -> truth`. The output is a table
of rules ranked by count, with the number of times the source sequence occurred in the OCR output and
the resulting probability that it was wrong. The tool also prints word-confidence statistics for
correct and wrong words, used to choose the confidence gate of the corrector.

  OMP_THREAD_LIMIT=1 nice -n 19 python3 -I bench/postcorrect/learn.py "$TMPDIR/lipi-pc/learn" \
      --work "$TMPDIR/lipi-pc/learn-ocr" --tessdata "<lipi data dir>/tessdata/best-e12c65a" \
      --out "$TMPDIR/lipi-pc/rules.tsv"

Only the derived rule table (code points and counts) leaves this tool; no page text is written to
the output.
"""
import argparse
import collections
import os
import pathlib
import statistics
import subprocess
import sys
import unicodedata

ZWJ, ZWNJ, VIRAMA = "‍", "‌", "්"


def is_sinhala(c: str) -> bool:
    return "඀" <= c <= "෿"


def normalize(text: str) -> str:
    """Mirror of lipi_script::normalize for Sinhala text."""
    s = unicodedata.normalize("NFC", text)
    out = []
    for i, c in enumerate(s):
        prev = s[i - 1] if i else ""
        nxt = s[i + 1] if i + 1 < len(s) else ""
        if c in "﻿​­⁠᠎":
            continue
        if c == ZWNJ:
            if prev != VIRAMA and is_sinhala(prev) and is_sinhala(nxt):
                out.append(c)
            continue
        if c == ZWJ:
            if is_sinhala(prev) and is_sinhala(nxt):
                out.append(c)
            continue
        out.append(c)
    return "".join(out)


def core(token: str) -> str:
    """The Sinhala part of a token: strip anything that is not Sinhala or a joiner from both ends."""
    ok = lambda c: is_sinhala(c) or c in (ZWJ, ZWNJ)
    i, j = 0, len(token)
    while i < j and not ok(token[i]):
        i += 1
    while j > i and not ok(token[j - 1]):
        j -= 1
    return token[i:j]


def align(a, b):
    """Levenshtein alignment; yields (op, i, j) with op in eq/sub/ins/del, ins = extra in b."""
    n, m = len(a), len(b)
    d = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        d[i][0] = i
    for j in range(1, m + 1):
        d[0][j] = j
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            d[i][j] = min(d[i - 1][j - 1] + (a[i - 1] != b[j - 1]), d[i - 1][j] + 1, d[i][j - 1] + 1)
    ops = []
    i, j = n, m
    while i or j:
        if i and j and d[i][j] == d[i - 1][j - 1] + (a[i - 1] != b[j - 1]):
            ops.append(("eq" if a[i - 1] == b[j - 1] else "sub", i - 1, j - 1))
            i, j = i - 1, j - 1
        elif i and d[i][j] == d[i - 1][j] + 1:
            ops.append(("del", i - 1, j))
            i -= 1
        else:
            ops.append(("ins", i, j - 1))
            j -= 1
    ops.reverse()
    return ops


def char_rules(ocr: str, truth: str, max_len: int):
    """Runs of differing characters between an OCR word and its truth, as (ocr_seq, truth_seq)."""
    ops = align(ocr, truth)
    rules, run_a, run_b, started = [], "", "", False
    prev_eq = ""  # last matching character before the run, for insertions / deletions
    for op, i, j in ops + [("eq", -1, -1)]:
        if op == "eq":
            if started:
                if not run_a or not run_b:  # pure insertion / deletion: anchor on the previous char
                    run_a, run_b = prev_eq + run_a, prev_eq + run_b
                if run_a != run_b and 0 < len(run_a) <= max_len and 0 < len(run_b) <= max_len + 1:
                    rules.append((run_a, run_b))
                run_a, run_b, started = "", "", False
            prev_eq = ocr[i] if i >= 0 else ""
        else:
            started = True
            if op in ("sub", "del"):
                run_a += ocr[i]
            if op in ("sub", "ins"):
                run_b += truth[j]
    return rules


def tesseract_words(image: pathlib.Path, work: pathlib.Path, tessdata: str | None):
    cache = work / (image.stem + ".tsv")
    if not cache.is_file():
        cmd = ["tesseract", str(image), "stdout", "-l", "sin", "--psm", "3", "-c", "tessedit_create_tsv=1", "-c",
               "tessedit_create_txt=0"]
        if tessdata:
            cmd[3:3] = ["--tessdata-dir", tessdata]
        env = dict(os.environ, OMP_THREAD_LIMIT="1")
        res = subprocess.run(cmd, capture_output=True, env=env, check=True)
        cache.write_bytes(res.stdout)
    words = []
    for line in cache.read_text(encoding="utf-8", errors="replace").splitlines()[1:]:
        cols = line.split("\t")
        if len(cols) >= 12 and cols[0] == "5" and cols[11].strip():
            words.append((normalize(cols[11].strip()), float(cols[10])))
    return words


def quantiles(xs):
    if len(xs) < 2:
        return "n/a"
    q = statistics.quantiles(xs, n=20)
    return "  ".join(f"p{p}={q[k]:.0f}" for p, k in ((10, 1), (25, 4), (50, 9), (75, 14), (90, 17)))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("pages", help="directory with <name>_clean.png, <name>_scan.png and <name>.gt.txt")
    ap.add_argument("--work", required=True, help="directory for cached Tesseract TSV output")
    ap.add_argument("--tessdata", help="directory with sin.traineddata (default: Tesseract's own)")
    ap.add_argument("--max-len", type=int, default=2, help="longest source sequence of a rule")
    ap.add_argument("--min-count", type=int, default=2)
    ap.add_argument("--out", required=True, help="rule table (TSV)")
    a = ap.parse_args()

    pages = pathlib.Path(a.pages)
    work = pathlib.Path(a.work)
    work.mkdir(parents=True, exist_ok=True)
    images = sorted(p for p in pages.iterdir() if p.suffix == ".png" and (pages / (p.stem.rsplit("_", 1)[0] + ".gt.txt")).is_file())
    if not images:
        sys.exit(f"no pages with ground truth in {pages}")

    subs = collections.Counter()
    conf_ok, conf_bad = [], []
    per_group = collections.defaultdict(lambda: [0, 0])  # group -> [words, wrong]
    ocr_cores = []
    for k, img in enumerate(images, 1):
        gt_words = [core(w) for w in normalize((pages / (img.stem.rsplit("_", 1)[0] + ".gt.txt")).read_text(encoding="utf-8")).split()]
        ocr = tesseract_words(img, work, a.tessdata)
        ocr_words = [core(w) for w, _ in ocr]
        ocr_cores.extend(w for w in ocr_words if w)
        group = img.stem.rsplit("_", 1)[1] + "/" + img.stem.rsplit("_", 2)[0]  # clean|scan / font prefix
        for op, i, j in align(ocr_words, gt_words):
            if op not in ("eq", "sub") or not ocr_words[i] or not gt_words[j]:
                continue
            conf = ocr[i][1]
            per_group[group][0] += 1
            if op == "eq":
                conf_ok.append(conf)
            else:
                conf_bad.append(conf)
                per_group[group][1] += 1
                for r in char_rules(ocr_words[i], gt_words[j], a.max_len):
                    subs[r] += 1
        print(f"[{k}/{len(images)}] {img.name}", file=sys.stderr)

    print("\nword accuracy by variant/font:")
    for g, (n, bad) in sorted(per_group.items()):
        print(f"  {g:<28} words {n:>6}  wrong {bad:>5}  ({bad / max(n, 1):.1%})")
    print(f"\nconfidence of correct words ({len(conf_ok)}): {quantiles(conf_ok)}")
    print(f"confidence of wrong words   ({len(conf_bad)}): {quantiles(conf_bad)}")
    for t in (60, 70, 75, 80, 85, 90, 95):
        bad = sum(c < t for c in conf_bad) / max(len(conf_bad), 1)
        ok = sum(c < t for c in conf_ok) / max(len(conf_ok), 1)
        print(f"  conf < {t}: wrong words {bad:.1%}   correct words {ok:.1%}")

    sources = {s for s, _ in subs}
    occ = collections.Counter()
    for w in ocr_cores:
        for s in sources:
            if s in w:
                occ[s] += w.count(s)
    rows = []
    for (s, t), n in subs.items():
        if n < a.min_count:
            continue
        rows.append((n, s, t, occ[s], n / max(occ[s], 1)))
    rows.sort(key=lambda r: (-r[0], r[1], r[2]))
    cp = lambda s: " ".join(f"U+{ord(c):04X}" for c in s)
    with open(a.out, "w", encoding="utf-8") as f:
        f.write("ocr\ttruth\tcount\tocr_occurrences\tprobability\tocr_codepoints\ttruth_codepoints\n")
        for n, s, t, o, p in rows:
            f.write(f"{s}\t{t}\t{n}\t{o}\t{p:.3f}\t{cp(s)}\t{cp(t)}\n")
    print(f"\n{len(rows)} rules written to {a.out}; top 25:")
    for n, s, t, o, p in rows[:25]:
        print(f"  {s!r:>12} -> {t!r:<12} count {n:>4}  of {o:>5} occurrences  p={p:.2f}   {cp(s)} -> {cp(t)}")


if __name__ == "__main__":
    main()
