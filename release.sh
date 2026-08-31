#!/usr/bin/env bash
# Release SubBar: build universal DMG, create GitHub release, update homebrew tap.
# Usage: ./release.sh [patch|minor|major]
set -euo pipefail

BUMP="${1:-patch}"
ROOT="$(cd "$(dirname "$0")" && pwd)"
SRC_DIR="$ROOT/src-tauri"
CARGO_TOML="$SRC_DIR/Cargo.toml"
INFO_PLIST="$SRC_DIR/Info.plist"

# --- Read current version ---
CURRENT=$(grep '^version' "$CARGO_TOML" | head -1 | sed 's/.*= *"//;s/"//')
IFS='.' read -r MAJOR MINOR PATCH <<< "$CURRENT"

case "$BUMP" in
  patch) PATCH=$((PATCH + 1)) ;;
  minor) MINOR=$((MINOR + 1)); PATCH=0 ;;
  major) MAJOR=$((MAJOR + 1)); MINOR=0; PATCH=0 ;;
  *) echo "Usage: $0 [patch|minor|major]"; exit 1 ;;
esac
NEW_VERSION="$MAJOR.$MINOR.$PATCH"
echo "==> Bumping $CURRENT -> $NEW_VERSION ($BUMP)"

# --- Update versions ---
sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$CARGO_TOML"
sed -i '' "s/<string>$CURRENT<\/string>/<string>$NEW_VERSION<\/string>/" "$INFO_PLIST"
OLD_BUILD=$(sed -n '/CFBundleVersion/{n;s/.*<string>//;s/<\/string>.*//p}' "$INFO_PLIST")
NEW_BUILD=$((OLD_BUILD + 1))
sed -i '' "/CFBundleVersion/{n;s/<string>$OLD_BUILD</<string>$NEW_BUILD</}" "$INFO_PLIST"
sed -i '' "s/\"version\": \".*\"/\"version\": \"$NEW_VERSION\"/" "$SRC_DIR/tauri.conf.json"
echo "==> Version bumped to $NEW_VERSION (build $NEW_BUILD)"

# --- Build universal binary ---
cd "$SRC_DIR"
echo "==> Building universal binary..."
cargo tauri build --target universal-apple-darwin

DMG=$(ls target/universal-apple-darwin/release/bundle/dmg/SubBar_${NEW_VERSION}_universal.dmg 2>/dev/null | head -1)
if [ -z "$DMG" ]; then
  # Fallback: check arch-specific DMG and rename
  DMG=$(ls target/universal-apple-darwin/release/bundle/dmg/SubBar_${NEW_VERSION}_*.dmg 2>/dev/null | head -1)
fi
if [ -z "$DMG" ]; then
  echo "ERROR: DMG not found" >&2
  exit 1
fi
DMG_NAME="SubBar_${NEW_VERSION}_universal.dmg"
cp "$DMG" "$(dirname "$DMG")/$DMG_NAME"
DMG="$(dirname "$DMG")/$DMG_NAME"
echo "==> Built $DMG_NAME"

# --- Git commit & tag ---
cd "$ROOT"
git add -A
git commit -m "feat: release v$NEW_VERSION"
git tag "v$NEW_VERSION"

# --- GitHub release ---
echo "==> Creating GitHub release v$NEW_VERSION..."
gh release create "v$NEW_VERSION" \
  --title "SubBar v$NEW_VERSION" \
  --generate-notes \
  "$DMG#$DMG_NAME"

# --- Update homebrew tap ---
TAP_DIR=$(mktemp -d)
echo "==> Cloning homebrew tap..."
git clone https://github.com/MarcoLeongDev/homebrew-tap.git "$TAP_DIR"

DMG_SHA=$(shasum -a 256 "$DMG" | awk '{print $1}')

# Copy DMG to tap dist/
cp "$DMG" "$TAP_DIR/dist/$DMG_NAME"

# Update cask formula
cat > "$TAP_DIR/Casks/subbar.rb" <<FORMULA_EOF
cask "subbar" do
  version "$NEW_VERSION"
  sha256 "$DMG_SHA"

  url "https://cdn.jsdelivr.net/gh/MarcoLeongDev/homebrew-tap@v#{version}/dist/SubBar_#{version}_universal.dmg"
  name "SubBar"
  desc "Universal macOS menu-bar app to track Opencode Go and Minimax usage"
  homepage "https://github.com/MarcoLeongDev/subbar"
  app "SubBar.app"

  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/SubBar.app"]
  end
end
FORMULA_EOF

cd "$TAP_DIR"
git add -A
git commit -m "SubBar v$NEW_VERSION"
git push origin main
cd "$ROOT"
rm -rf "$TAP_DIR"
echo "==> Homebrew tap updated"

# --- Push to remotes ---
echo "==> Pushing to origin (GitHub)..."
git push origin main --tags

if git remote get-url gitee &>/dev/null; then
  echo "==> Pushing to Gitee..."
  git push gitee main --tags || echo "WARN: Could not push to Gitee"
else
  echo "==> Adding Gitee remote..."
  git remote add gitee https://gitee.com/MarcoLeongDev/subbar.git 2>/dev/null || true
  git push gitee main --tags || echo "WARN: Could not push to Gitee"
fi

echo ""
echo "==> Release v$NEW_VERSION complete!"
echo "    GitHub:  https://github.com/MarcoLeongDev/subbar/releases/tag/v$NEW_VERSION"
echo "    DMG:     $DMG_NAME"
echo "    Install: brew tap MarcoLeongDev/tap && brew install --cask subbar"
