#!/bin/sh
# Usage: scripts/check-portable.sh [<file>...]
# Refuses Linux-only calls in the scripts Macs run (docs/implementing.md, "The
# portability check", hs-d355). The Gate runs it (`pnpm run verify`), so a
# Linux-only call fails on hs and not on a Mac. With no argument it checks the
# covered scripts of this directory and fails a script that is in neither list;
# with files it checks exactly those.
# Exit 0 and `check-portable: ok (<n> files, <m> exempt)`, or exit 1, one line per
# hit `<file>:<line>: <pattern>: use <alternative>` on stderr and a last line
# `check-portable: FAIL (<k> hits)`. A line that ends in `# portable: <reason>` is
# not checked, and is counted. A full-line comment is never checked. Needs only
# sh, awk and mktemp: the portable subset itself.
set -eu

dir=$(cd "$(dirname "$0")" && pwd)

# What a Mac runs (dotfiles' install and sync, bare cpd, a Worker or Lead there, the Gate there).
covered="install-cpd.sh pi-check.sh pi-check-pty.py check-portable.sh check-migrations-frozen.sh cargo.sh pnpm-fresh.sh new-migration.sh preview-tui.sh preview-web.sh repin.sh sync-version.sh tag-version.sh test-changed tests.sh tripwire/tmux"

# Linux-only, not covered, each with its reason; a script in neither list fails.
linux_only="prune-worktrees.sh: a one-time cleanup run on hs
test-time.sh: measures the Gate's run on hs (a fractional-second clock, uptime)
gate-run.sh: finds a run's processes in /proc on Linux; its macOS branch (ps -E) is unverified (hygiene.md, The Gate leak check)"

tbl=$(mktemp "${TMPDIR:-/tmp}/check-portable.XXXXXX")
trap 'rm -f "$tbl"' EXIT

# The refused patterns: `<extended regex> ## <alternative>`. `@` in a regex stands for
# "not the tail of a longer word or path". A regex is written so that its own text
# does not match it (`renameat[2]`), because this file is checked too.
cat >"$tbl" <<'TABLE'
renameat[2]|RENAME_EXCHAN[G]E ## os.replace (rename(2)) of a symlink over the path ("The adapter swap")
/pro[c]/|/sy[s]/ ## uname, ps -o <field>= -p <pid>; for a UUID node -p 'crypto.randomUUID()'
@readlin[k][[:space:]]+-[A-Za-z]*[fem]|@realpat[h]([^A-Za-z0-9_-]|$) ## cd "$dir" && pwd -P; python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))'; a plain readlink <link> is allowed
@sta[t][[:space:]]+(-[A-Za-z]*c|--) ## python3 -c 'import os,sys; print(os.stat(sys.argv[1]).st_mtime)', or [ a -nt b ]
@dat[e][[:space:]]+([^;|&]*[[:space:]])?(-d|-r|--date) ## date -u +<format> of now; parse and compute dates in python3
@dat[e][[:space:]][^;|&]*%N ## date -u +<format> of now (no fractional seconds); python3 for a finer clock
@se[d][[:space:]]+(-[A-Za-z]+[[:space:]]+)*(-[A-Za-z]*i([[:space:]]|$)|--in-place) ## write to a temporary file and mv it over; -i.bak with a suffix is allowed
@se[d][[:space:]]+(-[A-Za-z]+[[:space:]]+)*(-[A-Za-z]*r([[:space:]]|$)|--regexp-extended) ## sed -E
@floc[k]([[:space:]]|$) ## python3 -c 'import fcntl,sys; fcntl.flock(open(sys.argv[1], "a"), fcntl.LOCK_EX | fcntl.LOCK_NB)' (exit 1 when held): the same flock(2) lock cpd takes (core/src/lock.rs); macOS ships no such command
@timeou[t][[:space:]]+[^=[:space:]]|@setsi[d]([[:space:]]|$) ## a background process and kill; python3 subprocess with timeout=; neither ships on macOS
@xarg[s][[:space:]]+([^;|&]*[[:space:]])?(-r|-d|--no-run-if-empty) ## while read; a [ -n "$list" ] guard
@gre[p][[:space:]]+([^;|&]*[[:space:]])?(-[A-Za-z]*P|--perl-regexp) ## grep -E, sed -n, or awk
@(sha256su[m]|sha1su[m]|md5su[m])([^A-Za-z0-9_-]|$) ## shasum -a 256, shasum -a 1 (on both)
@npro[c]([^A-Za-z0-9_-]|$) ## getconf _NPROCESSORS_ONLN
@m[v][[:space:]]+([^;|&]*[[:space:]])?-[A-Za-z]*T|@l[n][[:space:]]+([^;|&]*[[:space:]])?-[A-Za-z]*T ## the POSIX form of the same, or python3 (os.replace for a rename)
@c[p][[:space:]]+([^;|&]*[[:space:]])?--(reflin[k]|parent[s]) ## cp -R and mkdir -p, the POSIX form of the same
@fin[d][[:space:]][^;|&]*-printf ## find ... -exec, or a while read loop; python3
@mktem[p][[:space:]]+([^;|&]*[[:space:]])?--[a-z] ## mktemp "${TMPDIR:-/tmp}/name.XXXXXX" (mktemp -d for a directory)
@touc[h][[:space:]]+([^;|&]*[[:space:]])?-[A-Za-z]*d ## python3 os.utime; touch -t <CCYYMMDDhhmm>
@base6[4][[:space:]]+([^;|&]*[[:space:]])?-w ## base64 | tr -d '\n'
@sor[t][[:space:]]+([^;|&]*[[:space:]])?-[A-Za-z]*V ## sort -n on a numeric field, or python3
@d[u][[:space:]]+([^;|&]*[[:space:]])?-[A-Za-z]*b ## du -k, or python3 os.stat
@d[f][[:space:]]+([^;|&]*[[:space:]])?--outpu[t] ## df -k
TABLE

files=
if [ "$#" -gt 0 ]; then
  for f in "$@"; do files="$files
$f"; done
else
  for name in $covered; do files="$files
scripts/$name"; done
fi

hits=0
exempt=0
count=0
out=$(mktemp "${TMPDIR:-/tmp}/check-portable.XXXXXX")
trap 'rm -f "$tbl" "$out"' EXIT

report() {
  echo "$1" >&2
  hits=$((hits + 1))
}

# Without arguments: every file of this directory is covered or Linux-only.
if [ "$#" -eq 0 ]; then
  for path in "$dir"/*; do
    [ -f "$path" ] || continue
    name=$(basename "$path")
    case " $covered " in *" $name "*) continue ;; esac
    case "
$linux_only" in
      *"
$name: "*) ;;
      *) report "scripts/$name: not classified: add it to the covered or the Linux-only list" ;;
    esac
  done
fi

# Checked as given; the covered list is relative to the checkout that holds this directory.
echo "$files" | while IFS= read -r file; do
  [ -n "$file" ] || continue
  path=$file
  [ "$#" -gt 0 ] || path="$dir/../$file"
  if [ ! -f "$path" ]; then
    echo "$file: cannot read it (a covered script that is missing)"
    continue
  fi
  echo "#file"
  awk -v name="$file" -v tbl="$tbl" '
    function trim(s) { gsub(/^[ \t(;|&$`"]+/, "", s); gsub(/[ \t]+$/, "", s); return s }
    # Whether the line has `tmux` in command position, not followed by `-u`.
    function bare_tmux(line,    segs, n, i, w, m, j, skip) {
      gsub(/[|;&(`{]/, ";", line)
      n = split(line, segs, ";")
      for (i = 1; i <= n; i++) {
        m = split(segs[i], w, /[ \t]+/)
        skip = 0
        for (j = 1; j <= m; j++) {
          if (w[j] == "") continue
          if (skip) { skip = 0; continue }
          if (w[j] == "env") { inenv = 1; continue }
          if (w[j] == "!" || w[j] == "then" || w[j] == "do" || w[j] == "else" || w[j] == "if" || w[j] == "while" || w[j] == "until" || w[j] == "cleanenv") continue
          if (w[j] ~ /^[A-Za-z_][A-Za-z0-9_]*=/) continue
          if (inenv && w[j] ~ /^-/) { if (w[j] == "-u" || w[j] == "-C" || w[j] == "-S") skip = 1; continue }
          inenv = 0
          if (w[j] == "tmux" || w[j] ~ /\/tmux$/) {
            if (j == m || w[j + 1] != "-u") return 1
          }
          break
        }
        inenv = 0
      }
      return 0
    }
    BEGIN {
      boundary = "(^|[^A-Za-z0-9_./-])"
      while ((getline row < tbl) > 0) {
        i = index(row, " ## ")
        re = substr(row, 1, i - 1)
        gsub(/@/, boundary, re)
        regex[++n] = re
        alt[n] = substr(row, i + 4)
      }
    }
    /^[ \t]*#/ { next }
    {
      if (match($0, /#[ \t]*portable:/)) {
        reason = substr($0, RSTART + RLENGTH)
        if (reason ~ /^[ \t]*$/) {
          printf "%s:%d: a portable-exemption comment with no reason: write why this line is portable after the colon\n", name, NR
        } else {
          print "#exempt"
        }
        next
      }
      if (bare_tmux($0)) {
        printf "%s:%d: bare tmux: use tmux -u, through the script'"'"'s one tmux function (hs-d463)\n", name, NR
        next
      }
      for (k = 1; k <= n; k++) {
        if (match($0, regex[k])) {
          printf "%s:%d: %s: use %s\n", name, NR, trim(substr($0, RSTART, RLENGTH)), alt[k]
          break
        }
      }
    }
  ' "$path"
done >"$out"

while IFS= read -r line; do
  case $line in
    "#file") count=$((count + 1)) ;;
    "#exempt") exempt=$((exempt + 1)) ;;
    *) report "$line" ;;
  esac
done <"$out"

if [ "$hits" -gt 0 ]; then
  echo "check-portable: FAIL ($hits hits)" >&2
  exit 1
fi
echo "check-portable: ok ($count files, $exempt exempt)"
