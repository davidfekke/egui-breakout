#!/usr/bin/env bash
# Wrap a built binary in a minimal double-clickable macOS .app bundle.
#
#   ./scripts/make-macos-app.sh x86_64-apple-darwin
#   ./scripts/make-macos-app.sh universal   # needs both arch builds first
#
# Result: dist/Brick Break.app (ad-hoc signed, so it launches without a
# "damaged app" complaint; it is still not notarized, see README).
set -euo pipefail

target="${1:?usage: make-macos-app.sh <target-triple|universal>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
app="$root/dist/Brick Break.app"

if [[ "$target" == universal ]]; then
  arm="$root/target/aarch64-apple-darwin/release/egui-breakout"
  intel="$root/target/x86_64-apple-darwin/release/egui-breakout"
  for b in "$arm" "$intel"; do
    if [[ ! -f "$b" ]]; then
      echo "missing $b — build both macOS targets first" >&2
      exit 1
    fi
  done
  bin="$root/target/universal/release/egui-breakout"
  mkdir -p "$(dirname "$bin")"
  lipo -create -output "$bin" "$arm" "$intel"
else
  bin="$root/target/$target/release/egui-breakout"
  if [[ ! -f "$bin" ]]; then
    echo "missing $bin — run: cargo build --release --target $target" >&2
    exit 1
  fi
fi

version="$(awk -F'"' '/^version = /{print $2; exit}' "$root/Cargo.toml")"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/brick-break"
chmod 755 "$app/Contents/MacOS/brick-break"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Brick Break</string>
    <key>CFBundleDisplayName</key>
    <string>Brick / Break</string>
    <key>CFBundleIdentifier</key>
    <string>dev.egui-breakout.brickbreak</string>
    <key>CFBundleExecutable</key>
    <string>brick-break</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSPrincipalClass</key>
    <string>NSApplication</string>
</dict>
</plist>
PLIST

# Ad-hoc signature. Replace "-" with a Developer ID identity to notarize.
codesign --force --sign - "$app" >/dev/null 2>&1 || true

echo "created $app"
