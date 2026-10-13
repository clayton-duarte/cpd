#!/usr/bin/env bash
# Kill TEST-spawned `cpd ask` processes older than 180 s (guard against the flaky ask test during Stage 2).
# Real Worker escalations use `--wf v0` and are never touched. DRY=1 only prints.
secs() { # macOS etime [[dd-]hh:]mm:ss -> seconds
  local t=$1 d=0 h=0 m=0 s=0 a b c
  [[ $t == *-* ]] && { d=${t%%-*}; t=${t#*-}; }
  IFS=: read -r a b c <<<"$t"
  if [ -n "$c" ]; then h=$a; m=$b; s=$c; else m=$a; s=$b; fi
  echo $(( 10#$d*86400 + 10#$h*3600 + 10#$m*60 + 10#$s ))
}
re='^[^ ]*/node [^ ]*/bin/cpd\.mjs ask '
while true; do
  ps -axo pid=,etime=,command= | while read -r pid et cmd; do
    [[ $cmd =~ $re ]] || continue
    [[ $cmd == *"--wf v0"* ]] && continue
    [ "$(secs "$et")" -gt 180 ] || continue
    if [ -n "${DRY:-}" ]; then echo "would kill $pid ($et): $cmd"; else echo "$(date) killing stale test ask $pid: $cmd" >> /tmp/cpd-build/ask-watchdog.log; kill "$pid"; fi
  done
  [ -n "${DRY:-}" ] && exit 0
  sleep 20
done
