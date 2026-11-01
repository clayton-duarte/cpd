//! hs-d522: needs and base PRs stay inside one Plan. `pr add` and `pr edit` refuse a need or a base PR that names
//! another Plan, `workflow_other_plan` (exit 3, `details` `{pr, reference, plan}`; contract §2.2 "Validation of a
//! reference", commands.md §9.4, §9.5), and `workflow_needs` has no `needs_plan` column. Two Plans of one Context.

use crate::common;

use std::path::PathBuf;

use common::Run;
use common::fx::Fx;
use serde_json::Value;

const TESTS_TABLE: &str = "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n";

/// The in-process env (fake ports, the git route) and the keyed repo `pay` of its `Universe`.
struct Pay {
    fx: Fx,
    repo: PathBuf,
}

/// Repo `pay` and Plans `pay-1` and `ops-1` (both in `grounding`, one Context), with PR `deploy` in `ops-1` and
/// PR `api` in `pay-1`.
fn fx() -> Pay {
    let fx = Fx::with_ports();
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    fx.env.repo_policy("acme", "pay", TESTS_TABLE);
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    fx.env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't'),
                  ('lead2', 'lead', 'ctx', 'ops-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Pay', 'grounding', 'lead1', '1999-12-31T00:00:00.000Z'),
                  ('ops-1', 'ctx', 'Ops', 'grounding', 'lead2', '1999-12-31T00:00:00.000Z');",
    );
    let fx = Pay { fx, repo };
    fx.add("ops-1", "deploy", &[]).ok();
    fx.add("pay-1", "api", &[]).ok();
    fx
}

impl Pay {
    fn add(&self, plan: &str, key: &str, extra: &[&str]) -> Run {
        let title = format!("Change {key}");
        let mut args = vec!["workflow", "add", key, "--repo", self.repo.to_str().unwrap(), "--title", &title, "--shape", "config"];
        args.extend(extra);
        args.extend(["--plan", plan]);
        self.fx.cpd(&args)
    }

    fn edit(&self, plan: &str, args: &[&str]) -> Run {
        let mut all = vec!["workflow", "edit"];
        all.extend(args);
        all.extend(["--plan", plan]);
        self.fx.cpd(&all)
    }

    fn writes(&self) -> (i64, i64) {
        (self.fx.env.query("SELECT count(*) FROM txns"), self.fx.env.query("SELECT count(*) FROM events"))
    }

    /// `run` is refused `workflow_other_plan`, exit 3, with these details, and wrote nothing.
    fn refused(&self, run: &Run, pr: &str, reference: &str, writes: (i64, i64)) {
        let error = common::refused(run, "workflow_other_plan", 3);
        assert_eq!(error["details"]["pr"], pr, "{error}");
        assert_eq!(error["details"]["reference"], reference, "{error}");
        assert_eq!(error["details"]["plan"], "ops-1", "{error}");
        assert_eq!(self.writes(), writes, "the refusal wrote something");
    }
}

#[test]
fn workflow_add_refuses_a_need_of_another_plan() {
    let fx = fx();
    let writes = fx.writes();
    fx.refused(&fx.add("pay-1", "web", &["--needs", "ops-1/deploy"]), "web", "ops-1/deploy", writes);
    assert_eq!(fx.fx.env.query::<i64>("SELECT count(*) FROM workflows WHERE key = 'web'"), 0);
}

#[test]
fn workflow_add_refuses_a_base_workflow_of_another_plan() {
    let fx = fx();
    let writes = fx.writes();
    fx.refused(&fx.add("pay-1", "web", &["--base", "ops-1/deploy"]), "web", "ops-1/deploy", writes);
    assert_eq!(fx.fx.env.query::<i64>("SELECT count(*) FROM workflows WHERE key = 'web'"), 0);
}

#[test]
fn workflow_edit_refuses_a_need_of_another_plan() {
    let fx = fx();
    fx.add("pay-1", "web", &[]).ok();
    let writes = fx.writes();
    fx.refused(&fx.edit("pay-1", &["web", "--needs", "ops-1/deploy"]), "web", "ops-1/deploy", writes);
    assert_eq!(fx.fx.env.query::<i64>("SELECT count(*) FROM workflow_needs WHERE workflow = 'web'"), 0);
}

#[test]
fn workflow_edit_refuses_a_base_workflow_of_another_plan() {
    let fx = fx();
    fx.add("pay-1", "web", &[]).ok();
    let writes = fx.writes();
    fx.refused(&fx.edit("pay-1", &["web", "--base", "ops-1/deploy"]), "web", "ops-1/deploy", writes);
    assert_eq!(fx.fx.env.query::<String>("SELECT base FROM workflows WHERE plan = 'pay-1' AND key = 'web'"), "main");
}

/// What stays: a need and a base PR of the Plan itself, by bare key (a base as the Plan's own `<plan>/<key>` long form, which may stay). And `workflow_needs` is keyed by the Plan alone
/// (no `needs_plan`, migration `local-needs`).
#[test]
fn needs_and_base_workflow_of_the_same_plan_still_work() {
    let fx = fx();
    fx.add("pay-1", "web", &["--needs", "api"]).ok();
    let out = fx.add("pay-1", "ui", &["--base", "pay-1/api"]).ok();
    assert_eq!(out["workflow"]["base"], "pay-1/api", "{out}");
    fx.edit("pay-1", &["web", "--needs", "api"]).ok();
    assert_eq!(fx.fx.env.query::<String>("SELECT group_concat(needs) FROM workflow_needs WHERE workflow = 'web'"), "api");
    let columns: Value = fx.fx.env.query::<String>("SELECT json_group_array(name) FROM pragma_table_info('workflow_needs')").parse().unwrap();
    assert!(!columns.as_array().unwrap().iter().any(|c| c == "needs_plan"), "workflow_needs still has needs_plan: {columns}");
}
