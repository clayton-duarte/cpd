#!/bin/sh
# Writes SCHEMA_HASH (core/src/store/schema.rs) and resolves a merge conflict on it
# (contract §4.3; design docs/research/schema-pin.md §2.4).
#
#   1. Each conflict hunk of schema.rs that holds SCHEMA_HASH takes our side. Any other
#      conflict hunk is left as git wrote it, and the script refuses (exit 2) before it
#      changes anything.
#   2. The hash is the value of the line `SCHEMA_HASH=<hex>` printed by $REPIN_HASH_COMMAND
#      (run with `sh -c` from the repository root). The default is the print test of core.
#      The variable exists for core/tests/repin.rs, which runs this script in a scratch
#      clone that has no Rust workspace.
#   3. The value is written on the SCHEMA_HASH line, and when the file was conflicted it is
#      staged (`git add`). Rerunning changes nothing.
set -eu

root=$(git rev-parse --show-toplevel)
cd "$root"
file=core/src/store/schema.rs
command=${REPIN_HASH_COMMAND:-scripts/cargo.sh test -p cpd-core --lib -- --ignored --nocapture prints_the_schema_hash}

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

conflicted=$(git ls-files --unmerged -- "$file")

# Resolve the hunks of the pin: keep our side.
awk '
  state == "" && /^<<<<<<< / { state = "ours"; ours = ""; theirs = ""; raw = $0 "\n"; next }
  state != "" {
    raw = raw $0 "\n"
    if (/^\|\|\|\|\|\|\|( |$)/) { state = "base"; next }
    if (/^=======$/) { state = "theirs"; next }
    if (/^>>>>>>> /) {
      if (ours ~ /SCHEMA_HASH/ || theirs ~ /SCHEMA_HASH/) printf "%s", ours;
      else { printf "%s", raw; left++ }
      state = ""; next
    }
    if (state == "ours") ours = ours $0 "\n"
    else if (state == "theirs") theirs = theirs $0 "\n"
    next
  }
  { print }
  END { if (state != "") { exit 4 } if (left > 0) exit 3 }
' "$file" > "$tmp" || {
  echo "scripts/repin.sh: $file has a conflict that is not the pin (or an unterminated one); resolve it first, then rerun. Nothing was changed." >&2
  exit 2
}

cat "$tmp" > "$file"

hash=$(sh -c "$command" | grep -o 'SCHEMA_HASH=[0-9a-f]\{64\}' | tail -n 1 | cut -d= -f2) || true
if [ -z "$hash" ]; then
  echo "scripts/repin.sh: the command printed no 'SCHEMA_HASH=<64 hex>' line: $command" >&2
  exit 1
fi

sed "s/^\\(pub const SCHEMA_HASH: &str = \"\\)[0-9a-f]*\\(\";\\)\$/\\1$hash\\2/" "$file" > "$tmp"
cat "$tmp" > "$file"

if [ -n "$conflicted" ]; then
  git add -- "$file"
fi
