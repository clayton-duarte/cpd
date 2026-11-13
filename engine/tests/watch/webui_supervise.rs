//! Red tests for engine supervision of WebUI prod (3333) and preview (33333) servers.
//!
//! Context:
//! When app-root and preview worktree exist, engine restart spawns both 3333
//! (prod server serve.ts with cwd=checkout from app-root) and 33333 (preview vite server).
//!
//! Tests:
//! - `engine_restart_spawns_prod_and_preview_servers`

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cpd_core::app::write_root;
use cpd_core::clock::ManualClock;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::FakeRunner;
use cpd_core::host::spawn::{FakeSpawn, Spawn};
use cpd_core::host::tmux::Tmux;
use cpd_core::ports::Ports;
use cpd_engine::watch::engine::Engine;
use cpd_engine::watch::webui::{self, ServiceKind};
use tempfile::TempDir;

/// Test fixture providing an isolated temporary environment (home, checkout app-root, preview worktree, fake ports).
struct SuperviseFx {
    _temp: TempDir,
    pub home: CpdHome,
    pub fake_user_home: PathBuf,
    pub checkout_dir: PathBuf,
    pub preview_dir: PathBuf,
    pub clock: Arc<ManualClock>,
    pub spawn: Arc<FakeSpawn>,
    pub runner: FakeRunner,
}

impl SuperviseFx {
    fn new() -> Self {
        let temp = TempDir::new().expect("tempdir");
        let temp_path = temp.path();

        let fake_user_home = temp_path.join("user_home");
        let home_dir = fake_user_home.join(".cpd");
        let checkout_dir = temp_path.join("projects").join("cpd");
        // Install creates preview worktree at checkout's parent: <checkout parent>/cpd.preview.web
        let preview_dir = temp_path.join("projects").join("cpd.preview.web");

        fs::create_dir_all(&home_dir).expect("create cpd home dir");
        fs::create_dir_all(&checkout_dir).expect("create checkout dir");
        fs::create_dir_all(&preview_dir).expect("create preview dir");

        let home = CpdHome::resolve(Some(home_dir.into_os_string()), None).expect("resolve home");
        fs::create_dir_all(home.run_dir()).expect("create run dir");

        // Write minimal valid config.toml
        let toml = r#"
version = 1
machine = "m1"
lead_pi_packages = []
[watch]
tick_seconds = 1
janitor_minutes = 60
[timeouts]
store_busy_ms = 5000
archive_sleep_seconds = 86400
"#;
        fs::write(home.root().join("config.toml"), toml).expect("write config.toml");

        // Write app-root pointing to checkout_dir in fake_user_home
        write_root(&fake_user_home, &checkout_dir).expect("write app-root");

        // Place web/src/server/serve.ts only under checkout_dir, NOT under CPD_HOME
        let serve_ts = checkout_dir.join("web/src/server/serve.ts");
        fs::create_dir_all(serve_ts.parent().unwrap()).expect("create web server dir");
        fs::write(&serve_ts, "// prod server").expect("write serve.ts");

        // Place package.json in preview_dir
        fs::write(
            preview_dir.join("package.json"),
            r#"{"name":"cpd-preview"}"#,
        )
        .expect("write preview package.json");

        let clock = Arc::new(ManualClock::new());
        let runner = FakeRunner::new((*clock).clone());
        let spawn = Arc::new(FakeSpawn::new());

        SuperviseFx {
            _temp: temp,
            home,
            fake_user_home,
            checkout_dir,
            preview_dir,
            clock,
            spawn,
            runner,
        }
    }

    fn ports(&self) -> Ports {
        Ports {
            clock: self.clock.clone(),
            runner: Arc::new(self.runner.clone()),
            spawn: self.spawn.clone(),
            ..Ports::real()
        }
    }

    fn engine(&self) -> Engine {
        let ports = self.ports();
        let tmux = Tmux::new("test");
        Engine::with_ports(self.home.clone(), tmux, &ports)
    }
}

/// Red test proving engine supervises 3333 from checkout app-root and 33333 from preview worktree on restart.
///
/// When app-root and preview worktree exist, engine restart spawns both 3333
/// (prod server serve.ts with cwd=checkout) and 33333 (preview vite server).
#[test]
pub fn engine_restart_spawns_prod_and_preview_servers() {
    let fx = SuperviseFx::new();

    // Set HOME to fake_user_home so read_root or home resolution finds app-root
    let prev_home = std::env::var_os("HOME");
    unsafe {
        std::env::set_var("HOME", &fx.fake_user_home);
        // Ensure CPD_WEB_PREVIEW_WORKTREE is unset so preview worktree is resolved via app-root/checkout
        std::env::remove_var("CPD_WEB_PREVIEW_WORKTREE");
    }

    struct RestoreEnv(Option<std::ffi::OsString>);
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            unsafe {
                if let Some(ref prev) = self.0 {
                    std::env::set_var("HOME", prev);
                } else {
                    std::env::remove_var("HOME");
                }
            }
        }
    }
    let _restore = RestoreEnv(prev_home);

    // First engine instance runs supervision
    let mut engine1 = fx.engine();
    webui::run(&mut engine1);

    // Simulate engine restart: new Engine instance on the same home and ports
    let mut engine2 = fx.engine();
    webui::run(&mut engine2);

    // 1. Both services must be tracked in supervised services
    assert!(
        engine2.webui.services.contains_key(&ServiceKind::Prod),
        "prod server (3333) should be supervised from app-root checkout on restart"
    );
    assert!(
        engine2.webui.services.contains_key(&ServiceKind::Preview),
        "preview server (33333) should be supervised from preview worktree on restart"
    );

    // 2. Both services must have launched child processes
    let prod_pid = engine2.webui.services[&ServiceKind::Prod]
        .pid
        .expect("prod pid should be set after restart");
    let preview_pid = engine2.webui.services[&ServiceKind::Preview]
        .pid
        .expect("preview pid should be set after restart");

    assert!(fx.spawn.alive(prod_pid), "prod process must be alive");
    assert!(fx.spawn.alive(preview_pid), "preview process must be alive");

    // 3. Verify launches: prod launched with cwd = checkout_dir and preview launched with cwd = preview_dir
    let launches = fx.spawn.launches();
    let prod_launch = launches
        .iter()
        .find(|l| l.args.iter().any(|a| a.contains("serve.ts")))
        .expect("prod server launch should be recorded");
    assert_eq!(
        prod_launch.cwd.as_ref(),
        Some(&fx.checkout_dir),
        "prod server must be launched with cwd = checkout_dir resolved from app-root"
    );

    let preview_launch = launches
        .iter()
        .find(|l| l.program == "pnpm" && l.args.iter().any(|a| a == "dev"))
        .expect("preview server launch should be recorded");
    assert_eq!(
        preview_launch.cwd.as_ref(),
        Some(&fx.preview_dir),
        "preview server must be launched with cwd = preview_dir resolved from checkout"
    );
}
