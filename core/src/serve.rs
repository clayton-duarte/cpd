//! Types and stubs for `cpd serve` and UI-only preview proxy (contract §3.7b, commands.md §12a).

use std::path::PathBuf;

use crate::error::CpdError;
use crate::home::CpdHome;
use crate::ports::Ports;

/// Production port for single-process cpd serve (contract §3.7b).
pub const PROD_PORT: u16 = 3333;

/// Preview port for UI-only preview (contract §3.7b).
pub const PREVIEW_PORT: u16 = 33333;

/// Start order steps for single-process `cpd serve` (contract §3.7b I3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeStep {
    Lock,
    BindPort,
    OpenStore,
    FirstTick,
}

/// Invariants and configuration for production serve (contract §3.7b).
#[derive(Debug, Clone)]
pub struct ServeConfig {
    pub home: CpdHome,
    pub port: u16,
    pub bind_addr: &'static str,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            home: CpdHome::resolve(None, None).expect("resolve default cpd home"),
            port: PROD_PORT,
            bind_addr: "0.0.0.0",
        }
    }
}

/// Invariants and configuration for UI-only preview server (contract §3.7b I10-I13).
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    pub port: u16,
    pub strict_port: bool,
    pub target_engine_host: &'static str,
    pub target_engine_port: u16,
    pub web_dir: PathBuf,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            port: PREVIEW_PORT,
            strict_port: true,
            target_engine_host: "127.0.0.1",
            target_engine_port: PROD_PORT,
            web_dir: PathBuf::from("web"),
        }
    }
}

/// Run `cpd serve` against the given `ports` and configuration.
///
/// Invariants verified:
/// - Single-process production server binding 0.0.0.0:3333 (I1, I2).
/// - Takes `locks/watch.lock` exclusively (I1).
/// - Start order: Lock -> Bind 3333 -> Open store -> First tick (I3).
/// - Fails loud if port 3333 is in use or lock is held (I4, I5).
/// - SIGTERM/SIGINT stops cleanly with exit code 0 and releases lock/port (I8).
pub fn run_serve(_ports: &Ports, _config: &ServeConfig) -> Result<(), CpdError> {
    todo!(
        "implement single-process cpd serve binding 3333 with engine and webui (pro-d566, pro-d570)"
    )
}

/// Run the UI-only preview proxy against the given `ports` and configuration.
///
/// Invariants verified:
/// - Binds 0.0.0.0:33333 with strictPort (I13).
/// - No cpd binary, no engine, no store, no locks/watch.lock (I6, I10).
/// - Proxies /cpd-ws to 127.0.0.1:3333/cpd-ws on the same host (I11).
/// - Forwards Origin header unchanged (I11).
/// - Does not start fallback engine if 3333 is down (I12).
pub fn run_preview(_ports: &Ports, _config: &PreviewConfig) -> Result<(), CpdError> {
    todo!("implement UI-only preview proxy binding 33333 forwarding to 3333 engine (pro-d570)")
}

/// Entry point helper called by CLI `cpd serve`.
pub fn serve() -> Result<(), CpdError> {
    let ports = Ports::current();
    let config = ServeConfig::default();
    run_serve(&ports, &config)
}

/// Entry point helper called by preview runner.
pub fn preview() -> Result<(), CpdError> {
    let ports = Ports::current();
    let config = PreviewConfig::default();
    run_preview(&ports, &config)
}
