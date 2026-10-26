# Would the Gates do better on homeserver (hs)? (Plan leak-flake, question 2)

## Verdict

- **Leaks: better, probably (inference, not proven).** hs ran 182 Gates (all with `leak-timeout` `result = "fail"`, 2026-10-20 13:00 to 10-25) with 0 `LEAK-FAIL` lines; `pro` shows 1-8 per run at load 55-100. `ssh homeserver` + `grep -rlE "LEAK-FAIL" ~/.cpd/run` found 0 files. The only 10 `LEAK` lines on hs are one real leaking test, 2026-10-20 before 12:57 (the fake old engine's `sleep`, 2.4-3.0 s: a true leak, fixed by `ms-leak`).
- **Why it differs is open.** hs is Linux: cargo-nextest 0.9.148 does NOT take its spawn lock there (`SPAWN_INHERITS_PIPES` excludes `linux`, source read below), so the same fd-inherit window exists; hs is also the same nextest version as `pro`. The window cannot be shown as the cause on hs; what differs is exec speed and load (below). Not killed, not confirmed: the Linux window (issue #1469, comment of beeauvin) is shown only under `strace`-delayed exec.
- **Exec latency: hs faster.** One pure named test, 50 execs: hs 2.2-2.4 ms each (hs load 0.9), `pro` 6.4-11 ms each (`pro` load 42). Different CPU and different load: not a controlled comparison.
- **Load: hs is calm today.** Load 0.8-3.7 now (16 threads, Ryzen 7 3800X, 62 GiB); the "load 82-144" of docs was measured on 2026-10-17..19 when several Gates and Workers ran at once. hs is NOT quieter by design: it runs k3s, postgres, media services and a live `cpd watch` (concurrency 5).
- **Operational cost is the real obstacle.** A Plan runs on one machine and Contexts are machine-local (contract.md:2055), so a Gate on hs means the Context, Lead and Workers live on hs, not a remote Gate for `pro`'s Plans. Nothing copies a worktree to another machine.
- **Same, not better: Gate length.** hs Gate wall, 182 Gates: median 137 s, p90 174 s, max 278 s (5,100-ish tests); `pro` calm 131-137 s, loaded 212-1,530 s.
- **Cheapest try:** one decided experiment, no build: run the Plans of `leak-flake`-style work on hs for a week and count `LEAK-FAIL` in `~/.cpd/run/*/logs/*pre-merge.log`; see the last section.

## Evidence

### hs vs pro

| | hs (`homeserver`, 192.168.3.3) | pro (this Mac) |
|---|---|---|
| OS | Linux 6.8.0-142-generic x86_64 (`ssh homeserver 'uname -srm'`) | macOS 26.7.1 arm64 (`sw_vers`) |
| CPU, cores | AMD Ryzen 7 3800X, 16 threads (`nproc`; `/proc/cpuinfo`) | Apple M4 Pro, 14 (`sysctl hw.ncpu`) |
| RAM | 62 GiB total, 30 used, 32 available, swap 7 (`free -g`) | 48 GiB (`sysctl hw.memsize`) |
| nextest, rustc, sccache | 0.9.148 (cd1d6d546), 1.99.0, 0.7.7 | 0.9.148, 1.99.1 |
| load now (1, 5, 15 min) | 0.76/2.51/3.74; 0.78/2.49/3.73; 0.93/2.38/3.66 (3 samples over 30 s, then 1 later) | 41.6/31.9/44.1; 43.6/32.5/44.2 |
| uptime | 27 days | 4 days |
| other load | k3s-server, decypharr, prometheus, argocd, 25 postgres, containerd (54 shims), 15 pi, 26 cpd, a live `cpd watch` (pid 261473) on `tmux -L cpd`, concurrency = 5 (`~/.cpd/config.toml`) | 4-5 Gates plus Workers, Cyberhaven (6 procs) and Falcon (CrowdStrike) running (`ps -Ao comm`) |
| endpoint products | none seen: `ps -eo comm` shows no crowd/falcon/cyber/sentinel/clam/defend/osquery/wazuh/sophos process; `kauditd` is the kernel's | Cyberhaven, Falcon, XProtect, PanGPS (gc-os finding) |
| IO scheduler | mq-deadline | n/a |
| disk | nvme, `~/projects/cpd/target` 19 G, 302 G free | n/a |

All hs reads are read-only (`uname`, `uptime`, `free`, `ps`, `grep`, `ls`). Docs (verified, file:line): `docs/implementing.md` quotes 16 cores and load 82-144 on 1,899 tests; `.config/nextest.toml:27-29` "Measured on hs ... 7 full runs at load 82-144 (1,899 tests; hs was never calm)"; `docs/commands.md:3743` "Several full suites and Gates at once loaded a host to 205 on 16 cores, and every Gate failed on timing".

### Did hs show the LEAK?

- `ssh homeserver 'grep -rlE "LEAK" ~/.cpd/run'` (not `specs/`): 10 Gate logs, all `2026-10-20 10:52-12:59`, each ONE line `LEAK [2.4-3.0s] cpd::it|cpd::tooling install_script::an_old_engine_that_never_frees_its_lock_fails_the_install_with_the_reason` (pass-by-default era; the repo's `leak-timeout` `result = "fail"` came in `739ec1cc`, 2026-10-20T12:57:25-04:00 (`git log -S'result = "fail"'`)). A real leak: 2.4-3 s, not the 100 ms flake.
- `grep -lE "LEAK-FAIL|leaked handles"` over all `~/.cpd/run/*/logs/*.log` on hs: **0 files**. 379 `pre-merge` logs, 289 with a `Summary`; 182 of those are dated after `739ec1cc`: median 136.8 s, p90 174.4 s, max 278.2 s, up to 5,168 tests; 0 with a LEAK-FAIL.
- Caveat (verified by the file names): each log is the LAST run of a PR, an earlier run of the same PR is overwritten, so the count is a floor on Gates, not all runs; hs load during each Gate is not recorded in the logs (unverified).
- `pro`: `~/.cpd/ref/gatemac/leak-logs/` has 15 logs, 10 with LEAK-FAIL lines, e.g. `bifrost-pr-discover.pre-merge.1792985606.log` `Summary [341.9s] 5147 tests run: 5142 passed, 5 failed (5 due to being leaky)`; `bifrost-cyan...` 136.5 s, 0 leaks (calm).
- Compare: hs 0 of 182 post-change Gates; `pro` 10 of 15 saved logs (selected as failures: biased), plus `/tmp/cpd-leak-flake/extra-evidence.md` (about 8 listed Gates, each 1-8 victims).

### Issue nextest-rs/nextest#1469 ("leaky" tests; open) on Linux and elsewhere

`gh issue view 1469 -R nextest-rs/nextest --comments`:

- asomers (FreeBSD 15, 16 cores, mockall: "does not contain _any_ tests that spawn processes"): "-j1 ... no failures. In fact, the failure probability is closely related to the value of the -j option": -j4 never?, -j8 a few minutes, -j16 a few dozen runs, -j32 3-5 runs, -j171 instant.
- sunshowers (maintainer): "In normal use, the stdout and stderr fds should always be closed before the child process exits in a way detectable by the parent. That seems to be how it is on Linux, macOS and illumos." and "We may just have to bump the leak timeout on some platforms like Windows and FreeBSD." Also a commit "dc6a40e ... the `biased` change" to leak detection.
- beeauvin (a Linux report; an LLM-generated diagnostic, its own label): "We ran into this on linux and I verified the issue myself." Cause given: "`SPAWN_INHERITS_PIPES` leaves out `target_os = "linux"` ... so `PROCESS_SPAWN_LOCK` is never taken on Linux ... The child that `posix_spawn` clones ... starts with a copy of every descriptor the runner has open." Shown "in docker `rust:1.95` arm64 ... with `strace` delaying the runner's first exec ... by 300 ms: Untraced 360 passed; exec delayed, leak-timeout 100 ms: 160 LEAK-FAIL; exec delayed, one test thread: 0; leak-timeout 10 s: 0". Maintainer's reply: "Is this something you've seen in a non-simulated context?" (unanswered in the read output).
- Verified in source: `gh api repos/nextest-rs/nextest/contents/nextest-runner/src/test_command.rs?ref=cargo-nextest-0.9.148` lines 35-47: `SPAWN_INHERITS_PIPES` is false for `linux` (and freebsd, illumos, ...); lines 248-272 take `PROCESS_SPAWN_LOCK` only when it is true. So macOS takes the lock (not in the list), Linux does not. This contradicts none of the macOS findings (rs-src); it says Linux has no protection, only a faster exec.
- Reading: the issue says the problem is not macOS-only and rises with `-j` (FreeBSD table) and with a delayed exec. hs, with 16 threads and concurrency 5, did not show it in 182 Gates: inference: lower exec latency and load, not a different code path.

### Exec comparison (one measurement, as specified)

One pure test `cpd-core clock::tests::a_time_is_written_in_utc_with_milliseconds` (it exists in both binaries: `--list | grep -c` = 1 each), run 50 times, each a process of its own, exit 0 checked (no FAIL printed). Same loop, `sh -c`, output to /dev/null.

| | binary | load while running | real / user / sys for 50 execs | per exec (real) |
|---|---|---|---|---|
| pro | `cpd_core-63a596a602020fb0` (37 MB class, an italic Gate worktree) | 41.6-43.6 | 0.55 s / 0.07 / 0.10 (first, incl. cold); 0.32 / 0.07 / 0.09; 0.46 / 0.08 / 0.11 | 6.4-11 ms |
| hs | `cpd_core-52cdfb5868a6e519` (37.3 MB, built 2026-10-25 18:57) | 0.89 | 0.12 s / 0.01 / 0.10; 0.11 / 0.01 / 0.10; 0.11 / 0.01 / 0.10 | 2.2-2.4 ms |

Reading: hs is 3-5 times faster per exec and about 7 times less user time (0.2 ms vs 1.4 ms per exec; sys about 2 ms on both: `time -l` vs GNU `time -f`, children included). NOT controlled: different CPU, kernel and load (pro was at load 42 from other Gates), different commits of the binary. It does not show that hs would be faster under pro's load (inference: Linux fork/exec scales better, unverified). No test suite was run.

### What a Gate on hs needs (facts, file:line)

- hs already runs the same cpd: `~/.cpd/config.toml` `machine = "hs"`, `concurrency = 5`; `~/.cpd/policy/clayton-duarte/cpd.toml` is the same policy (hooks `pre-merge` `verify = "npm run verify || exit 64"`); `~/projects/cpd` at `339e47ca` (2026-10-25 23:54), `target/` 19 G; `cargo`, `cargo-nextest`, `sccache` 0.7.7, `node`, `npm`, `tmux`, `gh` present (`which`).
- The Gate runs where the Plan's worktree is: contract.md:3267 ("cpd runs them itself"), contract.md:3275 (`pre-merge` "in the Attempt worktree at HEAD"), contract.md:3398 (the PR's worktree, `pre-start` then `pre-merge` under a slot). There is no remote-Gate command: `docs/commands.md` has none (a Plan runs on one machine).
- Contexts and positions are machine-local: contract.md:2055, :2058, :30 ("one database per machine"); the Context `building-cpd`/`cpd` of `pro` is not on hs (hs's Context ids are its own; record prefix `hs-` is hs's machine name, `pro-` is pro's).
- Policy is per machine: contract.md:3649 ("machine scope ... one machine"); `pro`'s `host.toml` says it is rendered from dotfiles `machines.yaml` (its header; hs's own not read).
- Build cache: `scripts/cargo.sh:5-10` starts one sccache per machine; hs has its own `~/.cache/sccache` (not read); `tests-mock-build-time.md:40`: a cold Gate build with sccache 22-36 s, "homeserver's 133 s for the whole Gate" (docs).
- Missing for a `pro`-driven Gate on hs: a worktree sync (rsync or git push of the Lead Branch to hs), a driver for `pre-merge` over ssh, the log copy back, secrets (contract.md:3778 vaults `cpd` and `homeserver` per machine). None exists in the repo (searched `docs/contract.md`, `docs/commands.md`: unverified for scripts).
- hs mixes cpd with services: a Gate there shares the CPU with k3s, postgres, media (the `ps` above) and with hs's own Workers.

## Reproduction

```sh
# 1. hs facts (read-only)
timeout 20 ssh -o BatchMode=yes -o ConnectTimeout=5 homeserver 'uname -srm; nproc; uptime; free -g; cargo nextest --version | head -1'
timeout 20 ssh -o BatchMode=yes homeserver 'ps -eo comm,pcpu --sort=-pcpu | head -15; ps -eo comm | grep -iE "crowd|falcon|cyber|sentinel|clam|defend|osquery|auditd|wazuh|sophos"'
# 2. leak search on hs
timeout 90 ssh -o BatchMode=yes homeserver 'cd ~/.cpd/run; grep -rlE "LEAK-FAIL|leaked handles" */logs | wc -l; grep -rlE "LEAK" . | grep -v specs/ | head'
timeout 90 ssh -o BatchMode=yes homeserver 'cd ~/.cpd/run; ls */logs/*pre-merge*.log | wc -l; grep -h "^ *Summary" */logs/*pre-merge*.log | head'
git log --format='%h %aI' -S'result = "fail"' -- .config/nextest.toml
# 3. nextest source on Linux
gh api 'repos/nextest-rs/nextest/contents/nextest-runner/src/test_command.rs?ref=cargo-nextest-0.9.148' -H 'Accept: application/vnd.github.raw' | grep -n -A14 'SPAWN_INHERITS_PIPES'
gh issue view 1469 -R nextest-rs/nextest --comments
# 4. exec loop (pro; pick any built cpd_core test binary B)
N=clock::tests::a_time_is_written_in_utc_with_milliseconds
B=$(ls -t ~/projects/*/target/debug/deps/cpd_core-* | grep -v '\.d$' | head -1)   # check: $B --list | grep -c "$N" = 1
/usr/bin/time -l sh -c 'i=0; while [ $i -lt 50 ]; do '"$B"' --exact '"$N"' >/dev/null 2>&1 || echo FAIL; i=$((i+1)); done'
# hs (no file written there):
timeout 150 ssh -o BatchMode=yes homeserver 'B=~/projects/cpd/target/debug/deps/cpd_core-52cdfb5868a6e519; N=clock::tests::a_time_is_written_in_utc_with_milliseconds; /usr/bin/time -f "real %e user %U sys %S" sh -c "i=0; while [ \$i -lt 50 ]; do $B --exact $N >/dev/null 2>&1 || echo FAIL; i=\$((i+1)); done"'
```

## What this means for the fix

- **Move Gates to hs wholesale** (switch the Context's machine): cost = the User works from hs (tmux over ssh; the Contexts and records on pro do not move, contract.md:2055). Gain: the evidence says 0 LEAK-FAIL in 182 Gates, exec 3-5 times faster; risk: hs's other tenants and its own load (82-144 measured in 2026-10-17..19, with 6 Workers). Unverified: a Gate on hs at load 80+ with 16 threads x 5.
- **Decided experiment (cheapest, no build):** for one week run the next Plans' Gates on hs only (its cpd, `concurrency` 5, as now) and count `LEAK-FAIL` with the Reproduction's step 2 plus `uptime` samples taken in the same Gate (a `pro`-style load of 55-100 needs 4-5 Gates at once: the test that decides if Linux's missing lock matters). Decision rule written first: any `LEAK-FAIL` at hs load >= 50 means the Linux window (beeauvin) is real and hs is not an answer; none at load >= 50 over >= 10 Gates means hs is better because of speed/load.
- **A remote Gate for `pro` (ssh `pre-merge`)** needs the worktree sync, the log copy and the hook (see "missing"): a build, not an experiment; do not do it before the experiment says hs is better.
- **Does hs's own load make it worse than pro?** Not now (load 0.8-3.7 vs 41-44), and the 182 Gates since 10-20 had 0 leaks; but its 82-144 measurements (docs) are higher than `pro`'s 55-100 and were made before `test-threads` and the 10-20 changes, so the load alone predicts no better: unverified until the experiment.
- Independent of hs: the `pro` fix (lower `test-threads`, the other Jobs of this Plan) is not changed by this finding; hs does not need it (16 threads, no leak seen), `pro` does.
