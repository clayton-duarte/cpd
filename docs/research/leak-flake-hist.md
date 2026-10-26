# Why now: the history of the test setup, GitHub Actions and the leak check (Q1, rs-hist)

Attempt rs-hist-a3, 2026-10-26. Machine `pro` (macOS, 14 cores), cargo-nextest 0.9.148. Labels: `verified` (command or file:line), `inference`, `unverified`.

## Verdict

1. **The LEAK is new on this setup; it is not an old weakness made visible by `result = "fail"`.** Nextest ran on GitHub Actions for 1 day only (2026-10-18 17:25 to 2026-10-19 00:36 EDT, 61 nextest logs, 59 finished, 92,238 test executions) and printed `LEAK` 0 times, a passing leaky test included (default `result = "pass"` still prints `LEAK`). Verified: `gh api .../jobs/<id>/logs` over every run since the nextest commit (Evidence 2).
2. **D, confirmed (the strongest cause): a different machine and a different load.** CI = one run alone per fresh VM, `ubuntu-24.04` (verified), 1,410-1,727 tests, 282-656 s. The Gate on `pro` = macOS, 14 threads, 3,237 to 5,169 tests, 4-5 Gates at once at load 55-100. The Gate also ran on hs (Linux: inference; 16 cores, 1,307 tests, 0 LEAK in 3 runs, commit ba01e4fc) before the Mac.
3. **B, confirmed (contributing): the suite grew 1,307 (10-18) to 5,137 (10-25), 3.9 times, in 7 days.** More processes per run = more chances for the 100 ms timer to fire late.
4. **C/F, confirmed (contributing, a new finding): test slots capped full runs at 2 from 2026-10-20 (fa7fb29b) to 2026-10-25 18:14 EDT (84c274bf, #425).** Gates now run at the merge and the slots are gone. Gate runs overlapping on pro: max 2 on 10-22..10-24, max 4 on 10-25. Of 8 LEAK runs 6 fell on 10-25; but 4 of the 8 overlapped no other nextest run: other load (builds, scanners) also counts (inference).
5. **A, killed as the cause, confirmed as a fact:** `result = "fail"` was added 2026-10-20 (739ec1cc) for a real leak (`ms-leak`, a fake engine's `sleep`). It made leaks fail runs; it did not create them. Logs before it do not exist on this machine (earliest 2026-10-20 14:40), so the pass-LEAK count there is 0 of 0 logs (unverified for macOS before 10-22).
6. **E, killed:** nextest 0.9.148 is pinned from the first nextest commit (CI log `cargo-nextest 0.9.148`, same on `pro`); no upgrade happened. rustc: not pinned, CI used `stable` (unverified which).
7. **New, F, confirmed: the repo's `period = "100ms"` is half of upstream's default.** The changelog says "default leak timeout increased from 100ms to 200ms" in 0.9.108 (2025-10-21; https://nexte.st/changelog/), and a scratch run of 0.9.148 with no config shows it: a test leaking a `sleep 0.5` is `LEAK` at 0.268 s (test about 0.07 s + a 0.2 s wait), a `sleep 0.15` passes without LEAK (diagnostic, outside the repo; the reference page nexte.st/docs/configuration/reference still says 100ms: stale). So CI and hs ran with 200 ms and pass; since 10-20 the Gate runs 100 ms and fail. That halves the slack nextest's own scheduling delay gets (rs-cpd: nextest is 1-11 s late under load).
8. For the User: the leak check is stricter than any CI ever was, the machine that runs it is new, and the guard that held Gates down (slots) was removed 10-25. Nothing says GitHub-hosted runners would leak: they never saw one in 92k executions at 2 threads (2 vCPU: unverified, GitHub's documented size for private repos).

## Evidence

### 1. Timeline (dates EDT; `git log --date=short --format='%h %ad %s'`, `gh run list`)

| Date | Commit | What changed |
|---|---|---|
| 10-12 | b72b9ca8 | first commit of the repo |
| 10-16 | 044ce97a (#10) | `.github/workflows/ci.yml` added: `ubuntu-latest`, `scripts/cargo.sh test --workspace` (cargo test, one binary after another) |
| 10-18 17:25 | ba01e4fc (#56, hs-d195) | Gate and CI run `cargo nextest run --workspace`; ci.yml installs `cargo-nextest@0.9.148`; proof on hs: 1307 tests, 3 runs, 105/123/97 s, 0 LEAK, load 27-38, 16 cores. Test groups `tmux-env`, `driver-reload` (max-threads 1) |
| 10-18 | (same ci.yml) | `runs-on: ubuntu-24.04` pinned (comment: ubuntu-latest moves to Ubuntu 26 on 10-19), no `test-threads` set = nextest's default (cores) |
| 10-18 | 05b85e64 (#82) | `fail-fast = false` |
| 10-19 04:36Z | (GitHub) | Actions stop: "recent account payments have failed" (check-run annotation of run 39200345950); every later run fails in 0 s, no steps |
| 10-19 09:55 | (GitHub) | CI workflow `disabled_manually` (docs/research/gate-transition.md:11) |
| 10-19 | 2c08689b (#113, hs-d283) | `ci.yml` deleted: "the Gate is the check". Last GitHub run with tests: 10-19 04:37Z |
| 10-19 | 7b140d99 (#121) | where the test time goes; 1,899 tests at hs load 82-144 (nextest.toml comment) |
| 10-20 12:57 | 739ec1cc (#169) | **`leak-timeout = { period = "100ms", result = "fail" }` added** (nextest.toml +5 lines). Reason in the file: "ms-leak: the fake old engine's `sleep` outlived an `install_script` test". Commit line: "old engine fake execs flock (--no-fork) so no sleep leaks; nextest fails a LEAK". No research file |
| 10-20 | fa7fb29b (#153, hs-d336) | **test slots**: the Gate and every full run flock one of `test_concurrency` slots (core/src/config/file.rs:255, default **2**) |
| 10-20/21 | 8b52beca, c27097c2, 9e239ded | tests 2x faster, one binary per area; Workers and Gates at nice 19 |
| 10-22 | (logs) | first nextest Summary log on `pro` (~/.cpd/run): the Gate runs on the Mac. First macOS LEAK: `cpd-core driver::stall::tests::a_stall_is_killed_unless_a_check_span_is_open` (`FAIL + LEAK`), a real code fault: `/proc` does not exist on macOS (docs/research/macos-gate-rest.md:29) |
| 10-23 | 09b72595 (#278) | "Make the local Gate pass on macOS": 3,237 tests per Gate (nextest.toml comment) |
| 10-24 | 31df3171 (#305) | tmux render tests deleted; 3,757 tests per Gate (log pr-mcp.pre-merge.log) |
| 10-25 18:14 | 84c274bf (#425, hs-d679) | **test slots dropped; Gates run at the merge**; 5,137-5,169 tests; 30 LEAK-FAIL lines this day |

Test count over time (the docs' own figures, plus logs): 1,307 (10-18, ba01e4fc message) - 1,410..1,727 (CI logs 10-18..19) - 1,899 (10-19, nextest.toml) - 3,237 (10-23) - 3,757 (10-24) - 5,137/5,169 (10-25 logs).
Worker cap (`concurrency`): default 3 since 10-15 (core/src/config/file.rs:277, 49dc6ab0); `pro` config 8 on 10-22 (~/.cpd/config.toml.bak-20261022 line 6, `machines.yaml cpd.concurrency`); 5 since 10-25 "TEMPORARY, Gate load" (~/.cpd/config.toml:6). `cpd config` prints 5 now. No git history of the machine file (unverified when it first became 8).

### 2. Candidates, ranked

| # | Candidate | Result | Evidence |
|---|---|---|---|
| 1 | D: runs on `pro`, not a hosted runner | **confirmed** | CI: `ubuntu-24.04`, one run per VM; 59 finished nextest runs on it, 0 LEAK (below). Gate on `pro`: 8 LEAK runs in 189 nextest logs, 98 LEAK-FAIL lines in all logs. hs (Linux, 16 cores): 3 runs, 0 LEAK (ba01e4fc message). Same nextest 0.9.148 everywhere. Which part of `pro` (scheduling late under load, not the OS) is in leak-flake-os.md |
| 2 | B: the suite grew | **confirmed, contributing** | 1,307 to 5,137 tests in 7 days (above); LEAK-FAIL per Gate rose with count (1 in 3,757; 3 in 5,137). Count alone does not decide: single Gates ended 0 LEAK at 5,100 (Lead's Context) |
| 3 | C/F: more Gates at once; slots removed | **confirmed, contributing** | slots default 2 from 10-20 to 10-25 18:14 (fa7fb29b, 84c274bf); overlapping nextest runs on `pro` max 2 (10-22..24), max 4 (10-25) (Evidence 4). 3 of the 6 LEAK runs of 10-25 overlapped 2-4 runs; 3 overlapped none, so other load counts (inference) |
| 4 | A: `result = "fail"` is new | **killed as cause; true as fact** | added 10-20 (739ec1cc) for a real leak. Before it a leak printed `LEAK` and passed (https://nexte.st/docs/features/leaky-tests/ : "By default, leaky tests are considered to be successful"). On CI nextest printed 0 `LEAK`, so there was nothing hidden to see there. No `pro` log before 10-20 exists |
| 5 | E: nextest/rustc upgrade | **killed** (but see 8: the default changed in 0.9.108, before the pin) | pinned 0.9.148 (released 2026-10-07) from ba01e4fc; CI log line `cargo-nextest 0.9.148 (cd1d6d546 2026-10-07)`; local `cargo nextest --version` same; 0.9.149 (10-20) is not installed. Changelog (https://nexte.st/changelog/) has no leak entry for 0.9.147-0.9.149 (grep of the page: 0.9.148 "leak timeout no longer restarted each time a handle produces output" #3646, 0.9.145 the pipe-inherit fix #3553, 0.9.108 default 100 to 200 ms; none changes behaviour between 10-18 and now) |
| 6 | F: the Gate never ran its tests on Actions | **killed** | it did: 61 nextest logs 10-18..19, 54 green, 5 red for real test failures (`cpd::watch ...`), 2 cancelled; and the pre-nextest `cargo test` runs before. Short life: 1 day with nextest, then billing stopped Actions |
| 7 | F: leak-timeout 100 ms vs upstream 200 ms | **confirmed, contributing** | 739ec1cc set `period = "100ms"` with `result = "fail"`; upstream default 200 ms since 0.9.108 (changelog); scratch run: `sleep 0.5` leak = `LEAK [0.268s]` with no config; `sleep 0.05` leak = `LEAK-FAIL [0.183s]` with the repo's 100 ms config (diagnostic, scratch project and `--config-file` outside the repo, removed) |
| 8 | F: the nextest CI threads | **inference** | no `test-threads` in ci.yml: default = vCPU count. Hosted private Linux runner = 2 vCPU (GitHub doc, not fetched: unverified): 2 tests at a time, 1,716 tests in 654 s. `pro`: 14 at a time per Gate |

### 3. LEAK vs LEAK-FAIL (all logs I could find)

Logs: `~/.cpd` holds **3,158** `*.log` (3,138 outside `ref/`, which only copies Gate logs); earliest 2026-10-20 14:40, so **none before the `fail` commit (12:57 on 10-20)**; per date outside `ref/`: 10-20: 1, 10-21: 4, 10-22: 368, 10-23: 396, 10-24: 710, 10-25: 1,635, 10-26: 24. Of these **189 hold a nextest Summary** (10-22: 32, 10-23: 28, 10-24: 46, 10-25: 81, 10-26: 2 by mtime).
Count of result lines (`LEAK [`) = leaky test that passed; `LEAK-FAIL [` = failed by the leak check; `FAIL + LEAK [` = failed by itself and leaky):

| Date | `LEAK` (passed) | `LEAK-FAIL` | `FAIL + LEAK` | runs with a leak / nextest runs |
|---|---|---|---|---|
| 10-22 | 0 | 0 | 4 (2 tests, each listed twice) | 1 / 32 |
| 10-23 | 0 | 0 | 0 | 0 / 28 |
| 10-24 | 0 | 4 | 0 | 1 / 46 |
| 10-25 | 0 | 30 | 0 | 6 / 81 |
| 10-26 | 0 | 0 | 0 | 0 / 2 |

(Each failing test is listed twice: in the run and in the summary tail, so halve for tests. Outside `ref/`: 34 LEAK-FAIL lines; with the `ref/` copies 96 LEAK-FAIL result lines.) **A passing leak (`LEAK [`) appears in no log: 0 of 3,158.** On GitHub (61 nextest job logs, 2026-10-18 21:26Z to 10-19 04:37Z): `grep -c LEAK` = 0 in every one. Victims on `pro` 10-24/25 are pure unit tests (`cpd-core policy::tests::a_symlinked_file_has_its_realpath_as_source`, `cpd-tui plan::tests::the_pulse_re_reads_on_its_contexts_events`, ...), as the Lead found.

### 3b. Scratch diagnostic (outside the repo)

```sh
D=$(mktemp -d); cd $D; cargo init --lib lk; cd lk   # tests spawn `sleep 0.05|0.15|0.5` and return (leak)
CARGO_TARGET_DIR=$D/t TMPDIR=$D cargo nextest run --test-threads 1                    # default: 0.5 -> LEAK [0.268s], 0.15 -> pass
printf '[profile.default]\nleak-timeout = { period = "100ms", result = "fail" }\n' > $D/n.toml
CARGO_TARGET_DIR=$D/t TMPDIR=$D cargo nextest run --test-threads 1 --config-file $D/n.toml  # repo's setting: LEAK-FAIL [0.183s]
```

### 4. Gates at once

- Events (`cpd events list --kind attempt-status --kind attempt-started`, 3,403 events, 10-22 17:22Z to 10-26 04:13Z): `what=gate` check start/end pairs (the Gate-as-check, 10-22..24; replaced by `merging` on 10-25): **229 intervals, concurrent: while at least one runs p50 4, p90 7, max 8**; hourly max p50 5, p90 7, max 8 (10-23 had 8). These are Attempt checks (pre-commit gate + tests of a Worker), most of which are small; they are not 5,000-test runs.
- Full nextest runs (189 Gate/test logs, interval = mtime - Summary seconds, inference): while at least one runs **p50 1, p90 2, max 4**; per day max 2 (10-22), 2 (10-23), 2 (10-24), 4 (10-25, 2 hours with 3 or more). This matches the cap of 2 slots until 10-25 18:14.
- LEAK runs and overlapping nextest runs: 10-22 23:02 (1), 10-24 16:04 (1), 10-25 13:29 (4, 4), 23:33 (2, 2), 23:39 (1), 23:57 (1).
- GitHub concurrency: `concurrency: group: ci-${{ github.ref }}` with cancel-in-progress (ci.yml), one VM per run: no sibling Gates by construction.
- Machine now: load 62.4 54.9 50.4 (`uptime`, 10-26 00:14 EDT), 3 Workers' Gates and builds.

## Reproduction

All read-only; scratch dir from `mktemp -d`, removed at the end.

```sh
# CI history: every run, then the rust job log of each run since the nextest commit
gh run list --limit 1000 --json createdAt,conclusion,headSha,databaseId,event > runs.json
gh api repos/clayton-duarte/cpd/actions/runs/$id/jobs > $id.json       # job ids and conclusions
gh api --allow-escape-sequences repos/clayton-duarte/cpd/actions/jobs/$job/logs > $id.$job.log
grep -c LEAK *.log                      # 0 in every log; 'Starting N tests', 'Summary [ Xs ]' give size and time
gh api repos/clayton-duarte/cpd/check-runs/$job/annotations   # the billing stop, run 39200345950
# the git history
git log --follow --date=short --format='%h %ad %s' -- .config/nextest.toml
git log -S'leak-timeout' --date=short --format='%h %ad %s' -- .config/nextest.toml
git log --diff-filter=AD --name-status --date=short --format='%h %ad %s' -- .github
git show 044ce97a:.github/workflows/ci.yml; git show 2c08689b^:.github/workflows/ci.yml
git show fa7fb29b:core/src/config/file.rs | grep -n test_concurrency
# leaks per kind in the logs
find ~/.cpd -name '*.log' -print0 | xargs -0 grep -h -E 'LEAK' | sed 's/\x1b\[[0-9;]*m//g' | grep -E '^ *(FAIL \+ )?LEAK(-FAIL)? *\[' | cut -c1-20 | sed 's/\[.*//' | sort | uniq -c
```
```python
# conc.py ev-all.jsonl gate|tests : concurrent check runs per Attempt from the Events (pages of 500 with --since <last id>)
import json,sys,datetime as D,collections
what=sys.argv[2]; ev=[json.loads(l) for l in open(sys.argv[1])]
P=lambda s:D.datetime.fromisoformat(s.replace('Z','+00:00')).timestamp()
by=collections.defaultdict(list)
for e in ev: by[e['attempt']].append(e)
iv=[]
for a,es in by.items():
    es.sort(key=lambda e:e['id']); st=None
    for e in es:
        d=e['data']
        if d.get('what')==what and d.get('check')=='start': st=P(e['at'])
        elif d.get('what')==what and d.get('check')=='end' and st is not None: iv.append((st,P(e['at']))); st=None
        elif st is not None and d.get('check') is None and e['kind']=='attempt-status': iv.append((st,min(P(e['at']),st+1800))); st=None
    if st is not None: iv.append((st,st+1800))
pts=sorted([(s,1) for s,e in iv]+[(e,-1) for s,e in iv]); cur=0; last=None; tw=collections.Counter()
for t,dl in pts:
    if last is not None and cur>0: tw[cur]+=t-last
    last=t; cur+=dl
tot=sum(tw.values()); acc=0
for k in sorted(tw):
    acc+=tw[k]; print(k,'concurrent:',round(100*acc/tot),'% of busy time at or below')
```
```python
# nxconc.py : Gate/test runs in ~/.cpd/run/*/logs with a nextest Summary; interval = [mtime - Summary s, mtime]
import glob,re,os,collections
iv=[]
for f in glob.glob(os.path.expanduser('~/.cpd/run/*/logs/*.log')):
    s=re.sub(r'\x1b\[[0-9;]*m','',open(f,errors='replace').read())
    m=re.search(r'Summary \[\s*([\d.]+)s\]\s+(\d+) tests run',s)
    if m: e=os.path.getmtime(f); iv.append((e-float(m.group(1)),e))
pts=sorted([(a,1) for a,b in iv]+[(b,-1) for a,b in iv]); cur=0; last=None; tw=collections.Counter()
for t,d in pts:
    if last is not None and cur>0: tw[cur]+=t-last
    last=t; cur+=d
print(len(iv),'runs; busy seconds by concurrency',dict(sorted(tw.items())))
```

## What this means for the fix

- **The check itself is the same on CI and on `pro`**; what changed is where and under what load it runs, plus the removal of the slots. So the defect is a load-sensitivity of the 100 ms timer that no CI-like run ever met, not a new regression of nextest.
- **Cheapest, matches the User's decision pro-d44** (fixed `test-threads` below the core count): restores the effective ceiling the slots gave (max 2 full runs x 14 threads = 28 processes until 10-25; 8 Gates x 4 = 32 now). Cost: slower single Gate (hosted runner: 2 threads, 654 s for 1.7k tests; `pro` alone at 14 threads: 131-302 s for 5.1k, from the logs above). The slot era is evidence that a cap of about 28 concurrent test processes machine-wide ran Gates 10-22..10-24 with 1 LEAK in 46 runs; 4 x 8 = 32 is just above it (inference).
- **Convention over configuration** points at the line `period = "100ms"`: upstream's default is 200 ms (verified above). Deleting the period (keeping `result = "fail"`) is a return to the default, not a raise above it: the Lead decides whether that counts under the Plan's rule; it only halves the miss rate in principle (unverified: a 1-11 s scheduling delay exceeds both).
- **Do not** raise `leak-timeout` or set `result` back to pass: A shows the check caught a true leak (ms-leak) on its first day; and on CI it showed 0 in 92k executions, so the check is not noise by design.
- Not decided here: whether a hosted runner (D) would stay clean for 5,100 tests (the CI evidence is for 1.7k tests, 2 threads, alone): `unverified`; running the 10-25 suite on a GitHub-hosted runner is the one test of it (Actions is off for billing, 10-19).
- Open (outside this Job): the cause on `pro` is the timer firing while nextest is not scheduled (rs-cpd, H3a); this history only says when and under what conditions it became visible.
