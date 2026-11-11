//! WebUI server supervisor (pro-d541, pro-d542).
//!
//! Supervises WebUI production server on port 3333 and preview server on port 33333.
//! Checks port availability using lsof (fallback to ss) before starting.
//! Restarts crashed servers with backoff (>3 crashes in 10 minutes latches stopped).
//! Restarts once when payload install marker (`run/install-restart.json`) is present.

use std::collections::HashMap;

use serde_json::json;
use time::OffsetDateTime;

use super::SOURCE;
use super::engine::Engine;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{NewEvent, append};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, NO_DEADLINE, Runner};
use cpd_core::host::spawn::{Launch, Pid, Signal};

/// WebUI Production port.
pub const PROD_PORT: u16 = 3333;
/// WebUI Preview port.
pub const PREVIEW_PORT: u16 = 33333;

/// The restart bound: 3 restarts inside 10 minutes, then latch stopped (pro-d541).
const MAX_RESTARTS: u32 = 3;
const WINDOW: time::Duration = time::Duration::minutes(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServiceKind {
    Prod,
    Preview,
}

impl ServiceKind {
    pub fn name(&self) -> &'static str {
        match self {
            ServiceKind::Prod => "webui-prod",
            ServiceKind::Preview => "webui-preview",
        }
    }

    pub fn port(&self) -> u16 {
        match self {
            ServiceKind::Prod => PROD_PORT,
            ServiceKind::Preview => PREVIEW_PORT,
        }
    }
}

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

/// Per-service state tracked by the engine supervisor.
#[derive(Debug, Clone, Default)]
pub struct ServiceState {
    pub pid: Option<Pid>,
    pub latched_stopped: bool,
    pub restart_count: u32,
    pub window_start: Option<OffsetDateTime>,
}

#[derive(Default)]
pub struct WebUiState {
    pub services: HashMap<ServiceKind, ServiceState>,
    pub last_marker_seen: Option<String>,
}

impl WebUiState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Tick step: supervises production and preview WebUI servers.
pub fn run(engine: &mut Engine) {
    if let Err(e) = run_inner(engine) {
        engine.log.line(&format!("webui: supervision error: {e}"));
    }
}

pub fn run_inner(engine: &mut Engine) -> Result<(), CpdError> {
    let home = engine.home.clone();

    // Check payload install restart marker
    let install_restart_marker = home.run_dir().join("install-restart.json");
    let has_restart_marker = install_restart_marker.exists();

    let mut services_to_supervise = vec![ServiceKind::Prod];
    if preview_worktree_exists(&home) {
        services_to_supervise.push(ServiceKind::Preview);
    }

    for kind in services_to_supervise {
        let state = engine.webui.services.entry(kind).or_default();
        if state.latched_stopped {
            continue;
        }

        // If install restart marker is present, restart without counting as crash
        if has_restart_marker && state.pid.is_some() {
            if let Some(pid) = state.pid.take() {
                let _ = engine.ports.spawn.signal(pid, Signal::Term);
            }
            // Trigger restart
            start_service(engine, kind)?;
            continue;
        }

        if let Some(pid) = state.pid {
            if engine.ports.spawn.alive(pid) {
                // Service is alive, leave alone
                continue;
            }
            // Service crashed! Handle restart with bound
            handle_crash(engine, kind)?;
        } else {
            // Not started yet
            start_service(engine, kind)?;
        }
    }

    Ok(())
}

fn preview_worktree_exists(home: &CpdHome) -> bool {
    let preview_dir = home.root().join("projects").join("cpd.preview");
    if preview_dir.exists() {
        return true;
    }
    home.run_dir().join("preview-web").exists()
}

fn handle_crash(engine: &mut Engine, kind: ServiceKind) -> Result<(), CpdError> {
    let now = engine.ports.clock.utc();
    let state = engine.webui.services.entry(kind).or_default();
    state.pid = None;

    if state.window_start.is_none_or(|start| now - start >= WINDOW) {
        state.window_start = Some(now);
        state.restart_count = 0;
    }

    if state.restart_count >= MAX_RESTARTS {
        state.latched_stopped = true;
        let msg = format!(
            "WebUI service {} is stopped: crashed {} times in 10 minutes.",
            kind.name(),
            MAX_RESTARTS
        );
        engine.log.line(&format!("webui: {msg}"));
        engine.store.write(SOURCE, |tx| {
            let event = NewEvent::error(
                msg.clone(),
                SOURCE,
                "webui_crash_loop",
                &msg,
                json!({ "service": kind.name() }),
            );
            append(tx, event)?;
            Ok(())
        })?;
        return Ok(());
    }

    state.restart_count += 1;
    start_service(engine, kind)
}

fn start_service(engine: &mut Engine, kind: ServiceKind) -> Result<(), CpdError> {
    let port = kind.port();

    // Check port availability first
    if let Some(holder) = detect_port_holder(engine.ports.runner.as_ref(), port) {
        let err_msg = format_port_in_use_message(Some(&holder), port);
        engine.log.line(&format!("webui: {err_msg}"));
        engine.store.write(SOURCE, |tx| {
            let event = NewEvent::error(
                err_msg.clone(),
                SOURCE,
                "port_in_use",
                &err_msg,
                json!({ "port": port, "holder": holder.process, "pid": holder.pid }),
            );
            append(tx, event)?;
            Ok(())
        })?;
        let state = engine.webui.services.entry(kind).or_default();
        state.latched_stopped = true;
        return Err(CpdError::new(ErrorCode::SetupFailed, err_msg));
    }

    // Launch service
    let log_path = engine.home.run_dir().join(format!("{}.log", kind.name()));
    let (program, args, cwd) = match kind {
        ServiceKind::Prod => (
            "node".to_owned(),
            vec!["web/src/server/serve.ts".to_owned()],
            Some(engine.home.root().to_owned()),
        ),
        ServiceKind::Preview => (
            "pnpm".to_owned(),
            vec!["run".to_owned(), "dev".to_owned()],
            Some(engine.home.root().join("projects").join("cpd.preview")),
        ),
    };

    let launch = Launch {
        program,
        args,
        cwd,
        stdout_to: Some(log_path),
        ..Launch::default()
    };

    match engine.ports.spawn.spawn(&launch) {
        Ok(pid) => {
            let state = engine.webui.services.entry(kind).or_default();
            state.pid = Some(pid);
            engine
                .log
                .line(&format!("webui: started {} (PID {:?})", kind.name(), pid));
            Ok(())
        }
        Err(e) => {
            let err = CpdError::new(
                ErrorCode::SpawnFailed,
                format!("webui: failed to start {}: {e}", kind.name()),
            );
            engine.log.line(err.message());
            Err(err)
        }
    }
}
