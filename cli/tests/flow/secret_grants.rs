//! `cpd secret allow` and `cpd secret approve` (commands.md §24.6, §24.7): the refusals and the grant files.

use crate::secret::{Fx, Who, approve_args, denied, strs};

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn allow(fx: &Fx, who: Who, attempt: &str, vault: &str, item: &str) -> crate::common::Run {
    let a = [
        "secret".to_string(),
        "allow".into(),
        format!("--attempt={attempt}"),
        format!("--vault={vault}"),
        "--field=password".into(),
        "--".into(),
        item.into(),
    ];
    fx.run(who, &strs(&a), None)
}

#[test]
fn allow_names_an_attempt_of_the_leads_own_plan() {
    let fx = Fx::new();
    for attempt in ["b-a1", "no-such-a1"] {
        let r = allow(&fx, Who::Lead, attempt, "cpd", "deployment");
        denied(&r, "attempt_not_found", 4);
        fx.assert_op_not_called(&r);
    }
    assert!(
        !fx.env
            .cpd_home()
            .join("run/secret/allow/b-a1.json")
            .exists()
    );
}

#[test]
fn allow_refuses_an_ended_attempt() {
    let fx = Fx::new();
    let r = allow(&fx, Who::Lead, "e-a1", "cpd", "deployment");
    denied(&r, "secret_attempt_ended", 3);
    assert!(
        !fx.env
            .cpd_home()
            .join("run/secret/allow/e-a1.json")
            .exists()
    );
    fx.assert_op_not_called(&r);
}

#[test]
fn allow_writes_a_private_grant_file_and_is_idempotent() {
    let fx = Fx::new();
    let file = fx.env.cpd_home().join("run/secret/allow/a-a1.json");

    let r = allow(&fx, Who::Lead, "a-a1", "cpd", "deployment");
    let out = r.ok();
    assert_eq!(out["already"], false, "{r:?}");
    assert_eq!(out["attempt"], "a-a1", "{r:?}");
    assert_eq!(mode(&file), 0o600);
    assert_eq!(mode(file.parent().unwrap()), 0o700);
    let first = fs::read_to_string(&file).unwrap();
    let held: Value = serde_json::from_str(&first).unwrap();
    assert_eq!(
        held,
        json!([{"vault": "cpd", "item": "deployment", "field": "password"}])
    );

    let r = allow(&fx, Who::Lead, "a-a1", "cpd", "deployment");
    assert_eq!(r.ok()["already"], true, "{r:?}");
    assert_eq!(fs::read_to_string(&file).unwrap(), first);

    let r = allow(&fx, Who::Lead, "a-a1", "homeserver", "token");
    assert_eq!(r.ok()["already"], false, "{r:?}");
    let held: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(
        held,
        json!([
            {"vault": "cpd", "item": "deployment", "field": "password"},
            {"vault": "homeserver", "item": "token", "field": "password"}
        ])
    );
    assert_eq!(mode(&file), 0o600);
    fx.assert_op_not_called(&r);
}

#[test]
fn allow_refuses_another_vault() {
    let fx = Fx::new();
    let r = allow(&fx, Who::Lead, "a-a1", "other", "deployment");
    denied(&r, "secret_vault_denied", 3);
    assert!(
        !fx.env
            .cpd_home()
            .join("run/secret/allow/a-a1.json")
            .exists()
    );
    fx.assert_op_not_called(&r);
}

fn approvals(fx: &Fx) -> Vec<Value> {
    let dir = fx.env.cpd_home().join("run/secret/approve");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .map(|e| {
            let path = e.unwrap().path();
            assert_eq!(
                path.extension().and_then(|e| e.to_str()),
                Some("json"),
                "{path:?}"
            );
            assert_eq!(mode(&path), 0o600, "{path:?}");
            assert_eq!(mode(&dir), 0o700);
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap()
        })
        .collect()
}

fn approve(
    fx: &Fx,
    who: Who,
    action: &str,
    item: Option<&str>,
    field: Option<&str>,
) -> crate::common::Run {
    fx.run(
        who,
        &strs(&approve_args(action, "other", item, field)),
        None,
    )
}

#[test]
fn approve_writes_a_private_grant_file() {
    let fx = Fx::new();
    for (who, session) in [(Who::Lead, "lead1"), (Who::Manager, "mgr1")] {
        let before = approvals(&fx).len();
        let r = approve(&fx, who, "get", Some("deployment"), Some("password"));
        let out = r.ok();
        assert_eq!(out["action"], "get", "{r:?}");
        assert_eq!(out["vault"], "other", "{r:?}");
        let files = approvals(&fx);
        assert_eq!(files.len(), before + 1, "{r:?}");
        let file = files
            .iter()
            .find(|f| f["session"] == session)
            .unwrap_or_else(|| panic!("no file of {session}: {files:?}"));
        let mut keys: Vec<&str> = file
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["action", "at", "field", "item", "session", "vault"]);
        assert_eq!(file["action"], "get");
        assert_eq!(file["vault"], "other");
        assert_eq!(file["item"], "deployment");
        assert_eq!(file["field"], "password");
        let at = OffsetDateTime::parse(file["at"].as_str().unwrap(), &Rfc3339).unwrap();
        let expires = OffsetDateTime::parse(out["expires_at"].as_str().unwrap(), &Rfc3339).unwrap();
        assert_eq!(expires - at, time::Duration::minutes(10), "{r:?}");
    }

    let r = approve(&fx, Who::Lead, "list", None, None);
    r.ok();
    let files = approvals(&fx);
    let list = files.iter().find(|f| f["action"] == "list").unwrap();
    assert_eq!(list["item"], Value::Null);
    assert_eq!(list["field"], Value::Null);
    assert_eq!(list["vault"], "other");
    fx.assert_op_not_called(&r);
}

#[test]
fn approve_is_for_a_lead_or_a_manager_and_another_vault() {
    let fx = Fx::new();
    // the neighbouring case only the implementation answers
    let r = approve(
        &fx,
        Who::Worker,
        "get",
        Some("deployment"),
        Some("password"),
    );
    denied(&r, "secret_role_denied", 3);

    for vault in ["cpd", "homeserver"] {
        for who in [Who::Lead, Who::Manager] {
            let a = approve_args("get", vault, Some("deployment"), Some("password"));
            let r = fx.run(who, &strs(&a), None);
            denied(&r, "usage", 2);
        }
    }
    let r = approve(&fx, Who::Lead, "get", None, Some("password"));
    denied(&r, "usage", 2);
    assert!(approvals(&fx).is_empty());
    fx.assert_op_not_called(&r);
}
