#!/usr/bin/env bash
# Bootstrap merge: rebase + fast-forward into cpd/v0 (t3 recipe). Usage: merge.sh <attempt-id>
set -euo pipefail
ATT=$1
LEAD=~/projects/cpd.cpd-v0
WT=~/projects/cpd.cpd-v0--$ATT
[ -z "$(git -C "$LEAD" status --porcelain)" ] || { echo "lead dirty"; exit 10; }
[ -z "$(git -C "$WT" status --porcelain)" ] || { echo "attempt dirty"; exit 11; }
PRE=$(git -C "$LEAD" rev-parse cpd/v0)
set +e; wt --yes -C "$WT" merge cpd/v0 --no-squash --no-remove --format json </dev/null > /tmp/cpd-build/merge-$ATT.json 2>/tmp/cpd-build/merge-$ATT.err; rc=$?; set -e
echo "wt merge rc=$rc"; cat /tmp/cpd-build/merge-$ATT.json; tail -5 /tmp/cpd-build/merge-$ATT.err
[ $rc -eq 0 ] || exit $rc
POST=$(git -C "$LEAD" rev-parse cpd/v0); echo "lead $PRE -> $POST"
git -C "$LEAD" log --oneline "$PRE..$POST"
