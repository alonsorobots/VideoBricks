#!/usr/bin/env bash
# Guarantees the app inside a built DMG carries a valid, deep ad-hoc signature.
#
# Tauri only ad-hoc signs the main binary via the linker; it does not seal the
# app *bundle*. The signature then claims resources are sealed while
# Contents/Resources (icons, scripts, and the bundled Python runtime) has no
# seal, so `codesign --verify` fails with:
#
#   code has no resources but signature indicates they must be present
#
# A locally built app is never quarantined, so macOS never checks and it runs
# fine -- which is why this stayed invisible until we published a DMG people
# download. A downloaded copy IS checked, fails, and macOS reports
# "VideoBricks is damaged and can't be opened", which right-click > Open
# cannot bypass.
#
# This re-signs the app inside the DMG and repackages it, preserving Tauri's
# DMG layout. It is idempotent and verifies before and after.
#
# Usage:  bash src-tauri/scripts/sign-macos-dmg.sh path/to/App.dmg

set -euo pipefail

DMG="${1:?usage: sign-macos-dmg.sh <path-to-dmg>}"
[ -f "$DMG" ] || { echo "No such DMG: $DMG" >&2; exit 1; }

WORK="$(mktemp -d)"
MNT="$WORK/mnt"
DEV=""
trap '[ -n "$DEV" ] && hdiutil detach "$DEV" -force -quiet 2>/dev/null; rm -rf "$WORK"' EXIT

echo "==> Repackaging $(basename "$DMG") with a deep ad-hoc signature"

# Compressed DMGs are read-only; convert so the app can be signed in place.
hdiutil convert "$DMG" -format UDRW -o "$WORK/rw.dmg" -quiet
mkdir -p "$MNT"
# Remember the device node: a timed-out detach can remove the mount point
# while leaving the device attached, after which the path no longer works.
DEV="$(hdiutil attach "$WORK/rw.dmg" -nobrowse -owners off -mountpoint "$MNT" \
  | awk '/^\/dev\// && !dev { dev = $1 } END { print dev }')"
[ -n "$DEV" ] || { echo "Could not attach $WORK/rw.dmg" >&2; exit 1; }

APP="$(find "$MNT" -maxdepth 1 -name "*.app" -print -quit)"
[ -n "$APP" ] || { echo "No .app found inside the DMG" >&2; exit 1; }
echo "    app: $(basename "$APP")"

echo "    before: $(codesign --verify --deep --strict "$APP" 2>&1 || true)"

# --deep so the bundled Python interpreter and its .so files are sealed too.
codesign --force --deep --sign - "$APP"

# Hard gate: never ship a bundle that fails verification.
if ! codesign --verify --deep --strict "$APP" 2>"$WORK/err"; then
  echo "FAILED to produce a valid signature:" >&2
  cat "$WORK/err" >&2
  exit 1
fi
echo "    after:  signature valid"

# On the macos-15-intel runner the first detach regularly fails with
# "timeout for DiskArbitration expired" (and a convert right after can hit
# "Resource busy").
# Retry, then force; errors are left visible rather than silenced.
retry() {
  local i
  for i in 1 2 3 4 5; do
    "$@" && return 0
    echo "    retrying in $((i * 2))s: $*" >&2
    sleep $((i * 2))
  done
  return 1
}
retry hdiutil detach "$DEV" || hdiutil detach "$DEV" -force
DEV=""
rm -rf "$MNT"

retry hdiutil convert "$WORK/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$WORK/final.dmg" -ov
mv -f "$WORK/final.dmg" "$DMG"

echo "==> Done: $DMG ($(du -h "$DMG" | cut -f1))"
