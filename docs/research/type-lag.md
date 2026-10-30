# Type lag

Measurement recipe and baseline for the User report of 2026-10-30 10:36 EDT: each key stroke takes over half a second to render, slow even while idle (decision pro-d218, notes pro-n188 and pro-n190).

## How to run

Extract the one `sh` block below and run it from the repo root (so `-e pi/...` loads this worktree's code):

```
awk '/^```sh$/{f=1;next} /^```$/{f=0} f' docs/research/type-lag.md > "$TMPDIR/type-lag.sh" && sh "$TMPDIR/type-lag.sh" <mode> <label> [pi args...]
```

`mode` is `idle` (CPU percent of one core over 5 windows of 6 s, no input) or `keys` (ms per key over a 40-key `tmux send-keys -l` burst, 3 runs). It starts pi in a 179x46 pane on a private tmux server (`tmux -L type-lag-<pid>`), waits (bounded) until the pane stops changing, prints the socket and pi pid, and kills the server on EXIT, INT and TERM. `ps -o time=` resolution is 0.01 s, so one key reads in steps of 0.25 ms. It needs real tmux, ps and pi; no test may run it.

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
pane() { tmux -L "$sock" capture-pane -p -t "$sock"; }
drawn() { pane | grep -q '[^[:space:]]'; }
has_keys() { pane | tr -d '\n' | grep -qF "$KEYS"; }
no_keys() { ! has_keys; }

tmux -L "$sock" new-session -d -s "$sock" -x 179 -y 46 "$@" || exit 1
shpid=$(tmux -L "$sock" display-message -p -t "$sock" '#{pane_pid}')
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
      tmux -L "$sock" send-keys -t "$sock" -l "$k"
      i=$((i + 1))
    done
    wait_for 60 has_keys || { echo "timeout: last key never drawn" >&2; exit 2; }
    c1=$(cpu)
    d=$(awk -v a="$c0" -v b="$c1" 'BEGIN{printf "%.2f", b-a}')
    m=$(awk -v d="$d" 'BEGIN{printf "%.2f", d/40*1000}')
    echo "run $r: cpu ${d} s, $m ms per key"
    out="$out$m
"
    tmux -L "$sock" send-keys -t "$sock" C-u
    wait_for 30 no_keys || { echo "timeout: editor not cleared" >&2; exit 2; }
    wait_for 60 quiet || true
    r=$((r + 1))
  done
  echo "median: $(printf '%s' "$out" | median) ms per key"
  ;;
*) echo "unknown mode $mode" >&2; exit 64 ;;
esac
```

## Launches

Common flags `C` = `--no-extensions --offline --no-mcp --no-skills` (pi 1.3.0: `--no-extensions` still honours `-e`). `CT` = `-e pi/compact-tools/extensions/compact-tools.ts`, `EX` = `-e pi/extension.ts`, `AQ` = `-e pi/ask-user-question/index.ts`.

- CONTROL: `pi C --no-session`
- OURS-FRESH: `pi C --no-session CT`
- REPRODUCER: `pi C CT --session-dir <tmp> --session <copy>`, where `<copy>` is a COPY (in `mktemp -d`) of the eec-general Manager transcript (id prefix 4d2ca838, 2321243 bytes, 965 lines). Only the copy is ever opened.
- Code under test is origin/main (606f4b9f); this branch changes only this doc, so the worktree is that code.

The original session file is the live Manager's own transcript, so it grew while I worked, by the live process and not by me (I only ever copied it and opened the copy): `ls -l` before was 2321243 bytes, Oct 30 11:05; after it was 2322026 bytes, Oct 30 11:55. It was never opened, written, moved or deleted by this work; the copy was taken at 2321243 bytes.

**Validity: idle NOT reproduced by a reloaded copy (REPRODUCER 0.00 % against 58 % live); per-key reproduced (per the 2x2: 21 to 28 ms per key against 0.75 to 1.0 for CONTROL and OURS-FRESH).**

## Before

| Build | idle % of a core (5 windows, median) | ms per key (3 runs, median) |
| --- | --- | --- |
| CONTROL | 0.00 (0.50, 0, 0, 0, 0) | 0.75 |
| OURS-FRESH at origin/main | 0.00 (1.33, 0, 0, 0, 0) | 1.00 |
| REPRODUCER at origin/main (copy + CT) | 0.00 (1.00, 0, 0, 0, 0) | 1.75 |
| LIVE eec-general Manager (read-only ps, pid 64772) | 56.67 (55.00, 60.00, 56.67, 61.67, 56.67) | not measured (no input allowed) |

Idle is NOT reproduced: REPRODUCER is below 5 percent. Variants tried, each loaded the copy at 179x46, idle median:

- copy + CT: 0.00 (windows 0 to 1 %)
- copy with the last toolResult and later lines cut (ends in an assistant toolCall without a result), + CT: 0.00
- copy + CT + EX + AQ: 0.00 (0, 0.67, 0.50, 0, 0)
- copy + CT + EX + `pi/mcp-extension.ts` + AQ, without `--no-mcp --no-skills`: 0.00 (0, 1.50, 0, 0, 0)
- copy + CT + EX + AQ + the three files of `~/.pi/agent/extensions` (cpd-aux-agents, herdr-agent-state, notify): 0.00 (0, 1.17, 0, 0, 0)

The live process shows no flags (its command line has no `-` token and no `-e`). Its settings add the package `@plannotator/pi-extension` (not loaded in any variant) besides the three files above. Cost of holding state: live lsof 28 files, rss 433232 KB; OURS-FRESH lsof 24, rss 135776 KB.

### Per-key 2x2 (keys mode, 3 runs, median)

| transcript | extensions | ms per key |
| --- | --- | --- |
| fresh | CT | 1.00 |
| fresh | CT + EX + AQ | 2.50 |
| copy | CT | 1.75 |
| copy | CT + EX + AQ | 21.25 (runs 21.50, 21.25, 20.50) |
| fresh | CT + EX | 2.00 |
| fresh | CT + AQ | 1.00 |
| copy | CT + EX | 22.50 |
| copy | CT + AQ | 2.50 |

Both factors carry the cost together, as an interaction: the long transcript alone (1.75) and `pi/extension.ts` alone (2.0) are cheap, and together they cost 22.5 ms per key. `ask-user-question` adds nothing. A fresh Lead measured 17.5 in pro-n188 with the whole adapter; this recipe's fresh run with EX is 2.0, so the adapter piece that needs CPD_* env or MCP is still not in. UNVERIFIED which part of EX interacts with the transcript.

### Noise (one unchanged build, measured 3 times)

Any later number must beat these spreads.

- OURS-FRESH idle median: 0.00, 0.00, 0.00 (window 1 reads 1.0 to 1.33 every time: startup tail)
- OURS-FRESH keys median: 1.00, 1.00, 1.00 (runs 0.75 to 1.25)
- copy + CT + EX + AQ keys median: 21.25, 23.50, 28.50 (single runs 20.5 to 48.0)

### Live screen, masked diff

Six `capture-pane` reads 1 s apart of the live Manager's pane. Letters masked to `x`; output is the count of differing lines per consecutive pair:

```
pair 1->2: 0 differing lines
pair 2->3: 0 differing lines
pair 3->4: 0 differing lines
pair 4->5: 0 differing lines
pair 5->6: 0 differing lines
```

The visible screen does not change at all while pi burns 57 percent of a core.

## Idle: not reproduced

A reloaded copy rebuilds only from the file; it lacks state built by live events inside the process (timers, listeners, components made from event streams, elapsed counters). Ordered by what the masked diff supports, all UNVERIFIED:

1. Work whose output is invisible or identical: the screen never changes, so the burn is render-side re-measuring of unchanged or off-screen text (pro-n190: Intl.Segmenter under a timer), not a counter that paints.
2. A timer or listener created by a live event (a tool run, a compaction, an extension notice) that never clears, so something keeps requesting renders.
3. A component made from an event stream whose text changes in memory every tick but is clipped or hidden (misses the pro-d208 content key each frame).
4. State from the live process that a copy cannot carry: plannotator package, MCP connections, 15 hours of uptime and 433 MB of heap.

## Trace

## Candidates

## After
