//! The `workflow` command group and the new object vocabulary of the CLI (commands.md §9.4–§9.7, R34,
//! R3, R4; Plan `glossary-rename`, map row `ren-red-cli`, Context hs-d805): the new `workflow`
//! commands work and `plan show` reports `workflows`, while the old `cpd pr add|edit|describe|cancel|start`
//! and `--pr <key>` on the moved commands are usage errors with no alias (pro-d138).
//!
//! Red now: there is no `workflow` command, and `--pr <key>` is still a valid flag. `ren-impl-cli`
//! makes these pass. In-process on [`Fx::with_ports`] (the fake git route, the fake `gh`), against a
//! temporary `$CPD_HOME`, with Plan `pay-1` written straight into the database.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use serde_json::json;

/// The repo policy file's `[test-commands]` (contract §7.7): what a `tdd` Workflow needs of its repo.
const TESTS_TABLE: &str = "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n";

/// A keyed repo of the `Universe` whose policy file has `[test-commands]`.
fn keyed_repo(fx: &Fx, name: &str) -> PathBuf {
    let repo = fx.keyed_repo(name).canonicalize().unwrap();
    fx.env.repo_policy("acme", name, TESTS_TABLE);
    repo
}

/// The env with its config, repo `pay`, and Context `ctx` (`acme`) with Plan `pay-1` in `phase` and its Lead.
fn fixture(phase: &str) -> (Fx, PathBuf) {
    let fx = Fx::with_ports();
    let pay = keyed_repo(&fx, "pay");
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL"
    } else {
        "'2000-01-01T00:00:00.000Z'"
    };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', '{phase}', 'lead1', '1999-12-31T00:00:00.000Z', {locked});"
    ));
    (fx, pay)
}

/// The `workflow` subcommands of the group: `workflow add`, `edit`, `describe`, `cancel`, `start` (§9.4–§9.7).
const SUBCOMMANDS: [&str; 5] = ["add", "edit", "describe", "cancel", "start"];

// ---------- the new commands work ----------

#[test]
fn new_commands_work() {
    let (fx, pay) = fixture("grounding");
    let repo = pay.to_str().unwrap();

    // The whole group exists: each subcommand parses and its help is a successful JSON value.
    for sub in SUBCOMMANDS {
        let helped = fx.cpd(&["workflow", sub, "--help"]).ok();
        assert!(
            helped["help"].as_str().is_some_and(|h| h.contains(sub)),
            "workflow {sub} --help: {helped}"
        );
    }

    // `workflow add` creates a `waiting` Workflow with its own title, repo and base (commands.md §9.4).
    let added = fx
        .cpd(&[
            "workflow",
            "add",
            "api",
            "--repo",
            repo,
            "--title",
            "Add the api",
            "--shape",
            "config",
            "--plan",
            "pay-1",
        ])
        .ok();
    assert_eq!(
        (
            &added["workflow"]["key"],
            &added["workflow"]["status"],
            &added["workflow"]["title"],
            &added["workflow"]["base"]
        ),
        (
            &json!("api"),
            &json!("waiting"),
            &json!("Add the api"),
            &json!("main")
        )
    );

    // `workflow edit` changes a field and reports it (commands.md §9.5).
    let edited = fx
        .cpd(&[
            "workflow",
            "edit",
            "api",
            "--title",
            "Add the api, again",
            "--plan",
            "pay-1",
        ])
        .ok();
    assert_eq!(
        (&edited["workflow"]["key"], &edited["fields"]),
        (&json!("api"), &json!(["title"]))
    );

    // `workflow describe` stores the description (commands.md §9.5a).
    let description = fx.env.root().join("description.md");
    fs::write(&description, "What the api changes.\n").unwrap();
    let described = fx
        .cpd(&[
            "workflow",
            "describe",
            "api",
            "--file",
            description.to_str().unwrap(),
            "--plan",
            "pay-1",
        ])
        .ok();
    assert_eq!(described["workflow"]["key"], json!("api"));

    // `plan show` speaks the new object: `workflows`, never `prs` (commands.md §4.3, R34).
    let shown = fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
    assert!(
        shown.get("prs").is_none(),
        "plan show has no `prs` key: {shown}"
    );
    let workflows = shown["workflows"]
        .as_array()
        .unwrap_or_else(|| panic!("plan show reports `workflows`: {shown}"));
    assert_eq!(workflows.len(), 1);
    assert_eq!(workflows[0]["key"], json!("api"));

    // `workflow cancel` ends it, keeping its key (commands.md §9.6).
    let cancelled = fx
        .cpd(&["workflow", "cancel", "api", "--plan", "pay-1"])
        .ok();
    assert_eq!(cancelled["workflow"]["status"], json!("cancelled"));
    let shown = fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
    assert_eq!(shown["workflows"][0]["status"], json!("cancelled"));
}

// ---------- the old forms are usage errors ----------

#[test]
fn old_forms_are_usage_errors() {
    let (fx, pay) = fixture("grounding");
    let repo = pay.to_str().unwrap();
    let description = fx.env.root().join("description.md");
    fs::write(&description, "What the api changes.\n").unwrap();

    // The old object verbs are gone: `cpd pr add|edit|describe|cancel|start` exit as a usage error, no alias (pro-d138).
    common::refused(
        &fx.cpd(&[
            "pr",
            "add",
            "api",
            "--repo",
            repo,
            "--title",
            "Change api",
            "--shape",
            "config",
            "--plan",
            "pay-1",
        ]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&[
            "pr",
            "edit",
            "api",
            "--title",
            "Change api",
            "--plan",
            "pay-1",
        ]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&[
            "pr",
            "describe",
            "--pr",
            "api",
            "--file",
            description.to_str().unwrap(),
            "--plan",
            "pay-1",
        ]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&["pr", "cancel", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&["pr", "start", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );

    // `--pr <key>` is a usage error on every moved command; the selector is `--workflow <key>` (R34, §1.3).
    common::refused(
        &fx.cpd(&["pr", "open", "--pr", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&["pr", "rebase", "--pr", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&["pr", "push", "--pr", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&["pr", "view", "--pr", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );
    common::refused(
        &fx.cpd(&["job", "edit", "api-docs", "--pr", "api", "--plan", "pay-1"]),
        "usage",
        2,
    );
}
