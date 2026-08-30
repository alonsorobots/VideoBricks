#!/usr/bin/env bash
# Builds the self-contained Python runtime that ships with the macOS app, so
# shot detection works without the user installing anything.
#
# Produces src-tauri/python/ (~90 MB), which is gitignored and bundled as a
# Tauri resource by tauri.macos.conf.json.
#
# The Windows equivalent is vendor-python.ps1. Windows uses python.org's
# embeddable distribution, which has no macOS counterpart; here we use
# python-build-standalone, whose `install_only` builds are relocatable.
#
# Usage:  bash src-tauri/scripts/vendor-python.sh [arm64|x86_64]

set -euo pipefail

PBS_TAG="20260825"
PY_VERSION="3.11.16"
PY_MINOR="3.11"

ARCH="${1:-$(uname -m)}"
case "$ARCH" in
  arm64|aarch64) PBS_ARCH="aarch64-apple-darwin" ;;
  x86_64|amd64)  PBS_ARCH="x86_64-apple-darwin" ;;
  *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="$ROOT/src-tauri/python"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "Vendoring CPython $PY_VERSION ($PBS_ARCH) into $DEST"

URL="https://github.com/astral-sh/python-build-standalone/releases/download/$PBS_TAG/cpython-$PY_VERSION+$PBS_TAG-$PBS_ARCH-install_only.tar.gz"
curl -fsSL "$URL" -o "$WORK/python.tar.gz"
tar -xzf "$WORK/python.tar.gz" -C "$WORK"   # extracts to $WORK/python

PY="$WORK/python/bin/python3"

# Shot-detection dependencies. install_only ships pip, so these land in the
# runtime's own site-packages and stay relocatable.
"$PY" -m pip install --quiet --upgrade pip
"$PY" -m pip install --quiet \
  "onnxruntime>=1.17.0" "numpy>=1.24.0" "ffmpeg-python>=0.2.0"

# Drop what the shot-detection code path never reaches.
SP="$WORK/python/lib/python$PY_MINOR/site-packages"
find "$SP/numpy" -type d -name tests -prune -exec rm -rf {} + 2>/dev/null || true
for p in numpy/_core/include numpy/_core/lib numpy/f2py \
         onnxruntime/training onnxruntime/transformers \
         onnxruntime/quantization onnxruntime/tools; do
  rm -rf "${SP:?}/$p"
done
find "$WORK/python" -type d -name __pycache__ -prune -exec rm -rf {} + 2>/dev/null || true
# Static libs and headers are only needed to build against Python, not to run it.
rm -rf "$WORK/python/lib/python$PY_MINOR/test" "$WORK/python/include" 2>/dev/null || true
find "$WORK/python/lib" -name "libpython*.a" -delete 2>/dev/null || true

# Verify before installing it into the repo.
"$PY" -c "import onnxruntime, numpy, ffmpeg; print('deps OK')"

rm -rf "$DEST"
mv "$WORK/python" "$DEST"

echo "Done: $DEST ($(du -sh "$DEST" | cut -f1))"
