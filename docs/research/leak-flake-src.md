# Leak flake: sources and std-level repro (rs-src, attempt 1)

## Verdict

- H1 (pipe window) at std level: CONFIRMED. std 1.99.1 on macOS uses `pipe()` then `set_cloexec` (`library/std/src/sys/pipe/unix.rs:30-41`); with no lock, 1-21 of 2,000 children inherited a sibling's pipe (T>=4, load 63-77), 0 at T=1. verified (Part 2).
- H1 as the cause in nextest 0.9.148: KILLED as stated, open in a narrower form. nextest 0.9.148 already has the fix (PR #3553, in 0.9.145): own pipes via `std::io::pipe()` under a write lock, `posix_spawn` under a read lock (`nextest-runner/src/test_command.rs:57,246-273`). Same scheme in my repro (`nxlock`): 0 of 16,000 children inherited. verified. Residual (unverified, call it H1b): spawns share the read lock, so a sibling child that is still between `posix_spawn` and its exec holds copies of every CLOEXEC fd of nextest, including a running test's write ends, for that moment only; under load that moment could last longer than 100 ms. Not measured (needs a kernel/dtrace trace).
- H2 (a test's own child holds the pipe): OPEN. The sources cannot decide it; it cannot explain the pure unit tests.
- H3a (no holder, nextest not scheduled in time): OPEN, plausible at source level. The 100 ms timer starts after `child.wait()` returns (`runner/executor.rs:1487`, called at `:1075`) and EOF is read by the same executor task through tokio's I/O driver, so a starved task/driver can expire it with no holder. `biased` (`:1494`) reads first, which narrows but does not close it. unverified: needs a holder-or-not check at the LEAK moment.
- H3b (something else): OPEN, nothing in the sources points at it.
- Upstream: no release after 0.9.148 (only 0.9.149, security) changes leak detection. nextest#1469 (same LEAK under high -j on FreeBSD/Windows/Linux) is open and undiagnosed.
- Nothing to fix in this repo from these sources alone: next step is a run that records the holder at the LEAK moment (H2/H3a/H1b).

## Evidence

All paths are in the clones of tag `cargo-nextest-0.9.148` (commit cd1d6d5467322dbc0d39a80cb671d000ac302798, equal to `cargo-nextest 0.9.148 (cd1d6d546 2026-10-07)` printed by the installed binary), Rust tag `1.99.1`, and tokio 1.53.1 (nextest `Cargo.lock`, crate downloaded from static.crates.io).

### Part 1, sources

1. How nextest creates and starts a test process (all verified, read at the tag):
   - Pipes are nextest's own, not `Stdio::piped()`: `nextest-runner/src/test_command.rs:246-254` `create_pipe()` calls `std::io::pipe()`; `spawn_piped` `:275-302` makes one pipe per stream, sets the write end with `cmd.stdout(tx)`/`cmd.stderr(tx)`, keeps the read end and later wraps it as tokio `ChildStdout` (`test_command/unix.rs:11-17`, `test_command/imp.rs:38-45`). Strategy `Combined` uses one pipe and `tx.try_clone()` (`test_command/imp.rs:~75-90`).
   - Start: `spawn_process` (`test_command.rs:259-273`) calls `tokio::process::Command::from(cmd).spawn()`, which calls `std::process::Command::spawn` (tokio `src/process/mod.rs:863-866`); tokio creates no pipe for it (stdin is `Stdio::null()`/inherit, `test_command/imp.rs:54-62`). std on macOS uses `posix_spawn` unless a `pre_exec` closure, uid/gid/chroot, or env-PATH-with-relative-program forces fork/exec (`library/std/src/sys/process/unix/unix.rs:73`, `:446-460`). The process group is set with `Command::process_group(0)` (`runner/unix.rs:32-33`, called `runner/executor.rs:828`), which std passes as `POSIX_SPAWN_SETPGROUP` (`unix.rs:729-731`); no `pre_exec`, no `setsid`. Tests run through the double-spawn stub (`double_spawn.rs`, `current_exe`), also via `spawn_process`.
   - Threads: the spawn runs inside the per-test async task on the tokio runtime (`runner/executor.rs:~850-860`), so any worker thread. Lock: yes, `static PROCESS_SPAWN_LOCK: RwLock<()>` (`test_command.rs:57`): `create_pipe` takes the write lock (`:249`); `spawn_process` takes the write lock for fork/exec (non-Apple, or relative program) and the read lock for an absolute program on Apple (`:259-272`), only where `SPAWN_INHERITS_PIPES` (`:33-47`, true on macOS). Child pipe ends are therefore never created while another thread spawns. Test in the tree: `concurrent_spawns_do_not_inherit_capture_pipes` (`test_command/imp.rs:~374`).
2. std 1.99.1 on macOS: `pipe()` then `set_cloexec` on both ends, no `pipe2` (`library/std/src/sys/pipe/unix.rs:8-42`, verified). `Command::spawn` tries `posix_spawn` first (`sys/process/unix/unix.rs:73`, `:446-`) with `adddup2` file actions for 0/1/2 (`:700-720`); `POSIX_SPAWN_CLOEXEC_DEFAULT` appears nowhere in std (`grep -rn CLOEXEC_DEFAULT library/std/src`: no hit). Fork/exec fallback creates its own error pipe with `sys::pipe::pipe()` (`unix.rs:81`). std takes only the env lock (`sys::env::env_read_lock`, `unix.rs:93`, `:~770`), no ForkLock; tokio takes none (verified by `grep -n lock tokio-1.53.1/src/process/unix/mod.rs`: none). The only serialisation is nextest's own lock above.
3. How nextest decides LEAK (verified): in the test loop `child.wait()` is one arm of a non-biased `tokio::select!` together with `child_acc.fill_buf()` (`runner/executor.rs:931-935`). After it returns, `detect_fd_leaks` (`:1478-1545`, called `:1075`) creates `tokio::time::sleep(leak_timeout.period)` at `:1487` (so the timer starts when the wait future completed and was polled, not at the kernel exit) and loops a `biased` select (`:1494`): (a) `fill_buf` while stdout/stderr are not both at EOF, (b) the sleep, which returns `Leaked` (`:1501`), (c) requests, (d) `else` when both are done: `NoLeak`. EOF is `fill_buf` returning an empty buffer (`test_command/imp.rs:108-121`). There are no separate reader tasks: reads run inside the same executor task, on tokio's I/O driver (kqueue on macOS), so EOF is seen only when that task is polled after the driver delivered readiness. Consequence (inference, unverified at runtime): LEAK needs one of (i) a process holding the write end, (ii) the executor task or driver not running for the whole 100 ms. In case (ii) `biased` makes `fill_buf` win when both are ready, so the timer wins only if the read returned Pending, that is, the readiness event was not yet delivered. The "[0.2 s]" in the report is the test's own stopwatch, not the wait.
4. Upstream (state read on 2026-10-26; `gh release list`, `gh search issues`):
   - Newest release 0.9.149 (2026-10-20): security fix only (`gh release view cargo-nextest-0.9.149`); 0.9.148 (2026-10-07) changelog line 32 (`site/src/changelog.md`): leak timer no longer restarted by leaked-handle output/signals (#3646). 0.9.145 changelog line 87: capture pipe fix for Apple (#3553).
   - https://github.com/nextest-rs/nextest/pull/3553 merged 2026-09-15: stop concurrent spawns inheriting capture pipes (the lock above).
   - https://github.com/nextest-rs/nextest/issues/1469 open: "lots of tests running in parallel may cause leaky warnings" (Windows, FreeBSD with pipe2, 2026-10 report on Linux); no cause found; maintainer asks for traces, added `biased` and USDT probes (commit dc6a40ed5eec8abd623887073f532d50ca5e54d9). One comment (beeauvin, 2026-10-12) says Linux skips the lock (`SPAWN_INHERITS_PIPES` excludes linux): LLM-written, unverified.
   - https://github.com/rust-lang/rust/issues/95584 open: `Command::spawn` leaks fds on platforms without pipe2 (macOS); suggests `POSIX_SPAWN_CLOEXEC_DEFAULT`; not fixed in 1.99.1.
   - https://github.com/rust-lang/rust/issues/98209 open: Command does not close unused pipe endpoints when it should.
   - https://github.com/rust-lang/rust/issues/147624 open: fds passed to `Command::stdout/err/in` are not closed after dup2.
   - https://github.com/rust-lang/rust/issues/114554 open: `Command::spawn` ETXTBSY race (the same fork-inherits-fd family).
   - https://github.com/tokio-rs/tokio/issues/7399 open: Valgrind reveals more file descriptor leaks (unrelated to child pipes). tokio: no issue about the child pipe window found with `gh search issues` for `leak`, `CLOEXEC`, `pipe fork`, `inherit pipe`.

### Part 2, std-level reproduction (all verified: real runs on mac-pro, macOS, rustc 1.99.1, `rustc -O`, 2,000 spawns per cell)

Spawns = children started; Inherited = children whose probe found an open fd >= 3 (the only things the spawner opens at run time are pipes, inference). `posix` = `Command` with piped stdout/stderr (std uses `posix_spawn`); `fork` = same with an empty `pre_exec` (fork/exec); `nxlock` = nextest's scheme (std pipes under a write lock, spawn under a read lock). "load 0 burners" = the machine as it was (load average 63-96, other Plans' Gates); "4 burners" = 4 `yes > /dev/null` added (load 67-77). The `nxlock` rows ran later, at load 64 (0 burners) and 34 (4 burners, others' load had dropped).

| T | posix, 0 burners | posix, 4 burners | fork, 0 burners | fork, 4 burners | nxlock, 0 | nxlock, 4 |
|---|---|---|---|---|---|---|
| 1  | 2000 / 0  | 2000 / 0 | 2000 / 0 | 2000 / 0 | 2000 / 0 | 2000 / 0 |
| 4  | 2000 / 2  | 2000 / 0 | 2000 / 0 | 2000 / 1 | 2000 / 0 | 2000 / 0 |
| 16 | 2000 / 1  | 2000 / 4 | 2000 / 3 | 2000 / 9 | 2000 / 0 | 2000 / 0 |
| 64 | 2000 / 12 | 2000 / 1 | 2000 / 3 | 2000 / 12 | 2000 / 0 | 2000 / 0 |

(each cell: spawns / inherited). Totals: unlocked posix 24 of 16,000, unlocked fork 28 of 16,000, nxlock 0 of 16,000. A second, uncounted `posix` run at T=16/64 showed 1 and 21 (unlocked, output of a run I restarted). Rate is not proportional to T or burners in these runs (4 burners did not raise it; the machine was already at load 63+): the claim is only that the window exists on this machine and that the lock closes it.

Spawn latency (time inside `spawn()`, ms): posix p50 0.2-1.3, p99 1.4-5.6, max 2.3-340 (one 340 at T=4 with burners). fork p50 2-129, p99 440-936, max 745-1,910. So the fork/exec path stalls for hundreds of ms in `spawn()` at this load, the `posix_spawn` path rarely above 50 ms. This is the only data point for H1b: a child that is mid-spawn for 100 ms+ would hold a sibling's pipe that long; nextest uses `posix_spawn` for an absolute path, whose worst stall was 340 ms (T=4, 4 burners; once in 8,000 spawns).

## Reproduction

Smallest recipe (std only, no nextest): build and run; set `THREADS`/path as in the table. Nothing is left running; `run.sh` kills its burners.

```sh
mkdir -p $(mktemp -d)/rp && cd $_ && cat > main.rs   # paste main.rs below
rustc --edition 2021 -O -o leakrp main.rs
./leakrp spawner 16 2000 posix      # unlocked: expect a few "inherited"
./leakrp spawner 16 2000 nxlock     # nextest scheme: expect inherited=0
./run.sh ./leakrp 4                 # the whole table, 4 burners (PATHS=nxlock for one path)
```

`main.rs` (59 lines):

```rust
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
use std::io::Read;
use std::sync::{Arc, Mutex, RwLock};
static LOCK: RwLock<()> = RwLock::new(());
use std::time::Instant;
extern "C" { fn fcntl(fd: i32, cmd: i32, ...) -> i32; }
const F_GETFD: i32 = 1;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("probe") {
        let open: Vec<String> = (3..256).filter(|&fd| unsafe { fcntl(fd, F_GETFD) } != -1)
            .map(|fd| fd.to_string()).collect();
        println!("{}", open.join(","));
        return;
    }
    // spawner <threads> <total> <posix|fork>
    let (t, total, fork, nx): (usize, usize, bool, bool) = (a[2].parse().unwrap(), a[3].parse().unwrap(), a[4] == "fork", a[4] == "nxlock");
    let exe = std::env::current_exe().unwrap();
    let next = Arc::new(AtomicUsize::new(0));
    let bad = Arc::new(AtomicUsize::new(0));
    let lat = Arc::new(Mutex::new(Vec::<u128>::new()));
    let hs: Vec<_> = (0..t).map(|_| {
        let (exe, next, bad, lat) = (exe.clone(), next.clone(), bad.clone(), lat.clone());
        std::thread::spawn(move || {
            let mut mine = Vec::new();
            while next.fetch_add(1, SeqCst) < total {
                let mut c = Command::new(&exe);
                c.arg("probe");
                if fork { unsafe { c.pre_exec(|| Ok(())); } }
                let (mut rx, mut rx2) = (None, None);
                if nx {
                    // nextest 0.9.148 scheme: pipes under the write lock, spawn under the read lock
                    let g = LOCK.write().unwrap();
                    let ((r1, w1), (r2, w2)) = (std::io::pipe().unwrap(), std::io::pipe().unwrap());
                    drop(g);
                    c.stdout(w1).stderr(w2); rx = Some(r1); rx2 = Some(r2);
                } else { c.stdout(Stdio::piped()).stderr(Stdio::piped()); }
                let _r = nx.then(|| LOCK.read().unwrap());
                let t0 = Instant::now();
                let mut child = c.spawn().unwrap();
                mine.push(t0.elapsed().as_micros());
                drop(_r); drop(c);
                let mut s = String::new();
                if nx { rx.unwrap().read_to_string(&mut s).unwrap(); let mut e = String::new(); rx2.unwrap().read_to_string(&mut e).unwrap(); child.wait().unwrap(); }
                else { let out = child.wait_with_output().unwrap(); s = String::from_utf8_lossy(&out.stdout).to_string(); }
                if !s.trim().is_empty() { bad.fetch_add(1, SeqCst); }
            }
            lat.lock().unwrap().extend(mine);
        })
    }).collect();
    for h in hs { h.join().unwrap(); }
    let mut l = lat.lock().unwrap().clone();
    l.sort();
    println!("T={t} path={} spawns={} inherited={} spawn_ms p50={:.1} p99={:.1} max={:.1}",
        if fork { "fork" } else if nx { "nxlock" } else { "posix" }, l.len(), bad.load(SeqCst),
        l[l.len()/2] as f64/1e3, l[l.len()*99/100] as f64/1e3, *l.last().unwrap() as f64/1e3);
}
```

`run.sh` (8 lines):

```sh
#!/bin/bash
# usage: run.sh <bin> <burners> ; prints one line per (T, path)
bin=$1; nb=$2; pids=()
for i in $(seq 1 $nb); do yes > /dev/null & pids+=($!); done
echo "load: burners=$nb $(uptime | sed 's/.*load/load/')"
for p in ${PATHS:-posix fork}; do for t in 1 4 16 64; do
  timeout 600 $bin spawner $t 2000 $p; done; done
for p in "${pids[@]}"; do kill $p; done; wait 2>/dev/null
```

For the part this Job could not run (the holder at the LEAK moment): run the Gate's nextest (`PATH="$PWD/scripts/tripwire:$PATH" scripts/cargo.sh nextest run --workspace`) at load, and when a test is reported `LEAK` list processes holding its pipe (`lsof` on the victim's pipe, or a dtrace/`proc_info` trace) and the scheduler lag of the executor task; compare with a scratch `--config-file` with a longer `leak-timeout` (diagnostic only).

## What this means for the fix

- Upstream pin: nothing to pin. 0.9.148 already contains the macOS pipe fix (#3553) and the leak-timer fix (#3646); 0.9.149 changes nothing here. Staying on 0.9.148 is right; do not look for a newer nextest to fix this.
- Upstream file (the Lead decides): if a later Job shows a holder that is a sibling's mid-spawn child (H1b), the issue text is: "On macOS, `posix_spawn` copies every fd of the parent, CLOEXEC or not, into the new process until its exec completes. Under heavy load (load average 60 on 14 cores) a spawn can take 100 ms+; the sibling pipe write end is held that long and the sibling test is reported LEAK although it exited. Nextest 0.9.148's read/write lock does not exclude concurrent `posix_spawn`s from one another. A fix would be to serialise spawns (write lock) or give the capture pipes a lifetime that excludes concurrent spawns (create the pipe after the spawn returns / use `POSIX_SPAWN_CLOEXEC_DEFAULT` with explicit file actions)." Unverified until the holder is seen.
- Ours: the std window (H1) is closed by nextest, so nothing in the repo's code can fix it. Rules of the Plan hold (no `leak-timeout` change, no retries). What is ours to do is to find the cause on the machine: H2 (tests that spawn children inheriting stdio and outliving the test: the two long victims), H3a (executor starvation at load 55-65) and H1b are open; each is decided by the holder at the LEAK moment, or by its absence. If it is H3a, then "nothing in this repo can fix it" except running Gates at a lower load (fewer concurrent Gates, or `--test-threads` lower), a cost in Gate time; if H2, the fix is in the victim test (close or null the child's stdio).
- Cost of the options: more Gates at once raises load, which this evidence ties to the failures (all failing Gates ran with 4-5 others, 3 Gates at once produced 0 LEAK); lowering `--test-threads` costs Gate time linearly; a nextest source patch costs a fork to maintain (against the axioms: take the dependency).
