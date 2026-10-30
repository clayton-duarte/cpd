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
- Code under test is origin/main as of the branch base (606f4b9f); this branch changes only this doc, so the worktree is that code.

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

## Probe

A pi extension that shows, from inside the running pi, which JS call chains burn CPU, which timers fire, and which call chains reach `Intl.Segmenter.prototype.segment`. It is kept here, not as a repo file (the Gate typechecks the repo). Extract the one `ts` block with `awk '/^```ts$/{f=1;next} /^```$/{f=0} f' docs/research/type-lag.md > type-lag-probe.ts`.

- At load it does nothing at all (no timer, no wrapper, no read but one `existsSync`) unless `/tmp/type-lag-probe.arm` exists. If it does, the probe deletes it (the arm is consumed once) and runs once. A second load while a run is active does nothing.
- A run waits `TYPE_LAG_PROBE_DELAY_S` (default 0), then lasts `TYPE_LAG_PROBE_SECONDS` (default 10): an in-process CPU profile (`node:inspector`, 200 us sampling, no port), a wrapper on `process.stdout.write` (calls and bytes), wrappers on `setTimeout` and `setInterval` (callbacks fired in the window per creation site, first 3 non-probe frames; only timers created after load), and a wrapper on `Intl.Segmenter.prototype.segment` (exact call count and input characters; the stack of one call in 16 is kept for the chains).
- Why the Segmenter wrapper: V8 shows the Segmenter builtin as no frame at all in the profile (checked: no `segment`, `next` or `Segments` node in the .cpuprofile), so its cost lands as self time of the pi-tui function that calls it (`truncateToWidth`, `visibleWidth`, `sliceWithWidth`). The profile alone cannot name the callers of `segment`; the wrapper can.
- At the end it stops the profile, restores every wrapper, disconnects the inspector session, and writes `${TMPDIR:-/tmp}/type-lag-probe-<pid>.txt` and the raw `.cpuprofile` beside it. The text holds only code identifiers and numbers (function, file, line, counts, milliseconds); no transcript text is read. Absolute paths are cut to their last 3 segments. Timer callbacks that were created inside the window stay wrapped and only pass through after it ends.

```ts
// type-lag probe: does nothing unless /tmp/type-lag-probe.arm exists at load (then deletes it and runs once).
// Output holds code identifiers and numbers only. Env: TYPE_LAG_PROBE_DELAY_S (default 0), TYPE_LAG_PROBE_SECONDS (default 10).
import * as fs from "node:fs";
import * as inspector from "node:inspector";
import * as path from "node:path";

const ARM = "/tmp/type-lag-probe.arm";
/** The stack of one Segmenter call in SAMPLE is kept: counting every call stays exact, the chains are a sample. */
const SAMPLE = 16;
const ACTIVE = Symbol.for("type-lag-probe.active");
const SELF = /([^()\s]+):\d+:\d+\)?$/.exec((new Error().stack ?? "").split("\n")[1] ?? "")?.[1] ?? "type-lag-probe";

type Frame = { functionName: string; url: string; lineNumber: number };
type Node = { id: number; callFrame: Frame; children?: number[] };
type Profile = { nodes: Node[]; samples: number[]; timeDeltas: number[] };

const short = (p: string): string => p.replace(/^file:\/\//, "").split("/").slice(-3).join("/");
const label = (f: Frame): string => `${f.functionName || "(anon)"} ${short(f.url) || "(native)"}:${f.lineNumber + 1}`;
const pct = (n: number, total: number): string => (total ? ((n / total) * 100).toFixed(2) : "0.00") + "%";

function site(): string {
	const lines = (new Error().stack ?? "").split("\n").slice(1).filter((l) => !l.includes(SELF) && !l.includes("node:internal"));
	return (
		lines
			.slice(0, 3)
			.map((l) => {
				const m = /at (?:(.*?) \()?(.*?):(\d+):\d+\)?$/.exec(l.trim());
				return m ? `${m[1] ?? "(anon)"} ${short(m[2]!)}:${m[3]}` : "?";
			})
			.join(" < ") || "(unknown)"
	);
}

function report(profile: Profile, windowS: number, writes: number, bytes: number, fires: Map<string, number>, seg: { calls: number; chars: number; stacks: Map<string, number> }): string {
	const byId = new Map(profile.nodes.map((n) => [n.id, n]));
	const parent = new Map<number, number>();
	for (const n of profile.nodes) for (const c of n.children ?? []) parent.set(c, n.id);
	const self = new Map<number, number>();
	const total = profile.samples.length;
	profile.samples.forEach((id, i) => self.set(id, (self.get(id) ?? 0) + (profile.timeDeltas[i] ?? 0)));
	const byFn = new Map<string, number>();
	for (const [id, us] of self) {
		const k = label(byId.get(id)!.callFrame);
		byFn.set(k, (byFn.get(k) ?? 0) + us);
	}
	const sumUs = [...byFn.values()].reduce((a, b) => a + b, 0);
	const hot = new Map<string, number>();
	for (const id of profile.samples) {
		const f = byId.get(id)!.callFrame;
		if (f.functionName === "(idle)" || f.functionName === "(program)") continue;
		const stack: Frame[] = [];
		for (let x: number | undefined = id; x !== undefined; x = parent.get(x)) stack.push(byId.get(x)!.callFrame);
		const k = stack.filter((fr) => fr.url && !fr.url.startsWith("node:") && !fr.url.includes(SELF)).slice(0, 10).map(label).join(" < ") || "(native only)";
		hot.set(k, (hot.get(k) ?? 0) + 1);
	}
	const chains = new Map<string, number>();
	for (const [raw, n] of seg.stacks) {
		const frames = raw
			.split("\n")
			.slice(1)
			.filter((l) => !l.includes(SELF) && !l.includes("node:"))
			.slice(0, 10)
			.map((l) => {
				const m = /at (?:(.*?) \()?(.*?):(\d+):\d+\)?$/.exec(l.trim());
				return m ? `${m[1] ?? "(anon)"} ${short(m[2]!)}:${m[3]}` : "?";
			});
		const k = frames.join(" < ") || "(no non-node frame)";
		chains.set(k, (chains.get(k) ?? 0) + n);
	}
	const sampled = [...chains.values()].reduce((a, b) => a + b, 0);
	const out = [
		`pid ${process.pid}  node ${process.version}  window ${windowS} s`,
		`stdout writes ${writes}  bytes ${bytes}  profile samples ${total}`,
		"",
		"## timers created after load, callbacks fired in the window",
		...[...fires].sort((a, b) => b[1] - a[1]).map(([k, n]) => `${n}\t${k}`),
		"",
		"## top 25 self-time functions  (function file:line  self-ms  share)",
		...[...byFn].sort((a, b) => b[1] - a[1]).slice(0, 25).map(([k, us]) => `${k}  ${(us / 1000).toFixed(1)}  ${pct(us, sumUs)}`),
		"",
		`## Intl.Segmenter.prototype.segment in the window: ${seg.calls} calls (exact), ${seg.chars} input chars; chains below sample 1 call in ${SAMPLE}`,
		"## call chains of those calls (10 nearest non-node frames, nearest first)  sampled-calls  share of calls",
		...[...chains].sort((a, b) => b[1] - a[1]).slice(0, 15).map(([k, n]) => `${n}  ${pct(n, sampled)}  ${k}`),
		"",
		"## hottest CPU call chains by profile samples (10 nearest non-node frames of the leaf, idle excluded)  samples  share of all samples",
		...[...hot].sort((a, b) => b[1] - a[1]).slice(0, 15).map(([k, n]) => `${n}  ${pct(n, total)}  ${k}`),
		"",
	];
	return out.join("\n");
}

function run(): void {
	const g = globalThis as unknown as Record<symbol, boolean>;
	if (g[ACTIVE]) return;
	g[ACTIVE] = true;
	const delayS = Number(process.env.TYPE_LAG_PROBE_DELAY_S ?? 0);
	const windowS = Number(process.env.TYPE_LAG_PROBE_SECONDS ?? 10);
	const realSetTimeout = globalThis.setTimeout;
	const realSetInterval = globalThis.setInterval;
	const realWrite = process.stdout.write;
	const begin = (): void => {
		const session = new inspector.Session();
		session.connect();
		const post = (m: string, p?: object): Promise<any> =>
			new Promise((ok, no) => session.post(m, p as never, (e, r) => (e ? no(e) : ok(r))));
		let writes = 0;
		let bytes = 0;
		const fires = new Map<string, number>();
		let live = true;
		const count = (cb: unknown, key: string): unknown =>
			typeof cb === "function"
				? function (this: unknown, ...a: unknown[]) {
						if (live) fires.set(key, (fires.get(key) ?? 0) + 1);
						return (cb as Function).apply(this, a);
					}
				: cb;
		const wrapTimer = <T extends Function>(real: T): T => {
			const w = function (this: unknown, cb: unknown, ...rest: unknown[]) {
				return real.call(this, count(cb, site()), ...rest);
			} as unknown as T;
			return Object.assign(w, real);
		};
		process.stdout.write = function (this: unknown, chunk: unknown, ...rest: unknown[]) {
			writes++;
			bytes += typeof chunk === "string" ? Buffer.byteLength(chunk) : ((chunk as Uint8Array | undefined)?.length ?? 0);
			return (realWrite as Function).call(process.stdout, chunk, ...rest);
		} as typeof process.stdout.write;
		const seg = { calls: 0, chars: 0, stacks: new Map<string, number>() };
		const realSegment = Intl.Segmenter.prototype.segment;
		const realLimit = Error.stackTraceLimit;
		Intl.Segmenter.prototype.segment = function (this: Intl.Segmenter, input: string) {
			seg.calls++;
			seg.chars += String(input).length;
			if (seg.calls % SAMPLE === 1) {
				const holder: { stack?: string } = {};
				Error.stackTraceLimit = 14;
				Error.captureStackTrace(holder);
				Error.stackTraceLimit = realLimit;
				const raw = holder.stack ?? "";
				seg.stacks.set(raw, (seg.stacks.get(raw) ?? 0) + 1);
			}
			return realSegment.call(this, input);
		};
		globalThis.setTimeout = wrapTimer(realSetTimeout);
		globalThis.setInterval = wrapTimer(realSetInterval);
		const finish = async (): Promise<void> => {
			live = false;
			globalThis.setTimeout = realSetTimeout;
			globalThis.setInterval = realSetInterval;
			process.stdout.write = realWrite;
			Intl.Segmenter.prototype.segment = realSegment;
			try {
				const { profile } = await post("Profiler.stop");
				await post("Profiler.disable");
				const dir = process.env.TMPDIR ?? "/tmp";
				const base = path.join(dir, `type-lag-probe-${process.pid}`);
				fs.writeFileSync(`${base}.cpuprofile`, JSON.stringify(profile));
				fs.writeFileSync(`${base}.txt`, report(profile, windowS, writes, bytes, fires, seg));
			} catch (e) {
				fs.writeFileSync(path.join(process.env.TMPDIR ?? "/tmp", `type-lag-probe-${process.pid}.txt`), `probe failed: ${(e as Error).message}\n`);
			} finally {
				session.disconnect();
				g[ACTIVE] = false;
			}
		};
		void (async () => {
			try {
				await post("Profiler.enable");
				await post("Profiler.setSamplingInterval", { interval: 200 });
				await post("Profiler.start");
				realSetTimeout(() => void finish(), windowS * 1000).unref();
			} catch (e) {
				live = false;
				globalThis.setTimeout = realSetTimeout;
				globalThis.setInterval = realSetInterval;
				process.stdout.write = realWrite;
				Intl.Segmenter.prototype.segment = realSegment;
				session.disconnect();
				g[ACTIVE] = false;
				fs.writeFileSync(path.join(process.env.TMPDIR ?? "/tmp", `type-lag-probe-${process.pid}.txt`), `probe failed: ${(e as Error).message}\n`);
			}
		})();
	};
	if (delayS > 0) realSetTimeout(begin, delayS * 1000).unref();
	else begin();
}

export default function (_pi: unknown): void {
	if ((globalThis as unknown as Record<symbol, boolean>)[ACTIVE] || !fs.existsSync(ARM)) return;
	fs.unlinkSync(ARM);
	run();
}
```

## Trace

## Proposed fix

## Live probe

## Candidates

## After
