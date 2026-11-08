#!/bin/sh
# The bootstrap (it must exist before cpd does): builds cpd in release mode with the
# checkout's short commit and installs it through `cpd install`. Safe to rerun. scripts/check-portable.sh
# guards this file: it runs on Linux and on macOS.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
CPD_COMMIT=$(git -C "$root" rev-parse --short HEAD) "$root/scripts/cargo.sh" build --release --locked --manifest-path "$root/Cargo.toml" -p cpd
"${CARGO_TARGET_DIR:-$root/target}/release/cpd" install --root "$root" "$@"

# Package the Electron app and install it into the app dir (air-d89 point 1), so `cpd desktop`
# launches the installed bundle and not the checkout's node_modules Electron. macOS only: only
# web/electron-builder.yml's `mac` target is defined, and the launcher spawns the installed
# CPD.app on macOS (core/src/app/mod.rs, `launch`). Packaging needs the checkout's dist-electron,
# which `cpd install` step 7a built just above.
if [ "$(uname -s)" = "Darwin" ] && [ -f "$root/web/electron-builder.yml" ]; then
  pnpm --dir "$root" --filter cpd-web-client exec electron-builder --dir
  bundle=
  for candidate in "$root"/web/dist/mac*/CPD.app; do
    if [ -d "$candidate" ]; then bundle=$candidate; fi
  done
  if [ -z "$bundle" ]; then
    echo "install-cpd: no packaged CPD.app under $root/web/dist" >&2
    exit 1
  fi
  app_dir="$HOME/.local/share/cpd/app"
  rm -rf "$app_dir"
  mkdir -p "$app_dir"
  cp -R "$bundle" "$app_dir/"
fi

