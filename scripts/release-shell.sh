#!/bin/sh
# scripts/release-shell.sh: Builds shell on macOS, ad-hoc signs, zips,
# and creates a GitHub Release on clayton-duarte/cpd with tag app-v<version>.
# Contract (commands.md §14.4b, docs/implementing.md "The shell release and the payload").
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)

if [ "$(uname -s)" != "Darwin" ]; then
  echo "release-shell: error: shell release can only be built on macOS" >&2
  exit 1
fi

"$ROOT/scripts/check-shell-version.sh"

VERSION=$(sed -n 's/.*SHELL_VERSION = "\([^"]*\)".*/\1/p' "$ROOT/web/src/electron/version.ts" | head -n 1)
if [ -z "$VERSION" ]; then
  echo "release-shell: error: cannot read SHELL_VERSION from web/src/electron/version.ts" >&2
  exit 1
fi

TAG="app-v$VERSION"
TMP_DIR=$(mktemp -d "${TMPDIR:-/tmp}/cpd-shell-release.XXXXXX")
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# 1. Build shell into temporary output directory
pnpm --dir "$ROOT" --filter cpd-web-client exec electron-builder --mac --dir --config.directories.output="$TMP_DIR/dist"

BUNDLE="$TMP_DIR/dist/mac-arm64/CPD.app"
if [ ! -d "$BUNDLE" ]; then
  for candidate in "$TMP_DIR"/dist/mac*/CPD.app; do
    if [ -d "$candidate" ]; then BUNDLE=$candidate; fi
  done
fi

if [ ! -d "$BUNDLE" ]; then
  echo "release-shell: error: no packaged CPD.app in $TMP_DIR/dist" >&2
  exit 1
fi

# 2. Ad-hoc sign
codesign --force --deep --sign - "$BUNDLE"

# 3. Zip
ARCHIVE="$TMP_DIR/CPD-$VERSION-mac-arm64.zip"
(cd "$TMP_DIR/dist" && zip -r -y "$ARCHIVE" "$(basename "$BUNDLE")")

# 4. GitHub Release
NOTES="CPD.app $VERSION, ad-hoc signed, not notarized by Apple. macOS quarantines an app downloaded from the internet and refuses to open it. After you move CPD.app into /Applications, clear the quarantine once:

xattr -dr com.apple.quarantine /Applications/CPD.app

cpd desktop does this for you when it installs the app (§14.4a)."

gh release create "$TAG" "$ARCHIVE" \
  --repo clayton-duarte/cpd \
  --title "CPD.app $VERSION" \
  --notes "$NOTES"
