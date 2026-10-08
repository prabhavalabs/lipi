#!/usr/bin/env python3
"""Split clean Unicode documents into lexicon, confusion-learning and held-out sets, by document.

Each document is assigned to one set from the SHA-1 of its path relative to the root, so the split is
stable across runs and machines and no document ever appears in two sets. Default shares are
80 / 10 / 10.

  python3 -I bench/postcorrect/split.py <root> --glob '*-si/doc.txt' --out "$TMPDIR/lipi-pc"

Writes under --out (never commit these; they are corpus text):
  train.txt     documents for the lexicon, concatenated and separated by blank lines
  learn.txt     documents for learning OCR confusions
  heldout.txt   documents for the evaluation pages
  split.json    which document went where
"""
import argparse
import hashlib
import json
import os
import pathlib
import random
import sys


def bucket(rel: str) -> int:
    return int(hashlib.sha1(rel.encode("utf-8")).hexdigest()[:8], 16) % 100


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("root", help="directory searched recursively for documents")
    ap.add_argument("--glob", default="*-si/doc.txt", help="path pattern relative to root")
    ap.add_argument("--train", type=int, default=80, help="percent of documents for the lexicon")
    ap.add_argument("--learn", type=int, default=10, help="percent of documents for confusion learning")
    ap.add_argument("--seed", type=int, default=7, help="order of documents inside each set")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()

    root = pathlib.Path(a.root)
    docs = sorted(p.relative_to(root).as_posix() for p in root.rglob(a.glob) if p.is_file())
    if not docs:
        sys.exit(f"no documents matching {a.glob!r} under {root}")
    sets = {"train": [], "learn": [], "heldout": []}
    for rel in docs:
        b = bucket(rel)
        key = "train" if b < a.train else "learn" if b < a.train + a.learn else "heldout"
        sets[key].append(rel)
    rng = random.Random(a.seed)
    os.makedirs(a.out, exist_ok=True)
    for key, rels in sets.items():
        rng.shuffle(rels)
        words = 0
        with open(os.path.join(a.out, f"{key}.txt"), "w", encoding="utf-8") as f:
            for rel in rels:
                text = (root / rel).read_text(encoding="utf-8", errors="replace").strip()
                words += len(text.split())
                f.write(text + "\n\n")
        print(f"{key:<8} {len(rels):>5} documents {words:>8} words")
    with open(os.path.join(a.out, "split.json"), "w", encoding="utf-8") as f:
        json.dump(sets, f, indent=1, ensure_ascii=False)


if __name__ == "__main__":
    main()
