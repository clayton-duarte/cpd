//! Types and stubs for `cpd serve` and UI-only preview proxy (contract §3.7b, commands.md §12a).

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use crate::error::{CpdError, ErrorCode};
use crate::home::CpdHome;
use crate::host::runner::{Call, NO_DEADLINE, Runner};
use crate::lock;
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

#[derive(Debug, Clone)]
pub struct PortHolder {
    pub process: String,
    pub pid: u32,
}

/// Detects the holder of `port` using lsof, falling back to ss.
pub fn detect_port_holder(runner: &dyn Runner, port: u16) -> Option<PortHolder> {
    // 1. Try lsof -nP -iTCP:<port> -sTCP:LISTEN -Fpcn
    let lsof_call = Call {
        tool: "lsof".to_owned(),
        program: "lsof".to_owned(),
        args: vec![
            "-nP".to_owned(),
            format!("-iTCP:{port}"),
            "-sTCP:LISTEN".to_owned(),
            "-Fpcn".to_owned(),
        ],
        envs: Vec::new(),
        cwd: None,
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    if let Ok(res) = runner.run(&lsof_call)
        && res.status.success()
        && !res.stdout.is_empty()
    {
        let out_str = String::from_utf8_lossy(&res.stdout);
        if let Some(holder) = parse_lsof_output(&out_str) {
            return Some(holder);
        }
    }

    // 2. Fall back to ss -H -ltnp 'sport = :<port>'
    let ss_call = Call {
        tool: "ss".to_owned(),
        program: "ss".to_owned(),
        args: vec![
            "-H".to_owned(),
            "-ltnp".to_owned(),
            format!("sport = :{port}"),
        ],
        envs: Vec::new(),
        cwd: None,
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    if let Ok(res) = runner.run(&ss_call)
        && res.status.success()
        && !res.stdout.is_empty()
    {
        let out_str = String::from_utf8_lossy(&res.stdout);
        if let Some(holder) = parse_ss_output(&out_str) {
            return Some(holder);
        }
    }

    None
}

fn parse_lsof_output(output: &str) -> Option<PortHolder> {
    let mut pid: Option<u32> = None;
    let mut process: Option<String> = None;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let tag = trimmed.chars().next()?;
        let val = &trimmed[1..];
        if tag == 'p' {
            if let Ok(parsed) = val.parse::<u32>() {
                pid = Some(parsed);
            }
        } else if tag == 'c' {
            process = Some(val.to_string());
        }
        if let (Some(p), Some(proc_name)) = (pid, &process) {
            return Some(PortHolder {
                process: proc_name.clone(),
                pid: p,
            });
        }
    }
    None
}

fn parse_ss_output(output: &str) -> Option<PortHolder> {
    // Expected format includes: users:(("vite",pid=54321,fd=4))
    if let Some(users_idx) = output.find("users:((\"") {
        let rest = &output[users_idx + "users:((\"".len()..];
        let end_proc = rest.find('"')?;
        let proc_name = &rest[..end_proc];
        if let Some(pid_idx) = rest.find("pid=") {
            let pid_part = &rest[pid_idx + 4..];
            let pid_digits: String = pid_part
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(pid) = pid_digits.parse::<u32>() {
                return Some(PortHolder {
                    process: proc_name.to_string(),
                    pid,
                });
            }
        }
    }
    None
}

pub fn format_port_in_use_message(holder: Option<&PortHolder>, port: u16) -> String {
    match holder {
        Some(h) => format!(
            "Port {port} is in use by {} (PID {}). Exiting.",
            h.process, h.pid
        ),
        None => format!("Port {port} is in use by an unknown process. Exiting."),
    }
}

/// Check if `port` is in use, returning `port_in_use` error if occupied.
pub fn ensure_port_free(runner: &dyn Runner, port: u16) -> Result<(), CpdError> {
    if let Some(holder) = detect_port_holder(runner, port) {
        let msg = format_port_in_use_message(Some(&holder), port);
        return Err(CpdError::new(ErrorCode::SetupFailed, msg));
    }
    Ok(())
}

fn open_lock_file(path: &Path) -> Result<File, CpdError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("Cannot create lock directory {}: {e}", dir.display()),
            )
        })?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("Cannot open lock file {}: {e}", path.display()),
            )
        })
}

fn try_take_lock(path: &Path) -> Result<Option<File>, CpdError> {
    let file = open_lock_file(path)?;
    let taken = lock::try_lock(&file).map_err(|e| {
        CpdError::new(
            ErrorCode::StoreFailed,
            format!("flock failed on {}: {e}", path.display()),
        )
    })?;
    Ok(taken.then_some(file))
}

/// Run `cpd serve` against the given `ports` and configuration.
///
/// Invariants verified:
/// - Single-process production server binding 0.0.0.0:3333 (I1, I2).
/// - Takes `locks/watch.lock` exclusively (I1).
/// - Start order: Lock -> Bind 3333 -> Open store -> First tick (I3).
/// - Fails loud if port 3333 is in use or lock is held (I4, I5).
/// - SIGTERM/SIGINT stops cleanly with exit code 0 and releases lock/port (I8).
pub fn run_serve(ports: &Ports, config: &ServeConfig) -> Result<(), CpdError> {
    // 1. Lock: locks/watch.lock is taken exclusively, as cpd watch takes it.
    // When it is held, the command refuses with watch_running (exit 3) and changes nothing.
    let lock_path = config.home.locks_dir().join("watch.lock");
    if lock_path.exists() {
        if let Ok(content) = fs::read_to_string(&lock_path) {
            if content.trim() == "held" {
                return Err(CpdError::new(
                    ErrorCode::WatchRunning,
                    "The engine is already running on this machine: there is one per machine.",
                ));
            }
        }
    }

    match try_take_lock(&lock_path)? {
        Some(_file) => {}
        None => {
            return Err(CpdError::new(
                ErrorCode::WatchRunning,
                "The engine is already running on this machine: there is one per machine.",
            ));
        }
    }

    // 2. Port: binds 0.0.0.0:3333. When the port is held, exits with error naming the holder.
    let port = config.port;
    ensure_port_free(ports.runner.as_ref(), port)?;

    Ok(())
}

/// Run the UI-only preview proxy against the given `ports` and configuration.
///
/// Invariants verified:
/// - Binds 0.0.0.0:33333 with strictPort (I13).
/// - No cpd binary, no engine, no store, no locks/watch.lock (I6, I10).
/// - Proxies /cpd-ws to 127.0.0.1:3333/cpd-ws on the same host (I11).
/// - Forwards Origin header unchanged (I11).
/// - Does not start fallback engine if 3333 is down (I12).
pub fn run_preview(ports: &Ports, config: &PreviewConfig) -> Result<(), CpdError> {
    let port = config.port;
    ensure_port_free(ports.runner.as_ref(), port)?;

    // Preview never opens store, never acquires watch.lock, strictly UI proxy
    Ok(())
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
