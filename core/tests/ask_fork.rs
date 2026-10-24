//! The retired `@juicesharp/rpiv-ask-user-question` package and the `lead_pi_packages` default (contract: the
//! ask_user_question extension; the cpd adapter carries its own copy).

use std::path::Path;

use cpd_core::config::Config;
use cpd_core::host::pi::{PiDirs, extensions_to_pass};

const PLANNOTATOR: &str = "npm:@plannotator/pi-extension@0.28.17";
const RETIRED: &str = "npm:@juicesharp/rpiv-ask-user-question@2.13.8";

#[test]
fn retired_package_is_not_passed_and_logged() {
    let root = tempfile::tempdir().unwrap();
    let dirs = PiDirs {
        home: root.path().join("home"),
        agent_dir: root.path().join("home/.pi/agent"),
        session_dir: None,
    };
    let cwd = root.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();
    let candidates: Vec<String> = ["/opt/pi", PLANNOTATOR, RETIRED, "npm:@other/pkg@1.0.0"].map(str::to_owned).to_vec();
    let mut log = Vec::new();
    let kept = extensions_to_pass(&candidates, &cwd, &dirs, &mut log);
    assert_eq!(kept, ["/opt/pi", PLANNOTATOR, "npm:@other/pkg@1.0.0"]);
    assert_eq!(
        String::from_utf8(log).unwrap(),
        format!("cpd: not passing -e {RETIRED}: cpd's adapter carries its own copy of it\n")
    );
}

#[test]
fn default_lead_pi_packages_is_plannotator_only() {
    let (config, _) = Config::parse("version = 1\nmachine = \"hs\"\n", Path::new("/config.toml")).unwrap();
    assert_eq!(config.lead_pi_packages, [PLANNOTATOR]);
}
