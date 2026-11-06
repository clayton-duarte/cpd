//! `cpd app` (commands.md §14.4a): the CLI entry. The work is `cpd_core::app` over `Ports`; this module reads the
//! environment once and maps the answers to `Output`.

use std::path::PathBuf;

use serde_json::json;

use cpd_core::app::{self, App, LaunchAction, Preview};
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::Ports;

/// Launch the Electron app of the installed checkout, or run its web preview (commands.md §14.4a)
#[derive(clap::Args, Clone, Debug, PartialEq, Eq)]
#[command(group(clap::ArgGroup::new("form").args(["preview", "reload", "off", "status"]).multiple(false)))]
#[command(group(clap::ArgGroup::new("takes_worktree").args(["preview", "reload"])))]
pub struct Args {
    /// Start the web preview of `<worktree>` (`scripts/preview-web.sh on`)
    #[arg(long)]
    pub preview: bool,
    /// Rebuild and reload the web preview (`scripts/preview-web.sh reload`)
    #[arg(long)]
    pub reload: bool,
    /// Stop the web preview (`scripts/preview-web.sh off`)
    #[arg(long)]
    pub off: bool,
    /// Report the web preview and the Electron app (`scripts/preview-web.sh status`)
    #[arg(long)]
    pub status: bool,
    /// The worktree of `--preview` or `--reload`
    #[arg(requires = "takes_worktree", conflicts_with_all = ["off", "status"])]
    pub worktree: Option<PathBuf>,
}

pub fn run(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let app = resolve()?;
    let tree = args.worktree.clone();
    if args.preview {
        let output = app::preview(ports, &app, Preview::On(tree))?;
        return Ok(Output::success(
            json!({"action": "preview_on", "output": output}),
        ));
    }
    if args.reload {
        let output = app::preview(ports, &app, Preview::Reload(tree))?;
        return Ok(Output::success(
            json!({"action": "reloaded_preview", "output": output}),
        ));
    }
    if args.off {
        let output = app::preview(ports, &app, Preview::Off)?;
        return Ok(Output::success(
            json!({"action": "preview_off", "output": output}),
        ));
    }
    if args.status {
        let status = app::status(ports, &app)?;
        let electron = status
            .electron
            .map(|e| json!({"pid": e.pid, "running": e.running}));
        return Ok(Output::success(
            json!({"action": "status", "output": status.output, "electron": electron}),
        ));
    }
    let launched = app::launch(ports, &app)?;
    let action = match launched.action {
        LaunchAction::Launched => "launched",
        LaunchAction::Reloaded => "reloaded",
    };
    Ok(Output::success(
        json!({"action": action, "pid": launched.pid}),
    ))
}

/// The `App` of `app-root` and the environment, read once at this edge.
fn resolve() -> Result<App, CpdError> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
        CpdError::new(
            ErrorCode::Internal,
            "HOME is not set: cannot find app-root.",
        )
    })?;
    let preview_dir = std::env::var_os("CPD_PREVIEW_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache/cpd-preview"));
    Ok(App {
        root: app::read_root(&home)?,
        cpd_home: CpdHome::from_env()?.root().to_path_buf(),
        preview_dir,
        macos: cfg!(target_os = "macos"),
        home,
    })
}
