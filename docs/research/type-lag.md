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


### Probe check: loaded by `/reload` from a private agent directory

Done 2026-10-30 12:43 EDT, pi 1.3.0 (`PI_CODING_AGENT_DIR` relocates the agent directory, `docs/environment-variables.md`). Nothing was written under `~/.pi/agent`. Steps, all in a private tmux server:

1. `AD=$(mktemp -d); mkdir $AD/extensions`; copy only `auth.json`, `models.json`, `settings.json` from `~/.pi/agent` into `$AD`; extract the `ts` block above to `$AD/extensions/type-lag-probe.ts`; `rm -f /tmp/type-lag-probe.arm`.
2. `export PI_CODING_AGENT_DIR=$AD TYPE_LAG_PROBE_SECONDS=5`, then `tmux -L type-lag-$$ new-session -d -s type-lag-$$ -x 179 -y 46 "pi --offline --no-mcp --no-skills --no-session"`, with `trap cleanup EXIT INT TERM` killing that server and removing `$AD`.
3. Wait (bounded poll) until the pane is non-empty, `touch /tmp/type-lag-probe.arm`, then `tmux send-keys -l "/reload"` and `Enter`.
4. Poll (bounded, 30 s) for `$TMPDIR/type-lag-probe-<pane pid>.txt`.

Result: it worked. The arm file was gone after the reload (consumed), the .txt (6880 bytes) and the .cpuprofile appeared within the 5 s window, and the .txt had the header (`pid`, `node`, `window 5 s`, `stdout writes 2  bytes 1787`), the timers table (`TuiAltScreen.scheduleRender ... 1`), the 25 self-time functions, the Segmenter section (`2 calls (exact)`, chain `truncateToWidth < FooterComponent.render`) and the hot chains. The pi was killed and the server with it; `ps` showed no pi and no tmux server of mine afterwards. Two things I hit and fixed in the probe: V8 shows no frame for the Segmenter builtin, so a profile-only chain table came out empty (hence the wrapper); and `pgrep -P <pane pid>` can return a transient child instead of pi, so use the pane pid itself (pi is the pane's process) when reading the output file by pid. That second point also applies to the recipe's `pid=$(pgrep -P "$shpid" | head -n 1)`: on a run where a child of pi exists at that instant, it would sample the child. The recipe was not edited; in the three runs below the pids printed were pi's own (cost per key in the expected range).

## Trace

Measured 2026-10-30 between 12:10 and 12:55 EDT at the branch base (origin/main 606f4b9f code), pi 1.3.0, 179x46, from this worktree, on a private tmux server, a private agent directory (`PI_CODING_AGENT_DIR`, three config files copied) and the COPY of the transcript. The recipe's `keys` mode types after pi settles, which cannot be lined up with the probe's window without editing the recipe block (not allowed), so for the traces I used a throwaway script (outside the repo) that does the same thing: `touch /tmp/type-lag-probe.arm`, `TYPE_LAG_PROBE_DELAY_S=40 TYPE_LAG_PROBE_SECONDS=12`, start pi with `-e <probe>` first, wait until 42 s after launch, then type the same 40 keys with the same `tmux send-keys -l <char>` calls. The ms-per-key numbers below are the unedited recipe (`keys`, 3 runs, median) with the same launches.

- 22 ms run: `pi C -e CT -e EX --session-dir <tmp> --session <copy>` (copy + CT + EX).
- 2 ms run: `pi C --no-session -e CT -e EX` (FRESH + CT + EX).
- The probe is a third `-e`; its overhead is in the profile as `Intl.Segmenter.segment probe.ts` and `defaultPrepareStackTrace` (stack kept for 1 call in 16).

| in the 12 s window around 40 keys | copy + CT + EX | FRESH + CT + EX |
| --- | --- | --- |
| `Intl.Segmenter.prototype.segment` calls (exact) | **439,603** | **2,477** |
| input characters | 14,779,291 | 190,521 |
| timers fired (clock `reschedule` at `compact-tools-clock.ts:23`, `TuiAltScreen.scheduleRender`) | 20 and 18 | 20 and 18 |
| stdout writes, bytes | 21, 7,034 | 58, 17,686 |
| samples not idle (probe overhead included) | 56 % (`truncateToWidth` 15.8 %, `visibleWidth` 11.8 %, `sliceWithWidth` about 4 %) | 1.3 % |

The timers table is the same in both runs, so the timers are not the difference: the same number of frames cost very different amounts. The difference is how much text each frame segments.

Segmenter call chains (nearest caller first; the share is of the sampled calls):

| chain | copy + CT + EX | FRESH + CT + EX |
| --- | --- | --- |
| `truncateToWidth < fit` (`compact-tools-balloon.ts:34`) `< drawBalloon` (`:38`) `< drawnBalloon` (`compact-tools-silent.ts:126`) `< balloonOf` (`:84`) `< assistant override render` (`compact-tools-silent.ts:355`) `< AssistantMessageComponent.render` | **60.7 %** (about 267,000 calls) | absent |
| the other chains through `balloonOf` (the user override at `compact-tools-silent.ts:340`, 2.0 %; `visibleWidth < wrapSingleLine < wrapTextWithAnsi < Markdown.render < drawnBalloon` at `:116`, 4.1 % and the table and list wraps) | 8.0 % | absent |
| `compact-tools-layout.ts`: `CachedComponent.renderLines` (`:187`, `:192`, `hardWrapTextWithAnsi` `:244`, `hardSliceAnsi` `:383`) under `compact-tools.ts:346`, `:652` and `LiveCallContainer.render`, and `WithinWidth.render` (`:530`) | 27.6 % | absent |
| `sliceWithWidth < compositeTuiLine < paintBox < renderLayoutFrame < TuiAltScreen.doRender` (pi-tui) | 1.1 % | 37.4 % (58 sampled calls) |
| `sliceWithWidth < compositeTuiLine < HStack.render < measureHeight` (pi-tui) | not in the top 15 | 24.5 % |
| `truncateToWidth < FooterComponent.render` (pi) | not in the top 15 | 16.8 % |
| `visibleWidth < TuiAltScreen.extractCursorPosition` (pi-tui) | not in the top 15 | 5.8 % |

(The 15 chains shown by the probe cover 97.9 % of the sampled calls of the 22 ms run.)

The chain that explains the difference is the first: every message's balloon in the transcript (`balloonOf` in `pi/compact-tools/extensions/compact-tools-silent.ts`, drawn by `drawBalloon` in `compact-tools-balloon.ts`) is drawn again on each frame, 61 % of the 439,603 calls (69 % with the rest of `balloonOf`), plus the same pattern in `CachedComponent` of `compact-tools-layout.ts` (27.6 %). The fresh session has few messages and segments 62 times per key (2,477 calls over 40 keys); the copy segments about 11,000 times per key.

Why they redraw: with the instrumented dev copy (below, reverted) I logged, per `balloonOf` call, which cache condition failed (code identifiers and numbers only, no text). Over one 40-key run: width differs 3,417 times, hit 850, no entry yet 68, external `invalidate()` 34, text, side, timestamp, margins and painter 0. So the painter lead of docs/contract.md (a mentions refresh publishing a new function) is REFUTED here: the painter never changed. The widths alternate 179 and 177 on the same Markdown child, every frame, and the cache holds one width. The stacks of the two calls differ at the bottom: width 179 comes from pi-tui's `layoutComponent < measureWidth < renderCached < VStack.render`, width 177 from `layoutComponent < measureHeight < renderCached < HStack.render`. The HStack is `PaddedRoot` of `pi/lib/pad.ts` (the 1-cell chat pad, hs-d684 item 3), installed by EX through `installChatPad` (`registerChatPad` hooks `TuiAltScreen.prototype.setLayoutRoot`): pi-tui lays the root out twice per frame, once at the full pane width to measure width and once inside the pad's HStack (width minus 2) to measure height, and renders the whole transcript both times. That is why the cost needs both factors: EX supplies the second width (CT alone only ever renders at 179: 2,086 hits and no width miss on the copy, 203 Segmenter calls in the window), and only the long transcript has enough balloons for the thrash to cost 22 ms.

### Cause confirmed by experiments in a dev copy

Each row is a throwaway edit in this worktree (reverted: `git status` clean before every run and at submit, `git diff --stat origin/main` is this doc only), then the unedited recipe `keys` mode on copy + CT + EX. Noise from this document: the same unchanged build gave medians 21.25, 23.50, 28.50 (single runs 20.5 to 48.0); today's unchanged build gave 33.75 (46.50, 21.25, 33.75) and 24.25 (22.25, 24.25, 24.25).

| change | ms per key, median (runs) | Segmenter calls in the 12 s window |
| --- | --- | --- |
| none (unchanged) | 33.75 (46.50, 21.25, 33.75); again 24.25 (22.25, 24.25, 24.25) | 439,603 |
| the pad off (`registerChatPad` returns at once): one width only | **2.00** (2.75, 2.00, 1.50); again 2.25 (2.25, 1.75, 2.50) | 205 |
| `balloonCache` keeps one entry per width (`compact-tools-silent.ts`) | 18.75 (18.75, 26.50, 17.50) | not measured |
| that, and `CachedComponent` and `CachedContainer` keep one entry per width (`compact-tools-layout.ts`) | 9.25 (8.75, 9.75, 9.25) | 15,697 |

Confirmed: with the pad off the copy + CT + EX run falls from 24 to 34 ms to 2.0 to 2.25 ms per key, inside the fresh run's 2.0, far beyond the noise, and Segmenter calls drop 439,603 to 205. That the cost is the second width, and the single-width caches that cannot hold two, is confirmed by the width-keyed caches taking 439,603 calls to 15,697 and the time to 9.25 ms. The remaining 9 ms with the width-keyed caches is pi-tui's own work for the padded root (87 % of the remaining calls: `sliceWithWidth < compositeTuiLine < paintBox < renderLayoutFrame`, `HStack.render` compositing every transcript row, which is the pad's HStack itself); it needs the pad's layout to change, not a cache (UNVERIFIED which change).

Not verified: IDLE. The 57 % idle burn of the live Manager is not reproduced by any of these runs (the copy idles at 0.00 %), so this finding does not yet explain it. A consistent but UNVERIFIED reading: the live pane redraws on the 80 ms clock (`compact-tools-clock.ts`, `TICK_MS`) while a working indicator or tool row runs, and each such frame costs about what a keystroke frame costs here (22 ms of 80 ms is 27 % of a core, same order as the 57 % seen). The live probe below shows whether the idle burn is the same chain.

## Proposed fix

Options, in order. The pad is a decision (hs-d684 item 3) and pi-tui is never forked, so every option lives in our code.

1. **Supported: make the render caches hold the two widths pi-tui asks for.** `balloonCache` in `pi/compact-tools/extensions/compact-tools-silent.ts` and `CachedComponent` and `CachedContainer` in `compact-tools-layout.ts` key their rows by width (a small map per component, the latest two widths) instead of one slot, and update the Render cache bullet of docs/contract.md (section 6.9) to say so. Why: pi-tui measures width and height with different widths in the same frame as a matter of course (measured: 179 through `measureWidth`, 177 through the padded `HStack`), so a one-slot cache is wrong for any padded layout, not only this transcript; the code is ours and it keeps the pad. Measured in the dev copy: 439,603 to 15,697 Segmenter calls, 24 to 34 ms to 9.25 ms per key. It does not reach 2 ms.
2. **Make the pad cost nothing: lay the side columns out without an `HStack` over the whole transcript** (`PaddedRoot` in `pi/lib/pad.ts`). Floor measured by switching the pad off: 205 calls, 2.0 to 2.25 ms. How to keep the 1-cell pad without the second width is UNVERIFIED (not tried); it should be tried after option 1 since option 1 shows the rest of the cost is pi-tui compositing the padded `HStack`.
3. **Drop the chat pad.** The verified floor (2.0 ms), but it reverses hs-d684 item 3; only the User can decide that. Not recommended.

Regression test, at the `Intl.Segmenter` call level (a mocked-width pi-tui is not needed; pi-tui and its Markdown are real code here but no terminal, tmux or pi process is): build 50 `Markdown` children (25 user, 25 agent, with a few wrapped paragraphs and one table) and render them through the patched user and assistant overrides, the same calls the layout makes: for each of 10 frames, render every message at width W and then at W - 2 (the measure width and the padded width). Wrap `Intl.Segmenter.prototype.segment` with a counter for the test (a spy on a builtin, not an external dependency). What to count and what must hold: the calls in frame 1 are the cost of drawing 50 messages at two widths; the calls in frames 2 to 10 must be **zero** (not "fewer"), and total calls must not grow with the number of frames; then change one message's text and assert that only that message's calls reappear (constant, at most the calls for that one message at the two widths), and that a width change to a third value draws again once. The same shape for a `CachedComponent` with a `renderLines` that calls `truncateToWidth`: two alternating widths, zero calls after the first pair. Today this test fails with the counts of the 22 ms run (every frame draws every message twice).

## Live probe

For the slow session. The probe only runs when `/tmp/type-lag-probe.arm` exists, and it deletes that file the moment it loads, so touch it right before the reload. It sees timers created after it loads (the profile sees everything); its window is 10 s from the reload. It does not touch the pane or open a port.

1. Copy the `ts` block of `## Probe` to `~/.pi/agent/extensions/type-lag-probe.ts` (`awk '/^```ts$/{f=1;next} /^```$/{f=0} f' docs/research/type-lag.md > ~/.pi/agent/extensions/type-lag-probe.ts`, from a checkout of this branch).
2. `touch /tmp/type-lag-probe.arm`.
3. In the slow session run `/reload` (or `/cpd-reload-after-install` if that is what you use) and leave the session alone, idle, for about 15 s. Do not type: the point is the idle burn.
4. Read `$TMPDIR/type-lag-probe-<pid>.txt` (`<pid>` is the pi's pid; `ls -t $TMPDIR/type-lag-probe-*.txt | head -1` is the newest; on macOS `$TMPDIR` is the `/var/folders/.../T/` directory of the shell that runs pi). Then `rm ~/.pi/agent/extensions/type-lag-probe.ts` and `/reload` again. The `.cpuprofile` beside it opens in Chrome DevTools if a person wants it.

What to read, top to bottom:

- Header, `stdout writes`: how many frames reached the terminal in 10 s. Near zero with a busy profile means the burn draws nothing the screen sees (the masked diff said the screen never changes).
- `timers created after load`: which creation site fires most. About 125 per 10 s at `compact-tools-clock.ts:23` is the 80 ms clock; a different site names the timer that keeps requesting renders. A timer created before the reload is not listed; then read the next two sections.
- `top 25 self-time functions`: if `truncateToWidth`, `visibleWidth` or `sliceWithWidth` lead, the burn is text measuring, as the native sample of pro-n190 said.
- `Intl.Segmenter.prototype.segment` section: calls in 10 s and the chains. If the top chain is `balloonOf < drawnBalloon` (`compact-tools-silent.ts`) or `CachedComponent.renderLines` (`compact-tools-layout.ts`), the live burn is the same cause as the per-key cost (a cache that misses every frame) and the fix above applies. If it is another chain, that chain's top non-pi-tui frame is the owner.
- `hottest CPU call chains`: the same by samples, including work with no Segmenter call. A chain ending in a frame of ours (`pi/...`, `compact-tools/...`) names the file and line.

## Candidates

| Change | Median ms per key, before | after | Kept |
|---|---|---|---|
| Balloon cache, CachedComponent and CachedContainer hold the two latest widths (CT + EX on a copy of the Manager transcript; `keys`, 3 runs, two A/B rounds in one window) | 22.75 and 27.00 (runs 21.75 to 30.25) | 9.25 and 10.00 (runs 9.25 to 10.25) | kept |

## After

Recipe `keys` mode, unedited, one window (2026-10-30 afternoon EDT), CT + EX on a copy of the Manager transcript (a separate copy per build, in `mktemp -d`; the original untouched), BEFORE built in a throwaway worktree at 41fed448 (tip of PR `trace`, the merge-base with this branch), AFTER at this head (95f562d4), CONTROL `--no-session`.

| Build | Run 1 | Run 2 | Run 3 | Median ms per key | Ratio to CONTROL |
| --- | --- | --- | --- | --- | --- |
| BEFORE (41fed448) | 23.75 | 23.00 | 23.25 | 23.25 | 31.0x |
| AFTER (95f562d4) | 9.50 | 8.25 | 9.50 | 9.50 | 12.7x |
| CONTROL | 0.75 | 0.75 | 0.50 | 0.75 | 1.0x |

Factor BEFORE/AFTER: 23.25 / 9.50 = 2.4x (AFTER is 41 % of BEFORE, below half).

Validity: the BEFORE still reproduces (23.25 ms per key, above 15). The printed pid was the pane's child, whose CPU (0.92 to 0.95 s per 40 keys at BEFORE) matches pi's, not a transient; unverified beyond that. The remaining 9.5 ms is pi-tui compositing the padded `HStack` (see Cause confirmed); IDLE is not covered here.
