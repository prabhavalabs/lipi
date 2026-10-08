#!/usr/bin/env python3
"""Render synthetic benchmark pages from Unicode text, with ground truth, for `lipi bench`.

Each page is written three times:
  <id>.gt.txt        the exact text drawn on the page (ground truth)
  <id>_clean.png     A4 at 300 dpi, black on white
  <id>_scan.png      the same page degraded like a photocopy: 200 dpi, blur, noise, slight skew

Complex-script shaping needs Pillow built with libraqm (the PyPI wheels include it).

  python3 -m venv .venv && .venv/bin/pip install -r bench/requirements.txt
  .venv/bin/python bench/synth.py --text corpus.txt --lang si \
      --font "/System/Library/Fonts/Supplemental/Sinhala Sangam MN.ttc" --pages 6 --out bench/corpus/si

The text file is any UTF-8 text in the target language; words are taken in order.
"""
import argparse
import os
import random
import sys

from PIL import Image, ImageDraw, ImageFilter, ImageFont, features

W, H, MARGIN = 2480, 3508, 200  # A4 at 300 dpi


def wrap(draw, words, font, width):
    lines, cur = [], ""
    for w in words:
        t = (cur + " " + w).strip()
        if draw.textlength(t, font=font) > width and cur:
            lines.append(cur)
            cur = w
        else:
            cur = t
    if cur:
        lines.append(cur)
    return lines


def degrade(img, rng):
    small = img.resize((W * 2 // 3, H * 2 // 3)).filter(ImageFilter.GaussianBlur(1.1)).rotate(0.6, fillcolor=255)
    return Image.eval(small, lambda p: max(0, min(255, p + rng.randint(-35, 35))))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--text", required=True, help="UTF-8 text file in the target language")
    ap.add_argument("--lang", required=True, help="language code used in file names (si, ta, en)")
    ap.add_argument("--font", action="append", required=True, help="font file; repeat to cycle fonts across pages")
    ap.add_argument("--pages", type=int, default=6)
    ap.add_argument("--words", type=int, default=260, help="words per page")
    ap.add_argument("--size", type=int, default=42, help="font size in pixels at 300 dpi")
    ap.add_argument("--seed", type=int, default=7)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()

    if not features.check("raqm"):
        sys.exit("Pillow was built without libraqm: Sinhala and Tamil would be shaped incorrectly")
    rng = random.Random(a.seed)
    words = open(a.text, encoding="utf-8").read().split()
    if len(words) < a.words:
        sys.exit(f"{a.text}: need at least {a.words} words")
    os.makedirs(a.out, exist_ok=True)
    for i in range(a.pages):
        font_path = a.font[i % len(a.font)]
        font = ImageFont.truetype(font_path, a.size, layout_engine=ImageFont.Layout.RAQM)
        img = Image.new("L", (W, H), 255)
        draw = ImageDraw.Draw(img)
        start = (i * a.words) % max(1, len(words) - a.words)
        lines = wrap(draw, words[start:start + a.words], font, W - 2 * MARGIN)[: (H - 2 * MARGIN) // (a.size * 2)]
        y = MARGIN
        for line in lines:
            draw.text((MARGIN, y), line, font=font, fill=0)
            y += int(a.size * 1.85)
        base = os.path.join(a.out, f"{a.lang}_{i}")
        with open(base + ".gt.txt", "w", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
        img.save(base + "_clean.png")
        degrade(img, rng).save(base + "_scan.png")
        print(f"{base}  {os.path.basename(font_path)}  {len(lines)} lines")


if __name__ == "__main__":
    main()
