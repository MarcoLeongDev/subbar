#!/usr/bin/env bash
# Build and install SubBar into /Applications.
# ONLY deploys when the build succeeds (never a stale bundle), and kills the
# running app first so the new binary is actually loaded on next launch.
#
# Always builds `--target universal-apple-darwin`: an arm64-only bundle shows a
# stop-sign badge and refuses to launch on Intel Macs (e.g. machines on
# macOS 14 Sonoma), even though LSMinimumSystemVersion allows it.
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*= *"//;s/"//')
BUNDLE="target/universal-apple-darwin/release/bundle/macos/SubBar.app"

echo "==> Ensuring Rust targets for universal build..."
rustup target add aarch64-apple-darwin x86_64-apple-darwin

echo "==> Building SubBar v${VERSION} (universal: x86_64 + arm64)..."
cargo tauri build --target universal-apple-darwin

if [ ! -d "$BUNDLE" ]; then
  echo "ERROR: build did not produce $BUNDLE" >&2
  exit 1
fi

echo "==> Verifying universal binary..."
ARCHS=$(lipo -archs "$BUNDLE/Contents/MacOS/subbar")
echo "    archs: $ARCHS"
if [[ "$ARCHS" != *"x86_64"* ]] || [[ "$ARCHS" != *"arm64"* ]]; then
  echo "ERROR: expected universal binary (x86_64 arm64), got: $ARCHS" >&2
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
