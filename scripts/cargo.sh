#!/bin/sh
# Runs cargo with the given arguments. cargo is not on PATH for Workers and
# the Gate, so it is resolved as: $CARGO, else `cargo` on PATH, else
# ${CARGO_HOME:-$HOME/.cargo}/bin/cargo. Fails loudly when none exists.
# When sccache is on PATH and RUSTC_WRAPPER is unset, it exports
# RUSTC_WRAPPER=sccache (one compile cache per machine). A RUSTC_WRAPPER that
# is already set is kept, including the empty value: `RUSTC_WRAPPER= scripts/cargo.sh ...`
# opts out.
# A full test run, `test` or `nextest` as the first argument, runs inside one of the machine's test
# slots (`cpd slot --`, contract §7.6); every other subcommand, `build` included, takes none. When
# `cpd` is not on PATH (a checkout without cpd) it runs the command directly.
set -eu

if [ -n "${CARGO:-}" ]; then
  cargo=$CARGO
elif command -v cargo >/dev/null 2>&1; then
  cargo=cargo
else
  cargo="${CARGO_HOME:-${HOME:-}/.cargo}/bin/cargo"
fi

if ! command -v "$cargo" >/dev/null 2>&1; then
  echo "scripts/cargo.sh: cargo not found (tried \$CARGO, cargo on PATH, $cargo)." >&2
  echo "scripts/cargo.sh: install Rust (https://rustup.rs) or set CARGO to the cargo binary." >&2
  exit 127
fi

if [ -z "${RUSTC_WRAPPER+x}" ] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER=sccache
fi

case "${1:-}" in
  test | nextest)
    if command -v cpd >/dev/null 2>&1; then
      exec cpd slot -- "$cargo" "$@"
    fi
    ;;
esac

exec "$cargo" "$@"
