//! `cpd secret` guard rules (commands.md §24.0, contract §8.4): who may do what, in which vault, and the grants
//! they need. Op is never reached: the fake `op`'s log stays empty.

use crate::secret::{Fx, Who, args, denied, strs};

const COMMANDS: [&str; 7] = [
    "get", "list", "create", "edit", "delete", "allow", "approve",
];

fn run(fx: &Fx, who: Who, command: &str, vault: &str, stdin: Option<&str>) -> crate::common::Run {
    let a = args(command, vault, "item", "field");
    fx.run(who, &strs(&a), stdin)
}

#[test]
fn no_session_is_refused_for_every_command() {
    let fx = Fx::new();
    for who in [Who::Nobody, Who::Empty] {
        for command in COMMANDS {
            let vault = if command == "approve" { "other" } else { "cpd" };
            let r = run(&fx, who, command, vault, Some("value\n"));
            denied(&r, "secret_session_required", 3);
            fx.assert_op_not_called(&r);
        }
    }
}

#[test]
fn an_unknown_session_is_session_not_found() {
    let fx = Fx::new();
    for command in COMMANDS {
        let vault = if command == "approve" { "other" } else { "cpd" };
        let r = run(&fx, Who::Unknown, command, vault, Some("value\n"));
        denied(&r, "session_not_found", 4);
        fx.assert_op_not_called(&r);
    }
}

#[test]
fn a_worker_may_only_get() {
    let fx = Fx::new();
    for command in ["list", "create", "edit", "delete", "allow", "approve"] {
        let vault = if command == "approve" { "other" } else { "cpd" };
        let r = run(&fx, Who::Worker, command, vault, Some("value\n"));
        denied(&r, "secret_role_denied", 3);
        fx.assert_op_not_called(&r);
    }
}

#[test]
fn a_worker_get_needs_the_leads_allowance() {
    let fx = Fx::new();
    for vault in ["cpd", "homeserver"] {
        let r = run(&fx, Who::Worker, "get", vault, None);
        let e = denied(&r, "secret_grant_required", 3);
        assert_eq!(e["details"]["grant"], "allowance", "{r:?}");
        let next = e["details"]["next"].as_str().unwrap_or_default();
        assert!(next.contains("cpd ask"), "{r:?}");
        fx.assert_op_not_called(&r);
    }
}

#[test]
fn a_worker_in_another_vault_is_denied() {
    let fx = Fx::new();
    let r = run(&fx, Who::Worker, "get", "other", None);
    denied(&r, "secret_vault_denied", 3);
    fx.assert_op_not_called(&r);
}

#[test]
fn a_lead_in_another_vault_needs_the_users_approval() {
    let fx = Fx::new();
    for who in [Who::Lead, Who::Manager] {
        for command in ["get", "list", "create", "edit", "delete"] {
            let r = run(&fx, who, command, "other", Some("value\n"));
            let e = denied(&r, "secret_grant_required", 3);
            assert_eq!(
                e["details"]["grant"], "approval",
                "{who:?} {command}: {r:?}"
            );
            assert_eq!(e["details"]["action"], command, "{who:?} {command}: {r:?}");
            assert_eq!(e["details"]["vault"], "other", "{who:?} {command}: {r:?}");
            let next = e["details"]["next"].as_str().unwrap_or_default();
            assert!(
                next.contains("cpd_secret_approve"),
                "{who:?} {command}: {r:?}"
            );
            fx.assert_op_not_called(&r);
        }
    }
}

#[test]
fn a_manager_cannot_allow() {
    let fx = Fx::new();
    let r = run(&fx, Who::Manager, "allow", "cpd", None);
    denied(&r, "secret_role_denied", 3);
    fx.assert_op_not_called(&r);
}

#[test]
fn names_are_validated_before_anything_else() {
    let fx = Fx::new();
    // the neighbouring case only the implementation answers: a valid command line with no session
    let r = run(&fx, Who::Nobody, "get", "cpd", None);
    denied(&r, "secret_session_required", 3);

    let long = "x".repeat(201);
    let bad = ["", "-x", "a\u{1}b", "a\nb", long.as_str()];
    for who in [Who::Lead, Who::Nobody] {
        for name in bad {
            let mut lines: Vec<(&str, Vec<String>)> = Vec::new();
            for command in ["get", "create", "edit", "delete", "allow"] {
                lines.push((command, args(command, "cpd", name, "field"))); // item
                lines.push((command, args(command, name, "item", "field"))); // vault
            }
            for command in ["get", "create", "edit", "allow"] {
                lines.push((command, args(command, "cpd", "item", name))); // field
            }
            lines.push(("list", args("list", name, "item", "field")));
            for (command, line) in lines {
                let r = fx.run(who, &strs(&line), Some("value\n"));
                let e = denied(&r, "usage", 2);
                assert!(
                    e["message"].is_string(),
                    "{who:?} {command} {name:?}: {r:?}"
                );
                fx.assert_op_not_called(&r);
            }
        }
    }
}

#[test]
fn a_missing_or_empty_value_is_usage() {
    let fx = Fx::new();
    for command in ["create", "edit"] {
        for stdin in ["", "\n"] {
            let r = run(&fx, Who::Lead, command, "cpd", Some(stdin));
            let e = denied(&r, "usage", 2);
            assert!(
                e["message"]
                    .as_str()
                    .unwrap_or_default()
                    .to_lowercase()
                    .contains("value"),
                "{r:?}"
            );
            fx.assert_op_not_called(&r);

            // the guard comes before the value is read
            let r = run(&fx, Who::Lead, command, "other", Some(stdin));
            let e = denied(&r, "secret_grant_required", 3);
            assert_eq!(e["details"]["grant"], "approval", "{r:?}");
            fx.assert_op_not_called(&r);
        }
    }
}
