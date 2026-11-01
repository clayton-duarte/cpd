//! `cpd session list` and archived Sessions (commands.md §10.1, hs-d516): archived ones are left out unless
//! `--archived` is given, which lists them too. The command tree runs in this process (`Fx`).

use crate::common::TestEnv;
use crate::common::fx::Fx;
use crate::common::session::{home, setup};
use cpd_core::store::Store;

use serde_json::Value;

fn ids(out: &Value) -> Vec<String> {
    out["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_owned())
        .collect()
}

/// The standard Sessions with Worker `worker1` archived, as E12 archives an ended Attempt's Worker.
fn archived_worker(env: &TestEnv) {
    setup(env);
    Store::open(&home(env), 5000)
        .unwrap()
        .write("fixture", |tx| {
            tx.execute_batch(
                "UPDATE sessions SET state = 'stopped', archived_at = 't9' WHERE id = 'worker1'",
            )?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn list_leaves_archived_sessions_out() {
    let fx = Fx::new();
    archived_worker(&fx.env);

    let listed = ids(&fx.cpd(&["session", "list"]).ok());

    assert!(!listed.contains(&"worker1".to_owned()), "{listed:?}");
    for id in ["lead1", "manager1"] {
        assert!(
            listed.contains(&id.to_owned()),
            "{id} is listed: {listed:?}"
        );
    }
}

#[test]
fn archived_flag_lists_them_too() {
    let fx = Fx::new();
    archived_worker(&fx.env);

    let out = fx.cpd(&["session", "list", "--archived"]).ok();

    let worker = out["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == "worker1")
        .expect("worker1 is listed");
    assert_eq!(worker["archived_at"], "t9");
}
