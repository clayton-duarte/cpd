//! The six archive and unarchive commands refuse an agent (`agent_forbidden`, commands.md §3.9, §3.10, §4.10, §4.11,
//! §10.10, §10.11), in this process (`Fx::with_ports`): a caller is an agent when `CPD_SESSION` or `CPD_ATTEMPT` is set
//! and non-empty in the command's own environment.

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

type Vars<'a> = &'a [(&'a str, &'a str)];

/// Context `ctx` with Plan `pay` (and its Lead).
fn world() -> (Fx, String) {
    let fx = Fx::with_ports();
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
    fx.cpd(&[
        "plan",
        "new",
        "pay",
        "--context",
        "ctx",
        "--title",
        "Plan pay",
    ])
    .ok();
    let lead = fx
        .env
        .query("SELECT lead_session FROM plans WHERE id = 'pay'");
    (fx, lead)
}

fn archive_everything(fx: &Fx) {
    fx.env.sql(
        "UPDATE contexts SET archived_at = 't' WHERE id = 'ctx';
         UPDATE plans SET archived_at = 't' WHERE id = 'pay';
         UPDATE sessions SET archived_at = 't' WHERE context = 'ctx';",
    );
}

fn archived_ats(fx: &Fx) -> i64 {
    fx.env.query(
        "SELECT count(*) FROM (SELECT archived_at FROM contexts UNION ALL SELECT archived_at FROM plans
                               UNION ALL SELECT archived_at FROM sessions) WHERE archived_at IS NOT NULL",
    )
}

/// Runs `args` as an agent three ways and pins the refusal and that nothing changed.
fn assert_refused(fx: &Fx, command: &str, args: &[&str]) {
    let cases: [(Vars, Value, Value); 3] = [
        (&[("CPD_SESSION", "lead-1")], json!("lead-1"), Value::Null),
        (&[("CPD_ATTEMPT", "api-a1")], Value::Null, json!("api-a1")),
        (
            &[("CPD_SESSION", "lead-1"), ("CPD_ATTEMPT", "api-a1")],
            json!("lead-1"),
            json!("api-a1"),
        ),
    ];
    let before = archived_ats(fx);
    for (vars, session, attempt) in cases {
        let run = fx.cpd_with(fx.env.root(), args, vars);
        let error = refused(&run, "agent_forbidden", 3);
        let message = error["message"].as_str().unwrap();
        assert!(
            message.starts_with(&format!(
                "cpd {command} is for the User and the engine, not an agent"
            )),
            "{error}"
        );
        assert_eq!(
            error["details"],
            json!({"command": command, "session": session, "attempt": attempt}),
            "{error}"
        );
        assert_eq!(archived_ats(fx), before, "{error}");
    }
}

#[test]
fn context_archive_is_refused_for_an_agent() {
    let (fx, _lead) = world();
    assert_refused(&fx, "context archive", &["context", "archive", "ctx"]);
}

#[test]
fn context_unarchive_is_refused_for_an_agent() {
    let (fx, _lead) = world();
    archive_everything(&fx);
    assert_refused(&fx, "context unarchive", &["context", "unarchive", "ctx"]);
}

#[test]
fn plan_archive_is_refused_for_an_agent() {
    let (fx, _lead) = world();
    assert_refused(&fx, "plan archive", &["plan", "archive", "--plan", "pay"]);
}

#[test]
fn plan_unarchive_is_refused_for_an_agent() {
    let (fx, _lead) = world();
    archive_everything(&fx);
    assert_refused(
        &fx,
        "plan unarchive",
        &["plan", "unarchive", "--plan", "pay"],
    );
}

#[test]
fn session_archive_is_refused_for_an_agent() {
    let (fx, lead) = world();
    assert_refused(&fx, "session archive", &["session", "archive", &lead]);
}

#[test]
fn session_unarchive_is_refused_for_an_agent() {
    let (fx, lead) = world();
    archive_everything(&fx);
    assert_refused(&fx, "session unarchive", &["session", "unarchive", &lead]);
}

#[test]
fn unset_or_empty_variables_are_not_an_agent() {
    let commands = [
        ["context", "archive", "ctx"],
        ["context", "unarchive", "ctx"],
        ["plan", "archive", ""],
        ["plan", "unarchive", ""],
        ["session", "archive", ""],
        ["session", "unarchive", ""],
    ];
    for args in commands {
        for vars in [&[][..], &[("CPD_SESSION", ""), ("CPD_ATTEMPT", "")][..]] {
            let (fx, lead) = world();
            let words: Vec<&str> = match args[0] {
                "plan" => vec![args[0], args[1], "--plan", "pay"],
                "session" => vec![args[0], args[1], &lead],
                _ => args.to_vec(),
            };
            let run = fx.cpd_with(fx.env.root(), &words, vars);
            let json = run.json();
            if json["ok"] != true {
                assert_ne!(json["error"]["code"], "agent_forbidden", "{run:?}");
            }
        }
    }
}
