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
# The Gate has no separate concurrency (hs-d679): every run takes no test slot.
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

exec "$cargo" "$@"
