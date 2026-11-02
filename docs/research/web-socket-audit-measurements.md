# Web-socket audit measurements

Raw numbers for the web-socket audit, each produced by a command run now on hs. Every section
states the exact command, the raw output (or a faithful excerpt), the run count, and the time in
America/New_York with the zone. Anything not measured is under `Not measured` (§6) with the
reason. No message was sent to and no `submit` made into any live Session; the durable-smoke
Session (`1c2bf5cb-4fba-45d6-ba15-b08aa414616e`) was only read through its host socket
(`hello` + `attach` + `ping`, all read-only) and its state is unchanged (verified after the runs:
`cpd session show 1c2bf5cb-4fba-45d6-ba15-b08aa414616e` → `state: "idle"`, `archived_at:
"2026-11-01T02:47:15.440Z"`, host pid 2174437 alive, socket file still dated Oct 31 22:17).

Machine: hs. `$CPD_HOME` = `/home/clayton/.cpd`. Node v22.23.2. Every command wrapped in
`timeout`; probe scripts lived in `/tmp` and were removed after the runs.

Probe method (scripts, exact protocol sequence per repetition):

- `hello`+`attach` probe (`/tmp/wsaudit-probe1.mjs`): `node:net.connect(<sock>)` → write
  `{"type":"hello","protocol":1,"min_protocol":1,"client":"cpd measure probe"}` → on `welcome`
  write `{"type":"attach","req":"r1"}` → on the first `snapshot` write
  `{"type":"detach","req":"r2"}` → on `bye` destroy the socket. Timed with
  `process.hrtime.bigint()`: (a) connect-start → `welcome` received, (b) attach-write → first
  `snapshot` line received.
- `ping` probe (`/tmp/wsaudit-probe-ping.mjs`): same connect + `hello`; on `welcome` write
  `{"type":"ping","req":"p1"}`; timed connect-start → `pong` received (no `attach`).
- Idle-cadence probe (`/tmp/wsaudit-probe-idle.mjs`): connect + `hello` + `attach`, then listen
  with no further input and print every frame with its offset from the attach write.
- CLI probe (`/tmp/wsaudit-probe-cli.mjs`): `node:child_process.execFile` of `cpd` — the exact
  spawn path `web/vite.config.ts realDeps()` uses — timing `session list --role lead --role
  manager` and `session show <id>` with `process.hrtime.bigint()`.
- Frames probe (`/tmp/wsaudit-probe5.mjs`): drives the repo's own `createLiveHandler`
  (`web/src/api/live-server.ts`) in-process with a fake `ServerResponse` (no port, no server),
  with `deps` mirroring `realDeps()` for one idle pi Session (real `fs.stat`, real file read).
- Tail/budget probe (`/tmp/wsaudit-probe3.mjs`): imports the repo's `parsePiSession` /
  `toSnapshotFrame` (`web/src/transcript.ts`) and copies `live-server.ts`'s `snapshotTail` loop
  verbatim (`SNAPSHOT_TAIL = 40`, `SNAPSHOT_BUDGET_BYTES = 1_000_000`).

## 1. `hello`+`attach` handshake cost and attach-to-first-frame latency (durable host socket)

Target: the live durable host of Session `durable-smoke`
(`1c2bf5cb-4fba-45d6-ba15-b08aa414616e`, socket `/home/clayton/.cpd/run/host/1c2bf5cb-4fba-45d6-ba15-b08aa414616e.sock`).
Measured 2026-11-01 19:22:43–19:22:45 EST.

Command (25 repetitions, 50 ms apart; script sequence as above):

    timeout 120 node /tmp/wsaudit-probe1.mjs /home/clayton/.cpd/run/host/1c2bf5cb-4fba-45d6-ba15-b08aa414616e.sock 25

Raw output (columns: `rep  connectHelloMs  attachToSnapMs  snapBytes  snapSeq`):

```
0	1.927	0.137	338	1
1	0.520	0.174	338	1
2	0.651	0.125	338	1
3	0.564	0.169	338	1
4	0.631	0.121	338	1
5	0.464	0.158	338	1
6	0.528	0.141	338	1
7	0.644	0.167	338	1
8	0.945	0.110	338	1
9	0.574	0.141	338	1
10	0.389	0.102	338	1
11	0.470	0.144	338	1
12	0.463	0.116	338	1
13	0.438	0.137	338	1
14	0.480	0.146	338	1
15	0.493	0.111	338	1
16	0.485	0.108	338	1
17	0.456	0.134	338	1
18	1.090	0.138	338	1
19	0.393	0.105	338	1
20	0.667	0.138	338	1
21	0.451	0.097	338	1
22	0.498	0.115	338	1
23	0.413	0.093	338	1
24	0.496	0.148	338	1
STATS
connectHelloMs	{"min":0.389078,"median":0.496438,"max":1.926641}
attachToSnapMs	{"min":0.09304,"median":0.136959,"max":0.173819}
snapBytes	{"min":338,"median":338,"max":338}
```

Runs: 25/25 succeeded. Results (ms, measured now on hs):

- connect + `hello` → `welcome`: min 0.39, median 0.50, max 1.93
- `attach` → first `snapshot`: min 0.09, median 0.14, max 0.17
- first `snapshot` frame size: 338 bytes every rep (full handshake-to-transcript under 1 ms;
  an earlier cold-start rep measured 33 ms, see §4's first-run note — the same probe's first
  rep in the 19:12 EST run was 33.05 ms, the only run on this page above 2 ms).

An earlier run of the same probe at 2026-11-01 19:13 EST (25 reps) gave connect+hello
min 0.36 / median 0.55 / max 33.05 ms and attach→snapshot min 0.10 / median 0.15 / max 4.74 ms;
its first rep carried the 33 ms and 4.74 ms maxima (cold connect).

## 2. Per-event push latency vs the 1 s poll

### 2a. Push latency from a host state change to the `events` frame: not measurable here

No event can be produced on the durable host without a `submit` into the Session, which is
forbidden (no `submit` into any live Session; User decision hs-d839). So the delay from a
commit to the pushed `events` frame is under `Not measured` (§6). What is reachable without a
submit was measured and is below.

### 2b. Idle cadence of the durable host socket (no frames while idle)

Measured 2026-11-01 19:22:48–19:23:18 EST. Command:

    timeout 90 node /tmp/wsaudit-probe-idle.mjs /home/clayton/.cpd/run/host/1c2bf5cb-4fba-45d6-ba15-b08aa414616e.sock 30000

Raw output (one client attached, then 30 s of listening with no input; offsets are ms from the
attach write):

```
frame	type=welcome	offsetMs=0.000
frame	type=attached	offsetMs=0.399
frame	type=snapshot	offsetMs=0.000
frame	type=events	offsetMs=0.563
frame	type=host_state	offsetMs=0.594
idle listen done
```

Runs: 1 (30 s listen). Result: after the attach burst (`attached` + `snapshot` + `events` +
`host_state`, all within 0.6 ms of the attach write), the host pushed 0 frames in 30 s while
idle — there is no periodic frame, so an idle attached client costs the host nothing and sees
nothing until a state change or commit. (Measured twice: the first run at 19:16 EST showed the
same five-frame burst then 30 s of silence.)

### 2c. Ping/pong round-trip on the attached socket (idle-channel latency floor)

Measured 2026-11-01 19:22:46–19:22:47 EST, 30 fresh connections (§4 has the raw samples). The
`welcome` → `pong` leg — one request/response round trip on an established hello connection —
was min 0.06, median 0.09, max 1.62 ms. This is the per-message latency floor of the socket
path with the client already connected.

### 2d. Dev adapter's polling cost per tick on a pi Session

`web/src/api/live-server.ts` ticks every `POLL_MS` = 1000 ms. For a pi Session an unchanged
tick is exactly one `fs.stat` mtime read (`deps.stat`); a changed tick additionally re-reads
and re-parses the whole JSONL file (`readPi` → `parsePiSession` → `snapshotTail` → one
`res.write`). Measured 2026-11-01 19:23:24 EST on the largest pi file
(`building-cpd-manager`, `97de2471-0158-47a9-af55-ef61a42b622d`, 29,626,897 bytes):

Command:

    timeout 60 node /tmp/wsaudit-probe2-stat.mjs /home/clayton/.pi/agent/sessions/--home-clayton-.cpd--/2026-10-18T13-12-16-356Z_97de2471-0158-47a9-af55-ef61a42b622d.jsonl

Raw output (120 sequential `fs.stat` calls; per-sample raw values ranged 0.0171–0.3541 ms —
stat excerpt, the two summary lines are verbatim):

```
statMs	{"min":0.01713,"median":0.01992,"max":0.354149}
changedTick	{"readMs":80.580407,"parseMs":127.213035,"tailAndStringifyMs":1.300284,"totalMs":209.093726,"frameBytes":185649}
```

Runs: 120 stat samples + 1 changed-tick cycle. Results:

- unchanged tick (`fs.stat` mtime read): min 0.017, median 0.020, max 0.354 ms — negligible
  against `POLL_MS` = 1000 ms.
- changed tick on the 29.6 MB file (single rep, label: single run): full read 80.6 ms +
  parse 127.2 ms + tail+stringify 1.3 ms = 209.1 ms total, emitting one 185,649-byte frame.
  A change to the largest Session therefore costs ~0.2 CPU-seconds of re-read per change, and
  the tick that detects it runs inside the same 1 s interval loop (the stream skips ticks only
  while the previous one is still running, the `ticking` guard).

### 2e. Dev adapter's durable-Session tick: the `cpd session tail` fallback never returns

`web/vite.config.ts realDeps()` wires no `host` read, so a durable Session's per-tick re-read
is `cpd session tail <id>` — and `session tail` is documented as "Stream a durable Session's
transcript" (`cpd session tail --help`), i.e. it follows and does not exit while the host is
running. Measured 2026-11-01 19:23:24–19:23:44 EST:

Command:

    timeout 20 cpd session tail 1c2bf5cb-4fba-45d6-ba15-b08aa414616e > /tmp/wsaudit-tailcap.out 2>&1; echo "tailExit=$?"; wc -c /tmp/wsaudit-tailcap.out

Raw output:

```
tailExit=124
0 /tmp/wsaudit-tailcap.out
```

Runs: 1 (plus one identical 20 s capture at 19:20 EST). Result: in 20 s, 0 bytes of stdout,
killed by `timeout` (exit 124). The adapter's `await deps.tail(id)` therefore never resolves
against a running durable host, so the durable per-tick cost is not a number but a hang; the
host-socket read (`deps.host`) is the only bounded durable read, and its per-tick cost is the
handshake of §1 (median 0.50 + 0.14 ms, 338-byte snapshot).

## 3. Snapshot size of a long Session

Measured 2026-11-01 19:23:21 EST. Command (one invocation per file; script as described in the
header — imports the repo's `parsePiSession`/`toSnapshotFrame`, copies `live-server.ts`'s
`snapshotTail` loop verbatim with `SNAPSHOT_TAIL = 40`, `SNAPSHOT_BUDGET_BYTES = 1_000_000`):

    timeout 90 node /tmp/wsaudit-probe3.mjs "/home/clayton/.pi/agent/sessions/--home-clayton-.cpd--/2026-10-18T13-12-16-356Z_97de2471-0158-47a9-af55-ef61a42b622d.jsonl"
    timeout 90 node /tmp/wsaudit-probe3.mjs "/home/clayton/.pi/agent/sessions/--home-clayton--/2026-10-31T02-56-46-259Z_eb89e47c-a277-4156-a571-d4557a4ee3f1.jsonl"
    timeout 90 node /tmp/wsaudit-probe3.mjs "/home/clayton/.pi/agent/sessions/--home-clayton--/2026-11-01T05-32-27-008Z_14bd6551-5fe9-4332-bef7-224b1eff71a1.jsonl"

Raw output (one JSON object per file, verbatim):

```
{"file":".../2026-10-18T13-12-16-356Z_97de2471-0158-47a9-af55-ef61a42b622d.jsonl","wholeFileBytes":29626897,"lines":9564,"parsedMessages":5632,"parseMs":108.635493,"readMs":116.283597,"tailTurns":40,"tailChars":184735,"snapshotTailSumChars":184694,"frameBytes":185649,"oldestTailId":"18f0cbd2","newestTailId":"3a073b3e"}
{"file":".../2026-10-31T02-56-46-259Z_eb89e47c-a277-4156-a571-d4557a4ee3f1.jsonl","wholeFileBytes":4132845,"lines":1006,"parsedMessages":517,"parseMs":15.368128,"readMs":14.463691,"tailTurns":40,"tailChars":122436,"snapshotTailSumChars":122395,"frameBytes":122901,"oldestTailId":"953af3d6","newestTailId":"f60196bd"}
{"file":".../2026-11-01T05-32-27-008Z_14bd6551-5fe9-4332-bef7-224b1eff71a1.jsonl","wholeFileBytes":16492664,"lines":2798,"parsedMessages":1436,"parseMs":54.025133,"readMs":56.297872,"tailTurns":40,"tailChars":186610,"snapshotTailSumChars":186569,"frameBytes":187642,"oldestTailId":"dd38f1fc","newestTailId":"a7aa64c7"}
```

(file paths abbreviated with `...` = `/home/clayton/.pi/agent/sessions/…`; full paths above.)

Results, measured now on hs:

| Session | file bytes | JSONL lines | parsed messages | whole-file read+parse (ms) | tail turns | `snapshotTail` chars | `frames` frame bytes |
|---|---|---|---|---|---|---|---|
| building-cpd-manager (97de2471, ~29.6 MB) | 29,626,897 | 9,564 | 5,632 | 116.3 + 108.6 | 40 | 184,735 | 185,649 |
| web-chat-live (14bd6551, ~16.5 MB) | 16,492,664 | 2,798 | 1,436 | 56.3 + 54.0 | 40 | 186,610 | 187,642 |
| durable-leads (eb89e47c, ~4.1 MB) | 4,132,845 | 1,006 | 517 | 14.5 + 15.4 | 40 | 122,436 | 122,901 |
| durable-smoke durable host first `snapshot` (§1, 25 reps) | — | — | — | — | — | — | 338 |

Notes that hold for every row:

- `tail turns` hit the `SNAPSHOT_TAIL` = 40 bound in every case; the
  `SNAPSHOT_BUDGET_BYTES` = 1,000,000 bound never engaged (the 40 newest turns sum to at most
  186,610 `JSON.stringify` chars). The adapter's frame is therefore turn-count-bound at 40, not
  byte-bound.
- `snapshotTail` sizes are `JSON.stringify(...).length` (UTF-16 chars, as the adapter counts
  them); the last column is the exact bytes of the `data: <frame>\n\n` event the route writes
  (`toSnapshotFrame(tail, "live", seq)`), UTF-8.
- Single-rep label: the read+parse numbers are one run per file (the 29.6 MB file measured
  136.5 + 119.6 ms in the 19:19 EST run, same order of magnitude).

Comparison against `pi/durable-host/protocol.ts` `LIMITS` and against the EventSource limits
recorded in `web/src/api/live-server.ts` (comment, measured there 2026-11-01 — cited, not
re-measured here: a 4 MB, 8 MB and 20 MB single `data:` event never fired `onmessage` in
Chrome; 1 MB and 2 MB did):

- Durable host first `snapshot`: 338 B — far under `clientLine` 1,048,576, `slowClient`
  8,388,608 and `hostLine` 67,108,864; fires in any EventSource.
- Pi-session `frames` frames: 185,649 / 187,642 / 122,901 B (0.18 / 0.19 / 0.12 MB) — under
  `clientLine`, under the 1 MB `SNAPSHOT_BUDGET_BYTES` (frame envelope adds ~0.9 KB), and under
  the recorded 2 MB known-to-fire size.
- Whole-file transcript as one event: 29,626,897 B and 16,492,664 B — both above the recorded
  20 MB never-fires size (the 29.6 MB one also above 8 MB); this is why the route sends the
  40-turn tail and pages the rest.
- Not probed here: whether any single JSONL line in these files exceeds `clientLine` 1 MiB
  (relevant to a host ingesting pasted lines) — see §6.

## 4. CLI process-spawn cost vs a socket round-trip per send

Measured 2026-11-01 19:23:21 EST (one probe run; 12 reps of each command; `execFile` of the
`cpd` binary on PATH = the realDeps spawn path; both commands read-only).

Commands:

    timeout 120 node /tmp/wsaudit-probe-cli.mjs 12          # reps of: execFile(cpd, ["session","list","--role","lead","--role","manager"]) and execFile(cpd, ["session","show","97de2471-0158-47a9-af55-ef61a42b622d"])
    timeout 60 node /tmp/wsaudit-probe-ping.mjs /home/clayton/.cpd/run/host/1c2bf5cb-4fba-45d6-ba15-b08aa414616e.sock 30

Raw output, CLI (columns: `rep  listMs  showMs  listBytes  showBytes`):

```
0	list=18.591	show=13.138	listBytes=19617	showBytes=819
1	list=15.867	show=12.836	listBytes=19617	showBytes=819
2	list=17.345	show=13.571	listBytes=19617	showBytes=819
3	list=16.409	show=13.702	listBytes=19617	showBytes=819
4	list=17.291	show=13.757	listBytes=19617	showBytes=819
5	list=16.275	show=13.031	listBytes=19617	showBytes=819
6	list=16.524	show=14.453	listBytes=19617	showBytes=819
7	list=16.213	show=14.165	listBytes=19617	showBytes=819
8	list=16.016	show=13.016	listBytes=19617	showBytes=819
9	list=15.436	show=12.655	listBytes=19617	showBytes=819
10	list=15.625	show=12.677	listBytes=19617	showBytes=819
11	list=15.443	show=23.201	listBytes=19617	showBytes=819
STATS
listMs	{"min":15.435747,"median":16.274622,"max":18.591042}
showMs	{"min":12.65489,"median":13.570775,"max":23.201349}
```

Raw output, socket (`rep  helloMs  welcomeToPongMs  helloPingPongMs`, fresh connection per rep,
`hello`+`ping` → `pong`, timed connect→pong; 30 reps):

```
0	2.044	0.120	2.164
1	0.619	1.622	2.241
2	0.670	0.111	0.781
3	0.439	0.096	0.535
4	0.756	0.278	1.034
5	0.326	0.084	0.411
6	0.320	0.333	0.653
7	0.483	0.086	0.569
8	0.860	0.108	0.968
9	0.431	0.086	0.516
10	0.280	0.087	0.367
11	0.281	0.086	0.367
12	1.475	0.063	1.538
13	0.338	0.091	0.428
14	0.421	0.089	0.510
15	0.315	0.087	0.402
16	0.327	0.081	0.409
17	0.260	0.079	0.339
18	0.290	0.083	0.373
19	0.280	0.080	0.361
20	0.287	0.077	0.364
21	1.064	0.078	1.142
22	0.254	0.080	0.334
23	0.301	0.097	0.398
24	0.296	0.080	0.375
25	0.401	0.088	0.489
26	0.298	0.118	0.415
27	0.485	0.086	0.571
28	0.456	0.095	0.551
29	0.325	0.056	0.382
STATS
helloMs	{"min":0.254028,"median":0.337888,"max":2.044151}
welcomeToPongMs	{"min":0.05632,"median":0.086549,"max":1.622042}
helloPingPongMs	{"min":0.334048,"median":0.489037,"max":2.241029}
```

Runs: 12 CLI reps × 2 commands, 30 socket reps, all succeeded.

Results (measured now on hs):

- `cpd session list --role lead --role manager` (fresh process per call): min 15.44, median
  16.27, max 18.59 ms (output 19,617 B). An earlier bash-loop run at 19:17 EST (`date +%s%N`
  around `timeout 20 cpd session list`) measured 17–38 ms per call, consistent.
- `cpd session show 97de2471…` (fresh process per call): min 12.65, median 13.57, max
  23.20 ms (output 819 B).
- Socket: fresh connection `hello`+`ping`→`pong` (connect → pong): min 0.33, median 0.49,
  max 2.24 ms; on an already-connected hello channel, one `ping`→`pong`: min 0.06, median
  0.09, max 1.62 ms.

So one CLI call costs ~34–180× a fresh-connection socket round trip and ~180× an on-channel
round trip (16.3 ms vs 0.49 ms vs 0.09 ms, medians — ratios stated from the two measured
medians). A `submit` number was not measured (see §6).

## 5. The frames route's poll behaviour on an idle pi Session

Measured 2026-11-01 19:23:44–19:24:45 EST. Command:

    timeout 90 node /tmp/wsaudit-probe5.mjs

The probe drives the repo's own `createLiveHandler` (`web/src/api/live-server.ts`) in-process
(no port, no dev server) with a fake `ServerResponse`, against the idle pi Session
`host-manager` (`021687fd-b488-449c-8c5a-b6a4e39db3e0`, 255,172-byte file, `state: "idle"`,
unchanged throughout): `list()` is the captured `cpd session list` row, `readPi()` the real
file read, `stat()` the real `fs.stat` timed, `tail`/`send` throw if ever called (they were
not). Duration 60 s = 60 `POLL_MS` ticks.

Raw output:

```
status=200 headers={"content-type":"text/event-stream","cache-control":"no-cache","connection":"keep-alive"}
DURATION_MS=60000
statCalls=60 readPiCalls=1
writes=1
write 0	atMs=5.4	bytes=90675
statMs	{"min":0.08714,"median":0.117809,"max":0.341598,"totalMs":7.711928000000002}
statTotalMs=7.712
```

Runs: 1 (60 s; a first identical run at 19:20–19:22 EST gave the same shape: 1 write, 60 stat
calls).

Results (measured now on hs):

- Snapshots emitted over 60 s: 1 — the initial snapshot at stream open (t +5.4 ms, 90,675
  bytes), then 0 further snapshots while the file mtime is unchanged. Expected-0 confirmed.
- Per-tick cost of the stat/compare loop: one `fs.stat` per tick, 60 calls in 60 s, min 0.087,
  median 0.118, max 0.342 ms; 7.71 ms of stat time total over the whole 60 s (≈0.013% of the
  interval). `readPi` ran once (the initial snapshot); an idle pi Session costs one
  sub-millisecond stat per second on this stream.
- The write is unbuffered per tick (one `res.write` per change), and no keep-alive/heartbeat
  bytes were written while idle: an idle stream writes nothing after the initial snapshot.

## 6. Not measured

- **Per-event push latency from a commit to the `events` frame** (§2a): needs a host state
  change, which requires a `submit` into the Session — forbidden (no message to, no `submit`
  into, any live Session; User decision hs-d839). The reachable floor is §2c (0.09 ms median
  ping→pong).
- **A real `submit` round-trip time** (agent wake-up to first frame): same reason — a `submit`
  would wake a real agent. Not attempted; ask the Lead if the audit needs it.
- **`clientLine` (1 MiB) against real pi-session line sizes**: the largest single JSONL line in
  the measured files was not computed; no limit-approaching line is known, but no number was
  taken, so the comparison in §3 covers the snapshot/frame sizes only.
- **EventSource drop thresholds** (4 MB / 8 MB / 20 MB never fire, 1 MB / 2 MB fire): recorded
  in `web/src/api/live-server.ts`'s comments from a measurement dated 2026-11-01 on Chrome;
  cited, not re-measured here (needs a browser).
- **Multi-client fan-out cost** (`clients: N` in `host_state`, broadcast fan-out latency):
  only one client was attached at a time; no second client was attached to avoid touching the
  live host's client bookkeeping beyond the read path.
- **`helloMs` timeout behaviour** (`LIMITS.helloMs` = 5000): not exercised — sending no hello
  and waiting 5 s would hold a host connection open for no measurement value; the code path was
  not timed.
- **Reconnect/back-off cost** (`web/src/api/host.ts` 100 ms→1 s doubling, 30 s deadline): not
  exercised — it needs the host socket to disappear, which would mean stopping or breaking a
  live host.
- **The durable `frames` route tick on a durable Session through the real adapter**: the real
  `realDeps()` has no `host` read and its `tail` fallback never returns (§2e), so the durable
  tick cost through the real wiring is a hang, not a number; §1/§2c bound what a wired
  `deps.host` read would cost per tick.
