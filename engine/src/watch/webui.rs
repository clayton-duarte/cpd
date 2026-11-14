//! WebUI server supervisor (pro-d541, pro-d542).
//! Supervises WebUI production server on port 3333 and preview server on port 33333.
//! Checks port availability using lsof (fallback to ss) before starting.
//! Restarts crashed servers with backoff (>3 crashes in 10 minutes latches stopped).
//! Restarts once when payload install marker (`run/install-restart.json`) is present.

use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::json;
use time::OffsetDateTime;

use super::SOURCE;
use super::engine::Engine;
use cpd_core::app::read_root;
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

pub fn resolve_checkout_path(home: &CpdHome) -> Option<PathBuf> {
    // The home's own checkout wins over the one `HOME` names, so a home under test never reaches the user's.
    if home.root().join("web/src/server/serve.ts").exists() {
        return Some(home.root().to_path_buf());
    }
    if let Some(user_home) = std::env::var_os("HOME").map(PathBuf::from)
        && let Ok(checkout) = read_root(&user_home)
        && checkout.join("web/src/server/serve.ts").exists()
    {
        return Some(checkout);
    }
    None
}

pub fn run_inner(engine: &mut Engine) -> Result<(), CpdError> {
    let home = engine.home.clone();

    // Check payload install restart marker
    let install_restart_marker = home.run_dir().join("install-restart.json");
    let has_restart_marker = install_restart_marker.exists();

    let mut services_to_supervise = Vec::new();
    if resolve_checkout_path(&home).is_some() {
        services_to_supervise.push(ServiceKind::Prod);
    }
    if preview_worktree_exists(&home) {
        services_to_supervise.push(ServiceKind::Preview);
    }

    for kind in services_to_supervise {
        if kind == ServiceKind::Prod {
            let preparing = std::fs::read_to_string(home.run_dir().join("webui-preparing.pid"))
                .ok()
                .and_then(|text| text.trim().parse::<u32>().ok())
                .filter(|pid| *pid > 0)
                .filter(|pid| engine.ports.spawn.alive(Pid(*pid)))
                .filter(|pid| {
                    engine
                        .ports
                        .spawn
                        .cmdline(Pid(*pid))
                        .or_else(|| read_process_command(&*engine.ports.runner, *pid))
                        .is_some_and(|cmd| is_same_service(ServiceKind::Prod, &cmd))
                });
            let record: serde_json::Value =
                std::fs::read_to_string(home.run_dir().join("webui-prepared.json"))
                    .ok()
                    .and_then(|text| serde_json::from_str(&text).ok())
                    .unwrap_or_default();
            if preparing.is_some()
                && (record["pid"].as_u64() != preparing.map(u64::from)
                    || record["phase"] != "active")
            {
                // Install owns this handoff. Neither engine generation may retire the serving listener early.
                continue;
            }
            if preparing.is_some()
                && engine
                    .webui
                    .services
                    .get(&kind)
                    .and_then(|s| s.pid)
                    .map(|p| p.0)
                    != preparing
            {
                // Adopt the promoted listener through the normal port/command checks, without counting an install as a crash.
                start_service(engine, kind)?;
                continue;
            }
        }
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

pub fn preview_worktree_path(home: &CpdHome) -> Option<PathBuf> {
    if let Some(env_path) = std::env::var_os("CPD_WEB_PREVIEW_WORKTREE") {
        let p = PathBuf::from(env_path);
        if p.exists() {
            return Some(p);
        }
    }
    if let Some(checkout) = resolve_checkout_path(home)
        && let Some(parent) = checkout.parent()
    {
        let p = parent.join("cpd.preview.web");
        if p.exists() {
            return Some(p);
        }
    }
    // projects/cpd.preview.web under home root
    let p1 = home.root().join("projects").join("cpd.preview.web");
    if p1.exists() {
        return Some(p1);
    }
    // sibling to home root (e.g. repo parent / install root's cpd.preview.web)
    if let Some(parent) = home.root().parent() {
        let p2 = parent.join("cpd.preview.web");
        if p2.exists() {
            return Some(p2);
        }
    }
    // legacy paths for backwards compatibility with tests / earlier specs:
    // home.root().join("projects/cpd.preview") or run_dir/preview-web
    let p3 = home.root().join("projects").join("cpd.preview");
    if p3.exists() {
        return Some(p3);
    }
    let p4 = home.run_dir().join("preview-web");
    if p4.exists() {
        return Some(p4);
    }
    None
}

fn preview_worktree_exists(home: &CpdHome) -> bool {
    preview_worktree_path(home).is_some()
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

/// Reads the command line of a process using `ps -o command= -p <pid>`.
fn read_process_command(runner: &dyn Runner, pid: u32) -> Option<String> {
    let ps_call = Call {
        tool: "ps".to_owned(),
        program: "ps".to_owned(),
        args: vec![
            "-o".to_owned(),
            "command=".to_owned(),
            "-p".to_owned(),
            pid.to_string(),
        ],
        envs: Vec::new(),
        cwd: None,
        deadline: NO_DEADLINE,
        stdin: None,
        stderr_to: None,
    };
    if let Ok(res) = runner.run(&ps_call)
        && res.status.success()
    {
        let cmd = String::from_utf8_lossy(&res.stdout).trim().to_string();
        if !cmd.is_empty() {
            return Some(cmd);
        }
    }
    None
}

/// Checks whether the holder is considered the "same service" that can be taken over.
///
/// Rules:
/// - `webui-prod` on 3333: command line naming `web/src/server/serve.ts` or `serve-prod.mjs`
/// - `webui-preview` on 33333: command line running `vite`
fn is_same_service(kind: ServiceKind, cmd: &str) -> bool {
    match kind {
        ServiceKind::Prod => cmd.contains("src/server/serve.ts") || cmd.contains("serve-prod.mjs"),
        ServiceKind::Preview => cmd.contains("vite"),
    }
}

/// The pid `run/webui.pid` names: the prod WebUI an install prepared.
fn prepared_webui_pid(home: &CpdHome) -> Option<u32> {
    std::fs::read_to_string(home.run_dir().join("webui.pid"))
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn start_service(engine: &mut Engine, kind: ServiceKind) -> Result<(), CpdError> {
    let port = kind.port();

    // Check port availability first
    if let Some(holder) = detect_port_holder(engine.ports.runner.as_ref(), port) {
        let cmd_opt = read_process_command(engine.ports.runner.as_ref(), holder.pid);
        let can_takeover = cmd_opt
            .as_deref()
            .is_some_and(|cmd| is_same_service(kind, cmd));

        if can_takeover {
            // The holder is the WebUI an install prepared (run/webui.pid names it, pro-d592): adopt it as the
            // engine's own, and neither signal it nor spawn a second one.
            if prepared_webui_pid(&engine.home) == Some(holder.pid) {
                engine.log.line(&format!(
                    "webui: adopted port holder PID {} for {}",
                    holder.pid,
                    kind.name()
                ));
                engine.webui.services.entry(kind).or_default().pid = Some(Pid(holder.pid));
                return Ok(());
            }
            let cmd = cmd_opt.as_deref().unwrap_or("");
            // Send SIGTERM to holder
            let holder_pid = Pid(holder.pid);
            if engine.ports.spawn.signal(holder_pid, Signal::Term).is_ok() {
                // Wait up to 10 seconds checking if port cleared or holder died
                let mut cleared = false;
                for _ in 0..10 {
                    let still_held = if !engine.ports.spawn.alive(holder_pid) {
                        false
                    } else if let Some(h) = detect_port_holder(engine.ports.runner.as_ref(), port) {
                        // If runner still returns a holder, check if it's the dead process in spawn
                        h.pid == holder.pid && engine.ports.spawn.alive(holder_pid)
                    } else {
                        false
                    };

                    if !still_held {
                        cleared = true;
                        break;
                    }
                    engine
                        .ports
                        .clock
                        .sleep(std::time::Duration::from_millis(100));
                }

                if cleared {
                    // Successful takeover writes one line to run/watch.log and no Event
                    engine.log.line(&format!(
                        "webui: replaced port holder PID {} ({}) for {}",
                        holder.pid,
                        cmd,
                        kind.name()
                    ));
                } else {
                    // Takeover failed: port still held after wait
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
                    return Err(CpdError::new(ErrorCode::SetupFailed, err_msg));
                }
            } else {
                // Signal failed
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
                return Err(CpdError::new(ErrorCode::SetupFailed, err_msg));
            }
        } else {
            // Not the same service, or command line cannot be read:
            // Service does not start, raises error event, latches stopped.
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
    }

    // Launch service
    let log_path = engine.home.run_dir().join(format!("{}.log", kind.name()));
    let (program, args, cwd) = match kind {
        ServiceKind::Prod => (
            "node".to_owned(),
            vec!["web/src/server/serve.ts".to_owned()],
            resolve_checkout_path(&engine.home).or_else(|| Some(engine.home.root().to_owned())),
        ),
        ServiceKind::Preview => (
            "pnpm".to_owned(),
            vec!["run".to_owned(), "dev".to_owned()],
            preview_worktree_path(&engine.home).map(|p| {
                if p.join("web/package.json").exists() {
                    p.join("web")
                } else {
                    p
                }
            }),
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

/// The install's explicit preparation/readiness/activation boundary.
#[derive(clap::Args, Debug)]
#[group(required = true, multiple = false)]
pub struct WebUiArgs {
    #[arg(long)]
    pub prepare: bool,
    #[arg(long)]
    pub ready: bool,
    #[arg(long)]
    pub activate: bool,
}

pub fn command(
    ports: &cpd_core::ports::Ports,
    args: WebUiArgs,
) -> Result<cpd_core::output::Output, CpdError> {
    use cpd_core::output::Output;
    let home = CpdHome::from_env()?;
    let run = home.run_dir();
    std::fs::create_dir_all(&run)
        .map_err(|e| CpdError::new(ErrorCode::SetupFailed, e.to_string()))?;
    let lock_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(run.join("webui-prepare.lock"))
        .map_err(|e| CpdError::new(ErrorCode::SetupFailed, e.to_string()))?;
    if !cpd_core::lock::try_lock(&lock_file)
        .map_err(|e| CpdError::new(ErrorCode::SetupFailed, e.to_string()))?
    {
        return Err(CpdError::new(
            ErrorCode::SetupFailed,
            "Another WebUI preparation command is running.",
        ));
    }
    let staging = run.join("webui-preparing.pid");
    let preparing = std::fs::read_to_string(&staging)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .filter(|pid| *pid > 0 && ports.spawn.alive(Pid(*pid)))
        .filter(|pid| {
            ports
                .spawn
                .cmdline(Pid(*pid))
                .is_some_and(|cmd| is_same_service(ServiceKind::Prod, &cmd))
        });
    let record: serde_json::Value = std::fs::read_to_string(run.join("webui-prepared.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if args.prepare {
        if let Some(pid) = preparing
            && !(record["pid"].as_u64() == Some(u64::from(pid)) && record["phase"] == "active")
        {
            return Ok(Output::success(json!({"pid": pid})));
        }
        let Some(checkout) = resolve_checkout_path(&home) else {
            return Ok(Output::success(json!({})));
        };
        let launch = Launch {
            program: "node".into(),
            args: vec!["web/src/server/serve.ts".into()],
            cwd: Some(checkout),
            setsid: true,
            env_set: vec![
                ("CPD_HOME".into(), home.root().display().to_string()),
                ("CPD_WEBUI_PREPARE".into(), "1".into()),
            ],
            stdout_to: Some(run.join("webui-prepared.log")),
            ..Launch::default()
        };
        let pid = ports
            .spawn
            .spawn(&launch)
            .map_err(|e| CpdError::new(ErrorCode::SpawnFailed, e.to_string()))?;
        cpd_core::atomic::write_atomic(&staging, pid.0.to_string().as_bytes())
            .map_err(|e| CpdError::new(ErrorCode::SetupFailed, e.to_string()))?;
        return Ok(Output::success(json!({"pid": pid.0})));
    }
    let ready = preparing.is_some()
        && record["pid"].as_u64() == preparing.map(u64::from)
        && record["ready"] == true;
    if args.activate {
        if !ready {
            return Err(CpdError::new(
                ErrorCode::SetupFailed,
                "The prepared WebUI is not ready; the current listener is untouched.",
            ));
        }
        cpd_core::atomic::write_atomic(
            &run.join("webui-activate.pid"),
            preparing.unwrap().to_string().as_bytes(),
        )
        .map_err(|e| CpdError::new(ErrorCode::SetupFailed, e.to_string()))?;
    }
    Ok(Output::success(
        json!({"ready": ready, "phase": record["phase"], "pid": preparing}),
    ))
}
