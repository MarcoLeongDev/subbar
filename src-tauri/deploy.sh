#!/usr/bin/env bash
# Build and install SubBar into /Applications.
# ONLY deploys when the build succeeds (never a stale bundle), and kills the
# running app first so the new binary is actually loaded on next launch.
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*= *"//;s/"//')
BUNDLE="target/release/bundle/macos/SubBar.app"

echo "==> Building SubBar v${VERSION}..."
cargo tauri build

if [ ! -d "$BUNDLE" ]; then
  echo "ERROR: build did not produce $BUNDLE" >&2
  exit 1
fi

echo "==> Killing running SubBar (if any)..."
pkill -9 -f "SubBar.app/Contents/MacOS/subbar" 2>/dev/null || true
sleep 1

echo "==> Installing to /Applications/SubBar.app..."
rm -rf /Applications/SubBar.app
cp -R "$BUNDLE" /Applications/

echo "==> Deployed SubBar v${VERSION}"
defaults read /Applications/SubBar.app/Contents/Info CFBundleShortVersionString
