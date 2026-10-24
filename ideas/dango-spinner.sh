#!/usr/bin/env bash
# Idea, not used (User, 2026-10-23): a 3-column Dango spinner. Three balls in hanami dango colours
# (pink 13, white 15, green 10) slide onto a stick from the left and off to the right, 100 ms per frame.
# Cute, but its colours can't carry a state (failed, needs you), so cpd keeps the moon loader (hs-d579).
# Run it in a terminal to see it; Ctrl+C stops it.
m=$'\e[95m'; w=$'\e[97m'; g=$'\e[92m'; o=$'\e[0m'
frames=("${o}───" "${g}●${o}──" "${w}●${g}●${o}─" "${m}●${w}●${g}●" "${o}─${m}●${w}●" "${o}──${m}●")
trap 'tput cnorm; echo; exit' INT
tput civis
i=0
while :; do
  printf '\r  %s%s task' "${frames[i % ${#frames[@]}]}" "$o"
  i=$((i + 1))
  sleep 0.1
done
