#!/bin/sh
# Release step (docs/release.md). On hs only: cargo-release and git-cliff come from ~/dotfiles.
#   scripts/release.sh <version>      open the 'Release <version>' PR (bump, CHANGELOG, Gate)
#   scripts/release.sh tag <version>  after the squash merge: tag v<version> on that commit, push the tag
# Works in its own temporary worktree from origin/main, so the checkout that runs it is never
# switched to another branch.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)

die() {
  echo "release: $*" >&2
  exit 1
}

usage() {
  echo "usage: scripts/release.sh <version> | scripts/release.sh tag <version>" >&2
  exit 2
}

# The workspace version of Cargo.toml read from stdin.
workspace_version() {
  sed -n '/^\[workspace\.package\]/,/^\[/s/^version *= *"\([^"]*\)".*/\1/p' | head -n 1
}

valid_version() {
  case "$1" in
    *[!0-9.]* | .* | *. | *..* | '') return 1 ;;
  esac
  [ "$(printf '%s' "$1" | tr -cd . | wc -c)" -eq 2 ]
}

# 0 when $1 > $2 (both x.y.z).
greater() {
  _old_ifs=$IFS
  IFS=.
  # shellcheck disable=SC2086
  set -- $1 $2
  IFS=$_old_ifs
  [ "$1" -ne "$4" ] && { [ "$1" -gt "$4" ]; return; }
  [ "$2" -ne "$5" ] && { [ "$2" -gt "$5" ]; return; }
  [ "$3" -gt "$6" ]
}

git_root() {
  git -C "$root" "$@"
}

tag_release() {
  version=$1
  git_root fetch --quiet origin || die "cannot fetch origin."
  sha=$(git_root log origin/main --format='%H %s' | awk -v p="Release $version (#" 'index(substr($0, 42), p) == 1 { print $1; exit }')
  [ -n "$sha" ] || die "no commit on origin/main has a subject starting 'Release $version (#'. Merge the release PR first."
  found=$(git_root show "$sha:Cargo.toml" | workspace_version)
  [ "$found" = "$version" ] || die "Cargo.toml at $sha has version '$found', not $version."
  if existing=$(git_root rev-parse -q --verify "refs/tags/v$version^{commit}"); then
    [ "$existing" = "$sha" ] || die "tag v$version already exists on $existing, not on the release commit $sha."
    echo "tag v$version is already on $sha; nothing to do."
    exit 0
  fi
  git_root tag -a "v$version" "$sha" -m "v$version"
  git_root push origin "v$version"
  echo "tagged v$version on $sha and pushed it."
}

release() {
  version=$1
  valid_version "$version" || die "'$version' is not a version x.y.z."
  "$root/scripts/cargo.sh" release --version >/dev/null 2>&1 || die "cargo-release is not installed (it comes from ~/dotfiles, on hs only)."
  command -v git-cliff >/dev/null 2>&1 || die "git-cliff is not installed (it comes from ~/dotfiles, on hs only)."

  git_root fetch --quiet origin || die "cannot fetch origin."
  current=$(git_root show origin/main:Cargo.toml | workspace_version)
  valid_version "$current" || die "cannot read the workspace version on origin/main."
  greater "$version" "$current" || die "$version is not greater than the workspace version $current on origin/main."
  if git_root rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
    die "tag v$version already exists."
  fi

  branch="release/v$version"
  tmp=$(mktemp -d)
  wt="$tmp/worktree"
  cleanup() {
    git_root worktree remove --force "$wt" >/dev/null 2>&1 || true
    git_root worktree prune
    git_root branch -D "$branch" >/dev/null 2>&1 || true
    rm -rf "$tmp"
  }
  trap cleanup EXIT
  trap 'exit 1' INT TERM

  git_root worktree add -q -b "$branch" "$wt" origin/main
  (
    cd "$wt"
    # release.toml: the release commit "Release <version>", with CHANGELOG.md written by git-cliff.
    "$root/scripts/cargo.sh" release "$version" --workspace --no-publish --no-tag --no-push --no-confirm --execute
    [ -d node_modules ] || npm ci
    npm run verify
    git push origin "$branch"
    # The new CHANGELOG section: from its heading to the next one.
    awk -v h="## [$version] " 'index($0, h) == 1 { on = 1; next } /^## \[/ { on = 0 } on' CHANGELOG.md >"$tmp/body.md"
    gh pr create --title "Release $version" --base main --head "$branch" --body-file "$tmp/body.md"
  )
}

[ $# -ge 1 ] || usage
case "$1" in
  tag)
    [ $# -eq 2 ] || usage
    valid_version "$2" || die "'$2' is not a version x.y.z."
    tag_release "$2"
    ;;
  -*) usage ;;
  *)
    [ $# -eq 1 ] || usage
    release "$1"
    ;;
esac
