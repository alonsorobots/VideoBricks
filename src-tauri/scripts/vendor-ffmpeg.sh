#!/usr/bin/env bash
# Downloads the FFmpeg binaries the macOS app bundles, so it needs no Homebrew
# FFmpeg. Relying on Homebrew's broke in practice: its ffmpeg links a dozen
# Homebrew dylibs (SDL2 and friends), and when one of those goes missing
# ffprobe dies with "Library not loaded" before VideoBricks can read a video.
#
# These are Martin Riedl's static builds (https://ffmpeg.martin-riedl.de),
# which link only system libraries. Versions are pinned and checksummed so a
# release always ships exactly what was tested.
#
# Produces src-tauri/binaries/{ffmpeg,ffprobe} (gitignored), bundled as Tauri
# resources by tauri.macos.conf.json. The Windows equivalent is vendor-ffmpeg.ps1.
#
# Usage:  bash src-tauri/scripts/vendor-ffmpeg.sh [arm64|x86_64]

set -euo pipefail

ARCH="${1:-$(uname -m)}"
BASE="https://ffmpeg.martin-riedl.de/download/macos"
case "$ARCH" in
  arm64|aarch64)
    BUILD="arm64/1789931890_9.0.2"
    SHA_FFMPEG="c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924"
    SHA_FFPROBE="fcbe839537485eaee7a7a8bc5cbc0f90d53617e80943e8a5b2e31cb851197ea6"
    ;;
  x86_64|amd64)
    BUILD="amd64/1789931006_9.0.2"
    SHA_FFMPEG="7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4"
    SHA_FFPROBE="2322438ed2f6319a691291b247d09c69dcaa3a982460d1f269a7e1af335cfdfd"
    ;;
  *) echo "Unsupported arch: $ARCH" >&2; exit 1 ;;
esac

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DEST="$ROOT/src-tauri/binaries"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$DEST"

for pair in "ffmpeg:$SHA_FFMPEG" "ffprobe:$SHA_FFPROBE"; do
  name="${pair%%:*}"
  sha="${pair#*:}"
  echo "Downloading $name ($BUILD)"
  curl -fsSL "$BASE/$BUILD/$name.zip" -o "$WORK/$name.zip"
  echo "$sha  $WORK/$name.zip" | shasum -a 256 -c - >/dev/null \
    || { echo "Checksum mismatch for $name.zip" >&2; exit 1; }
  unzip -oq "$WORK/$name.zip" -d "$WORK/$name"
  install -m 755 "$WORK/$name/$name" "$DEST/$name"
done

# Verify before the build picks them up: they must run, and must not depend on
# anything outside the OS (a Homebrew path here is exactly the bug this fixes).
"$DEST/ffmpeg" -version | head -1
"$DEST/ffprobe" -version | head -1
if otool -L "$DEST/ffmpeg" "$DEST/ffprobe" | tail -n +2 | grep -v -e '^/' -e '/usr/lib/' -e '/System/'; then
  echo "Bundled FFmpeg links non-system libraries (listed above)" >&2
  exit 1
fi

echo "Done: $DEST"
