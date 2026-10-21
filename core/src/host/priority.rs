//! The idle priority prefix (hs-d528): the cockpit never freezes, so every Attempt's
//! process tree starts at nice 19 and I/O class idle, set at spawn so every child inherits it.

/// The argv that runs the command after it at idle priority.
///
/// Linux: `nice -n 19 ionice -c 3`. macOS has no `ionice`: its I/O priority is skipped
/// explicitly (hs-d528), only `nice -n 19` applies.
pub fn idle_prefix() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["nice", "-n", "19"]
    }
    #[cfg(not(target_os = "macos"))]
    {
        &["nice", "-n", "19", "ionice", "-c", "3"]
    }
}
