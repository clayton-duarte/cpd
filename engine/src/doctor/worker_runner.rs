//! The `worker_runner` check (commands.md §14.1): for `durable`, the runner file exists and pi's Node can run it.

use cpd_core::config::catalog::node;
use cpd_core::config::{Config, WorkerRunner};
use cpd_core::host::runner::Runner;
use cpd_core::ports::Ports;

use super::check::{Check, Severity};
use super::tools::{parse_version, run_tool};

const NAME: &str = "worker_runner";
/// The `engines.node` of the durable runner's package.
const MIN_NODE: [u64; 3] = [22, 19, 0];
const EXPECTED: &str = ">= 22.19.0";

/// `pi` passes without looking at anything; `durable` needs `<pi_adapter>/durable-worker/main.ts` and Node >= 22.19.0.
pub fn worker_runner(config: &Config) -> Check {
    worker_runner_with(config, &*Ports::current().runner)
}

/// [`worker_runner`] on `runner`.
pub fn worker_runner_with(config: &Config, runner: &dyn Runner) -> Check {
    if config.worker_runner == WorkerRunner::Pi {
        return Check::pass(NAME, Severity::Error);
    }
    let dir = match config.pi_adapter_dir() {
        Ok(dir) => dir,
        Err(e) => return Check::from_error(NAME, Severity::Error, &e),
    };
    let file = dir.join("durable-worker").join("main.ts");
    if !file.is_file() {
        return Check::fail(
            NAME,
            Severity::Error,
            format!(
                "{} is missing: the durable runner is not installed beside pi_adapter.",
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
        check.message("pi's Node is older than the durable runner needs: 22.19.0.")
    }
}
