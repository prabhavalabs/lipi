#!/usr/bin/env bash
# Render every D2 diagram in src/ to svg/. Requires D2 >= 0.9 (brew install d2).
set -euo pipefail
cd "$(dirname "$0")/src"
for src in [0-9]*.d2; do
  d2 --layout elk "$src" "../svg/${src%.d2}.svg"
done
