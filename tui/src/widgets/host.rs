//! What the Dispatch column reads of the machine (commands.md §13.2, §17): CPU %, one utilization % per GPU, and memory %. The
//! parsers are pure, on the text of `/proc/stat`, `/proc/meminfo`, `nvidia-smi` (Linux), `ps`, `vm_stat` and `ioreg` (macOS);
//! [`Sampler`] reads the real thing; on Linux it reads `/proc` through its [`ProcReader`], which a test scripts.
//!
//! The macOS GPU parse ([`parse_ioreg`]) is UNVERIFIED on hardware: it is written from the documented shape of `ioreg -r -d 1
//! -w 0 -c IOAccelerator` and tested on a captured sample only.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::sync::{Arc, Mutex};

use cpd_core::host::runner::{Call, NO_DEADLINE, Runner};
use cpd_core::host::spawn::{Launch, Pid, Signal, Spawn};
use cpd_core::ports::Ports;

/// CPU %, GPU % and memory %, whole numbers; `None` is drawn as `–`. `cpu` is `None` until a second CPU sample exists, and any
/// is `None` when it cannot be read: like the hostname, that is no error of the view (tui-W10). `gpus` has one entry per GPU,
/// in the tool's order; empty means no GPU was found (no tool, or no GPU in its output), and the row has no GPU part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    pub cpu: Option<u8>,
    pub gpus: Vec<Option<u8>>,
    pub mem: Option<u8>,
}

/// The counters of `/proc/stat`'s `cpu` line: time not idle (idle + iowait) and all time, in clock ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTimes {
    idle: u64,
    total: u64,
}

/// Reads [`Usage`]; it holds the previous CPU sample, which Linux needs for the share over an interval, and on Linux the
/// one `nvidia-smi` that reports the GPUs ([`Gpus`]), started at the first sample.
pub struct Sampler {
    previous: Option<CpuTimes>,
    gpus: Option<Gpus>,
    proc: ProcReader,
}

/// How Linux reads `/proc/stat` and `/proc/meminfo`: the text of the file at the path, `None` when it cannot be read. A file
/// read, never a process (no child counts against the view's idle I/O budget). Production reads the disk ([`read_file`]); a test
/// scripts it, and what it does not script is unreadable ([`unreadable`]).
pub type ProcReader = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// The real [`ProcReader`].
pub fn real_proc() -> ProcReader {
    Arc::new(read_file)
}

/// The [`ProcReader`] that reads nothing: every number from `/proc` is `–`.
pub fn unreadable() -> ProcReader {
    Arc::new(|_| None)
}

impl std::fmt::Debug for Sampler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sampler")
            .field("previous", &self.previous)
            .field("gpus", &self.gpus)
            .finish_non_exhaustive()
    }
}

impl Sampler {
    /// A sampler that reads `/proc` through `proc`.
    pub fn new(proc: ProcReader) -> Sampler {
        Sampler {
            previous: None,
            gpus: None,
            proc,
        }
    }

    /// The usage now; on Linux the GPU tool is started through `ports.spawn`.
    pub fn sample_with(&mut self, ports: &Ports) -> Usage {
        if cfg!(target_os = "macos") {
            macos_usage_with(&*ports.runner)
        } else {
            let times = (self.proc)("/proc/stat").and_then(|text| parse_proc_stat(&text));
            let cpu = self
                .previous
                .zip(times)
                .and_then(|(before, after)| cpu_share(before, after));
            if times.is_some() {
                self.previous = times;
            }
            let gpus = self
                .gpus
                .get_or_insert_with(|| Gpus::start(&ports.spawn))
                .now();
            Usage {
                cpu,
                gpus,
                mem: (self.proc)("/proc/meminfo").and_then(|text| parse_meminfo(&text)),
            }
        }
    }
}

fn read_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

/// `program`'s stdout, run on `runner`, when it ran and exited 0.
pub(crate) fn run_with(runner: &dyn Runner, program: &str, args: &[&str]) -> Option<String> {
    let call = Call {
        tool: program.to_owned(),
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: Vec::new(),
        cwd: None,
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    let out = runner.run(&call).ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The macOS [`Usage`] (`ioreg`, `sysctl`, `ps`, `vm_stat`) read through `runner`.
pub(crate) fn macos_usage_with(runner: &dyn Runner) -> Usage {
    Usage {
        cpu: macos_cpu(runner),
        gpus: macos_gpus(runner),
        mem: macos_mem(runner),
    }
}

/// The GPUs' utilization on Linux, from **one** `nvidia-smi` that prints it every 2 s for as long as the view runs (`-lms`).
/// A tool spawned and reaped at every sample would count its reads and writes against the view's idle budget (the kernel
/// adds a reaped child's I/O to its parent's: tui-F1, tui-F2, `cli/tests/it/tui_idle.rs`). A missing tool, or one that fails
/// or stops, is no GPU: an empty list, never an error. When the view dies without dropping it, the tool's next write finds
/// no reader and it ends (SIGPIPE).
struct Gpus {
    /// The latest % of each GPU, by index.
    latest: Arc<Mutex<Vec<Option<u8>>>>,
    /// The tool's process, and the `Spawn` that started it and kills it when this is dropped.
    child: Option<(Pid, Arc<dyn Spawn>)>,
}

impl std::fmt::Debug for Gpus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gpus")
            .field("latest", &self.latest)
            .field("pid", &self.child.as_ref().map(|c| c.0))
            .finish()
    }
}

impl Gpus {
    /// The tool is streamed through `spawn`: stdin from `/dev/null`, stdout piped to the reader, stderr discarded.
    fn start(spawn: &Arc<dyn Spawn>) -> Gpus {
        let latest = Arc::new(Mutex::new(Vec::new()));
        let launch = Launch {
            program: "nvidia-smi".into(),
            args: [
                "--query-gpu=index,utilization.gpu",
                "--format=csv,noheader,nounits",
                "-lms",
                "2000",
            ]
            .map(String::from)
            .to_vec(),
            ..Launch::default()
        };
        let streamed = spawn.stream(&launch).ok();
        let (child, out) = match streamed {
            Some((pid, out)) => (Some((pid, Arc::clone(spawn))), Some(out)),
            None => (None, None),
        };
        if let Some(out) = out {
            let latest = Arc::clone(&latest);
            std::thread::spawn(move || {
                let mut by_index = BTreeMap::new();
                for line in out.lines() {
                    let Ok(line) = line else { break };
                    if let Some((index, value)) = parse_nvidia_smi_line(&line) {
                        by_index.insert(index, value);
                        if let Ok(mut l) = latest.lock() {
                            *l = by_index.values().copied().collect();
                        }
                    }
                }
                // the tool stopped: no GPU from now on
                if let Ok(mut l) = latest.lock() {
                    l.clear();
                }
            });
        }
        Gpus { latest, child }
    }

    fn now(&self) -> Vec<Option<u8>> {
        self.latest.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

impl Drop for Gpus {
    fn drop(&mut self) {
        if let Some((pid, spawn)) = self.child.take() {
            // the reaper of `Spawn::stream` waits for it
            let _ = spawn.signal(pid, Signal::Kill);
        }
    }
}

fn macos_gpus(runner: &dyn Runner) -> Vec<Option<u8>> {
    run_with(
        runner,
        "ioreg",
        &["-r", "-d", "1", "-w", "0", "-c", "IOAccelerator"],
    )
    .map_or_else(Vec::new, |t| parse_ioreg(&t))
}

fn macos_cpu(runner: &dyn Runner) -> Option<u8> {
    let ncpu = run_with(runner, "sysctl", &["-n", "hw.ncpu"])?
        .trim()
        .parse()
        .ok()?;
    parse_ps(&run_with(runner, "ps", &["-A", "-o", "%cpu="])?, ncpu)
}

fn macos_mem(runner: &dyn Runner) -> Option<u8> {
    let bytes = run_with(runner, "sysctl", &["-n", "hw.memsize"])?
        .trim()
        .parse()
        .ok()?;
    parse_vm_stat(&run_with(runner, "vm_stat", &[])?, bytes)
}

/// `part` of `whole` as a whole percent, rounded, at most 100; `None` for an empty whole.
fn percent(part: u64, whole: u64) -> Option<u8> {
    (whole > 0).then(|| {
        u8::try_from((u128::from(part) * 100 + u128::from(whole) / 2) / u128::from(whole))
            .unwrap_or(100)
            .min(100)
    })
}

/// The `cpu` line (all CPUs) of `/proc/stat`: `cpu user nice system idle iowait irq softirq steal …`; the first eight counters
/// are the total (guest time is already in user), idle is idle + iowait.
pub fn parse_proc_stat(text: &str) -> Option<CpuTimes> {
    let line = text
        .lines()
        .find(|l| l.split_whitespace().next() == Some("cpu"))?;
    let n: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .take(8)
        .map(|f| f.parse().ok())
        .collect::<Option<_>>()?;
    (n.len() >= 5).then(|| CpuTimes {
        idle: n[3] + n[4],
        total: n.iter().sum(),
    })
}

/// The share of non-idle time between two samples; `None` when no time passed.
pub fn cpu_share(before: CpuTimes, after: CpuTimes) -> Option<u8> {
    let total = after.total.checked_sub(before.total)?;
    let idle = after.idle.checked_sub(before.idle)?.min(total);
    percent(total - idle, total)
}

/// Used / total of `/proc/meminfo`, used being `MemTotal - MemAvailable`.
pub fn parse_meminfo(text: &str) -> Option<u8> {
    let kb = |key: &str| -> Option<u64> {
        let line = text
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
        line.split_whitespace().next()?.parse().ok()
    };
    let (total, available) = (kb("MemTotal")?, kb("MemAvailable")?);
    percent(total.saturating_sub(available), total)
}

/// The sum of `ps -A -o %cpu=`'s lines divided by the number of CPUs, at most 100.
pub fn parse_ps(text: &str, ncpu: u32) -> Option<u8> {
    let sum = text
        .split_whitespace()
        .map(|f| f.parse::<f64>().ok())
        .sum::<Option<f64>>()?;
    (ncpu > 0).then(|| (sum / f64::from(ncpu)).round().clamp(0.0, 100.0) as u8)
}

/// macOS memory: `vm_stat`'s pages active + wired down + occupied by compressor, times the page size on its first line, over the
/// machine's `memsize` in bytes.
pub fn parse_vm_stat(text: &str, memsize: u64) -> Option<u8> {
    let page_size: u64 = text
        .lines()
        .next()?
        .split("page size of ")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let pages = |key: &str| -> Option<u64> {
        let rest = text
            .lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix(':'))?;
        rest.trim().trim_end_matches('.').parse().ok()
    };
    let used = pages("Pages active")?
        + pages("Pages wired down")?
        + pages("Pages occupied by compressor")?;
    percent(used * page_size, memsize)
}

/// A line of `nvidia-smi --query-gpu=index,utilization.gpu --format=csv,noheader,nounits`: `<index>, <percent>`. A percent that
/// is no number from 0 to 100 (`[N/A]`) is `None` for that GPU; a line with no index is no GPU.
pub fn parse_nvidia_smi_line(line: &str) -> Option<(u32, Option<u8>)> {
    let (index, value) = line.split_once(',')?;
    Some((
        index.trim().parse().ok()?,
        value.trim().parse::<u8>().ok().filter(|v| *v <= 100),
    ))
}

/// `ioreg -r -d 1 -w 0 -c IOAccelerator`: each accelerator starts at a line `+-o <name>` in column 0; its `"PerformanceStatistics"`
/// dictionary holds `"Device Utilization %"=N`. An accelerator without that number is `None`. Unverified on hardware.
pub fn parse_ioreg(text: &str) -> Vec<Option<u8>> {
    const KEY: &str = "\"Device Utilization %\"";
    let mut gpus: Vec<Option<u8>> = Vec::new();
    for line in text.lines() {
        if line.starts_with("+-o ") {
            gpus.push(None);
        }
        let value = line.split_once(KEY).and_then(|(_, rest)| {
            let digits: String = rest
                .trim_start()
                .strip_prefix('=')?
                .trim_start()
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            digits.parse::<u8>().ok().filter(|v| *v <= 100)
        });
        if let (Some(gpu), Some(value)) = (gpus.last_mut(), value) {
            gpu.get_or_insert(value);
        }
    }
    gpus
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROC_STAT: &str = "cpu  4705 150 3120 84000 900 0 45 0 0 0\ncpu0 1200 30 800 21000 200 0 10 0 0 0\nintr 12345\nctxt 999\n";
    const MEMINFO: &str = "MemTotal:       16000000 kB\nMemFree:         1000000 kB\nMemAvailable:    4000000 kB\nBuffers:          500000 kB\n";
    const IOREG_ONE: &str = "+-o AGXAcceleratorG13X  <class AGXAcceleratorG13X, id 0x100000385, registered, matched, active, busy 0 (0 ms), retain 28>\n\
        {\n\
        \x20 \"IOClass\" = \"AGXAcceleratorG13X\"\n\
        \x20 \"PerformanceStatistics\" = {\"Alloc system memory\"=50167808,\"Device Utilization %\"=37,\"Renderer Utilization %\"=21,\"Tiler Utilization %\"=30}\n\
        }\n\
        \n";
    const IOREG_TWO: &str = "+-o AMDRadeonX6000_AMDNavi14Graphics  <class AMDRadeonX6000_AMDNavi14Graphics, id 0x1000003e0>\n\
        \x20 {\n\
        \x20   \"PerformanceStatistics\" = {\"Device Utilization %\"=12,\"GPU Core Utilization\"=0}\n\
        \x20 }\n\
        \n\
        +-o IntelAccelerator  <class IntelAccelerator, id 0x1000003e1>\n\
        \x20 {\n\
        \x20   \"PerformanceStatistics\" = {\"Alloc system memory\"=1}\n\
        \x20 }\n";
    const VM_STAT: &str = "Mach Virtual Memory Statistics: (page size of 16384 bytes)\n\
        Pages free:                               12345.\n\
        Pages active:                            100000.\n\
        Pages inactive:                           90000.\n\
        Pages speculative:                         1000.\n\
        Pages wired down:                         50000.\n\
        Pages occupied by compressor:             10000.\n";

    #[test]
    fn nvidia_smi_is_one_integer_per_line_in_the_tools_order() {
        assert_eq!(parse_nvidia_smi_line("0, 61"), Some((0, Some(61))));
        assert_eq!(parse_nvidia_smi_line("1, 0\r"), Some((1, Some(0))));
        assert_eq!(parse_nvidia_smi_line(" 2 , 100 "), Some((2, Some(100))));
        assert_eq!(
            parse_nvidia_smi_line("1, [N/A]"),
            Some((1, None)),
            "a GPU with no number keeps its place"
        );
        assert_eq!(parse_nvidia_smi_line("0, 101"), Some((0, None)));
        assert_eq!(parse_nvidia_smi_line("61"), None, "no index, no GPU");
        assert_eq!(parse_nvidia_smi_line(""), None);
        assert_eq!(parse_nvidia_smi_line("NVIDIA-SMI has failed"), None);
    }

    #[test]
    fn ioreg_is_the_device_utilization_of_each_accelerator() {
        assert_eq!(parse_ioreg(IOREG_ONE), [Some(37)]);
        assert_eq!(
            parse_ioreg(IOREG_TWO),
            [Some(12), None],
            "an accelerator without the number is None"
        );
        assert_eq!(parse_ioreg(""), [], "no accelerator");
        assert_eq!(parse_ioreg("ioreg: no matching entries\n"), []);
    }

    #[test]
    fn proc_stat_counts_idle_plus_iowait_of_the_all_cpu_line() {
        // total = 4705 + 150 + 3120 + 84000 + 900 + 0 + 45 + 0
        assert_eq!(
            parse_proc_stat(PROC_STAT),
            Some(CpuTimes {
                idle: 84900,
                total: 92920
            })
        );
        assert_eq!(
            parse_proc_stat("cpu0 1 2 3 4 5\n"),
            None,
            "only the all-CPU line"
        );
        assert_eq!(parse_proc_stat("cpu  1 2 x 4 5\n"), None);
        assert_eq!(parse_proc_stat(""), None);
    }

    #[test]
    fn the_cpu_share_is_the_non_idle_part_of_the_time_between_two_samples() {
        let before = CpuTimes {
            idle: 800,
            total: 1000,
        };
        assert_eq!(
            cpu_share(
                before,
                CpuTimes {
                    idle: 850,
                    total: 1100
                }
            ),
            Some(50)
        );
        assert_eq!(
            cpu_share(
                before,
                CpuTimes {
                    idle: 800,
                    total: 1100
                }
            ),
            Some(100)
        );
        assert_eq!(
            cpu_share(
                before,
                CpuTimes {
                    idle: 900,
                    total: 1100
                }
            ),
            Some(0)
        );
        assert_eq!(
            cpu_share(
                before,
                CpuTimes {
                    idle: 802,
                    total: 1003
                }
            ),
            Some(33),
            "rounded"
        );
        assert_eq!(cpu_share(before, before), None, "no time passed");
    }

    #[test]
    fn meminfo_is_total_minus_available_over_total() {
        assert_eq!(parse_meminfo(MEMINFO), Some(75));
        assert_eq!(parse_meminfo("MemTotal: 100 kB\n"), None);
        assert_eq!(parse_meminfo("MemTotal: 0 kB\nMemAvailable: 0 kB\n"), None);
        assert_eq!(parse_meminfo(""), None);
    }

    #[test]
    fn ps_is_the_sum_of_the_cpu_column_over_the_cpu_count_capped_at_100() {
        let ps = "  12.5\n   0.0\n 100.0\n  37.5\n";
        assert_eq!(parse_ps(ps, 4), Some(38), "150 / 4 = 37.5");
        assert_eq!(parse_ps(ps, 1), Some(100), "capped");
        assert_eq!(parse_ps("", 8), Some(0));
        assert_eq!(parse_ps("1.0\nx\n", 8), None);
        assert_eq!(parse_ps(ps, 0), None);
    }

    #[test]
    fn vm_stat_is_active_wired_and_compressor_pages_times_the_page_size_over_memsize() {
        // (100000 + 50000 + 10000) * 16384 = 2_621_440_000 bytes of 8 GiB
        assert_eq!(parse_vm_stat(VM_STAT, 8 << 30), Some(31));
        assert_eq!(parse_vm_stat(VM_STAT, 2_621_440_000), Some(100));
        assert_eq!(parse_vm_stat(VM_STAT, 1 << 20), Some(100), "capped");
        assert_eq!(
            parse_vm_stat(&VM_STAT.replace("Pages wired down", "Pages wired"), 8 << 30),
            None
        );
        assert_eq!(parse_vm_stat("", 8 << 30), None);
        assert_eq!(parse_vm_stat(VM_STAT, 0), None);
    }
}
