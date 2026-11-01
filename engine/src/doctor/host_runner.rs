//! The `host_runner` check (contract §6.19, P1.10): for `durable`, `<pi_adapter>/durable-host/main.ts` exists and
//! pi's Node can run it, exactly as [`super::worker_runner`] does for the durable Worker.

use cpd_core::config::catalog::node;
use cpd_core::config::{Config, WorkerRunner};
use cpd_core::host::runner::Runner;
use cpd_core::ports::Ports;

use super::check::{Check, Severity};
use super::tools::{parse_version, run_tool};

const NAME: &str = "host_runner";
/// The `engines.node` of the durable host's package.
const MIN_NODE: [u64; 3] = [22, 19, 0];
const EXPECTED: &str = ">= 22.19.0";

/// Both runner keys `pi` passes without looking at anything, as no store is consulted. Kept for the unit test that
/// drives this path directly; `run` calls [`check_with`].
#[allow(dead_code)]
pub fn check(config: &Config) -> Check {
    check_with(config, false, &*Ports::current().runner)
}

/// Both runner keys `pi` and no live durable Session passes without looking at anything; otherwise the host needs
/// `<pi_adapter>/durable-host/main.ts` and Node >= 22.19.0 (contract §6.19).
pub fn check_with(config: &Config, durable_session: bool, runner: &dyn Runner) -> Check {
    if config.lead_runner == WorkerRunner::Pi
        && config.manager_runner == WorkerRunner::Pi
        && !durable_session
    {
        return Check::pass(NAME, Severity::Error);
    }
    let dir = match config.pi_adapter_dir() {
        Ok(dir) => dir,
        Err(e) => return Check::from_error(NAME, Severity::Error, &e),
    };
    let file = dir.join("durable-host").join("main.ts");
    if !file.is_file() {
        return Check::fail(
            NAME,
            Severity::Error,
            format!(
                "{} is missing: the durable host is not installed beside pi_adapter.",
                file.display()
            ),
        );
    }
    let text = match run_tool(runner, node().as_os_str(), &["--version"]) {
        Ok(text) => text,
        Err(message) => return Check::fail(NAME, Severity::Error, message).expected(EXPECTED),
    };
    let Some((found, version)) = parse_version(&text, 3) else {
        let first = text.lines().next().unwrap_or("no output");
        return Check::fail(
            NAME,
            Severity::Error,
            format!("no version in the output of `node --version`: {first}"),
        )
        .expected(EXPECTED);
    };
    let check = Check::new(NAME, Severity::Error, version >= MIN_NODE)
        .found(found)
        .expected(EXPECTED);
    if version >= MIN_NODE {
        check
    } else {
        check.message("pi's Node is older than the durable host needs: 22.19.0.")
    }
}
