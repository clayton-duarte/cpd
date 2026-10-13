#!/usr/bin/env bash
# Bootstrap stand-in for the pi adapter: wait up to $1 seconds for Lead queue messages, print and archive them.
# (This Lead session predates the cpd pi adapter, so it polls; the product delivers these with pi.sendMessage.)
Q=~/.cpd/leads/01a12783-8c01-7440-bd5b-c23dfe2709db/queue; ARCH=/tmp/cpd-build/lead-inbox; mkdir -p "$ARCH"
end=$(( $(date +%s) + ${1:-540} ))
while [ "$(date +%s)" -lt "$end" ]; do
  files=$(ls "$Q"/*.json 2>/dev/null)
  if [ -n "$files" ]; then
    for f in $files; do jq -c '{kind, job, attempt, text: (.text[0:600])}' "$f"; mv "$f" "$ARCH/"; done
    exit 0
  fi
  sleep 10
done
echo "(no messages)"
