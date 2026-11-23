use crate::common::{fx::Fx, refused};
use cpd_core::host::spawn::{Launch, Pid, Spawn};
use serde_json::json;

#[test]
fn real_webui_commands_prepare_validate_activate_and_reject_dead_records() {
    let fx = Fx::with_ports();
    let run = fx.env.cpd_home().join("run");
    std::fs::create_dir_all(&run).unwrap();
    let pid = fx.fakes.spawn.spawn(&Launch::default()).unwrap();
    fx.fakes
        .spawn
        .set_cmdline(pid, "node web/src/server/serve.ts");
    std::fs::write(run.join("webui-preparing.pid"), pid.0.to_string()).unwrap();
    let record = json!({"pid":pid.0,"ready":true,"phase":"staged"}).to_string();
    std::fs::write(run.join("webui-prepared.json"), &record).unwrap();
    let launches = fx.fakes.spawn.launches().len();

    assert_eq!(fx.cpd(&["webui", "--ready"]).ok()["ready"], true);
    for flag in ["--prepare", "--activate"] {
        let error = refused(&fx.cpd(&["webui", flag]), "setup_failed", 1);
        assert!(
            error["message"]
                .as_str()
                .unwrap()
                .contains("handoff is disabled")
        );
    }
    assert_eq!(fx.fakes.spawn.launches().len(), launches);
    assert!(fx.fakes.spawn.signals().is_empty());
    assert!(!run.join("webui-activate.pid").exists());
    assert_eq!(
        std::fs::read_to_string(run.join("webui-prepared.json")).unwrap(),
        record
    );
    assert_eq!(
        std::fs::read_to_string(run.join("webui-preparing.pid")).unwrap(),
        pid.0.to_string()
    );

    fx.fakes.spawn.exit(Pid(pid.0), 0);
    assert_eq!(fx.cpd(&["webui", "--ready"]).ok()["ready"], false);
}
