# Type lag

Skeleton.

```sh
#!/bin/sh
# usage: sh type-lag.sh <idle|keys> <label> [pi args...]
mode=${1:?mode idle|keys}
label=${2:?label}
shift 2
sock=type-lag-$$
KEYS=abcdefghijklmnopqrstuvwxyz0123456789ABCD

cleanup() {
  tmux -L "$sock" kill-server >/dev/null 2>&1
  if [ -n "$pid" ]; then kill "$pid" >/dev/null 2>&1; fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT
trap 'cleanup; exit 143' TERM

now() { date +%s; }
# cpu seconds of pid from ps -o time= (m:ss.cc or h:mm:ss.cc)
cpu() {
  ps -o time= -p "$pid" | awk '{n=split($1,a,":"); s=0; for(i=1;i<=n;i++) s=s*60+a[i]; print s}'
}
median() { sort -n | awk '{v[NR]=$1} END{print (NR%2)?v[(NR+1)/2]:(v[NR/2]+v[NR/2+1])/2}'; }
# wait_for <seconds> <command...>: poll the command once a second, bounded
wait_for() {
  limit=$1; shift
  end=$(( $(now) + limit ))
  while ! "$@"; do
    [ "$(now)" -ge "$end" ] && return 1
    sleep 0.2
  done
}
pane() { tmux -L "$sock" capture-pane -p -t "$sock:0.0"; }
drawn() { pane | grep -q '[^[:space:]]'; }
has_keys() { pane | tr -d '\n' | grep -qF "$KEYS"; }
no_keys() { ! has_keys; }

tmux -L "$sock" new-session -d -s "$sock" -x 179 -y 46 "$@" || exit 1
shpid=$(tmux -L "$sock" display-message -p -t "$sock:0.0" '#{pane_pid}')
pid=$(pgrep -P "$shpid" | head -n 1)
[ -n "$pid" ] || pid=$shpid
echo "socket=$sock pi_pid=$pid label=$label mode=$mode"

# pi has drawn its editor when the pane is non-empty and its text stops changing
settled() {
  a=$(pane | cksum); sleep 1; b=$(pane | cksum)
  [ "$a" = "$b" ] && drawn
}
wait_for 90 settled || { echo "timeout: pi never drew its editor (login or dialog?)" >&2; exit 2; }
# let startup work finish: CPU unchanged for 2 s
quiet() { a=$(cpu); sleep 2; b=$(cpu); [ "$a" = "$b" ]; }
wait_for 60 quiet || echo "note: pi still busy after 60 s of startup" >&2

case $mode in
idle)
  out=
  t0=$(now); c0=$(cpu)
  w=1
  while [ "$w" -le 5 ]; do
    end=$((t0 + 6))
    while [ "$(now)" -lt "$end" ]; do sleep 0.2; done
    t1=$(now); c1=$(cpu)
    p=$(awk -v a="$c0" -v b="$c1" -v s="$t0" -v e="$t1" 'BEGIN{printf "%.2f", (b-a)/(e-s)*100}')
    echo "window $w: $p % of one core"
    out="$out$p
"
    t0=$t1; c0=$c1; w=$((w + 1))
  done
  echo "median: $(printf '%s' "$out" | median) % of one core"
  ;;
keys)
  out=
  r=1
  while [ "$r" -le 3 ]; do
    c0=$(cpu)
    i=1
    while [ "$i" -le 40 ]; do
      k=$(printf '%s' "$KEYS" | cut -c"$i")
      tmux -L "$sock" send-keys -t "$sock:0.0" -l "$k"
      i=$((i + 1))
    done
    wait_for 60 has_keys || { echo "timeout: last key never drawn" >&2; exit 2; }
    c1=$(cpu)
    d=$(awk -v a="$c0" -v b="$c1" 'BEGIN{printf "%.2f", b-a}')
    m=$(awk -v d="$d" 'BEGIN{printf "%.2f", d/40*1000}')
    echo "run $r: cpu ${d} s, $m ms per key"
    out="$out$m
"
    tmux -L "$sock" send-keys -t "$sock:0.0" C-u
    wait_for 30 no_keys || { echo "timeout: editor not cleared" >&2; exit 2; }
    wait_for 60 quiet || true
    r=$((r + 1))
  done
  echo "median: $(printf '%s' "$out" | median) ms per key"
  ;;
*) echo "unknown mode $mode" >&2; exit 64 ;;
esac
```
