#!/usr/bin/env bash
# Run on a Mac before `electron-builder --mac`.
#  1. DMG background: merges 1x + @2x PNGs into one Retina TIFF.
#  2. Liquid Glass icon: compiles build/cpd.icon (made in Icon Composer) into build/Assets.car.
#     Needs macOS 26+ and Xcode 26+. Skipped with a warning otherwise.
set -euo pipefail
cd "$(dirname "$0")/.."

# 1 ── DMG background ────────────────────────────────────────────────
tiffutil -cathidpicheck build/dmg/background.png build/dmg/background@2x.png \
  -out build/dmg/background.tiff
echo "✓ build/dmg/background.tiff"

# 2 ── Icon Composer → Assets.car ────────────────────────────────────
if [[ ! -d build/cpd.icon ]]; then
  echo "• build/cpd.icon not found — make it in Icon Composer (see README). Skipping Assets.car."
  exit 0
fi
if ! xcrun actool --version >/dev/null 2>&1; then
  echo "• actool not available (install Xcode 26+). Skipping Assets.car."
  exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/out"

# Same invocation Electron Packager uses (src/icon-composer.ts); --app-icon = the .icon's basename.
xcrun actool build/cpd.icon \
  --compile "$tmp/out" \
  --output-format human-readable-text --notices --warnings \
  --output-partial-info-plist "$tmp/out/assetcatalog_generated_info.plist" \
  --app-icon cpd \
  --include-all-app-icons \
  --enable-on-demand-resources NO \
  --development-region en \
  --target-device mac \
  --minimum-deployment-target 26.0 \
  --platform macosx

cp "$tmp/out/Assets.car" build/Assets.car
echo "✓ build/Assets.car (CFBundleIconName: cpd)"
