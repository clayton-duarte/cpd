use crate::common::{fx::Fx, refused};
use cpd_core::host::spawn::{Pid, Signal, Spawn};
use serde_json::json;

#[test]
fn real_webui_commands_prepare_validate_activate_and_reject_dead_records() {
    let fx = Fx::with_ports();
    let entry = fx.env.cpd_home().join("web/src/server/serve.ts");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(entry, "// fixture").unwrap();
    let prepared = fx.cpd(&["webui", "--prepare"]).ok();
    let pid = prepared["pid"].as_u64().unwrap() as u32;
    fx.fakes
        .spawn
        .set_cmdline(Pid(pid), "node web/src/server/serve.ts");
    assert_eq!(fx.cpd(&["webui", "--prepare"]).ok()["pid"], pid);
    assert_eq!(fx.cpd(&["webui", "--ready"]).ok()["ready"], false);
    refused(&fx.cpd(&["webui", "--activate"]), "setup_failed", 1);
    let run = fx.env.cpd_home().join("run");
    std::fs::write(
        run.join("webui-prepared.json"),
        json!({"pid":pid,"ready":true,"phase":"staged"}).to_string(),
    )
    .unwrap();
    assert_eq!(fx.cpd(&["webui", "--ready"]).ok()["ready"], true);
    fx.cpd(&["webui", "--activate"]).ok();
    assert_eq!(
        std::fs::read_to_string(run.join("webui-activate.pid")).unwrap(),
        pid.to_string()
    );
    // A later install must prepare a fresh process, not reuse the previous active generation.
    std::fs::write(
        run.join("webui-prepared.json"),
        json!({"pid":pid,"ready":true,"phase":"active"}).to_string(),
    )
    .unwrap();
    let later = fx.cpd(&["webui", "--prepare"]).ok();
    assert_ne!(later["pid"], pid);
    fx.fakes.spawn.signal(Pid(pid), Signal::Term).unwrap();
    assert_eq!(fx.cpd(&["webui", "--ready"]).ok()["ready"], false);
}
