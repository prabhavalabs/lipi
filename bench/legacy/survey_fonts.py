#!/usr/bin/env python3
"""Survey the fonts used by Sinhala/Tamil PDFs in a local corpus.

Reads a sample of PDFs, collects the base font names of every page with PyMuPDF
and groups them into legacy font families. No document text is read or kept.

Usage:
    python -I bench/legacy/survey_fonts.py --root <raw-root> --list <paths.txt> [--sample 200]

`paths.txt` has one PDF path (relative to `--root`) per line. See README.md for how
such a list is produced from the sl-law catalog.
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import re
import sys
from pathlib import Path

import pymupdf

# Prefixes of normalised font names (lower case, letters and digits only, subset tag removed).
FAMILIES = [
    ("fmabhaya", "fm-abhaya"),
    ("fmabaya", "fm-abhaya"),
    ("fmabab", "fm-abhaya"),  # FMAbabld, the bold companion with the same encoding
    ("fmbindumathi", "fm-bindumathi"),
    ("fmmalithi", "fm-malithi"),
    ("fmderana", "fm-derana"),
    ("fmemanee", "fm-emanee"),
    ("fmganganee", "fm-ganganee"),
    ("fmsamantha", "fm-samantha"),
    ("fmarjun", "fm-arjunn"),
    ("fmbasuru", "fm-basuru"),
    ("fmgemunu", "fm-gemunu"),
    ("fmrashmi", "fm-rashmi"),
    ("fmsandhyanee", "fm-sandhyanee"),
    ("fmpodiyan", "fm-podiyan"),
    ("fmmadhura", "fm-madhura"),
    ("fm", "fm-other"),
    ("dlmanel", "dl-manel"),
    ("dlparas", "dl-paras"),
    ("dlaraliya", "dl-araliya"),
    ("dlhimaya", "dl-himaya"),
    ("dlsarasavi", "dl-sarasavi"),
    ("dlyasarasi", "dl-yasarasi"),
    ("dl", "dl-other"),
    ("kaputa", "kaputa"),
    ("thibus", "thibus"),
    ("amalee", "amalee"),
    ("sandaya", "sandaya"),
    ("wijaya", "wijaya"),
    ("isiwara", "isi"),
    ("kandy", "kandy"),
    ("bamini", "bamini"),
    ("baamini", "bamini"),
    ("vanavil", "vanavil"),
    ("kalaham", "kalaham"),
    ("amudham", "amudham"),
    ("kalaimakal", "kalaimakal"),
    ("shreetam", "shree-tam"),
    ("elango", "elango"),
    ("mylai", "mylai"),
    ("tamtam", "tam"),
    ("tscu", "tscii"),
    ("tscii", "tscii"),
    ("tam", "tam-tab"),
    ("tab", "tam-tab"),
]

UNICODE_HINTS = ("iskoola", "nirmala", "noto", "latha", "vijaya", "malithi web", "bhashitha", "potha")


def normalise(name: str) -> str:
    m = re.match(r"^[A-Z]{6}\+(.*)$", name)
    if m:
        name = m.group(1)
    return re.sub(r"[^a-z0-9]", "", name.lower())


def family(name: str) -> str | None:
    n = normalise(name)
    for prefix, fam in FAMILIES:
        if n.startswith(prefix):
            return fam
    return None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", required=True, type=Path)
    ap.add_argument("--list", required=True, type=Path, help="one relative PDF path per line")
    ap.add_argument("--sample", type=int, default=200, help="documents to inspect (deterministic sample)")
    ap.add_argument("--max-pages", type=int, default=50, help="pages inspected per document")
    a = ap.parse_args()

    paths = [p.strip() for p in a.list.read_text().splitlines() if p.strip()]
    paths.sort(key=lambda p: hashlib.sha1(p.encode()).hexdigest())
    paths = paths[: a.sample]

    docs_with = collections.Counter()  # family -> documents containing it
    pages_with = collections.Counter()  # family -> pages containing it
    raw_names = collections.Counter()  # raw (unsubsetted) names of legacy fonts
    unknown_nonlatin = collections.Counter()
    combos = collections.Counter()
    n_docs = n_pages = 0
    for rel in paths:
        path = a.root / rel
        try:
            doc = pymupdf.open(path)
        except Exception as e:  # noqa: BLE001 - untrusted input, report and continue
            print(f"skip {rel}: {e}", file=sys.stderr)
            continue
        n_docs += 1
        doc_fams: set[str] = set()
        for i, page in enumerate(doc):
            if i >= a.max_pages:
                break
            n_pages += 1
            fams: set[str] = set()
            for f in page.get_fonts():
                base = f[3]
                fam = family(base)
                if fam:
                    fams.add(fam)
                    raw_names[re.sub(r"^[A-Z]{6}\+", "", base)] += 1
                elif not any(h in base.lower() for h in ("times", "arial", "calibri", "minion", "helvetica", "courier", "verdana", "tahoma", "symbol", "wingding")):
                    unknown_nonlatin[re.sub(r"^[A-Z]{6}\+", "", base)] += 1
            for fam in fams:
                pages_with[fam] += 1
            doc_fams |= fams
        for fam in doc_fams:
            docs_with[fam] += 1
        combos[tuple(sorted(doc_fams))] += 1
        doc.close()

    print(f"documents: {n_docs}  pages: {n_pages}\n")
    print(f"{'family':<16} {'docs':>6} {'pages':>7}")
    for fam, n in docs_with.most_common():
        print(f"{fam:<16} {n:>6} {pages_with[fam]:>7}")
    print("\nlegacy font names (top 30):")
    for name, n in raw_names.most_common(30):
        print(f"  {n:>6}  {name}")
    print("\nfamily combinations per document (top 10):")
    for combo, n in combos.most_common(10):
        print(f"  {n:>6}  {'+'.join(combo) or '(none)'}")
    print("\nother non-Latin-looking font names (top 20):")
    for name, n in unknown_nonlatin.most_common(20):
        print(f"  {n:>6}  {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
