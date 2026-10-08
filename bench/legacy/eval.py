#!/usr/bin/env python3
"""Measure legacy-font conversion against OCR on held-out pages.

Every selected page is copied into a single-page PDF (fonts included) and extracted twice with
`lipi`: once with conversion (`--no-verify`, so no OCR runs) and once with
`--no-legacy-convert`, which OCRs the page. The two texts are compared with the character error
rate (OCR as the reference), an unordered word F1 and the character-bigram agreement lipi's
verification uses. OCR is itself about 5% CER on this material, so the numbers are an upper
bound on the conversion's error, not its true error rate.

With `--ocr-from-pairs` the OCR text and time recorded by `derive.py pairs` (same Tesseract
models, 300 dpi, page segmentation mode 3) are used instead of a second `lipi` run. This keeps
the evaluation independent of lipi's resource governor, which holds OCR back while the operating
system reports memory pressure.

Usage:
    python -I bench/legacy/eval.py --root <raw-root> --selection work/holdout.jsonl \
        --out work --lipi target/debug/lipi [--ocr-from-pairs] [--show 3]
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import os
import re
import statistics
import subprocess
import sys
import time
import unicodedata
from pathlib import Path

import pymupdf
from rapidfuzz.distance import Levenshtein

ZWNJ = "‌"


def canonical(s: str) -> str:
    s = unicodedata.normalize("NFC", s).replace(ZWNJ, "")
    return " ".join(s.split())


def plain_text(md: str) -> str:
    """Strip the Markdown lipi adds (mirrors lipi-core's plain-text renderer)."""
    out = []
    for line in md.splitlines():
        t = line.strip().lstrip("#").strip()
        if t.startswith("|") and all(c in "|-: " for c in t):
            continue
        t = t.replace("**", "").replace("__", "").replace("<u>", "").replace("</u>", "")
        if t.startswith("|"):
            t = "\t".join(c.strip() for c in t.strip("|").split("|"))
        out.append(t)
    return "\n".join(out)


def cer(ref: str, hyp: str) -> float:
    r, h = canonical(ref), canonical(hyp)
    return Levenshtein.distance(r, h) / max(1, len(r))


def word_f1(a: str, b: str) -> float:
    wa = collections.Counter(canonical(a).split())
    wb = collections.Counter(canonical(b).split())
    common = sum((wa & wb).values())
    if not wa or not wb:
        return 0.0
    p, r = common / sum(wb.values()), common / sum(wa.values())
    return 0.0 if p + r == 0 else 2 * p * r / (p + r)


def agreement(a: str, b: str) -> float:
    def bigrams(s: str) -> collections.Counter:
        cs = [c for c in canonical(s) if not c.isspace()]
        return collections.Counter(zip(cs, cs[1:]))

    x, y = bigrams(a), bigrams(b)
    nx, ny = sum(x.values()), sum(y.values())
    if nx + ny == 0:
        return 1.0
    return 2 * sum((x & y).values()) / (nx + ny)


def run_lipi(lipi: Path, pdf: Path, extra: list[str]) -> tuple[dict, float]:
    env = dict(os.environ, OMP_THREAD_LIMIT="1")
    t0 = time.time()
    out = subprocess.run(
        ["nice", "-n", "19", str(lipi), "extract", str(pdf), "-f", "json", "-w", "1", "-p", "gentle", "-q", *extra],
        capture_output=True,
        text=True,
        env=env,
        check=True,
    ).stdout
    return json.loads(out), time.time() - t0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", required=True, type=Path)
    ap.add_argument("--selection", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--lipi", required=True, type=Path)
    ap.add_argument("--show", type=int, default=0, help="print the first lines of N converted pages")
    ap.add_argument("--ocr-from-pairs", action="store_true", help="reuse the OCR text of `derive.py pairs`")
    a = ap.parse_args()

    rows = [json.loads(l) for l in a.selection.read_text().splitlines() if l.strip()]
    pages_dir = a.out / "eval_pages"
    pages_dir.mkdir(parents=True, exist_ok=True)
    results = []
    for n, r in enumerate(rows, 1):
        key = hashlib.sha1(f"{r['pdf']}#{r['page']}".encode()).hexdigest()[:16]
        single = pages_dir / f"{key}.pdf"
        if not single.exists():
            src = pymupdf.open(a.root / r["pdf"])
            dst = pymupdf.open()
            dst.insert_pdf(src, from_page=r["page"], to_page=r["page"])
            dst.save(single, garbage=3, deflate=True)
            dst.close()
            src.close()
        conv, conv_wall = run_lipi(a.lipi, single, ["--no-verify"])
        cp = conv["pages"][0]
        if a.ocr_from_pairs:
            pr = json.load(open(a.out / "pairs" / f"{key}.json"))
            op = {"method": {"kind": "ocr"}, "markdown": pr["ocr_text"], "seconds": pr["ocr_seconds"]}
            ocr_wall = pr["ocr_seconds"]
        else:
            ocr, ocr_wall = run_lipi(a.lipi, single, ["--no-legacy-convert", "--no-verify"])
            op = ocr["pages"][0]
        ct, ot = plain_text(cp["markdown"]), plain_text(op["markdown"])
        res = {
            "key": key,
            "pdf": r["pdf"],
            "page": r["page"],
            "conv_method": cp["method"]["kind"],
            "conv_flags": cp.get("flags", []),
            "ocr_method": op["method"]["kind"],
            "ocr_confidence": op.get("confidence"),
            "cer_vs_ocr": cer(ot, ct),
            "word_f1": word_f1(ot, ct),
            "agreement": agreement(ct, ot),
            "conv_page_seconds": cp["seconds"],
            "ocr_page_seconds": op["seconds"],
            "conv_wall": conv_wall,
            "ocr_wall": ocr_wall,
            "chars": len(canonical(ct)),
        }
        results.append(res)
        print(
            f"[{n}/{len(rows)}] {key} {res['conv_method']:<17} CER {res['cer_vs_ocr']:.3f}  F1 {res['word_f1']:.3f}  "
            f"agree {res['agreement']:.3f}  conv {res['conv_page_seconds']:.2f}s  ocr {res['ocr_page_seconds']:.1f}s",
            flush=True,
        )
        if a.show and n <= a.show:
            print("    " + " / ".join(ct.splitlines()[:6])[:600])
    json.dump(results, open(a.out / "eval_results.json", "w"), ensure_ascii=False, indent=1)

    conv_pages = [x for x in results if x["conv_method"] == "legacy_converted"]
    print(f"\n{len(conv_pages)}/{len(results)} pages converted; others: "
          f"{collections.Counter(x['conv_method'] for x in results if x['conv_method'] != 'legacy_converted')}")

    def stats(key: str, xs: list[dict]) -> str:
        v = [x[key] for x in xs]
        return f"mean {statistics.mean(v):.3f}  median {statistics.median(v):.3f}  min {min(v):.3f}  max {max(v):.3f}"

    if conv_pages:
        print("CER vs OCR     ", stats("cer_vs_ocr", conv_pages))
        print("word F1        ", stats("word_f1", conv_pages))
        print("agreement      ", stats("agreement", conv_pages))
        print("conv s/page    ", stats("conv_page_seconds", conv_pages))
        print("ocr s/page     ", stats("ocr_page_seconds", conv_pages))
        print("conv wall s    ", stats("conv_wall", conv_pages))
        print("ocr wall s     ", stats("ocr_wall", conv_pages))
        unmatched = sum(
            int(f.split(":")[1]) for x in conv_pages for f in x["conv_flags"] if f.startswith("legacy_unmatched_tokens:")
        )
        print(f"unmatched tokens: {unmatched} over {len(conv_pages)} pages")
    return 0


if __name__ == "__main__":
    sys.exit(main())
