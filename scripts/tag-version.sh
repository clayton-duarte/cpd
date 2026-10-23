#!/bin/sh
# Tags the commit of origin/main that last changed Cargo.toml's workspace version
# with the annotated tag v<version> and pushes it. Idempotent; every failure
# prints a `tag-version:` line on stderr and exits 1.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)

die() {
  echo "tag-version: $1" >&2
  exit 1
}

g() { git -C "$root" "$@"; }

g fetch --quiet origin main || die "cannot fetch origin/main"
toml=$(g show origin/main:Cargo.toml) || die "cannot read Cargo.toml on origin/main"
V=$(printf '%s\n' "$toml" | sed -n '/^\[workspace\.package\]/,$ { /^version = "/ { s/^version = "\([^"]*\)".*/\1/p; q; }; }')
[ -n "$V" ] || die "no workspace version in Cargo.toml on origin/main"
tag="v$V"

remote=$(g ls-remote --tags origin "refs/tags/$tag") || die "cannot list the tags of origin"
if [ -n "$remote" ]; then
  echo "tag-version: $tag exists; nothing to do"
  exit 0
fi

sha=$(g log -1 --first-parent -G'^version = ' --format=%H origin/main -- Cargo.toml) || die "cannot find the commit that set $tag"
[ -n "$sha" ] || die "no commit on origin/main changed the version"
at=$(g show "$sha:Cargo.toml" | sed -n '/^\[workspace\.package\]/,$ { /^version = "/ { s/^version = "\([^"]*\)".*/\1/p; q; }; }')
[ "$at" = "$V" ] || die "commit $sha has version '$at', not $V"

if g rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  have=$(g rev-parse "refs/tags/$tag^{commit}")
  [ "$have" = "$sha" ] || die "local tag $tag points at $have, not $sha"
else
  g tag -a "$tag" "$sha" -m "$tag" || die "cannot create $tag"
fi

if ! g push --quiet origin "refs/tags/$tag"; then
  theirs=$(g ls-remote --tags origin "refs/tags/$tag^{}" | sed -n '1s/[[:space:]].*//p') || theirs=
  [ "$theirs" = "$sha" ] || die "push of $tag failed"
fi
echo "tag-version: tagged $tag on $sha"
