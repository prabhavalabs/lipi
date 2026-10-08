#!/usr/bin/env python3
"""Tune the corrector's thresholds on the learning pages, reusing the OCR output cached by learn.py.

The same algorithm as `lipi_script::correct` is run over every aligned (OCR word, true word) pair of the
learning pages for a grid of options, and the configurations are ranked by the remaining character
edits. Nothing here touches the held-out pages.

  python3 -I bench/postcorrect/simulate.py "$TMPDIR/lipi-pc/learn" --work "$TMPDIR/lipi-pc/learn-ocr" \
      --train "$TMPDIR/lipi-pc/train.txt" --rules "$TMPDIR/lipi-pc/rules.tsv"
"""
import argparse
import collections
import itertools
import os
import pathlib
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import learn  # noqa: E402

WORD_RE = re.compile(r"[඀-෿‌‍]+")


def tokens(text):
    for m in WORD_RE.finditer(text):
        w = m.group().strip("‌‍")
        if w:
            yield w


def load_rules(path, min_count, min_p):
    rules = []
    for line in pathlib.Path(path).read_text(encoding="utf-8").splitlines()[1:]:
        src, dst, n, _occ, p = line.split("\t")[:5]
        if int(n) < min_count or float(p) < min_p or len(src) > 2 or len(dst) > 3 or "‌" in dst:
            continue
        rules.append((src, dst, max(float(p), 0.001)))
    return rules


def variants(word, rules, max_edits=2):
    seen = {}
    frontier = [(word, 1.0)]
    for depth in range(1, max_edits + 1):
        nxt = []
        for w, wt in frontier:
            for src, dst, p in rules:
                start = 0
                while (i := w.find(src, start)) != -1:
                    start = i + 1
                    v = w[:i] + dst + w[i + len(src):]
                    if v == word:
                        continue
                    score = wt * p
                    if v not in seen or seen[v][1] < score:
                        seen[v] = (depth, score)
                        nxt.append((v, score))
        frontier = nxt
    return seen


def correct(word, conf, lex, vars_, o):
    if len(word) < o["min_len"] or word in lex:
        return None
    if o["gate"] is not None and conf is not None and conf >= o["gate"]:
        return None
    cands = sorted(((lex[v] * wt, lex[v], v) for v, (_, wt) in vars_.items() if v in lex), reverse=True)
    if not cands:
        return None
    score, freq, best = cands[0]
    if freq < o["min_freq"] or score < o["min_score"]:
        return None
    if len(cands) > 1 and score < o["margin"] * cands[1][0]:
        return None
    return best


def lev(a, b):
    prev = list(range(len(b) + 1))
    for i, x in enumerate(a, 1):
        cur = [i]
        for j, y in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x != y)))
        prev = cur
    return prev[-1]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("pages")
    ap.add_argument("--work", required=True)
    ap.add_argument("--train", required=True, help="lexicon text")
    ap.add_argument("--rules", required=True)
    ap.add_argument("--top", type=int, default=15)
    a = ap.parse_args()

    lex = collections.Counter(tokens(learn.normalize(pathlib.Path(a.train).read_text(encoding="utf-8"))))
    print(f"lexicon: {sum(lex.values())} tokens, {len(lex)} types")
    pages = pathlib.Path(a.pages)
    work = pathlib.Path(a.work)
    pairs = []  # (ocr, truth, conf, group)
    for img in sorted(p for p in pages.iterdir() if p.suffix == ".png"):
        gt = [learn.core(w) for w in learn.normalize((pages / (img.stem.rsplit("_", 1)[0] + ".gt.txt")).read_text(encoding="utf-8")).split()]
        ocr = learn.tesseract_words(img, work, None)
        ocr_words = [learn.core(w) for w, _ in ocr]
        group = img.stem.rsplit("_", 1)[1] + "/" + img.stem.rsplit("_", 2)[0]
        for op, i, j in learn.align(ocr_words, gt):
            if op in ("eq", "sub") and ocr_words[i] and gt[j]:
                pairs.append((ocr_words[i], gt[j], ocr[i][1], group))
    oov = [p for p in pairs if p[0] not in lex]
    wrong = sum(p[0] != p[1] for p in pairs)
    print(f"{len(pairs)} word pairs, {wrong} wrong; {len(oov)} OCR words outside the lexicon, "
          f"of which {sum(p[0] != p[1] for p in oov)} wrong (correct OOV words: {sum(p[0] == p[1] for p in oov)})")
    base_dist = sum(lev(o, t) for o, t, _, _ in pairs)

    rule_sets = {(mc, mp): load_rules(a.rules, mc, mp) for mc in (4, 10) for mp in (0.0, 0.01, 0.05)}
    results = []
    for (mc, mp), rules in rule_sets.items():
        cache = {w: variants(w, rules) for w in {p[0] for p in oov}}
        grid = itertools.product((None, 93, 95, 97), (0.0, 0.02, 0.05, 0.1, 0.2, 1.0), (2.0, 3.0, 5.0), (1, 2))
        for gate, min_score, margin, min_freq in grid:
            o = dict(gate=gate, min_score=min_score, margin=margin, min_freq=min_freq, min_len=3)
            fixed = harmed = still = 0
            dist = base_dist
            by_group = collections.defaultdict(lambda: [0, 0])
            for ocr, truth, conf, group in oov:
                fix = correct(ocr, conf, lex, cache[ocr], o)
                if fix is None:
                    continue
                dist += lev(fix, truth) - lev(ocr, truth)
                if fix == truth:
                    fixed += 1
                    by_group[group][0] += 1
                elif ocr == truth:
                    harmed += 1
                    by_group[group][1] += 1
                else:
                    still += 1
            results.append((dist, fixed, harmed, still, mc, mp, len(rules), o, dict(by_group)))
    results.sort(key=lambda r: (r[0], r[2]))
    print(f"\nbaseline character edits: {base_dist}\n")
    print(f"{'edits':>6} {'fixed':>5} {'harm':>4} {'other':>5}  rules(min_count,min_p,n)  options")
    for dist, fixed, harmed, still, mc, mp, n, o, _ in results[: a.top]:
        print(f"{dist:>6} {fixed:>5} {harmed:>4} {still:>5}  ({mc},{mp},{n})  {o}")
    print("\nbest per confidence gate:")
    for gate in (None, 93, 95, 97):
        r = min((r for r in results if r[7]["gate"] == gate), key=lambda r: (r[0], r[2]))
        print(f"  gate {str(gate):>4}: edits {r[0]} fixed {r[1]} harmed {r[2]} other {r[3]}  rules({r[4]},{r[5]})  {r[7]}")
    best = results[0]
    print("\nbest configuration by variant/font (fixed, harmed):")
    for g, (f, h) in sorted(best[8].items()):
        print(f"  {g:<22} fixed {f:>4}  harmed {h:>3}")


if __name__ == "__main__":
    main()
