//! `cpd doctor` (commands.md §14.1): checks the machine. Every check yields a
//! [`Check`]; any failing `error` check makes the result `doctor_failed`.
//! Read-only, except the migration of a database's first open.

mod check;
mod pi;
mod state;
mod tools;

use serde_json::json;

use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::output::Output;
use crate::store::Store;
use check::{Check, Severity};

/// `cpd doctor` takes no arguments.
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
pub struct Args {}

pub fn run(_args: Args) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home);
    let mut store = config.as_ref().ok().map(|c| Store::open(&home, c.timeouts.store_busy_ms.into()));
    let needs_config = "config.toml is not valid.";

    let mut checks = vec![match &config {
        Ok(_) => Check::pass("config", Severity::Error).found(home.config().display().to_string()),
        Err(e) => Check::from_error("config", Severity::Error, e),
    }];
    let (store_check, mut store) = match store.take() {
        Some(opened) => state::store(opened),
        None => (Check::not_checked("store", Severity::Error, needs_config), None),
    };
    checks.push(store_check);
    checks.push(state::tamper(store.as_mut()));
    checks.extend(tools::tools());
    checks.push(tools::gh_auth());
    checks.push(tools::pi_node());
    let with_config = |name, severity, run: &dyn Fn(&Config) -> Check| match &config {
        Ok(config) => run(config),
        Err(_) => Check::not_checked(name, severity, needs_config),
    };
    checks.push(with_config("tiers", Severity::Error, &pi::tiers));
    checks.push(with_config("pi_adapter", Severity::Error, &pi::adapter));
    checks.push(with_config("lead_packages", Severity::Warn, &pi::lead_packages));
    checks.push(state::contexts(&home, store.as_mut()));
    checks.push(state::watch(&home, store.as_mut()));

    let failed: Vec<&str> = checks.iter().filter(|c| c.is_error_failure()).map(|c| c.name).collect();
    if failed.is_empty() {
        return Ok(Output::success(json!({ "checks": checks })));
    }
    Err(CpdError::new(ErrorCode::DoctorFailed, format!("These checks failed: {}.", failed.join(", ")))
        .with("checks", serde_json::to_value(&checks).expect("checks serialize")))
}
