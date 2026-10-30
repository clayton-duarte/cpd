//! hs-d738, hs-d739: the config of cpd without plannotator: no `plannotator_ports` key, its migration
//! `drop-plannotator-ports`, `lead_pi_packages` defaulting to `[]`, and a Plan's Lead pane without `PLANNOTATOR_*`.

use crate::common;

use std::fs;

use common::Run;
use common::fx::Fx;
use serde_json::json;

const MINIMAL: &str = "version = 1\nmachine = \"hs\"\n";

fn config_with(fx: &Fx, text: &str) -> Run {
    fx.env.write_config(text);
    fx.cpd(&["config"])
}

#[test]
fn drop_plannotator_config_without_ports_validates_and_shows_no_such_key() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert!(out["config"].get("plannotator_ports").is_none(), "{out}");
    assert!(out["sources"].get("plannotator_ports").is_none(), "{out}");
}

#[test]
fn drop_plannotator_config_with_ports_is_config_invalid_on_that_key() {
    let fx = Fx::new();
    let run = config_with(&fx, &format!("{MINIMAL}plannotator_ports = \"19432-19456\"\n"));
    let err = run.error();
    assert_eq!(run.exit, 1);
    assert_eq!(err["code"], "config_invalid", "{err}");
    assert_eq!(err["details"]["key"], "plannotator_ports", "{err}");
}

const WITH_PORTS: &str = "version = 1\nmachine = \"hs\"\n# keep me\nplannotator_ports = \"19432-19456\"\nconcurrency = 5  # and me\n";
const WITHOUT_PORTS: &str = "version = 1\nmachine = \"hs\"\n# keep me\nconcurrency = 5  # and me\n";

#[test]
fn drop_plannotator_config_migrate_drops_ports_and_keeps_comments() {
    let fx = Fx::new();
    let path = fx.env.cpd_home().join("config.toml");
    fx.env.write_config(WITH_PORTS);
    let out = fx.cpd(&["config", "--migrate"]).ok();
    let migrated = out["migrated"].as_array().unwrap();
    assert!(migrated.contains(&json!("drop-plannotator-ports")), "{out}");
    assert_eq!(fs::read_to_string(&path).unwrap(), WITHOUT_PORTS);
}

#[test]
fn drop_plannotator_config_migrate_twice_changes_nothing() {
    let fx = Fx::new();
    let path = fx.env.cpd_home().join("config.toml");
    fx.env.write_config(WITH_PORTS);
    let first = fx.cpd(&["config", "--migrate"]).ok();
    assert!(first["migrated"].as_array().unwrap().contains(&json!("drop-plannotator-ports")), "{first}");
    let before = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();

    let out = fx.cpd(&["config", "--migrate"]).ok();
    assert_eq!(out["migrated"], json!([]), "{out}");
    assert_eq!(fs::read(&path).unwrap(), before, "the file was rewritten");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified, "the file was written again");
}

#[test]
fn drop_plannotator_lead_pi_packages_default_is_empty() {
    let fx = Fx::new();
    let out = config_with(&fx, MINIMAL).ok();
    assert_eq!(out["config"]["lead_pi_packages"], json!([]), "{out}");
    assert_eq!(out["sources"]["lead_pi_packages"], "default", "{out}");
}

#[test]
fn drop_plannotator_plan_new_pane_env_has_no_plannotator_variable() {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    fx.keyed_repo("pay");
    let brief = fx.env.root().join("brief.md");
    fs::write(&brief, "Retry the failed payouts.\n").unwrap();
    let home_dir = fx.env.root().join("home");
    fs::create_dir_all(&home_dir).unwrap();

    let args = ["plan", "new", "pay", "--context", "ctx", "--title", "Retry payouts", "--brief-file", brief.to_str().unwrap()];
    let vars = [("PLANNOTATOR_PORT", "19432-19439"), ("PLANNOTATOR_REMOTE", "0"), ("PLANNOTATOR_URL_HOST", "review.test")];
    let out = fx.cpd_with(&home_dir, &args, &vars).ok();
    let session = out["plan"]["lead_session"].as_str().unwrap().to_owned();
    let pane = out["lead"]["pane"].as_str().unwrap().to_owned();
    let state = fx.tmux_state().expect("plan new started the cockpit server");
    let pane_vars = state.pane_env(&pane);
    for (key, value) in [("CPD_HOME", fx.env.cpd_home().display().to_string()), ("CPD_SESSION", session), ("CPD_PLAN", "pay".to_owned())] {
        assert!(pane_vars.contains(&(key.to_owned(), value.clone())), "{key}={value} not in {pane_vars:?}");
    }
    let leaked: Vec<_> = pane_vars.iter().filter(|(k, _)| k.starts_with("PLANNOTATOR_")).collect();
    assert!(leaked.is_empty(), "the Lead's pane holds {leaked:?}");
}
