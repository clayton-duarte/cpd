//! The Permanent Contexts `cpd`, `host` and `codereview` (commands.md §3.8, §3.9, §3.11, §3.15; contract §2.2;
//! decisions pro-d338..pro-d343), in this process (`Fx::with_ports`): `context archive|rename|delete` on one is refused
//! `context_permanent`, `context manager` creates a missing one, and every Manager and Lead starts in the User's home.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::refused;
use serde_json::json;

const PERMANENT: [&str; 3] = ["cpd", "host", "codereview"];

/// The Context `id` with a Plan `pay` and its Lead, and the config.
fn world(fx: &Fx, id: &str) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context",
        "new",
        id,
        "--title",
        "Permanent",
        "--owner",
        "acme",
    ])
    .ok();
    fx.cpd(&["plan", "new", "pay", "--context", id, "--title", "Plan pay"])
        .ok();
}

/// Every row the guarded commands could change: the Context, its Plans and Sessions, and the Events.
fn snapshot(fx: &Fx) -> String {
    let rows = |sql: &str| -> String { fx.env.query::<Option<String>>(sql).unwrap_or_default() };
    [
        rows("SELECT group_concat(id || '|' || title || '|' || ifnull(archived_at, '') || '|' || ifnull(deleted_at, ''), ';') FROM contexts"),
        rows("SELECT group_concat(id || '|' || context || '|' || title || '|' || ifnull(archived_at, ''), ';') FROM plans"),
        rows("SELECT group_concat(id || '|' || context || '|' || state || '|' || ifnull(archived_at, ''), ';') FROM sessions"),
        rows("SELECT CAST(count(*) AS TEXT) FROM events"),
        rows("SELECT CAST(count(*) AS TEXT) FROM txns"),
    ]
    .join("\n")
}

#[test]
fn rename_refuses_each_permanent_context_with_context_permanent_and_exit_3() {
    for id in PERMANENT {
        let fx = Fx::with_ports();
        world(&fx, id);
        let before = snapshot(&fx);
        let error = refused(
            &fx.cpd(&["context", "rename", id, "--title", "Another title"]),
            "context_permanent",
            3,
        );
        assert_eq!(error["details"]["context"], json!(id));
        assert_eq!(snapshot(&fx), before, "{id}: a refusal writes nothing");
    }
}

#[test]
fn archive_refuses_each_permanent_context_and_writes_nothing() {
    for id in PERMANENT {
        let fx = Fx::with_ports();
        world(&fx, id);
        let before = snapshot(&fx);
        let error = refused(&fx.cpd(&["context", "archive", id]), "context_permanent", 3);
        assert_eq!(error["details"]["context"], json!(id));
        assert_eq!(snapshot(&fx), before, "{id}: a refusal writes nothing");
    }
}

#[test]
fn delete_refuses_each_permanent_context_even_with_yes_and_writes_nothing() {
    for id in PERMANENT {
        let fx = Fx::with_ports();
        world(&fx, id);
        let before = snapshot(&fx);
        for args in [
            vec!["context", "delete", id],
            vec!["context", "delete", id, "--yes"],
        ] {
            let error = refused(&fx.cpd(&args), "context_permanent", 3);
            assert_eq!(error["details"]["context"], json!(id));
            assert_eq!(
                snapshot(&fx),
                before,
                "{id} {args:?}: a refusal writes nothing"
            );
        }
    }
}

#[test]
fn context_manager_creates_a_missing_permanent_context_and_its_manager() {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cockpit();
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM contexts WHERE id = 'codereview'"),
        0
    );
    let out = fx
        .cpd(&["context", "manager", "--context", "codereview", "--by-user"])
        .ok();
    let manager = &out["manager"];
    let id = manager["session"].as_str().unwrap();
    assert_eq!(manager["name"], json!("codereview"));
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM contexts WHERE id = 'codereview' AND archived_at IS NULL"
        ),
        1
    );
    assert_eq!(
        fx.env.query::<String>(&format!(
            "SELECT context || '|' || role FROM sessions WHERE id = '{id}'"
        )),
        "codereview|manager"
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM events WHERE kind = 'context-created' AND context = 'codereview'"
        ),
        1
    );
}

#[test]
fn a_managers_cwd_is_the_users_home_not_the_callers_directory() {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cockpit();
    fs::create_dir_all(fx.env.home()).unwrap();
    let elsewhere = fx.env.root().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let home = fx.env.home().canonicalize().unwrap().display().to_string();

    let out = fx
        .cpd_in(&elsewhere, &["context", "manager", "--context", "ctx"])
        .ok();
    let id = out["manager"]["session"].as_str().unwrap();
    assert_eq!(
        fx.cpd(&["session", "show", id]).ok()["session"]["cwd"],
        json!(home)
    );
}

#[test]
fn a_leads_cwd_is_the_users_home_not_the_callers_directory() {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fs::create_dir_all(fx.env.home()).unwrap();
    let elsewhere = fx.env.root().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let home = fx.env.home().canonicalize().unwrap().display().to_string();

    let out = fx
        .cpd_in(
            &elsewhere,
            &[
                "plan",
                "new",
                "pay",
                "--context",
                "ctx",
                "--title",
                "Plan pay",
            ],
        )
        .ok();
    assert_eq!(out["lead"]["cwd"], json!(home));
    let id = out["plan"]["lead_session"].as_str().unwrap();
    assert_eq!(
        fx.cpd(&["session", "show", id]).ok()["session"]["cwd"],
        json!(home)
    );
}

// ---------- regressions: these pass before the guards ----------

#[test]
fn a_context_with_a_similar_id_is_not_guarded() {
    for id in ["host-2", "cpd-2", "eec-codereview"] {
        let fx = Fx::with_ports();
        fx.env.write_tmux_config("");
        fx.cpd(&[
            "context", "new", id, "--title", "Similar", "--owner", "acme",
        ])
        .ok();
        fx.cpd(&["context", "rename", id, "--title", "Renamed"])
            .ok();
        fx.cpd(&["context", "archive", id]).ok();
        assert_eq!(
            fx.env.query::<i64>(&format!(
                "SELECT count(*) FROM contexts WHERE id = '{id}' AND title = 'Renamed' AND archived_at IS NOT NULL"
            )),
            1,
            "{id}"
        );
        fx.cpd(&["context", "delete", id, "--yes"]).ok();
        assert_eq!(
            fx.env.query::<i64>(&format!(
                "SELECT count(*) FROM contexts WHERE id = '{id}' AND deleted_at IS NOT NULL"
            )),
            1,
            "{id}"
        );
    }
}

#[test]
fn context_manager_on_an_ordinary_missing_context_is_still_context_not_found() {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cockpit();
    refused(
        &fx.cpd(&["context", "manager", "--context", "nosuch-ctx"]),
        "context_not_found",
        4,
    );
}

#[test]
fn a_plan_in_a_permanent_context_archives_like_any_other() {
    let fx = Fx::with_ports();
    world(&fx, "cpd");
    fx.cpd(&["plan", "archive", "--plan", "pay"]).ok();
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM plans WHERE id = 'pay' AND archived_at IS NOT NULL"
        ),
        1
    );
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM contexts WHERE id = 'cpd' AND archived_at IS NOT NULL"
        ),
        0
    );
}
