//! `workflow cancel` of an `open` Workflow whose pull request is `closed` keeps the pull request identity (pro-d362 A1), so a reopened PR finds its Workflow.

use super::*;
use crate::env::Env;
use crate::home::CpdHome;
use crate::ports::Ports;

const URL: &str = "https://github.com/acme/api/pull/7";

fn env() -> (tempfile::TempDir, Env) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"test\"\n").unwrap();
    let mut env = Env::open_home_with(&Ports::current(), home).unwrap();
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('pay', 'ctx', 'Pay', 'deploy', 'lead1', 't', 't');
                 INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                   VALUES ('acme/api', 7, '{URL}', 'T', 0, 'closed', 'none', 'none', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at)
                   VALUES ('pay', 'api', '/r/api', 'main', 'open', 'cpd/pay/api', '/w/api', 7, '{URL}', 't',
                           (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
                   VALUES ('pay', 'done', '/r/api', 'main', 'merged', 'cpd/pay/done', '/w/done', 8, 'https://github.com/acme/api/pull/8', 't',
                           (SELECT id FROM pull_requests WHERE number = 7), cpd_txn(), 't', 't', 't');"
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, env)
}

#[test]
fn cancelling_an_open_workflow_keeps_its_pull_request_identity() {
    let (_dir, mut env) = env();
    let args = CancelArgs {
        key: WorkflowKey::parse("api").unwrap(),
        superseded_by: None,
        plan: crate::args::PlanArg {
            plan: Some(PlanId::parse("pay").unwrap()),
        },
    };
    cancel_in(&mut env, args).expect("cancel of an open Workflow whose pull request is closed");

    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<i64>,
        String,
    );
    let row: Row = env
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, lead_worktree, started_at, pr_number, pr_url, pr_opened_at, pull_request,
                        (SELECT phase FROM plans WHERE id = 'pay') FROM workflows WHERE key = 'api'",
                [],
                |r| {
                    Ok((
                        r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?,
                    ))
                },
            )?)
        })
        .unwrap();
    let pull_id: i64 = env
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT id FROM pull_requests WHERE number = 7", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(
        row,
        (
            "cancelled".to_owned(),
            None,
            None,
            Some(7),
            Some(URL.to_owned()),
            Some("t".to_owned()),
            Some(pull_id),
            "done".to_owned()
        )
    );
}
