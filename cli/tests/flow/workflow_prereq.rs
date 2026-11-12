//! Red tests for workflow external prerequisites and merge/post/deploy gating (bug b39).

use crate::common;

use common::fx::Fx;
use common::refused;
use cpd_core::home::CpdHome;
use serde_json::json;

const REF: &str = "acme/api#7";
const SHA: &str = "0123456789abcdef0123456789abcdef01234567";

fn fixture() -> Fx {
    let fx = Fx::with_ports();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'p1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('p1', 'ctx', 'Plan 1', 'plan', 'lead1', '2000-01-01T00:00:00.000Z');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at,
                                    merge_state, authored, base_ref, default_branch, head_oid)
           VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add the thing', 0, 'open', 'approved', 'passed', 't', 't',
                   'clean', 1, 'main', 'main', '{SHA}');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, pr_number, pr_url, pr_opened_at, created_txn, created_at)
           VALUES ('p1', 'api', '/r/api', 'main', 'open', 'cpd/p1/api', 7, 'https://github.com/acme/api/pull/7', 't', cpd_txn(), 't');
         INSERT INTO swr_repos (repo, default_branch, is_archived, auto_merge_allowed, squash_merge_allowed, merge_commit_allowed,
                                rebase_merge_allowed, delete_branch_on_merge, fetched_at)
           VALUES ('acme/api', 'main', 0, 1, 1, 0, 0, 1, 't');"
    ));
    fx
}

#[test]
fn prereq_add_and_confirm_workflow_prereq() {
    let fx = fixture();
    let out = fx.cpd(&[
        "workflow", "prereq", "add",
        "--workflow", "api",
        "--kind", "manual",
        "--text", "Run db migration",
        "--plan", "p1",
    ]);
    let ok = out.ok();
    assert_eq!(ok["prereq"]["kind"], "manual");
}

#[test]
fn prereq_unmet_refuses_pr_merge() {
    let fx = fixture();
    let out = fx.cpd(&["pr", "merge", REF]);
    let err = refused(&out, "prereq_unmet", 3);
    assert_eq!(err["error"]["code"], "prereq_unmet");
}

#[test]
fn prereq_unmet_refuses_pr_merge_green() {
    let fx = fixture();
    let out = fx.cpd(&["pr", "merge-green", REF]);
    let err = refused(&out, "prereq_unmet", 3);
    assert_eq!(err["error"]["code"], "prereq_unmet");
}

#[test]
fn prereq_unmet_refuses_pr_post() {
    let fx = fixture();
    let out = fx.cpd(&["pr", "post", "--workflow", "api", "--plan", "p1"]);
    let err = refused(&out, "prereq_unmet", 3);
    assert_eq!(err["error"]["code"], "prereq_unmet");
}

#[test]
fn prereq_unmet_refuses_inbox_deploy_approve() {
    let fx = fixture();
    let out = fx.cpd(&["inbox", "approve", "1"]);
    let err = refused(&out, "prereq_unmet", 3);
    assert_eq!(err["error"]["code"], "prereq_unmet");
}

#[test]
fn plan_show_and_inbox_list_unmet_prereqs() {
    let fx = fixture();
    let show = fx.cpd(&["plan", "show", "--plan", "p1"]).ok();
    assert!(show["workflows"][0]["unmet_prereqs"].is_array());
}
