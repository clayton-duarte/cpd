# Exec, exit and EOF latency under load (gc-os)

## Verdict

- **H3c (the OS delays exec/exit/EOF so a pipe is still open 100 ms after exit): KILLED as the cause of the LEAK.** 9,000 real spawns (18 cells x 500, nextest-style pipes and locks, T = 1/8/32 spawners, machine at load 50-60, 0-19% idle): wait()-to-EOF on both pipes had max 0.5 ms; 0 waits followed by a pipe open >100 ms; the 100 ms watcher (`lsof`) never fired (`cells1.err` empty, 0 bytes; detector proven on a child that holds the pipe 1 s). verified.
- **H3a (nextest's own executor/IO task not scheduled within 100 ms): STAYS OPEN, rs-synth decides.** The OS did not stall a thread between `wait4` return and `read` return for more than 0.5 ms in 9,000 samples, so plain thread scheduling at this load does not explain it; a tokio task starved by other tasks is not excluded by this data. verified (what was measured), unverified (the inference).
- **H1: not touched (rs-src killed it). H2 (a spawning test's own child holds the pipe): not tested here**; no holder was found in any cell (the children were `true` and one pure test, which spawn nothing). **H3b: open.**
- **Exec is slow under load, and the cost is large, but it is on the way IN, not on the EOF.** spawn()-to-wait() p50 is 18-36 ms at T=1 and 80-560 ms at T=8/32 (p99 0.5-5 s; 86 of 400 execs >1 s at T=32). It rises with concurrency, not with the directory: APFS, `.noindex` and ramdisk are the same within noise (T=32 real WARM p50 424 / 234 / 564 ms). So Spotlight indexing of `target/` is KILLED as a per-exec cost; the ramdisk does not help. verified.
- **New finding: the FIRST exec of a never-run 35 MB test binary costs ~450-1000 ms, the 2nd ~30 ms** (8 fresh copies per dir: first 440-1002 ms, second 18-110 ms), in all three directories, also 20 s after the copy, so it is not a write-time scan and not Spotlight. `/usr/bin/true` copy: first ~75-580 ms (median ~105), second ~30 ms. Cost once per new binary, not per exec; who does it (Falcon Sensor, XProtect, syspolicyd, AMFI) could NOT be seen (no sudo: no `eslogger`/`fs_usage`). verified (numbers); unverified (the component).
- **CPU share:** the named scanners/indexers use ~85 CPU-s/min of ~700 used (840 = 14 cores x 60): about 10-12%; CyberhavenSystemMonitor 24-35, XprotectService 13-17, mds 7-14, mds_stores 2-14, PanGPS 9-12 (+GlobalProtectExtension 4-5), fseventsd 7-9. Gates' live processes show only ~2 CPU-s/min because tests live <2 s and a `ps` diff cannot see them; the ~550 CPU-s/min not accounted for belongs to short-lived processes (rustc, git, tests) and kernel. The Manager's "scanners make the load" is not supported: the load is mostly the Gates' own short-lived processes. verified (numbers), the attribution of the residual is unverified.

## Evidence

Machine: mac-pro, macOS 26.7.1, arm64, 14 cores. Samples: START 23:17:44, MIDDLE 23:21:11 (during my cells), END 23:26:57 (`sample.py`, pasted below).

| | START 23:17 | MIDDLE 23:21 | END 23:26 |
|---|---|---|---|
| load avg 1/5/15 | 54.9 / 61.1 / 63.7 | 49.3 / 57.2 / 61.6 | 61.4 / 56.2 / 59.4 |
| CPU user/sys/idle | not recorded (top header added later); Manager 23:12: 49/51/0 | 54.4 / 26.2 / 19.5 | 73.2 / 26.8 / 0.0 |
| CyberhavenSystemMonitor (all Cyberhaven, 6 procs) | 35.2 (35.8) | 24.1 (24.5) | 34.7 (35.1) |
| XprotectService | 16.8 | 14.2 | 13.2 |
| mds | 12.4 | 14.1 | 7.5 |
| mds_stores | 7.7 | 14.2 | 2.3 |
| contextstored | 3.7 | 1.3 | 0.4 |
| PanGPS | 11.5 | 10.4 | 9.0 |
| GlobalProtectExtension (`com.paloaltonetworks...`) | 0 (not matched by the first regex) | 5.0 | 4.4 |
| fseventsd | 7.1 | 7.7 | 9.2 |
| syspolicyd | n/a | 3.1 | 3.4 |
| CrowdStrike Falcon sysext (5 procs), endpointsecurityd, xprotectd | 0.0 | 0.0 | 0.0 (their CPU is not in `ps` TIME of a user process: unverified why) |
| Gates: cargo-nextest (35 / 49 / 44 procs) | 2.5 | 2.1 | 1.9 |
| Gates: test binaries alive at both snapshots | 0.0 | 0.0 | 0.0 |
| sum of ALL processes alive at both snapshots | n/a | 144.7 | 123.1 |

Unit: CPU-seconds per minute, difference of the `ps` `time` column over 60 s. Not seen: processes that exited between snapshots (tests, rustc, git) and kernel work. `kernel_task` TIME in the `top` rows: 20:01:11 (23:17) -> 20:02:19 (23:21) -> 20:04:18 (23:27) = ~187 s in 9.2 min = ~20 CPU-s/min. `top` second sample: kernel_task 22-34%, PanGPS 12-22%, XprotectService 39% in one instant (MIDDLE), a rustc 43-133%, `mds` 4-52% (bursty).
Memory: free 69% in all three (`memory_pressure`), `vm.swapusage` 0 (no swap), pages free ~45-70k x 16 KB (0.7-1.1 GB), pageouts 475,432 -> 476,264 -> 501,505 (25k pages in 9 min: some pressure). `mdutil -s /System/Volumes/Data`: Indexing enabled (all three). System extensions (names): GlobalProtectExtension, Tailscale Network Extension, Falcon Sensor (CrowdStrike; an older 7.40 one is terminated), VMware Workspace ONE System Extension [waiting for user].
Spotlight and `target/`: `mdfind -onlyin ~/projects -name libcpd_core` returns `.../cpd.italic--open-expand-a1/target/debug/libcpd_core.d`: files under `target/` ARE in the index (verified). `-name cpd_core-` found none and a probe file in my temp dir was not found after 20 s (the name match is by token; unverified why). xattrs of the executed binary and its dir: `com.apple.provenance` only (no `com.apple.quarantine`); `target/` has `com.apple.fileprovider.ignore#P` and `com.apple.metadata:com_apple_backup_excludeItem`, no `.metadata_never_index`.

### Step 2 table (`run.sh`, 500 spawns per cell; rows report FRESH = execs 0-19 and WARM = execs >=100 of the same copy)

Each cell is a new copy of the binary. Columns: (a) spawn()->wait4 return, (b) wait4 return->EOF on stdout and stderr, (c) the child's user+sys from `wait4` rusage; all as p50/p90/p99/max in ms; the last two: count of (b) > 100 ms, count of (a) > 1 s. Children: `true` = a copy of /usr/bin/true; `real` = a copy of `cpd_core-63a596a602020fb0` (35.8 MB debug test binary) run as `--exact policy::exposure::tests::the_six_text_domains_are_exposed_to_every_scope --nocapture`. At load 50-60 (see samples). Row n counts only the reported phases (the 80 execs 20-99 ran but are not reported): 9,000 spawns in all.

| cell | phase | n | (a) spawn->wait p50/p90/p99/max ms | (b) wait->EOF p50/p90/p99/max ms | (c) child user+sys ms | (b)>100ms | (a)>1s |
| apfs/true/T1 | FRESH | 20 | 13.6/22.7/201.2/201.2 | 0.0/0.0/0.0/0.0 | 1.5/2.1/2.4/2.4 | 0 | 0 |
| apfs/true/T1 | WARM | 400 | 17.9/37.9/160.7/310.5 | 0.0/0.0/0.0/0.0 | 1.9/2.5/2.9/3.0 | 0 | 0 |
| apfs/true/T8 | FRESH | 20 | 172.6/348.2/537.3/537.3 | 0.0/0.0/0.0/0.0 | 1.8/2.2/2.2/2.2 | 0 | 0 |
| apfs/true/T8 | WARM | 400 | 79.1/276.6/499.7/537.6 | 0.0/0.0/0.0/0.0 | 1.8/2.4/2.8/3.1 | 0 | 0 |
| apfs/true/T32 | FRESH | 20 | 424.0/1455.4/2618.6/2618.6 | 0.0/0.0/0.0/0.0 | 2.0/2.4/3.3/3.3 | 0 | 5 |
| apfs/true/T32 | WARM | 400 | 381.9/1656.5/3580.8/4330.0 | 0.0/0.0/0.0/0.0 | 1.9/2.6/3.0/3.3 | 0 | 86 |
| apfs/real/T1 | FRESH | 20 | 29.3/123.5/492.7/492.7 | 0.0/0.0/0.0/0.0 | 4.0/4.8/5.6/5.6 | 0 | 0 |
| apfs/real/T1 | WARM | 400 | 36.4/83.0/189.0/314.4 | 0.0/0.0/0.1/0.3 | 3.6/4.4/5.3/5.3 | 0 | 0 |
| apfs/real/T8 | FRESH | 20 | 349.7/959.5/1501.8/1501.8 | 0.0/0.0/0.0/0.0 | 3.1/4.2/4.6/4.6 | 0 | 2 |
| apfs/real/T8 | WARM | 400 | 157.7/506.8/1086.0/1753.8 | 0.0/0.0/0.0/0.1 | 2.9/3.5/4.1/4.7 | 0 | 5 |
| apfs/real/T32 | FRESH | 20 | 1012.1/2316.5/3731.5/3731.5 | 0.0/0.0/0.0/0.0 | 3.5/4.5/5.1/5.1 | 0 | 10 |
| apfs/real/T32 | WARM | 400 | 423.4/1287.0/3256.4/4155.0 | 0.0/0.0/0.0/0.0 | 3.8/4.4/4.9/5.7 | 0 | 71 |
| noindex/true/T1 | FRESH | 20 | 17.7/32.9/112.1/112.1 | 0.0/0.0/0.0/0.0 | 1.4/1.7/1.8/1.8 | 0 | 0 |
| noindex/true/T1 | WARM | 400 | 20.7/29.0/44.4/65.3 | 0.0/0.0/0.0/0.0 | 1.5/1.8/2.3/2.5 | 0 | 0 |
| noindex/true/T8 | FRESH | 20 | 116.3/273.1/669.1/669.1 | 0.0/0.0/0.0/0.0 | 1.5/2.0/2.6/2.6 | 0 | 0 |
| noindex/true/T8 | WARM | 400 | 97.5/316.0/620.8/1045.4 | 0.0/0.0/0.0/0.0 | 1.5/1.8/2.3/2.7 | 0 | 1 |
| noindex/true/T32 | FRESH | 20 | 455.6/1342.0/2699.5/2699.5 | 0.0/0.0/0.0/0.0 | 1.7/2.2/2.9/2.9 | 0 | 5 |
| noindex/true/T32 | WARM | 400 | 293.8/1084.5/2388.7/3183.3 | 0.0/0.0/0.0/0.1 | 1.7/2.1/2.7/3.3 | 0 | 49 |
| noindex/real/T1 | FRESH | 20 | 17.7/19.3/444.1/444.1 | 0.0/0.0/0.0/0.0 | 2.7/3.0/4.3/4.3 | 0 | 0 |
| noindex/real/T1 | WARM | 400 | 22.9/39.2/108.7/163.0 | 0.0/0.0/0.0/0.3 | 3.3/4.2/4.9/5.5 | 0 | 0 |
| noindex/real/T8 | FRESH | 20 | 684.7/1271.7/1940.7/1940.7 | 0.0/0.0/0.1/0.1 | 4.5/6.8/7.5/7.5 | 0 | 6 |
| noindex/real/T8 | WARM | 400 | 90.9/270.9/481.5/799.9 | 0.0/0.0/0.0/0.0 | 3.0/3.7/4.6/5.0 | 0 | 0 |
| noindex/real/T32 | FRESH | 20 | 762.3/1658.4/2082.7/2082.7 | 0.0/0.0/0.0/0.0 | 3.4/3.5/3.9/3.9 | 0 | 6 |
| noindex/real/T32 | WARM | 400 | 233.6/913.7/1527.9/2131.5 | 0.0/0.0/0.0/0.3 | 3.6/4.5/5.2/5.6 | 0 | 30 |
| ramdisk/true/T1 | FRESH | 20 | 28.5/65.7/94.3/94.3 | 0.0/0.0/0.0/0.0 | 2.1/2.5/2.8/2.8 | 0 | 0 |
| ramdisk/true/T1 | WARM | 400 | 23.5/81.5/199.9/432.5 | 0.0/0.0/0.0/0.5 | 2.0/2.5/3.0/3.2 | 0 | 0 |
| ramdisk/true/T8 | FRESH | 20 | 264.0/884.7/1662.6/1662.6 | 0.0/0.0/0.0/0.0 | 1.9/2.5/2.7/2.7 | 0 | 2 |
| ramdisk/true/T8 | WARM | 400 | 170.4/571.3/2446.5/3089.2 | 0.0/0.0/0.0/0.1 | 1.6/2.1/2.7/3.0 | 0 | 18 |
| ramdisk/true/T32 | FRESH | 20 | 493.9/1793.3/2438.2/2438.2 | 0.0/0.0/0.0/0.0 | 1.7/2.1/2.4/2.4 | 0 | 6 |
| ramdisk/true/T32 | WARM | 400 | 420.4/1578.1/3467.7/4631.6 | 0.0/0.0/0.0/0.1 | 2.0/2.6/3.2/3.5 | 0 | 78 |
| ramdisk/real/T1 | FRESH | 20 | 52.7/88.9/700.3/700.3 | 0.0/0.0/0.0/0.0 | 3.9/4.5/5.7/5.7 | 0 | 0 |
| ramdisk/real/T1 | WARM | 400 | 31.3/92.7/149.5/172.1 | 0.0/0.0/0.0/0.1 | 3.5/4.4/5.2/5.6 | 0 | 0 |
| ramdisk/real/T8 | FRESH | 20 | 284.5/731.1/1141.9/1141.9 | 0.0/0.0/0.0/0.0 | 3.1/3.5/4.5/4.5 | 0 | 1 |
| ramdisk/real/T8 | WARM | 400 | 136.3/436.0/1444.9/3637.1 | 0.0/0.0/0.0/0.3 | 3.6/4.2/5.0/5.7 | 0 | 8 |
| ramdisk/real/T32 | FRESH | 20 | 1960.7/4423.3/5292.9/5292.9 | 0.0/0.0/0.0/0.0 | 4.1/4.5/5.2/5.2 | 0 | 16 |
| ramdisk/real/T32 | WARM | 400 | 563.7/1752.3/2911.0/4785.0 | 0.0/0.0/0.0/0.1 | 3.6/4.3/5.1/5.6 | 0 | 119 |

### First exec of a fresh copy versus the second (T=1, 8 fresh copies per row, (a) in ms)

apfs true first: 104.1 106.4 102.1 82.5 353.6 159.9 214.4 228.1 | second: 34.2 19.3 65.1 28.5 121.7 25.4 27.3 32.9
apfs real first: 604.9 553.0 499.6 645.3 480.8 462.2 473.2 490.8 | second: 17.9 42.5 35.8 42.8 37.3 29.2 31.6 25.5
noindex true first: 75.5 111.6 113.7 98.9 255.7 123.4 85.0 222.3 | second: 25.0 74.8 26.5 30.1 29.8 19.7 32.9 17.2
noindex real first: 455.7 537.9 845.1 608.0 475.5 577.5 489.0 468.5 | second: 38.5 96.8 69.3 29.3 105.4 42.9 48.9 20.8
ramdisk true first: 88.5 90.6 581.8 93.0 169.4 115.7 161.3 135.5 | second: 23.1 20.2 20.1 17.1 39.9 77.5 49.7 110.8
ramdisk real first: 891.9 575.0 1002.4 600.1 550.0 466.5 472.6 482.2 | second: 80.9 109.8 44.5 49.4 26.2 29.3 24.9 20.6

Same test 20 s after `cp` (apfs, 4 copies, real): first exec 452.0 / 456.2 / 442.8 / 440.2 ms. The original binary (already run many times): 31.7 / 15.0 / 19.8 ms.

Detector check (verified): `./spawnlat detector-check 1 2 /bin/sh -c 'sleep 1 & exit 0'` printed `OPEN>100ms` and `LEAKWINDOW ... b=1667.9ms` and `b=1179.4ms`, so the 100 ms watcher works. Its holder search (a system-wide `lsof -nP` filtered on the pipe address) takes >1 s and listed only my own program's read end, because the `sleep` was already gone: the holder search is weak; it never fired in the real cells, so it did not matter here.

What I could not see: which component takes the ~450 ms of the first exec; who owns CrowdStrike/XProtect work per exec (no `eslogger`, `fs_usage`, `dtruss`, `spindump`: sudo asks for a password); the CPU of kernel and of short-lived processes per product. The cells run in my own program with plain threads, not tokio: they test the OS, not nextest's executor.

## Reproduction

Needs a debug test binary (any `target/debug/deps/cpd_core-<hash>`). `D` = a `mktemp -d` outside the repo; save the three files below in it; build with `~/.cargo/bin/rustc -O spawnlat.rs`; run `./run.sh <binary> 500 > cells.txt 2> cells.err` (about 8 min at this load; `cells.err` non-empty means a pipe was open >100 ms); `./sample.py START` for step 1. Remove `D` and the `/private/tmp/cpdram/XXXX` directory afterwards (`run.sh` removes its own).

`spawnlat.rs`
```rust
// usage: spawnlat LABEL T N CMD [ARGS..]   (prints one row per phase: FRESH first 20 execs, WARM execs >=100)
use std::io::{pipe, Read};
use std::process::Command;
use std::sync::{atomic::{AtomicUsize, Ordering::SeqCst}, Arc, Mutex, RwLock};
use std::time::Instant;
extern "C" { fn wait4(pid: i32, st: *mut i32, opt: i32, ru: *mut [i64; 18]) -> i32; }
static LOCK: RwLock<()> = RwLock::new(());
struct S { idx: usize, a: f64, b: f64, c: f64 }
fn pct(v: &mut Vec<f64>, p: f64) -> f64 { if v.is_empty() { return f64::NAN } v.sort_by(|x, y| x.partial_cmp(y).unwrap()); v[((v.len() - 1) as f64 * p).round() as usize] }
fn holder(label: &str, th: usize, pid: i32, fd: i32) {
    let me = std::process::id();
    let o = Command::new("lsof").args(["-nP", "-a", "-p", &me.to_string(), "-d", &fd.to_string()]).output().unwrap();
    let t = String::from_utf8_lossy(&o.stdout).to_string(); eprintln!("OPEN>100ms {label} thread={th} child={pid} fd={fd}: {}", t.lines().last().unwrap_or(""));
    let addr = t.split_whitespace().filter(|w| w.starts_with("0x")).next().unwrap_or("NOADDR").to_string();
    let all = Command::new("lsof").args(["-nP"]).output().unwrap(); for l in String::from_utf8_lossy(&all.stdout).lines().filter(|l| l.contains(&addr)) { eprintln!("  holder?: {l}") }
    let ps = Command::new("ps").args(["-o", "pid,ppid,stat,etime,command", "-p", &pid.to_string()]).output().unwrap(); eprintln!("  child ps: {}", String::from_utf8_lossy(&ps.stdout).lines().last().unwrap_or("gone"));
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (label, t, n) = (a[1].clone(), a[2].parse::<usize>().unwrap(), a[3].parse::<usize>().unwrap());
    let cmd = Arc::new(a[4..].to_vec()); let label = Arc::new(label); let slots: Arc<Mutex<Vec<Option<(Instant, i32, i32, bool)>>>> = Arc::new(Mutex::new(vec![None; t]));
    { let slots = slots.clone(); let label = label.clone(); std::thread::spawn(move || loop { std::thread::sleep(std::time::Duration::from_millis(10)); for (i, s) in slots.lock().unwrap().iter_mut().enumerate() { if let Some((t1, pid, fd, done)) = s { if !*done && t1.elapsed().as_millis() > 100 { *done = true; holder(&label, i, *pid, *fd) } } } }); }
    let next = Arc::new(AtomicUsize::new(0));
    let out = Arc::new(Mutex::new(Vec::<S>::new()));
    let hs: Vec<_> = (0..t).map(|ti| { let (cmd, next, out, slots, label) = (cmd.clone(), next.clone(), out.clone(), slots.clone(), label.clone()); std::thread::spawn(move || loop {
        let idx = next.fetch_add(1, SeqCst); if idx >= n { break }
        let (mut or, ow, mut er, ew) = { let _w = LOCK.write().unwrap(); let (or, ow) = pipe().unwrap(); let (er, ew) = pipe().unwrap(); (or, ow, er, ew) };
        let mut c = Command::new(&cmd[0]); c.args(&cmd[1..]).stdout(ow).stderr(ew);
        let t0 = Instant::now();
        let child = { let _r = LOCK.read().unwrap(); c.spawn().unwrap() };
        drop(c);
        let pid = child.id() as i32;
        let (mut st, mut ru) = (0i32, [0i64; 18]);
        unsafe { wait4(pid, &mut st, 0, &mut ru) };
        let t1 = Instant::now();
        use std::os::fd::AsRawFd; slots.lock().unwrap()[ti] = Some((t1, pid, or.as_raw_fd(), false));
        let mut buf = Vec::new(); or.read_to_end(&mut buf).unwrap(); er.read_to_end(&mut buf).unwrap();
        let t2 = Instant::now(); slots.lock().unwrap()[ti] = None;
        let b = (t2 - t1).as_secs_f64() * 1e3;
        if b > 100.0 { eprintln!("LEAKWINDOW {label} idx={idx} pid={pid} b={b:.1}ms"); }
        let tv = |s: i64, u: i64| s as f64 * 1e3 + (u as i32) as f64 / 1e3;
        out.lock().unwrap().push(S { idx, a: (t1 - t0).as_secs_f64() * 1e3, b, c: tv(ru[0], ru[1]) + tv(ru[2], ru[3]) });
    }) }).collect();
    for h in hs { h.join().unwrap() }
    let out = out.lock().unwrap();
    for (ph, f) in [("FRESH", (|i: usize| i < 20) as fn(usize) -> bool), ("WARM", |i| i >= 100)] {
        let s: Vec<&S> = out.iter().filter(|s| f(s.idx)).collect();
        let col = |g: fn(&S) -> f64| s.iter().map(|x| g(x)).collect::<Vec<f64>>();
        let (mut av, mut bv, mut cv) = (col(|x| x.a), col(|x| x.b), col(|x| x.c));
        let fmt = |v: &mut Vec<f64>| format!("{:.1}/{:.1}/{:.1}/{:.1}", pct(v, 0.5), pct(v, 0.9), pct(v, 0.99), pct(v, 1.0));
        println!("| {label} | {ph} | {} | {} | {} | {} | {} | {} |", s.len(), fmt(&mut av), fmt(&mut bv), fmt(&mut cv), bv.iter().filter(|x| **x > 100.0).count(), av.iter().filter(|x| **x > 1000.0).count());
    }
}
```

`run.sh`
```bash
#!/bin/bash
# usage: run.sh REALBIN [N]   (cells: 3 dirs x {true, real} x T in 1 8 32; fresh copy per cell; rows on stdout, LEAKWINDOW/holder lines on stderr)
REAL=$1; N=${2:-500}; W=$(dirname "$0")
RAM=$(mktemp -d /private/tmp/cpdram/XXXX); mkdir -p "$W/apfs" "$W/x.noindex"
NAME=policy::exposure::tests::the_six_text_domains_are_exposed_to_every_scope
echo "| cell | phase | n | (a) spawn->wait p50/p90/p99/max ms | (b) wait->EOF p50/p90/p99/max ms | (c) child user+sys ms | (b)>100ms | (a)>1s |"
for D in "$W/apfs" "$W/x.noindex" "$RAM"; do for K in true real; do for T in 1 8 32; do
  case $D in */apfs) L=apfs;; */x.noindex) L=noindex;; *) L=ramdisk;; esac
  C=$D/$K.$T.$$; if [ $K = true ]; then cp /usr/bin/true "$C"; A=(); else cp "$REAL" "$C"; A=(--exact $NAME --nocapture); fi
  timeout 600 "$W/spawnlat" "$L/$K/T$T" $T $N "$C" "${A[@]}"; rm -f "$C"
done; done; done
rm -rf "$RAM"
```

`sample.py`
```python
#!/usr/bin/env python3
# usage: sample.py LABEL  -> machine sample (step 1) : uptime, top, CPU-s/min per product (two ps snapshots 60 s apart), vm, mdutil, mdfind, xattr
import subprocess as sp, re, time, sys, collections, os
def sh(c): return sp.run(c, shell=True, capture_output=True, text=True).stdout
def snap():
    d = {}
    for l in sh("ps -Ao pid,time,command").splitlines()[1:]:
        m = re.match(r"\s*(\d+)\s+(\d+):([\d.]+)\s+(.*)", l)
        if m: d[int(m[1])] = (int(m[2]) * 60 + float(m[3]), m[4])
    return d
PRODUCTS = [("CyberhavenSystemMonitor", r"CyberhavenSystemMonitor"), ("Cyberhaven (all)", r"/Cyberhaven"), ("XprotectService", r"XprotectService"),
 ("xprotectd", r"/xprotectd"), ("mds", r"/mds$|/mds "), ("mds_stores", r"mds_stores"), ("contextstored", r"contextstored"), ("PanGPS", r"/PanGPS"),
 ("GlobalProtectExtension", r"com\.paloaltonetworks"), ("fseventsd", r"fseventsd"), ("CrowdStrike falcon", r"falcon|Falcon|com\.crowdstrike"), ("endpointsecurityd", r"endpointsecurityd"),
 ("GATES: cargo-nextest", r"cargo-nextest"), ("GATES: test binaries (target/*/deps/)", r"target/(debug|release)/deps/"), ("kernel_task (pid 0)", r"kernel_task"), ("syspolicyd", r"syspolicyd")]
print("##", sys.argv[1], time.strftime("%H:%M:%S")); print(sh("uptime").strip())
print(sh("top -l 2 -n 25 -o cpu -stats pid,command,cpu,time > /tmp/gcos.top; grep '^CPU usage' /tmp/gcos.top | tail -1; awk '/^PID/{n++} n==2' /tmp/gcos.top | head -28; rm -f /tmp/gcos.top"))
a = snap(); t0 = time.time(); time.sleep(60); b = snap(); dt = time.time() - t0
print(f"CPU-seconds per minute (two ps snapshots {dt:.0f}s apart, time column difference, normalised to 60 s; 14 cores = 840 CPU-s/min max)")
tot_all = 0
for name, rx in PRODUCTS:
    s = sum(max(0, b[p][0] - a[p][0]) for p in b if p in a and re.search(rx, b[p][1])) * 60 / dt
    n = sum(1 for p in b if re.search(rx, b[p][1]))
    print(f"  {name:42s} {s:8.1f} CPU-s/min   ({n} procs)")
allsum = sum(max(0, b[p][0] - a[p][0]) for p in b if p in a) * 60 / dt
print(f"  sum over ALL processes alive at both snapshots: {allsum:.1f} CPU-s/min (processes that exited in between are invisible to this method: tests live ~0.1-2 s; kernel_task if shown)")
print(sh("vm_stat | head -12")); print(sh("memory_pressure | tail -3")); print(sh("sysctl vm.swapusage")); print(sh("mdutil -s /System/Volumes/Data"))
print(sh("systemextensionsctl list | grep -E 'enabled' | awk -F'\t' '{print $5, $6}'"))
```

## What this means for the fix

- **A longer `leak-timeout`, retries, groups: not options (Plan rules).** And this data does not call for them: the OS never held a pipe past 100 ms in 9,000 spawns.
- **Spotlight / `.noindex` / ramdisk for `target/`: no measured gain.** `.noindex` and the ramdisk gave the same exec latency as APFS within noise (cost: none to apply, none to gain). Do not do it for the LEAK. A `target.noindex` or `CARGO_TARGET_DIR` move would only reduce `mds`/`mds_stores`/`fseventsd` work (~25 CPU-s/min together, ~3% of the machine): unmeasured value, cheap to apply, ours to apply.
- **First-exec cost of ~0.5 s per new test binary** (also on the ramdisk): it is paid once per rebuilt binary per Gate, with ~5,100 tests in a few dozen binaries it is ~tens of seconds of exec delay spread over the run, not a pipe defect. A warm-up exec of each new binary (`<bin> --list`) before the nextest run would move that cost out of the timed tests: cheap and ours to apply, but it only helps if gc-cost/rs-synth show a link to the LEAK; I did not show one. unverified benefit.
- **Exec latency grows with concurrency (T=8: p50 80-350 ms; T=32: 230-1960 ms; max 5.3 s)**: this is CPU contention at load 50-60 with 0% idle (the Gates' own short-lived processes: ~550 of ~700 CPU-s/min). What is ours: fewer parallel test processes per Gate (a nextest `-j`/test-threads cap or slots) lowers it, but the User chose no cap (pro-d30), so this stays a trade-off for him, with the number: p50 spawn-to-exit goes from ~20-36 ms (T=1) to 230-560 ms (T=32).
- **Not ours (the User's endpoint products):** the first-exec cost and the per-process CPU of Cyberhaven (24-35 CPU-s/min), XProtect (13-17), PanGPS+extension (14-16). Ask the owner of the Falcon/Cyberhaven/GlobalProtect policy for an exclusion of the repos' `target/` directories (or of the worktree root) and for the CPU of each per exec; the numbers to give: first exec of a 35 MB binary ~450-1000 ms vs ~30 ms, ~85 CPU-s/min of 700 used. They did not cause the LEAK in these measurements.
- **Next step for the LEAK is H3a** (rs-synth: nextest's executor starved by its own tasks while hundreds of children run), not the OS.
