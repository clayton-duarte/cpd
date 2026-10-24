#!/bin/sh
# Runs cargo with the given arguments. cargo is not on PATH for Workers and
# the Gate, so it is resolved as: $CARGO, else `cargo` on PATH, else
# ${CARGO_HOME:-$HOME/.cargo}/bin/cargo. Fails loudly when none exists.
# When sccache is on PATH and RUSTC_WRAPPER is unset, it first starts the sccache
# server in a clean environment (`env -i` with PATH, HOME, TMPDIR and SCCACHE_* only;
# output captured; a server already running is left alone; a failed start warns and
# builds uncached), then exports RUSTC_WRAPPER=sccache (one compile cache per machine). A RUSTC_WRAPPER that
# is already set is kept, including the empty value: `RUSTC_WRAPPER= scripts/cargo.sh ...`
# opts out.
# A full test run (`cargo test`, `cargo nextest run` with no test filter, `-p <crate>` and `--workspace` included)
# runs inside one of the machine's test slots (`cpd slot --`, contract §7.6 "What is full", hs-d464); a run
# filtered to named tests, `--no-run`, `nextest list` and every other subcommand, `build` included, take none. When
# `cpd` is not on PATH or has no `slot` (`cpd slot --help` does not exit 0: a cpd installed before the
# subcommand existed) it runs the command directly.
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

# Starts the sccache server in a clean environment and exports RUSTC_WRAPPER=sccache.
# On a real failure it warns and leaves RUSTC_WRAPPER unset. Leaves "$@" untouched.
start_sccache_server() {
  set -- "PATH=$PATH"
  [ -z "${HOME+x}" ] || set -- "$@" "HOME=$HOME"
  [ -z "${TMPDIR+x}" ] || set -- "$@" "TMPDIR=$TMPDIR"
  names=$(/usr/bin/env | while IFS= read -r line; do
    case $line in
      (SCCACHE_*=*)
        name=${line%%=*}
        case $name in (*[!A-Za-z0-9_]*) ;; (*) echo "$name" ;; esac
        ;;
    esac
  done)
  for name in $names; do
    eval "value=\${$name}"
    set -- "$@" "$name=$value"
  done
  if out=$(/usr/bin/env -i "$@" sccache --start-server 2>&1); then
    export RUSTC_WRAPPER=sccache
  else
    case $out in
      *'Address in use'*) export RUSTC_WRAPPER=sccache ;;
      *)
        first=
        while IFS= read -r line; do
          case $line in 'sccache: Starting the server'*) ;; *) first=$line; break ;; esac
        done <<EOF
$out
EOF
        echo "scripts/cargo.sh: warning: sccache server did not start (${first:-no message}); building without the compile cache" >&2
        ;;
    esac
  fi
}

if [ -z "${RUSTC_WRAPPER+x}" ] && command -v sccache >/dev/null 2>&1; then
  start_sccache_server
fi

# Whether `$@` (the arguments of cargo) is a full test run: `test`, or `nextest run`, with no test filter.
# A filter is `-E`/`--filterset`/`--filter-expr`, or a word that is neither an option nor the value of one,
# before or after `--`; `--no-run` runs no test.
is_full_test_run() {
  if [ "$1" = nextest ]; then
    case "${2:-}" in
      run | r) shift ;;
      *) return 1 ;;
    esac
  fi
  shift
  value=0
  for word in "$@"; do
    if [ "$value" = 1 ]; then
      value=0
      continue
    fi
    case "$word" in
      --no-run | -E | -E* | --filterset | --filterset=* | --filter-expr | --filter-expr=*) return 1 ;;
      -p | --package | --test | --bin | --example | --bench | --exclude | --features | -F | --profile | -P | --target | --target-dir | --manifest-path | -j | --jobs | --color | --message-format | --config | -Z | --test-threads | --skip | --retries | --run-ignored | --partition | --max-fail | --build-jobs | --cargo-profile) value=1 ;;
      -*) ;;
      *) return 1 ;;
    esac
  done
  return 0
}

case "${1:-}" in
  test | nextest)
    if is_full_test_run "$@" && command -v cpd >/dev/null 2>&1 && cpd slot --help >/dev/null 2>&1; then
      exec cpd slot -- "$cargo" "$@"
    fi
    ;;
esac

exec "$cargo" "$@"
