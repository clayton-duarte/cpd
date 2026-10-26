# LEAK-FAIL on cpd's real tests: findings (rs-cpd-a1)

## Verdict

Measured on mac-pro (load 51-71 at the start of each round, other Plans' Gates running), cpd's real tests from one nextest archive of the worktree (5,140 tests listed; the Gate's 5,137 + 3 skipped), the repo's `.config/nextest.toml` unchanged plus a probe wrapper. 5 LEAK-FAIL in 7 runs; 27,772 test processes probed.
- H1 (holder = another test process that inherited the pipe): KILLED for these 5 victims. Strongest line: the wrapper listed the descriptors of every test process at its start, 27,772 of them, and 0 had an inherited descriptor >= 3 (`grep -vc '\[\]' wrap` = 0 in all 7 runs). The pipe() / FD_CLOEXEC window was never seen (verified, scripts below).
- H2 (a child of a spawning test holds the pipe): KILLED for the pure victim (`cpd-tui tree::view::tests::x_refuses_the_session_in_the_chat_slot` builds a view and presses a key, `tui/src/tree/view.rs:1022`); OPEN for 4 victims that start processes (2 `cpd::` TestEnv tests, 2 `cpd-core fakes::git::*` that use real `git` in their fixtures): no holder was in the snapshots, which came 0.1-5 s late (unverified for those).
- H3a (no holder; nextest reports a leak although nothing holds the pipe): OPEN, the best supported. Strongest line: for each of the 5 victims, at the LEAK line the victim pid was gone and nextest held exactly 28 pipe ends = 14 running tests x 2, none the victim's (4 snapshots with `lsof -p <nextest>`; the first has no `.nx`); and the pure unit test cannot spawn. Not proven: no holder SEEN is not proof of none, and no longer-period diagnostic was run.
- H3b (something else, e.g. sccache, the Gate wrapper): OPEN, no evidence for it: no process outside the tests' own held a pipe of nextest's in any snapshot (unverified: the snapshots' whole-machine `lsof` takes 1-10 s and is not atomic).
- Side finding (verified): the wrapper started 1-11 s before the LEAK line of victims whose nextest time is 1.9-9.9 s: under this load the time between nextest's spawn and the test's `sh` running is seconds, so the 100 ms leak timer runs on a machine where nextest itself is late.

## Evidence

### Step 0: the six saved Gate logs (verified: `sed 's/\[[0-9;]*m//g' /tmp/cpd-leak-flake/leak-logs/*.log | grep -E 'Starting|^ +LEAK-FAIL'`)

| log | tests / binaries | victims (position, nextest time) |
|---|---|---|
| bifrost_pr-spike | 5137 / 17 | config::runner_tests::read_with_maps_... (2013, 0.217 s) |
| bifrost_pr-spike2 | 5137 / 17 | jobs fingerprint (3539, 0.902 s), exposure (3550, 0.228), poll_run (4017, 0.122), pulse (4484, 0.215), counter (4697, 0.189), view_ports (4984, 0.178) |
| mcp-policy_pr-mcp | 3757 / 19 | policy::tests::a_symlinked_file... (2649, 0.207) |
| no-slots_pr-slots | 3633 / 19 | ensure_starts_the_engine_for_a_caller_without_a_utf8_locale (266, 85.043 s), curation_lists_pending_retirements_and_doc_candidates (449, 21.876 s) |
| tests-mock_pr-mf-a1 | 3530 / 20 | tree::rows (3274, 0.159), tree::view (3344, 0.136) |
| tests-mock_pr-mf-b1 | 3530 / 20 | store::schema (2553, 0.128) |

What the logs show (verified): every victim's captured output is the normal `running 1 test ... ok`, `exited with code 0, but leaked handles`; nothing from a holder is in the output. No thread count is printed. The Gate's `Starting N tests across M binaries` line is the only run header.
For the two long victims the test's own `finished in` (84.85 s, 21.12 s) is 0.19 s and 0.76 s short of nextest's time (85.043, 21.876): the leak wait follows the test's end.
What the logs cannot show: no timestamps, so which tests ran together, and the load, are unknown.
Spawning victims: both long ones run `cpd` in-process on fake ports (`cli/tests/common/fx.rs` `Fx::run_raw` calls `cpd::main_with`/`run_with`), see the note after the victims table.

### Setup (verified)
- Build: `scripts/cargo.sh nextest archive --workspace --archive-file $W/cpd.tar.zst` (about 4 min), run from copies: `cargo nextest run --archive-file ... --extract-to $W/x<i> --extract-overwrite --workspace-remap <worktree> --config-file $W/nextest.toml`. `cargo nextest list` of the archive = 5,140 tests (Gate log: 5,137 run + 3 skipped).
- Gate environment kept: `PATH=<worktree>/scripts/tripwire:$PATH`, `CPD_COMMIT=<short sha>`, `TMPDIR` a directory of its own per run (`gate-run.sh`: `GATE_RUN`, `NODE_DISABLE_COMPILE_CACHE=1`), `CPD_HOME`/`CPD_SESSION`/`CPD_PLAN`/`CPD_ATTEMPT`/`PI_CODING_AGENT` unset (the Attempt's own; the tests remove `CPD_*`, `PI_*` for the commands they run, `cli/tests/common/mod.rs:isolation`).
- Wrapper scripts are experimental in 0.9.148: the scratch config needs `experimental = ["wrapper-scripts"]` (first line), then `[scripts.wrapper.probe]` + `[[profile.default.scripts]] filter = 'all()', run-wrapper = 'probe'`. nextest passes stdin = /dev/null, stdout and stderr = two separate pipes (verified with `lsof -p` in a wrapper), no inherited descriptor.
- Fixes I made on the way (the scripts below are the final ones): `/dev/fd/*` lists the glob's own directory descriptor (dropped), bash's script fd 255 (skipped); `stat -f %i /dev/fd/N` gives the same inode for every copy of a pipe end, so a stray fd is matched to a victim's fd 1/2 by inode (lsof's address differs from the inode).

### Rounds (verified: `$W/r<round>/<i>/{out,wrap,snap.N.*}`, all removed at the end)
| round | concurrent runs | tests per run | load average at the start (1/5/15 min) | LEAK | test processes with a stray descriptor |
|---|---|---|---|---|---|
| 1 | 1 (the other 5 failed: `--extract-to` directory missing; a script bug) | 5,140 (all, 721 s) | 51.2 / 75.5 / 78.5 | 1 | 0 of 5,140 |
| 2 | 6 | 3,682-3,862 started each; my `timeout 1500` SIGTERMed the 14 tests running at 1,463 s, so the 14 `SIGTERM` FAILs per run are mine, not a defect; 22,632 started in all | 71.1 / 80.3 / 76.0 | 4 | 0 of 22,632 |
| 3 | not run: 5 LEAK >= the 3 asked | | | | |

### Victims (pid = the wrapper's `$$`, which `exec` made the test; ppid = that run's cargo-nextest)
| round/run | victim, nextest time | pid / ppid | fd1, fd2 pipe inode | holder |
|---|---|---|---|---|
| 1/1 | cpd-tui tree::view::tests::x_refuses_the_session_in_the_chat_slot, 1.938 s (4948/5140), pure | 80931 / 23860 | 1595381839, 3226607920 | no holder found: 0 stray fds in 5,140 test processes; victim pid not in the snapshot's `ps` (taken 1 s after its exec); 13 other tests children of nextest; no per-nextest `lsof` in this round (watcher v1) |
| 2/2 | cpd::flow plan_new_by_user::plan_new_by_user_starts_pi_without_a_first_message, 9.407 s, spawns | 7119 / 30962 | 582530114, 1309243572 | no holder found: 0 stray fds in run 2; victim pid gone; nextest held 28 pipe ends, 14 other children running |
| 2/2 | cpd-core fakes::git::rebase_tests::in_progress_asks_the_git_dir_of_the_worktree_it_is_given, 9.884 s, runs real `git` in its fixture | 35938 / 30962 | 299783268, 3589462166 | no holder found: same (28 ends, victim gone, snapshot 3 s after its exec) |
| 2/3 | cpd::pr git::rule_p_an_ancestor_pushed_after_its_descendant_is_a_success_that_pushed_nothing, 4.847 s, TestEnv (reaper + teardown.sh children, `cli/tests/common/mod.rs:288-300,918-935`) | 92417 / 31000 | 2105877301, 167938070 | no holder found: same; snapshot 1 s after its exec |
| 2/3 | cpd-core fakes::git::worktree_tests::update_ref_with_a_flag_it_does_not_model_is_unsupported_exit_1_and_recorded, 5.488 s, real `git` | 44176 / 31000 | 3579612229, 725374687 | no holder found: same; snapshot 5 s after its exec |

A holder of a victim's pipe could be (a) a test process forked while the pipe was open (the wrapper would show the fd: none did), (b) a child of the victim (the snapshots show no process of the run besides the 14 running tests and their `teardown.sh`/reapers of other tests; unverified for 4 spawners because of the snapshot delay), (c) nothing. H1's window rate: 0 in 27,772 starts.
The two long victims of the saved logs (`ensure_starts_the_engine_for_a_caller_without_a_utf8_locale`, `curation_lists_pending_retirements_and_doc_candidates`) did not leak in my runs; step 0 note: they run `cpd` in-process on fake ports (`Fx::run_raw` calls `cpd::main_with`/`run_with`, `cli/tests/common/fx.rs:925-935`), so they start no `cpd` subprocess themselves (H2 holder for them: not found, `ensuring()` also sets a fake `sh`). The TestEnv `start_reaper` child has stdio on /dev/null; the teardown `sh` has stderr inherited but is waited for (`mod.rs:918-935`): neither outlives the test by design (verified by reading, not by a run).

## Reproduction

Build the archive in the worktree (`scripts/cargo.sh nextest archive --workspace --archive-file $W/cpd.tar.zst`), `mkdir $W/bin $W/x1..x6`, save the scripts below in `$W/bin`, make `$W/nextest.toml` = the repo's `.config/nextest.toml` + the first block, then `WT=<worktree> $W/bin/run.sh $W <round> 6`. LEAK-FAIL appears about once per 1-2 runs of 5,140 tests at 6 runs and load 50-70 (5 in about 7 runs here); a victim's wrapper line is `grep -- "--exact <test> " $W/r<round>/<i>/wrap`; stray strays: `grep -v '\[\]' wrap`. Do not use `pkill -f` with a pattern that is in your own command line (it killed my first shell).

`$W/nextest.toml` additions:
```toml
experimental = ["wrapper-scripts"]   # first line of the file
[scripts.wrapper.probe]
command = '$W/bin/wrap.sh'
[[profile.default.scripts]]
filter = 'all()'
run-wrapper = 'probe'
```
`wrap.sh`:
```sh
#!/bin/sh
# nextest wrapper: log what the test process inherits, then exec it unchanged.
# One line per process in $WRAP_LOG: pid ppid epoch fd1-inode fd2-inode [strays "fd:inode ..."] args
n=0; strays=; fds=
for f in /dev/fd/*; do
  fd=${f##*/}
  case $fd in 0|1|2|255) continue ;; esac   # 255 is bash's own script fd
  n=$((n+1)); strays="$strays $fd:$(stat -f %i "$f" 2>/dev/null)"; fds="$fds,$fd"
done
# the glob's own directory descriptor is one entry >= 3 (the last one listed): drop it
if [ "$n" -le 1 ]; then strays=; n=0; else strays=${strays% *}; fds=${fds%,*}; n=$((n-1)); fi
exec 7>&1 8>&2; i1=$(stat -f %i /dev/fd/7 2>/dev/null); i2=$(stat -f %i /dev/fd/8 2>/dev/null); exec 7>&- 8>&-
echo "$$ $PPID $(date +%s) $i1 $i2 [$strays] $*" >> "$WRAP_LOG"
if [ "$n" -gt 0 ]; then
  { echo "== $$ $PPID $* ($(ps -o lstart= -p $$))"; lsof -nP -w -p $$ -a -d "${fds#,}"; } >> "$WRAP_LOG.lsof" 2>&1
fi
exec "$@"
```
`watch.sh`:
```sh
#!/bin/sh
# usage: watch.sh <run.out> <snapdir> <extract-dir> ; on each LEAK line of the run's output snapshot ps + every PIPE descriptor of the machine
n=0
tail -F -n +1 "$1" 2>/dev/null | while IFS= read -r line; do
  case $line in *Summary*) break ;; esac
  case $line in *LEAK*)
    n=$((n+1)); s="$2/snap.$n"
    np=$(pgrep -f "cargo-nextest nextest run.*--extract-to $3 " | head -1); { date +%s; echo "$line"; lsof -nP -w -p "$np"; } > "$s.nx"
    { date +%s; echo "$line"; } > "$s.ps"; ps -Ao pid,ppid,pgid,lstart,etime,command >> "$s.ps"
    lsof -nP -w -d 0-40 2>/dev/null | awk 'NR==1 || $5=="PIPE"' > "$s.pipes" ;;
  esac
done
```
`run.sh`:
```sh
#!/bin/sh
# usage: run.sh <W> <round> <runs> ; WT = the worktree (cpd.tar.zst built by `scripts/cargo.sh nextest archive --workspace`)
W=$1; R=$2; N=$3; WT=${WT:?}; mkdir -p "$W/r$R"
export CPD_COMMIT=$(git -C "$WT" rev-parse --short HEAD) NODE_DISABLE_COMPILE_CACHE=1
pids=; i=1; while [ $i -le "$N" ]; do
  d=$W/r$R/$i; mkdir -p "$d" "$W/x$i"; T=$(mktemp -d "$W/r$R/tmp.$i.XXXXXX"); uptime > "$d/uptime"
  ( WRAP_LOG=$d/wrap GATE_RUN=${T##*.} TMPDIR=$T PATH="$WT/scripts/tripwire:$PATH" \
    exec env -u CPD_HOME -u CPD_SESSION -u CPD_PLAN -u CPD_ATTEMPT -u PI_CODING_AGENT \
    timeout 1500 cargo nextest run --archive-file "$W/cpd.tar.zst" --extract-to "$W/x$i" --extract-overwrite \
      --workspace-remap "$WT" --config-file "$W/nextest.toml" --color never > "$d/out" 2>&1 ) &
  pids="$pids $!"
  "$W/bin/watch.sh" "$d/out" "$d" "$W/x$i" > /dev/null 2>&1 &
  i=$((i+1))
done
wait $pids; sleep 2; pkill -f "[t]ail -F -n .1 $W/r$R" ; wait
```

## What this means for the fix

- A fix aimed at the pipe() / CLOEXEC window (H1; e.g. a nextest or std change to `pipe2`) would not touch these victims: no process inherited anything (0 of 27,772). Cost: none spent; do not start there.
- The evidence points at nextest's own timing under load (H3a): a leak timer of 100 ms measured while nextest is seconds late. Options: (1) the Gate runs with fewer other Gates at once or lower `-j` (cost: slower Gate, a measure, no repo change to the tests); (2) a leak-timeout the real runs do not reach is forbidden by the Plan's rules, so it stays a DIAGNOSTIC: run `--config-file` with `leak-timeout = { period = "2s", result = "fail" }` in the same 6-run recipe; if LEAK-FAIL disappears and no stray is seen, H3a is confirmed (cost: one 25-minute round, not run here: unverified); (3) a nextest upstream report with the 6-run recipe (out of scope here).
- A later Job should also snapshot faster: `lsof -p <nextest>` of the victim's read end before nextest closes it (the 100 ms) is not possible from outside; run the victim pipe's address through the wrapper only for the tests of a rerun (cost: one `lsof` per test).
