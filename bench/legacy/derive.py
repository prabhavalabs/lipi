#!/usr/bin/env python3
"""Derive a legacy-font (FM Abhaya) to Unicode mapping from a local corpus.

The mapping is learned, not copied: the text layer of a legacy-font page holds the
font's own byte codes (shown as Latin / Latin-1 characters), and OCR of the rendered
page holds Unicode Sinhala. Words are paired by bounding box, and a monotone
alignment model learns which code sequences produce which Unicode sequences.

Steps (see README.md):

    select   choose train and held-out pages (disjoint documents) with enough FM text
    pairs    render + OCR each page, pair legacy words with OCR words by bounding box
    learn    Viterbi-EM alignment, write mapping candidates and a review report
    emit     print a reviewed mapping JSON as a Rust table

Only the resulting mapping leaves this directory; pairs and OCR text stay in the
(git-ignored) work directory.
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import math
import os
import re
import subprocess
import sys
import tempfile
import time
import unicodedata
from pathlib import Path

import pymupdf

FM_PREFIXES = ("fmabhaya", "fmabaya", "fmabab")
ZWJ = "‍"
ZWNJ = "‌"
VIRAMA = "්"
PREBASE = ("ෙ", "ෛ")  # kombuva, kombu deka


def is_fm(font: str) -> bool:
    n = re.sub(r"[^a-z0-9]", "", re.sub(r"^[A-Z]{6}\+", "", font).lower())
    return n.startswith(FM_PREFIXES)


def is_si_consonant(c: str) -> bool:
    return "ක" <= c <= "ෆ"


# --------------------------------------------------------------------------- select


def page_fm_chars(page: pymupdf.Page) -> tuple[int, int]:
    fm = other = 0
    for b in page.get_text("rawdict")["blocks"]:
        for line in b.get("lines", []):
            for span in line["spans"]:
                n = sum(1 for ch in span["chars"] if not ch["c"].isspace())
                if is_fm(span["font"]):
                    fm += n
                else:
                    other += n
    return fm, other


def cmd_select(a: argparse.Namespace) -> int:
    paths = [p.strip() for p in a.list.read_text().splitlines() if p.strip()]
    paths.sort(key=lambda p: hashlib.sha1(p.encode()).hexdigest())
    a.out.mkdir(parents=True, exist_ok=True)
    train, hold = [], []
    for k, rel in enumerate(paths):
        if len(train) >= a.train_pages and len(hold) >= a.holdout_pages:
            break
        try:
            doc = pymupdf.open(a.root / rel)
        except Exception as e:  # noqa: BLE001
            print(f"skip {rel}: {e}", file=sys.stderr)
            continue
        picked = []
        for i, page in enumerate(doc):
            if i >= a.max_pages or len(picked) >= a.pages_per_doc:
                break
            fm, other = page_fm_chars(page)
            if fm >= a.min_chars:
                picked.append({"pdf": rel, "page": i, "fm_chars": fm, "other_chars": other})
        doc.close()
        if not picked:
            continue
        # Alternate documents between the two sets until each is full.
        target = train if (k % 3 != 0 or len(hold) >= a.holdout_pages) and len(train) < a.train_pages else hold
        target.extend(picked)
    for name, rows in (("train", train), ("holdout", hold)):
        with open(a.out / f"{name}.jsonl", "w") as f:
            for r in rows:
                f.write(json.dumps(r) + "\n")
        print(f"{name}: {len(rows)} pages from {len({r['pdf'] for r in rows})} documents")
    return 0


# --------------------------------------------------------------------------- pairs


def legacy_words(page: pymupdf.Page) -> list[dict]:
    """Words of the text layer with their bbox (points) and whether every glyph is FM."""
    words = []
    for b in page.get_text("rawdict")["blocks"]:
        for line in b.get("lines", []):
            cur: list[tuple[str, tuple, bool]] = []

            def flush() -> None:
                if not cur:
                    return
                text = "".join(c for c, _, _ in cur)
                x0 = min(bb[0] for _, bb, _ in cur)
                y0 = min(bb[1] for _, bb, _ in cur)
                x1 = max(bb[2] for _, bb, _ in cur)
                y1 = max(bb[3] for _, bb, _ in cur)
                words.append({"text": text, "bbox": (x0, y0, x1, y1), "fm": all(f for _, _, f in cur)})
                cur.clear()

            for span in line["spans"]:
                fm = is_fm(span["font"])
                for ch in span["chars"]:
                    if ch["c"].isspace():
                        flush()
                    else:
                        cur.append((ch["c"], ch["bbox"], fm))
            flush()
    return words


def tesseract_words(png: Path, tessdata: Path | None, langs: str, dpi: int) -> tuple[list[dict], str, float]:
    cmd = ["tesseract", str(png), "stdout", "-l", langs, "--psm", "3", "--dpi", str(dpi)]
    if tessdata:
        cmd += ["--tessdata-dir", str(tessdata)]
    cmd += ["-c", "tessedit_create_tsv=1", "-c", "tessedit_create_txt=0"]
    env = dict(os.environ, OMP_THREAD_LIMIT="1")
    t0 = time.time()
    out = subprocess.run(["nice", "-n", "19", *cmd], capture_output=True, text=True, env=env, check=True).stdout
    secs = time.time() - t0
    words, lines = [], collections.OrderedDict()
    scale = 72.0 / dpi
    for row in out.splitlines()[1:]:
        cols = row.split("\t")
        if len(cols) < 12 or cols[0] != "5" or not cols[11].strip():
            continue
        left, top, w, h = (int(cols[i]) for i in (6, 7, 8, 9))
        bbox = (left * scale, top * scale, (left + w) * scale, (top + h) * scale)
        words.append({"text": cols[11].strip(), "bbox": bbox, "conf": float(cols[10])})
        lines.setdefault((cols[2], cols[3], cols[4]), []).append(cols[11].strip())
    text = "\n".join(" ".join(ws) for ws in lines.values())
    return words, text, secs


def iou(a: tuple, b: tuple) -> float:
    ix = max(0.0, min(a[2], b[2]) - max(a[0], b[0]))
    iy = max(0.0, min(a[3], b[3]) - max(a[1], b[1]))
    inter = ix * iy
    if inter <= 0:
        return 0.0
    ua = (a[2] - a[0]) * (a[3] - a[1]) + (b[2] - b[0]) * (b[3] - b[1]) - inter
    return inter / ua if ua > 0 else 0.0


def cmd_pairs(a: argparse.Namespace) -> int:
    rows = [json.loads(l) for l in a.selection.read_text().splitlines() if l.strip()]
    out_dir = a.out / "pairs"
    out_dir.mkdir(parents=True, exist_ok=True)
    scratch = Path(tempfile.mkdtemp(prefix="lipi-legacy-", dir=os.environ.get("TMPDIR")))
    for n, r in enumerate(rows, 1):
        key = hashlib.sha1(f"{r['pdf']}#{r['page']}".encode()).hexdigest()[:16]
        target = out_dir / f"{key}.json"
        if target.exists() and not a.force:
            continue
        doc = pymupdf.open(a.root / r["pdf"])
        page = doc[r["page"]]
        lw = legacy_words(page)
        png = scratch / f"{key}.png"
        page.get_pixmap(dpi=a.dpi, colorspace=pymupdf.csGRAY).save(png)
        ow, ocr_text, secs = tesseract_words(png, a.tessdata, a.langs, a.dpi)
        png.unlink(missing_ok=True)
        # Mutual best match by IoU.
        best_l: dict[int, tuple[float, int]] = {}
        best_o: dict[int, tuple[float, int]] = {}
        for i, w in enumerate(lw):
            for j, o in enumerate(ow):
                s = iou(w["bbox"], o["bbox"])
                if s >= a.min_iou:
                    if s > best_l.get(i, (0, -1))[0]:
                        best_l[i] = (s, j)
                    if s > best_o.get(j, (0, -1))[0]:
                        best_o[j] = (s, i)
        pairs = []
        for i, (s, j) in best_l.items():
            if best_o.get(j, (0, -1))[1] == i:
                pairs.append(
                    {"legacy": lw[i]["text"], "fm": lw[i]["fm"], "ocr": ow[j]["text"], "conf": ow[j]["conf"], "iou": round(s, 3)}
                )
        json.dump(
            {"pdf": r["pdf"], "page": r["page"], "ocr_seconds": secs, "ocr_text": ocr_text, "pairs": pairs},
            open(target, "w"),
            ensure_ascii=False,
        )
        doc.close()
        print(f"[{n}/{len(rows)}] {key} {len(lw)} words, {len(pairs)} pairs, OCR {secs:.1f}s", flush=True)
    return 0


# --------------------------------------------------------------------------- learn


def clean_ocr(s: str) -> str:
    s = unicodedata.normalize("NFC", s)
    s = re.sub(f"{VIRAMA}{ZWNJ}", VIRAMA, s)
    s = s.replace(ZWNJ, "")
    return s.strip(ZWJ)


def to_visual(s: str) -> list[str]:
    """Logical-order Sinhala to the visual order a legacy font stores: decompose two-part vowels
    and move pre-base signs before their consonant cluster. Returns a list of units, where a
    conjunct joiner sequence (virama + ZWJ) stays attached to the virama as one unit."""
    d = list(unicodedata.normalize("NFD", s))
    i = 0
    while i < len(d):
        if d[i] in PREBASE:
            j = i - 1
            if j >= 0 and is_si_consonant(d[j]):
                while j - 3 >= 0 and d[j - 1] == ZWJ and d[j - 2] == VIRAMA and is_si_consonant(d[j - 3]):
                    j -= 3
                sign = d.pop(i)
                d.insert(j, sign)
        i += 1
    # Merge virama+ZWJ into one unit so emissions stay well formed.
    units: list[str] = []
    for c in d:
        if c == ZWJ and units and units[-1].endswith(VIRAMA):
            units[-1] += ZWJ
        else:
            units.append(c)
    return units


class Model:
    def __init__(self, tokens: set[str], max_v: int, lam: float, eps: float) -> None:
        self.tokens = tokens
        self.max_l = max(len(t) for t in tokens)
        self.max_v = max_v
        self.lam = lam
        self.eps = eps
        self.counts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
        self.init: dict[tuple[str, str], float] = {}

    def logp(self, l: str, v: str) -> float:
        c = self.counts.get(l)
        if c:
            total = sum(c.values())
            p = (c.get(v, 0.0) + self.eps) / (total + self.eps * 2000)
        else:
            p = self.init.get((l, v), self.eps / 2000)
        return math.log(p) + (len(l) - 1) * math.log(self.lam)

    def align(self, l: str, v: list[str]) -> tuple[float, list[tuple[str, str]]] | None:
        n, m = len(l), len(v)
        best = [[-math.inf] * (m + 1) for _ in range(n + 1)]
        back: list[list[tuple[int, int] | None]] = [[None] * (m + 1) for _ in range(n + 1)]
        best[0][0] = 0.0
        for i in range(n + 1):
            for j in range(m + 1):
                if best[i][j] == -math.inf:
                    continue
                for dl in range(1, self.max_l + 1):
                    if i + dl > n:
                        break
                    tok = l[i : i + dl]
                    if tok not in self.tokens:
                        continue
                    for dv in range(0, self.max_v + 1):
                        if j + dv > m:
                            break
                        s = best[i][j] + self.logp(tok, "".join(v[j : j + dv]))
                        if s > best[i + dl][j + dv]:
                            best[i + dl][j + dv] = s
                            back[i + dl][j + dv] = (i, j)
        if best[n][m] == -math.inf:
            return None
        path = []
        i, j = n, m
        while (i, j) != (0, 0):
            pi, pj = back[i][j]
            path.append((l[pi:i], "".join(v[pj:j])))
            i, j = pi, pj
        path.reverse()
        return best[n][m], path


def cmd_learn(a: argparse.Namespace) -> int:
    pairs: list[tuple[str, list[str]]] = []
    for f in sorted((a.out / "pairs").glob("*.json")):
        d = json.load(open(f))
        if a.selection:
            keep = {json.loads(l)["pdf"] for l in a.selection.read_text().splitlines() if l.strip()}
            if d["pdf"] not in keep:
                continue
        for p in d["pairs"]:
            if not p["fm"] or p["conf"] < a.min_conf or p["iou"] < a.min_iou:
                continue
            o = clean_ocr(p["ocr"])
            if not o or not any("඀" <= c <= "෿" for c in o):
                continue
            pairs.append((p["legacy"], to_visual(o)))
    print(f"{len(pairs)} word pairs")

    # Candidate tokens: every code, plus 2- and 3-code sequences that occur often enough.
    uni = collections.Counter()
    multi = collections.Counter()
    for l, _ in pairs:
        uni.update(l)
        for n in (2, 3):
            multi.update(l[i : i + n] for i in range(len(l) - n + 1))
    tokens = set(uni) | {t for t, c in multi.items() if c >= a.min_multi}

    # Co-occurrence initialisation (Dice over word pairs) for code -> unit.
    cl, cv, cb = collections.Counter(), collections.Counter(), collections.Counter()
    for l, v in pairs:
        ls, vs = set(l), set(v)
        cl.update(ls)
        cv.update(vs)
        cb.update((x, y) for x in ls for y in vs)
    dice = {k: 2 * n / (cl[k[0]] + cv[k[1]]) for k, n in cb.items()}
    model = Model(tokens, max_v=a.max_units, lam=a.lam, eps=1e-4)
    units = set(cv)
    for tok in tokens:
        for u in units:
            d = max(dice.get((c, u), 0.0) for c in tok)
            if d > 0.02:
                model.init[(tok, u)] = d
                for u2 in units:
                    d2 = max(dice.get((c, u2), 0.0) for c in tok)
                    if d2 > 0.02:
                        model.init[(tok, u + u2)] = 0.5 * min(d, d2)
    for it in range(a.iterations):
        counts: dict[str, collections.Counter] = collections.defaultdict(collections.Counter)
        total = 0.0
        aligned = 0
        for l, v in pairs:
            r = model.align(l, v)
            if r is None:
                continue
            score, path = r
            total += score
            aligned += 1
            for tok, emit in path:
                counts[tok][emit] += 1
        model.counts = counts
        print(f"iteration {it + 1}: aligned {aligned}/{len(pairs)}, log-likelihood {total:.0f}")

    # Report.
    result = {}
    lines = []
    for tok in sorted(model.counts, key=lambda t: (len(t), t)):
        c = model.counts[tok]
        n = sum(c.values())
        top, ntop = c.most_common(1)[0]
        share = ntop / n
        single = "".join(model.counts[ch].most_common(1)[0][0] if ch in model.counts and model.counts[ch] else "?" for ch in tok)
        if len(tok) > 1 and (n < a.min_multi or share < a.min_share or top == single):
            continue
        if len(tok) == 1 and n < 2:
            continue
        result[tok] = {"to": top, "count": n, "share": round(share, 3), "alternatives": [[e, k] for e, k in c.most_common(4)[1:]]}
        flag = "" if share >= 0.7 else "  REVIEW"
        alts = ", ".join(f"{e!r}×{k}" for e, k in c.most_common(4)[1:])
        lines.append(f"{tok!r:10} -> {top!r:14} n={n:<6} share={share:.2f}{flag}   alts: {alts}")
    json.dump(result, open(a.out / "mapping_candidates.json", "w"), ensure_ascii=False, indent=1)
    (a.out / "learn_report.txt").write_text("\n".join(lines) + "\n")
    print(f"{len(result)} tokens written to {a.out / 'mapping_candidates.json'}; report in learn_report.txt")
    return 0


# --------------------------------------------------------------------------- inspect


def cmd_inspect(a: argparse.Namespace) -> int:
    """Print word pairs whose legacy text contains a code sequence, to review a mapping by eye."""
    seen = collections.Counter()
    for f in sorted((a.out / "pairs").glob("*.json")):
        for p in json.load(open(f))["pairs"]:
            if p["fm"] and p["conf"] >= a.min_conf and a.code in p["legacy"]:
                seen[(p["legacy"], clean_ocr(p["ocr"]))] += 1
    for (l, o), n in seen.most_common(a.limit):
        print(f"{n:>4}  {l!r:28} {o}")
    print(f"{sum(seen.values())} occurrences, {len(seen)} distinct pairs")
    return 0


# --------------------------------------------------------------------------- emit


def rust_str(s: str) -> str:
    """Rust string literal: Sinhala stays readable; joiners, quotes and controls are escaped."""
    out = []
    for c in s:
        if c in '"\\':
            out.append("\\" + c)
        elif 0x20 <= ord(c) < 0x7F or "\u0d80" <= c <= "\u0dff":
            out.append(c)
        else:
            out.append(f"\\u{{{ord(c):04X}}}")
    return '"' + "".join(out) + '"'


def cmd_emit(a: argparse.Namespace) -> int:
    m = json.load(open(a.mapping))
    entries = sorted(m.items(), key=lambda kv: kv[0])
    print("const TABLE: &[(&str, &str)] = &[")
    for code, to in entries:
        target = to["to"] if isinstance(to, dict) else to
        print(f"    ({rust_str(code)}, {rust_str(target)}),")
    print("];")
    return 0


# --------------------------------------------------------------------------- main


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("select")
    s.add_argument("--root", required=True, type=Path)
    s.add_argument("--list", required=True, type=Path)
    s.add_argument("--out", required=True, type=Path)
    s.add_argument("--train-pages", type=int, default=90)
    s.add_argument("--holdout-pages", type=int, default=40)
    s.add_argument("--pages-per-doc", type=int, default=2)
    s.add_argument("--max-pages", type=int, default=12)
    s.add_argument("--min-chars", type=int, default=600)
    s.set_defaults(fn=cmd_select)

    s = sub.add_parser("pairs")
    s.add_argument("--root", required=True, type=Path)
    s.add_argument("--selection", required=True, type=Path)
    s.add_argument("--out", required=True, type=Path)
    s.add_argument("--tessdata", type=Path, default=None)
    s.add_argument("--langs", default="sin+eng")
    s.add_argument("--dpi", type=int, default=300)
    s.add_argument("--min-iou", type=float, default=0.4)
    s.add_argument("--force", action="store_true")
    s.set_defaults(fn=cmd_pairs)

    s = sub.add_parser("learn")
    s.add_argument("--out", required=True, type=Path)
    s.add_argument("--selection", type=Path, default=None, help="restrict to pages of these documents")
    s.add_argument("--min-conf", type=float, default=60.0)
    s.add_argument("--min-iou", type=float, default=0.5)
    s.add_argument("--min-multi", type=int, default=6)
    s.add_argument("--min-share", type=float, default=0.6)
    s.add_argument("--max-units", type=int, default=4)
    s.add_argument("--lam", type=float, default=0.25)
    s.add_argument("--iterations", type=int, default=8)
    s.set_defaults(fn=cmd_learn)

    s = sub.add_parser("inspect")
    s.add_argument("--out", required=True, type=Path)
    s.add_argument("code", help="code sequence to look for")
    s.add_argument("--min-conf", type=float, default=60.0)
    s.add_argument("--limit", type=int, default=25)
    s.set_defaults(fn=cmd_inspect)

    s = sub.add_parser("emit")
    s.add_argument("mapping", type=Path)
    s.set_defaults(fn=cmd_emit)

    a = ap.parse_args()
    return a.fn(a)


if __name__ == "__main__":
    sys.exit(main())
