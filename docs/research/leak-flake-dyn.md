# Dynamic test threads, and the Worker cap against the test threads (leak-flake, rs-dyn2, questions 3 and 4)

## Verdict

- **Q3, can the thread count be dynamic? Only at the start of a run, not in it.** nextest 0.9.148 takes `test-threads` as an integer, a negative integer (cores + n, at least 1) or `"num-cpus"`, from `-j` / `NEXTEST_TEST_THREADS` or the profile; it computes it once and hands the queue a constant. No jobserver, load-average or cross-process mechanism exists (verified, Evidence 1). "cores / running Workers" can only be computed by a script before `nextest run` starts.
- **A dynamic rule is not worth its complexity over the fixed number** (inference): a Gate is itself an open/update Attempt that counts against `concurrency`, so Gates at once <= the cap (+1 urgent); the fixed T already covers that worst case; a rule that divides by *Workers* (mostly idle LLM sessions) starves the Gate (14 cores / 10 Workers = 1 thread = 49 min of slot time against `gate_minutes` 30, Evidence 3, 4b).
- **Gates at once, measured** (Evidence 2, 3,277 minutes with a Worker): Gates at once p50 0, p90 4, max 8; Workers at once p50 5, p90 8; per-hour ratio Gates/Workers p50 0.00, p90 0.75, max 0.98. At 8 Workers the Gates were p90 6, max 8: "every Worker runs a Gate" has happened. Projected Gates at once at the p90 ratio: C=5: 4, C=8: 6, C=10: 8, C=14: 11; worst case C (+1 urgent). No data exists above 10 Workers (the cap was 8).
- **Which T the evidence supports** (Evidence 3): clean cells have G x T <= 42, failing runs had 56-70. Worst case (G = C): T=4 supports C <= 10 (40), T=3 supports C <= 14 (42), T=2 C <= 21, T=6 C <= 7. At the p90 projection (0.75 C): T=4 supports C <= 14, T=3 C <= 18. **So 4 is right for a cap of 10 (44 with one urgent Attempt, 2 over the clean 42: inference); for a cap of 14 the worst case needs 3 (42), or 2 with an urgent Job.** The Lead's 4 (on main, #461) holds for 10, not for 14 at the worst case.
- **Q4 "cap = core count, tests single-threaded"** (C=14, T=1: at most 14 test processes, far below 42, so it should remove the LEAK; inference). Price: a Gate alone is up to 2,926 slot-seconds = 49 min at T=1, past `gate_minutes` 30 (a bound: per-test time grew 6.1x under load, so it may shrink without it; unmeasured). The cap is not a CPU measure (Workers at once p50 5, Gates 0 in the median hour), so cap and T are two numbers tied by G x T <= 42, not one.
- **Q4 "tests take only free Worker slots"**: needs the engine to count a Gate as T slots, a cross-run coupling that pro-d30 abolished; nextest cannot cooperate (no jobserver). Not recommended; it needs a new User decision.
- **Recommendation (work, fast, good):** keep the fixed `test-threads`: 4 for a cap up to 10, 3 for 14. No dynamic rule, no slot coupling. Check `gate_minutes` 30 before the cap goes to 10 (CPU-bound upper bound for 10 Gates at once: 2,090 s; observed span p90 was already 1,847 s at cap 8).
- Open: the Gate's CPU cost is not measured (>= ~55 CPU-s lower bound from cost.md; 2,926 slot-s upper bound), so every wall time here is a bound; no run at 40 or 42 processes was made in this Job (the clean 42 is from the earlier Jobs).

## Evidence

Machine `pro`: 14 cores, cargo-nextest 0.9.148 (`cargo-nextest nextest --version`), cpd `concurrency` = 5 now (`cpd config`; temporary, the target is 10-14). `concurrency` counts **Attempts not ended**, of every kind (docs/commands.md:2819, the `N/M` of the status bar), the open and update Jobs that run the Gate included; one urgent Job may run beyond it (docs/contract.md:331, commands.md:1420). Most of those Workers are LLM sessions waiting for the model.

### 1. What nextest 0.9.148 supports

Source at tag `cargo-nextest-0.9.148`, fetched with `gh api repos/nextest-rs/nextest/contents/<path>?ref=cargo-nextest-0.9.148 -H 'Accept: application/vnd.github.raw'` (no `nextest-runner` directory exists under `~/.cargo/registry/src`: verified with `ls`). Docs: https://nexte.st/docs/configuration/reference/ (`profile.<name>.test-threads`: "Positive integer, negative integer (relative to CPU count), or "num-cpus"; default "num-cpus""; `threads-required`: "Positive integer, "num-cpus", or "num-test-threads"; default 1"), https://nexte.st/docs/configuration/env-vars/ (`NEXTEST_TEST_THREADS`: "Number of tests to run simultaneously"), https://nexte.st/docs/configuration/test-groups/ (`max-threads`).

- Forms: `nextest-runner/src/config/elements/test_threads.rs:8-14` (`Count(usize)` or `NumCpus`), `:29-47` (`"num-cpus"`; `0` is an error; a negative `j` gives `(cpus + j).max(1)`; a positive one `Count(j)`). Cores: `config/core/imp.rs:159-170` `get_num_cpus()` = `std::thread::available_parallelism()`, read once (`LazyLock`).
- CLI: `cargo-nextest/src/dispatch/core/run.rs:140-152`: `-j` / `--jobs` / `--test-threads`, `env = "NEXTEST_TEST_THREADS"`, `allow_negative_numbers`; `runner/imp.rs:355-360`: the CLI value, else `profile.test_threads()`, then `.compute()`; `--no-capture` forces 1.
- **Fixed during a run:** `runner/imp.rs:355-360` computes `test_threads: usize` once into `TestRunnerInner`; `:808` `.future_queue_grouped(self.test_threads, groups)` (crate `future-queue` 0.4.0 in Cargo.lock) takes a constant; group limits come from `config.max_threads.compute()` at `:707-712`. No code changes it (`grep -n "SIGUSR\|set_concurrency\|adjust" runner/imp.rs runner/executor.rs runner/dispatcher.rs` is empty). The value is only exported to tests and setup scripts as `NEXTEST_TEST_THREADS` (`runner/executor.rs:426, 807`).
- `threads-required`: `config/elements/threads_required.rs:23-29` (a test takes `n` of the `T` slots; `num-test-threads` = all of them); used at `runner/imp.rs:746-747`, passed to the queue at `:808`. It limits heavy tests at once; it does not change the number of test processes a Gate starts in all.
- Test groups: `max-threads` caps one group (`runner/imp.rs:707-712`); the repo has one (`tmux-restart`, 4). The Plan's rule: no new group for a victim.
- Jobserver, load average, cross-process: `grep -i "jobserver\|MAKEFLAGS\|loadavg\|getloadavg\|sysinfo"` over the five files read (the three runner files, `test_threads.rs`, `threads_required.rs`): no hit; the tree (936 paths, `git/trees/<tag>?recursive=1`) has no jobserver file name. nextest knows nothing of other runs (verified for the files read; the rest of the tree was only searched by file name).

### 2. How many Gates run at once

A Gate = the `checks` span of an `open` or `update` Attempt: for those Jobs it holds the base check and the PR Gate (docs/commands.md:1546, hs-d679), from the events `attempt-status` with `data.check` `start` / `end`; a span without an end ends at the Attempt's end. Workers = every Attempt, `started_at` to `ended_at`. Minute resolution, `~/.cpd/cpd.db` opened read-only, 2026-10-22 17:22Z to now (4,979 min, 67 active hours). The script is in Reproduction. Its output:

```
open/update checks: 247 of which >=30 s: 170
Gate duration s: p50=736.00 p90=1847.00 max=3600.50
minutes with >=1 Worker: 3277 of 4979
Gates at once (minutes with a Worker): p50=0.00 p90=4.00 max=8.00
Workers at once (minutes with a Worker): p50=5.00 p90=8.00 max=27.00
minutes with Gates>=k: {1: 870, 2: 740, 3: 548, 4: 387, 5: 247, 6: 118, 7: 30}
per-hour ratio mean Gates / mean Workers (67 hours): p50=0.00 p90=0.75 max=0.98
per-hour mean Gates: p50=0.00 p90=3.72 max=5.55
per-hour max Gates: p50=0.00 p90=5.00 max=8.00
per-hour mean Workers: p50=4.42 p90=8.10 max=9.11
overall mean Gates/mean Workers = 0.178
ratio Gates/Workers while >=1 Gate runs (870 min): p50=0.75 p90=1.00 max=2.00
  Workers= 1 minutes= 373 Gates p50=0 p90=1 max=2
  Workers= 2 minutes= 263 Gates p50=0 p90=2 max=3
  Workers= 3 minutes= 378 Gates p50=0 p90=3 max=4
  Workers= 4 minutes= 444 Gates p50=0 p90=4 max=5
  Workers= 5 minutes= 399 Gates p50=0 p90=4 max=6
  Workers= 6 minutes= 386 Gates p50=0 p90=1 max=5
  Workers= 7 minutes= 110 Gates p50=0 p90=6 max=7
  Workers= 8 minutes= 739 Gates p50=0 p90=6 max=8
  Workers= 9 minutes= 148 Gates p50=0 p90=5 max=7
  Workers=10 minutes=  24 Gates p50=0 p90=5 max=6
Gates of the 10-25 evening (>=4 running)? ['10-24 14:57', '10-24 15:01', '10-24 15:02', '10-24 15:03', '10-24 15:04', '10-24 15:05']
```

Read: (a) 170 Gates of 30 s or more (247 spans; the rest are quick); a span lasts p50 736 s, p90 1,847 s (a calm Gate is 180-210 s; the span also holds `npm test`, the typecheck and the base check, so it overstates the nextest part). (b) Gates at once p50 0, p90 4, max 8, over the minutes with a Worker; at least one Gate in 870 of 3,277. (c) Ratio Gates/Workers per hour: p50 0.00, p90 0.75, max 0.98; overall mean 0.178; while a Gate runs p50 0.75, p90 1.0 (a Gate is a Worker, so it cannot exceed 1; the 2.0 in the output is a minute-boundary artefact: unverified). (d) At 8 Workers (739 min) Gates p90 6, max 8; at 10 (24 min) max 6. The cap was 8 until today, so **nothing was observed at 10-14 Workers**: those rows are projections.

Projected Gates at once for a Worker cap C (G = C x ratio, rounded up; the worst case G = C, with +1 for an urgent Job in parentheses):

| C | at the overall mean (0.178) | at the per-hour p90 (0.75) | worst case | seen at that many Workers |
|--|--|--|--|--|
| 5 | 1 | 4 | 5 (6) | max 6 at 5 Workers |
| 8 | 2 | 6 | 8 (9) | max 8 at 8 Workers |
| 10 | 2 | 8 | 10 (11) | max 6 (24 min only) |
| 14 | 3 | 11 | 14 (15) | none |

The median hour has no Gate (p50 0): Gates come in bursts. Caveats (unverified): a Gate is long because the machine is slow, so slow Gates overlap more (a feedback), and the ratio may be too high for a calm machine; Gates the User runs by hand are not in the data; the `checks` span includes steps before `nextest`, so the processes of nextest alone are fewer than G x T at some moments.

### 3. The arithmetic

Inputs (verified unless said): 14 cores. W, the Gate's slot-seconds = the sum of the per-test times in the calmest full Gate log, `/tmp/cpd-leak-flake/leak-logs/bifrost_pr-spike2.pre-merge.log`: 5,143 test lines, **2,926 s**, Summary wall 212.7 s (so 13.8 of 14 slots busy). Other logs: adopt 3,792 s in 270.9 s; italic (4-5 Gates at 14 threads) **17,822 s in 1,276 s**, 6.1x the calm sum. `docs/research/leak-flake-cost.md` has no whole-Gate number (its Verdict: no full run was made), so W is slot time, **not CPU** (CPU >= ~55 s; the rest is waiting), and the CPU-bound wall below is an upper bound.

Formula: procs = G x T; over = procs / 14; wall = W / (T x min(1, 14 / procs)); Gates per hour = G x 3600 / wall. Machine-bound top rate: 14 x 3600 / 2926 = 17.2 Gates/h. Calibration: 5 Gates x 14 threads predicts 5 x 2926 / 14 = 1,045 s; the italic log says 1,276 s (the model is about 18% short under that load).

Cells: **clean** = G x T <= 42 (the measured clean points are a single Gate and 3 Gates x 14 = 42, from the earlier Jobs; smaller cells are clean by inference); **FAIL** = 56-70 (4 and 5 Gates x 14 threads: the logs); **unknown** = 43-55, and > 70 (worse than the failing runs, inference).

| G | T | procs | over | wall s (min) | Gates/h | cell |
|--|--|--|--|--|--|--|
| 1 | 1 | 1 | 0.07 | 2926 (48.8) | 1.2 | clean(<=42) |
| 1 | 2 | 2 | 0.14 | 1463 (24.4) | 2.5 | clean(<=42) |
| 1 | 3 | 3 | 0.21 | 975 (16.3) | 3.7 | clean(<=42) |
| 1 | 4 | 4 | 0.29 | 732 (12.2) | 4.9 | clean(<=42) |
| 1 | 6 | 6 | 0.43 | 488 (8.1) | 7.4 | clean(<=42) |
| 1 | 14 | 14 | 1.00 | 209 (3.5) | 17.2 | clean(<=42) |
| 2 | 1 | 2 | 0.14 | 2926 (48.8) | 2.5 | clean(<=42) |
| 2 | 2 | 4 | 0.29 | 1463 (24.4) | 4.9 | clean(<=42) |
| 2 | 3 | 6 | 0.43 | 975 (16.3) | 7.4 | clean(<=42) |
| 2 | 4 | 8 | 0.57 | 732 (12.2) | 9.8 | clean(<=42) |
| 2 | 6 | 12 | 0.86 | 488 (8.1) | 14.8 | clean(<=42) |
| 2 | 14 | 28 | 2.00 | 418 (7.0) | 17.2 | clean(<=42) |
| 4 | 1 | 4 | 0.29 | 2926 (48.8) | 4.9 | clean(<=42) |
| 4 | 2 | 8 | 0.57 | 1463 (24.4) | 9.8 | clean(<=42) |
| 4 | 3 | 12 | 0.86 | 975 (16.3) | 14.8 | clean(<=42) |
| 4 | 4 | 16 | 1.14 | 836 (13.9) | 17.2 | clean(<=42) |
| 4 | 6 | 24 | 1.71 | 836 (13.9) | 17.2 | clean(<=42) |
| 4 | 14 | 56 | 4.00 | 836 (13.9) | 17.2 | FAIL(56-70) |
| 5 | 1 | 5 | 0.36 | 2926 (48.8) | 6.2 | clean(<=42) |
| 5 | 2 | 10 | 0.71 | 1463 (24.4) | 12.3 | clean(<=42) |
| 5 | 3 | 15 | 1.07 | 1045 (17.4) | 17.2 | clean(<=42) |
| 5 | 4 | 20 | 1.43 | 1045 (17.4) | 17.2 | clean(<=42) |
| 5 | 6 | 30 | 2.14 | 1045 (17.4) | 17.2 | clean(<=42) |
| 5 | 14 | 70 | 5.00 | 1045 (17.4) | 17.2 | FAIL(56-70) |
| 8 | 1 | 8 | 0.57 | 2926 (48.8) | 9.8 | clean(<=42) |
| 8 | 2 | 16 | 1.14 | 1672 (27.9) | 17.2 | clean(<=42) |
| 8 | 3 | 24 | 1.71 | 1672 (27.9) | 17.2 | clean(<=42) |
| 8 | 4 | 32 | 2.29 | 1672 (27.9) | 17.2 | clean(<=42) |
| 8 | 6 | 48 | 3.43 | 1672 (27.9) | 17.2 | unknown(43-55) |
| 8 | 14 | 112 | 8.00 | 1672 (27.9) | 17.2 | unknown(>70, worse) |
| 10 | 1 | 10 | 0.71 | 2926 (48.8) | 12.3 | clean(<=42) |
| 10 | 2 | 20 | 1.43 | 2090 (34.8) | 17.2 | clean(<=42) |
| 10 | 3 | 30 | 2.14 | 2090 (34.8) | 17.2 | clean(<=42) |
| 10 | 4 | 40 | 2.86 | 2090 (34.8) | 17.2 | clean(<=42) |
| 10 | 6 | 60 | 4.29 | 2090 (34.8) | 17.2 | FAIL(56-70) |
| 10 | 14 | 140 | 10.00 | 2090 (34.8) | 17.2 | unknown(>70, worse) |
| 14 | 1 | 14 | 1.00 | 2926 (48.8) | 17.2 | clean(<=42) |
| 14 | 2 | 28 | 2.00 | 2926 (48.8) | 17.2 | clean(<=42) |
| 14 | 3 | 42 | 3.00 | 2926 (48.8) | 17.2 | clean(<=42) |
| 14 | 4 | 56 | 4.00 | 2926 (48.8) | 17.2 | FAIL(56-70) |
| 14 | 6 | 84 | 6.00 | 2926 (48.8) | 17.2 | unknown(>70, worse) |
| 14 | 14 | 196 | 14.00 | 2926 (48.8) | 17.2 | unknown(>70, worse) |

Throughput note: every cell with procs >= 14 is machine-bound at 17.2 Gates/h; a fixed T only decides who waits and how many processes are alive. At G x T <= 14 the wall is W / T: T=4 alone 731 s (12.2 min), T=3 975 s, T=2 1,463 s, T=1 2,926 s. These are slot-bound upper bounds (per-test times shrink when fewer threads compete: not measured here).

Largest Worker cap C that each T supports (G x T <= 42; second script):

| T | worst case (G = C) | with 1 urgent | at the p90 projection (G = 0.75 C) |
|--|--|--|--|
| 1 | 42 | 41 | 56 |
| 2 | 21 | 20 | 28 |
| 3 | 14 | 13 | 18 |
| 4 | 10 | 9 | 14 |
| 6 | 7 | 6 | 9 |
| 14 | 3 | 2 | 4 |

### 4. The options

**(a) Fixed `test-threads` in `.config/nextest.toml`** (pro-d44; main has `test-threads = 4`, #461). Needs: nothing more. Cost: a Gate alone takes up to 731 s (slot-bound) instead of ~190 s calm; throughput at several Gates is machine-bound anyway. Touches: pro-d44 allows it; pro-d30 untouched (no cross-run limit). Complexity: one line. Chance it removes the LEAK: high if the leak follows the process count, which the evidence says (56-70 failed, 42 clean; a correlation, the mechanism H3a is open in the other files). Which T: 4 for C <= 10 (40); 3 for C = 14 (42); 2 when urgent Jobs and by-hand Gates must stay inside 42 (C=14: 15 x 2 = 30); 6 only for C <= 7. A larger T buys nothing when Gates overlap (machine-bound) and only shortens a Gate that runs alone.

**(b) Dynamic at the start of a Gate** (`NEXTEST_TEST_THREADS` computed in the Gate's script). Needs: a count from cpd's store (`attempts` not ended; `cpd attempt list`), read by `scripts/gate-run.sh` before `nextest`, e.g. T = max(1, floor(42 / Gates)). nextest cannot change T mid-run (Evidence 1), so a Gate that starts alone at T=14 and is joined by four more keeps its 14: the 70-process failure again. Race: Gates that start in the same second all read the same N (their Attempts exist before the check starts), but a later Gate sees the earlier ones and they never see it, so the total can exceed 42 at the end of a burst. Dividing by *Workers* (cores / Workers = 14 / 10 = 1) starves the Gate: 49 min at T=1 against `gate_minutes` 30 (a timeout fails the Gate). Cost: a script, a read of cpd's store from the Gate (a coupling that does not exist now), and tests with cpd mocked. Touches: pro-d30 in spirit (a limit computed from other runs) and pro-d44 (it says fixed). Chance it removes the LEAK: no better than (a) at the same T, worse in a burst. Recommendation: no.

**(c) Worker cap = core count, tests single-threaded.** Needs: `concurrency = 14` and `test-threads = 1`. Cost: at most 14 test processes from Gates (below 42, below the cores), so the LEAK should go (inference); a Gate alone up to 2,926 s (49 min) by the slot bound, past `gate_minutes` 30 (a bound; measure one Gate before choosing). The cap is not a CPU measure: Workers at once p50 5, p90 8 with Gates p90 4 and none in the median hour (Evidence 2); a cap of 14 raises the worst case to 14 Gates, so T has to fall in step (T <= 3). Touches: pro-d44 (fixed, allowed); pro-d30 not touched. Complexity: two settings; but the cap is a User decision about how much work runs, so tying T to it would make every Gate slow for every cap. Recommendation: not as the fix; the two numbers follow G x T <= 42.

**(d) Tests take only free Worker slots.** Needs: the engine counts a Gate as T of the `concurrency` slots and holds it until T are free; a new state ("waiting for test slots"); the open Job's merge waits for other work. nextest cannot cooperate: no jobserver, and the count is fixed before the run, so the engine would be a lock before the run, not a limiter inside it. Cost: the scheduler, the store, the status bar and tests of all of them. Touches: **pro-d30** (it abolished test slots and `test_concurrency`; this brings them back under the Worker cap). Chance it removes the LEAK: yes (it caps the processes across runs), but so does (a) with far less. Recommendation: no; it needs a new User decision against pro-d30.

Order (make it work, make it fast, make it good): (1) work: fixed T with G x T <= 42 at the cap in force (4 now, valid to C = 10); (2) fast: nothing to gain, throughput is machine-bound (17.2 Gates/h CPU-bound, a bound); (3) good: measure the Gate's CPU and one Gate alone at T=4 (gc-cost), then decide if 3 or 5 is better; the dynamic rule adds nothing over that.

## Reproduction

Needs `python3` (stdlib `sqlite3`), a read-only open of `~/.cpd/cpd.db`, and `gh` for the nextest source. Nothing runs a test or `nextest`. First, `conc2.py` (Gates and Workers at once; prints the Evidence 2 output):

```python
# Gate = an open/update Attempt's "checks" check (events attempt-status data.check start..end), the PR Gate; Workers = all Attempts started..ended
import sqlite3, datetime as dt, statistics as st, collections, os, json
db=sqlite3.connect('file:'+os.path.expanduser('~/.cpd/cpd.db')+'?mode=ro',uri=True)
P=lambda s: dt.datetime.fromisoformat(s.replace('Z','+00:00'))
now=dt.datetime.now(dt.timezone.utc)
kind={(p,k):kd for p,k,kd in db.execute("select plan,key,kind from jobs")}
att={(p,i):(kind[(p,j)],s,e) for p,i,j,s,e in db.execute("select plan,id,job,started_at,ended_at from attempts")}
W=[(P(s),P(e) if e else now) for k,s,e in att.values()]
start={};G=[]
for p,a,d in db.execute("select plan,attempt,data from events where kind='attempt-status' and data like '%\"what\":\"checks\"%' order by id"):
    d=json.loads(d); k=(p,a)
    if att[k][0] not in('open','update'): continue
    t=None
for p,a,at,d in db.execute("select plan,attempt,at,data from events where kind='attempt-status' and data like '%\"what\":\"checks\"%' order by id"):
    d=json.loads(d); k=(p,a)
    if att[k][0] not in('open','update'): continue
    if d['check']=='start':
        if k in start: G.append((start[k],P(at)))
        start[k]=P(at)
    elif k in start: G.append((start.pop(k),P(at)))
for k,s in start.items():
    e=att[k][2]; G.append((s,P(e) if e else now))   # no end event: the Attempt's end (or still running)
t0=min(s for s,_ in W).replace(second=0,microsecond=0)
n=int((now-t0).total_seconds()//60)+1
def series(iv):
    d=[0]*(n+2)
    for s,e in iv: d[int((s-t0).total_seconds()//60)]+=1; d[int((e-t0).total_seconds()//60)+1]-=1
    c=0;o=[]
    for x in d[:n]: c+=x;o.append(c)
    return o
def pct(v,p): v=sorted(v); return v[min(len(v)-1,int(p/100*len(v)))]
def row(nm,v): print(f"{nm}: p50={pct(v,50):.2f} p90={pct(v,90):.2f} max={max(v):.2f}")
GL=[(s,e) for s,e in G if (e-s).total_seconds()>=30]
print("open/update checks:",len(G),"of which >=30 s:",len(GL))
d=[(e-s).total_seconds() for s,e in GL]; row("Gate duration s",d)
w=series(W);g=series(GL)
act=[i for i in range(n) if w[i]>0]
print("minutes with >=1 Worker:",len(act),"of",n)
row("Gates at once (minutes with a Worker)",[g[i] for i in act])
row("Workers at once (minutes with a Worker)",[w[i] for i in act])
print("minutes with Gates>=k:",{k:sum(1 for i in act if g[i]>=k) for k in range(1,8)})
hr=collections.defaultdict(lambda:[[],[]])
for i in act:
    h=(t0+dt.timedelta(minutes=i)).strftime('%m-%d %H'); hr[h][0].append(w[i]);hr[h][1].append(g[i])
rat=[st.mean(b)/st.mean(a) for a,b in hr.values()]
row("per-hour ratio mean Gates / mean Workers (%d hours)"%len(hr),rat)
row("per-hour mean Gates",[st.mean(b) for a,b in hr.values()]); row("per-hour max Gates",[max(b) for a,b in hr.values()])
row("per-hour mean Workers",[st.mean(a) for a,b in hr.values()])
print("overall mean Gates/mean Workers = %.3f"%(st.mean([g[i] for i in act])/st.mean([w[i] for i in act])))
gm=[i for i in act if g[i]>0]
row("ratio Gates/Workers while >=1 Gate runs (%d min)"%len(gm),[g[i]/w[i] for i in gm])
by=collections.defaultdict(list)
for i in act: by[w[i]].append(g[i])
for a in sorted(by):
    if len(by[a])>=20: print("  Workers=%2d minutes=%4d Gates p50=%d p90=%d max=%d"%(a,len(by[a]),pct(by[a],50),pct(by[a],90),max(by[a])))
print("Gates of the 10-25 evening (>=4 running)?",[ (t0+dt.timedelta(minutes=i)).strftime('%m-%d %H:%M') for i in act if g[i]>=4][:6])
```

Second, `table.py` (the G x T table and the cap per T; W from the calmest Gate log):

```python
# G x T table: procs=G*T; over=procs/14; wall=W/(T*min(1,14/(G*T))); thr=G*3600/wall (Gates/hour); W=2926 slot-s (sum of the per-test times of the calmest Gate log, bifrost pr-spike2)
W=2926;N=14
def mark(p): return "clean(<=42)" if p<=42 else ("FAIL(56-70)" if 56<=p<=70 else ("unknown(43-55)" if p<56 else "unknown(>70, worse)"))
print("| G | T | procs | over | wall s (min) | Gates/h | cell |\n|--|--|--|--|--|--|--|")
for G in (1,2,4,5,8,10,14):
  for T in (1,2,3,4,6,14):
    p=G*T; wall=W/(T*min(1,N/p)); print(f"| {G} | {T} | {p} | {p/N:.2f} | {wall:.0f} ({wall/60:.1f}) | {G*3600/wall:.1f} | {mark(p)} |")
print()
print("Cmax(T) worst case (every Worker a Gate, procs=C*T<=42), with one urgent Attempt ((C+1)*T<=42), and at ratio .75 (C*.75*T<=42):")
for T in (1,2,3,4,6,14): print(T, 42//T, 42//T-1, int(42/(0.75*T)))
```

Third, the slot-seconds of each Gate log (W and the 6.1x):

```python
import re,glob
for f in sorted(glob.glob('/tmp/cpd-leak-flake/leak-logs/*.log')):
    t=open(f,errors='replace').read()
    v=[float(m.group(2)) for m in re.finditer(r'^\s*(PASS|FAIL|LEAK-FAIL|LEAK|SLOW)\s+\[\s*([0-9.]+)s\]',t,re.M)]
    s=re.search(r'Summary \[\s*([0-9.]+)s\]\s+(\d+) tests',t); print(f.split('/')[-1],len(v),round(sum(v)),s and s.groups())
```

The runner constant: `gh api 'repos/nextest-rs/nextest/contents/nextest-runner/src/runner/imp.rs?ref=cargo-nextest-0.9.148' -H 'Accept: application/vnd.github.raw' | sed -n '355,360p;805,809p'`.

## What this means for the fix

- Keep a fixed `test-threads`. The rule the evidence supports is **G x T <= 42**, G up to the Worker cap: T=4 up to C=10, T=3 up to C=14 (T=2 if urgent Jobs and by-hand Gates must stay inside 42). The Lead can say: the fix with T=4 supports a cap of **10** at the worst case (about 14 at the p90 projection); for a cap of **14**, T=3 is needed.
- No code change in this Job. No dynamic rule (nextest cannot change T mid-run, and dividing by Workers starves the Gate). No slot coupling (pro-d30).
- Before raising the cap to 10: `gate_minutes` is 30; the CPU-bound upper bound for 10 Gates at once is 10 x 2,926 / 14 = 2,090 s (34.8 min), and the observed span was already p90 1,847 s at cap 8. A Gate that times out fails the PR: measure a real Gate at the new cap rather than infer it.
- Missing (unverified here): a real run at 40 processes (10 Gates x 4) or 42 at T=3; the Gate's CPU cost; Gates started by hand. The link between oversubscription and the LEAK is from the earlier Jobs (56-70 failed, 42 clean): a correlation, not a mechanism.
