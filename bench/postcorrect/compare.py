#!/usr/bin/env python3
"""Compare two `lipi bench --json` results, grouped by font and clean/degraded variant.

Pages are named `<lang>_<font>_<n>_<variant>.png` by bench/synth.py when it is run once per font, so
the group of a row is `<font>/<variant>`. Prints a Markdown table of mean CER and WER before and after.

  python3 -I bench/postcorrect/compare.py before.json after.json
"""
import collections
import json
import sys


def groups(rows):
    g = collections.defaultdict(list)
    for r in rows:
        stem = r["file"].rsplit(".", 1)[0]
        parts = stem.split("_")
        font = "_".join(parts[1:-2]) if len(parts) >= 4 else "all"
        g[(font, parts[-1])].append(r)
        g[("all", parts[-1])].append(r)
        g[("all", "all")].append(r)
    return g


def mean(rows, key):
    return 100.0 * sum(r[key] for r in rows) / max(len(rows), 1)


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    before = {r["file"]: r for r in json.load(open(sys.argv[1], encoding="utf-8"))}
    after = {r["file"]: r for r in json.load(open(sys.argv[2], encoding="utf-8"))}
    common = sorted(set(before) & set(after))
    if len(common) != len(before) or len(common) != len(after):
        print(f"warning: {len(before)} before, {len(after)} after, {len(common)} in common", file=sys.stderr)
    gb = groups([before[f] for f in common])
    ga = groups([after[f] for f in common])
    print("| Font | Variant | Pages | CER before | CER after | WER before | WER after |")
    print("|---|---|---|---|---|---|---|")
    for key in sorted(gb, key=lambda k: (k[0] == "all", k)):
        b, a = gb[key], ga[key]
        print(f"| {key[0]} | {key[1]} | {len(b)} | {mean(b, 'cer'):.2f}% | {mean(a, 'cer'):.2f}% | "
              f"{mean(b, 'wer'):.2f}% | {mean(a, 'wer'):.2f}% |")
    worse = [f for f in common if after[f]["cer"] > before[f]["cer"] + 1e-9]
    print(f"\npages with higher CER after: {len(worse)}" + (": " + ", ".join(worse) if worse else ""))


if __name__ == "__main__":
    main()
