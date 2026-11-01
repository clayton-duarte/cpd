//! The healthy machine of `cpd doctor` (commands.md §14.1) on an [`Fx`]: git, tmux, pi, gh and pi's Node answering what
//! passes every check, as in-process routes of the `FakeRunner`, and the pi install on disk the checks locate.
//!
//! A route answers only a call no rule of the `FakeRunner` matched, so a test that scripts a tool (`Fx::fake`, or a
//! rule with a matcher) overrides it whatever the order; the other tools stay healthy.
//!
//! What is on disk is no program: `locate_pi` (contract §9) wants an executable file named `pi` on `PATH` whose
//! realpath is two levels under the agent directory, with `install/current-version` and the release's two entry
//! points. They are empty files, never run: the `FakeRunner` answers the calls.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use cpd_core::host::runner::{Call, Handler, Reply};

use super::fx::Fx;

/// The versions the routes print: the minimum of each tool (commands.md §14.1) and pi's Node.
pub const GIT: &str = "git version 2.43.0";
pub const TMUX: &str = "tmux 3.6";
pub const PI: &str = "1.2.0";
pub const GH: &str = "gh version 2.45.0 (2025-07-18 Ubuntu 2.45.0-1ubuntu0.3)";
pub const NODE: &str = "v22.23.2";

/// The pi release the install on disk holds (`install/current-version`).
pub const PI_RELEASE: &str = "1.2.0";

/// The levels of pi's model `p/m-low`; every other `p/m-*` has them all. A model of another provider, or whose id does
/// not start with `m-`, is not in pi's catalog.
const LOW: [&str; 4] = ["off", "minimal", "low", "medium"];
const ALL: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

struct Healthy;

impl Handler for Healthy {
    fn handle(&self, call: &Call) -> Reply {
        let tool = call.program.rsplit('/').next().unwrap_or(&call.program);
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match (tool, args.as_slice()) {
            ("git", _) => Reply::ok(&format!("{GIT}\n")),
            ("tmux", _) => Reply::ok(&format!("{TMUX}\n")),
            ("pi", _) => Reply::ok(&format!("{PI}\n")),
            ("gh", ["auth", ..]) => Reply::ok(""),
            ("gh", _) => Reply::ok(&format!("{GH}\n")),
            ("node", ["--version"]) => Reply::ok(&format!("{NODE}\n")),
            ("node", ["--input-type=module", ..]) => Reply::ok(&catalog(&args)),
            _ => Reply::exit(97, "", &format!("no healthy answer for {tool} {args:?}")),
        }
    }
}

/// What the catalog module prints (`core/src/config/catalog.mjs`): for each `<letter>=<provider>/<model>` after `--`,
/// the model's thinking levels, or `null` when pi has no such model.
fn catalog(args: &[&str]) -> String {
    let start = args
        .iter()
        .position(|a| *a == "--")
        .map_or(args.len(), |i| i + 3);
    let mut out = serde_json::Map::new();
    for tier in args.get(start..).unwrap_or_default() {
        let (letter, model) = tier.split_once('=').expect("a tier is <letter>=<model>");
        let levels = match model.split_once('/') {
            Some(("p", "m-low")) => serde_json::json!(LOW),
            Some(("p", id)) if id.starts_with("m-") => serde_json::json!(ALL),
            _ => serde_json::Value::Null,
        };
        out.insert(letter.to_owned(), levels);
    }
    format!("{}\n", serde_json::Value::Object(out))
}

impl Fx {
    /// Makes the machine's tools healthy: routes for git, tmux, pi, gh and node on the `FakeRunner`, and the pi install
    /// on disk ([`Fx::install_pi`]).
    pub fn healthy_tools(&self) {
        let healthy: Arc<dyn Handler> = Arc::new(Healthy);
        let mut runner = (*self.fakes.runner).clone();
        for program in ["git", "tmux", "pi", "gh", "node"] {
            runner = runner.route(program, healthy.clone());
        }
        self.install_pi();
    }

    /// pi on the test's `PATH` (`bin/pi`) with its agent directory the test root: `install/current-version` and the two
    /// entry points of the release, which `locate_pi` looks for and nothing runs.
    pub fn install_pi(&self) {
        let pi = self.env.bin().join("pi");
        fs::write(&pi, "").expect("write bin/pi");
        fs::set_permissions(&pi, fs::Permissions::from_mode(0o755)).expect("chmod bin/pi");
        let install = self.env.root().join("install");
        fs::create_dir_all(&install).expect("create install");
        fs::write(install.join("current-version"), format!("{PI_RELEASE}\n"))
            .expect("write current-version");
        let scope = install
            .join("releases")
            .join(PI_RELEASE)
            .join("node_modules/@earendil-works");
        for package in ["pi-coding-agent", "pi-ai"] {
            let dist = scope.join(package).join("dist");
            fs::create_dir_all(&dist).expect("create dist");
            fs::write(dist.join("index.js"), "").expect("write index.js");
        }
    }
}
