//! `workflow::reopen::reopen` (pro-d362): a cancelled Workflow whose pull request GitHub reopened comes back `open`.
//! In process on the janitor's `Rig`: the git is the `Universe`, `gh` is a scripted handler on the runner, the store is
//! a migrated database in a temp dir. No test starts a process, runs real git, sleeps or reads the wall clock.
//! The helpers are shared by `pr/open_reopen_tests.rs`.

use std::path::PathBuf;
use std::sync::Arc;

use super::reopen::{Reopened, reopen};
use crate::env::Env;
use crate::error::{CpdError, ErrorCode};
use crate::fakes::git::Head;
use crate::git::workflow_lead_worktree_path;
use crate::host::runner::{Call, Handler, Reply};
use crate::ids::{PlanId, WorkflowKey};
use crate::janitor_tests::Rig;
use crate::ports::Ports;
use crate::store::Store;

const GITHUB: &str = "https://github.com/acme/pay.git";

/// The in-process `gh` of the reopen: `gh pr list --head <branch>` lists the pull requests of `heads`, `(branch, number, state)`;
/// any other call exits 9.
pub(crate) struct Gh {
    pub(crate) heads: Vec<(String, i64, &'static str)>,
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["pr", "list", "--head", head, "--state", "all", "--json", _] => {
                let listed: Vec<String> = self
                    .heads
                    .iter()
                    .filter(|(h, _, _)| h == head)
                    .map(|(_, number, state)| {
                        format!(
                            r#"{{"number":{number},"url":"https://github.com/acme/pay/pull/{number}","state":"{state}","createdAt":"2026-10-20T10:00:00Z"}}"#
                        )
                    })
                    .collect();
                Reply::ok(&format!("[{}]", listed.join(",")))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// A `Rig` whose Plan `pay` is in `phase` (`deploy` or `done`), its Lead asleep, and whose repo has a real `origin` of the
/// Universe (behind `https://github.com/acme/pay.git`) with `main` pushed.
pub(crate) fn rig(phase: &str) -> Rig {
    let rig = Rig::new();
    let bare = rig.repo.bare_origin(&rig.tmp.path().join("origin.git"));
    {
        let mut u = rig.universe.lock().unwrap();
        u.config_set(rig.repo.id(), "remote.origin.url", GITHUB);
        let dir = u.git_dir(bare.id());
        u.config_add(
            rig.repo.id(),
            &format!("url.{}.insteadOf", dir.display()),
            GITHUB,
        );
    }
    rig.repo.push();
    let done = if phase == "done" { "'t'" } else { "NULL" };
    rig.sql(&format!(
        "UPDATE plans SET phase = '{phase}', done_at = {done} WHERE id = 'pay';
         UPDATE sessions SET asleep_at = 't', state = 'stopped' WHERE id = 'lead1';"
    ));
    rig
}

/// `branch` exists on `origin` (and as its remote-tracking branch) at `main`'s commit.
pub(crate) fn publish(rig: &Rig, branch: &str) {
    let mut u = rig.universe.lock().unwrap();
    let main = u
        .ref_oid(rig.repo.id(), "refs/heads/main")
        .or_else(|| u.ref_oid(rig.repo.id(), "refs/heads/master"))
        .expect("the repo has its base commit");
    let url = u.resolve_url(rig.repo.id(), GITHUB);
    let origin = u.repo_at(&url).expect("origin is a repo of the Universe");
    u.set_ref(origin, &format!("refs/heads/{branch}"), &main, "push");
    u.set_ref(
        rig.repo.id(),
        &format!("refs/remotes/origin/{branch}"),
        &main,
        "fetch",
    );
}

/// Routes `gh` to [`Gh`] with `heads`.
pub(crate) fn gh(rig: &Rig, heads: &[(&str, i64, &'static str)]) {
    let heads = heads
        .iter()
        .map(|(h, n, s)| ((*h).to_owned(), *n, *s))
        .collect();
    let _ = (*rig.fakes.runner)
        .clone()
        .route("gh", Arc::new(Gh { heads }));
}

/// The stored pull request `acme/pay#number` in `state`, as the poll left it.
pub(crate) fn stored_pull(rig: &Rig, number: i64, state: &str) {
    rig.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, authored)
           VALUES ('acme/pay', {number}, 'https://github.com/acme/pay/pull/{number}', 'T', 0, '{state}', 'none', 'none', 'opened', 'opened', 1);"
    ));
}

/// A cancelled Workflow `key` of `plan` on Lead Branch `cpd/pay/<key>` with no worktree. With `kept` it still has its pull request
/// `acme/pay#n` (stored `closed`, as `workflow cancel` required), else it is a legacy row with no pull request.
pub(crate) fn cancelled(rig: &Rig, plan: &str, key: &str, kept: Option<i64>) {
    cancelled_on(rig, plan, key, kept, &format!("cpd/{plan}/{key}"));
}

/// [`cancelled`] on Lead Branch `branch`.
fn cancelled_on(rig: &Rig, plan: &str, key: &str, kept: Option<i64>, branch: &str) {
    let repo = rig.repo.dir().display();
    let (number, url, opened, pull) = match kept {
        Some(n) => {
            stored_pull(rig, n, "closed");
            (
                n.to_string(),
                format!("'https://github.com/acme/pay/pull/{n}'"),
                "'opened'",
                format!("(SELECT id FROM pull_requests WHERE number = {n})"),
            )
        }
        None => ("NULL".into(), "NULL".into(), "NULL", "NULL".into()),
    };
    rig.sql(&format!(
        "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('{plan}', '{key}', '{repo}', 'pay', 'main', 'cancelled', '{branch}', NULL, cpd_txn(), 't', NULL,
                   {number}, {url}, {opened}, {pull});"
    ));
}

/// Two legacy cancelled Workflows of one repo on the same Lead Branch `cpd/pay`: `pay`/`api` (the legacy `cpd/<plan>` form) and
/// `cpd`/`pay` (`<plan>/<key>`). The `lead_branch` CHECK allows nothing else to collide.
pub(crate) fn twins(rig: &Rig) {
    rig.sql(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead2', 'lead', 'ctx', 'cpd', 'Lead2', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('cpd', 'ctx', 'Cpd', 'deploy', 'lead2', 't', 't');",
    );
    cancelled_on(rig, "pay", "api", None, "cpd/pay");
    cancelled_on(rig, "cpd", "pay", None, "cpd/pay");
}

/// Reopens `plan`/`key` on the rig's ports.
pub(crate) fn run(rig: &Rig, plan: &str, key: &str) -> Result<Reopened, CpdError> {
    let mut env = Env::open_home_with(&rig.fakes.ports, rig.home.clone()).unwrap();
    let (plan, key) = (
        PlanId::parse(plan).unwrap(),
        WorkflowKey::parse(key).unwrap(),
    );
    Ports::scope(&rig.fakes.ports, || reopen(&mut env, &plan, &key))
}

/// One text query of the store: every row's columns joined by `|`.
pub(crate) fn rows(rig: &Rig, sql: &str) -> Vec<String> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|conn| {
            let mut stmt = conn.prepare(sql)?;
            let width = stmt.column_count();
            let rows = stmt
                .query_map([], |r| {
                    (0..width)
                        .map(|i| {
                            r.get::<_, rusqlite::types::Value>(i).map(|v| match v {
                                rusqlite::types::Value::Null => "NULL".to_owned(),
                                rusqlite::types::Value::Integer(i) => i.to_string(),
                                rusqlite::types::Value::Text(t) => t,
                                other => format!("{other:?}"),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map(|c| c.join("|"))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
        .unwrap()
}

/// The id of the newest Event, 0 for none: what [`events_since`] counts from.
pub(crate) fn last_event(rig: &Rig) -> i64 {
    rows(rig, "SELECT COALESCE(max(id), 0) FROM events")[0]
        .parse()
        .unwrap()
}

/// The kinds of the Events after `since`, in order.
pub(crate) fn events_since(rig: &Rig, since: i64) -> Vec<String> {
    rows(
        rig,
        &format!("SELECT kind FROM events WHERE id > {since} ORDER BY id"),
    )
}

/// The rows [`reopen`] may change: the Workflows, the Plans, the Sessions' sleep, and the Event count.
fn snapshot(rig: &Rig) -> Vec<String> {
    let mut all = rows(
        rig,
        "SELECT plan, key, status, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request FROM workflows ORDER BY plan, key",
    );
    all.extend(rows(
        rig,
        "SELECT id, phase, done_at FROM plans ORDER BY id",
    ));
    all.extend(rows(rig, "SELECT id, asleep_at FROM sessions ORDER BY id"));
    all.extend(rows(rig, "SELECT count(*) FROM events"));
    all
}

/// A cancelled Workflow of a Plan in `deploy` whose pull request is open again comes back `open`; the Plan stays in `deploy`
/// (nothing moves it), with no `plan-phase` Event (A3).
#[test]
fn reopen_leaves_a_deploy_plan_in_deploy() {
    let rig = rig("deploy");
    cancelled(&rig, "pay", "api", Some(801));
    gh(&rig, &[("cpd/pay/api", 801, "OPEN")]);
    publish(&rig, "cpd/pay/api");
    let since = last_event(&rig);

    let done = run(&rig, "pay", "api").unwrap();

    assert!(done.changed);
    assert_eq!(
        rows(
            &rig,
            "SELECT status, pr_number FROM workflows WHERE key = 'api'"
        ),
        ["open|801"]
    );
    assert_eq!(
        rows(&rig, "SELECT phase, done_at FROM plans WHERE id = 'pay'"),
        ["deploy|NULL"]
    );
    assert_eq!(events_since(&rig, since), ["workflow-reopened"]);
}

/// A second reopen, on a Workflow that is already `open`, writes nothing and asks nothing of GitHub (A5).
#[test]
fn reopen_of_an_open_workflow_changes_nothing() {
    let mut rig = rig("deploy");
    rig.workflow("api", "open");
    let before = snapshot(&rig);

    let done = run(&rig, "pay", "api").unwrap();

    assert!(!done.changed);
    assert_eq!(snapshot(&rig), before);
    assert!(
        rig.fakes.runner.calls().iter().all(|c| c.tool != "gh"),
        "no gh call"
    );
}

/// `api`: path and local branch are gone, so the Lead worktree is recreated from `origin/cpd/pay/api`. `web`: its worktree is
/// still registered on its branch, with work in it: it is kept, not discarded (`prepare_workflow_lead_worktree` would discard it).
#[test]
fn reopen_recreates_the_lead_worktree_and_keeps_a_registered_one() {
    let rig = rig("deploy");
    let plan = PlanId::parse("pay").unwrap();
    let path = |key: &str| -> PathBuf {
        workflow_lead_worktree_path(
            rig.repo.dir(),
            &plan,
            &WorkflowKey::parse(key).unwrap(),
            &format!("cpd/pay/{key}"),
        )
        .unwrap()
    };
    let (api, web) = (path("api"), path("web"));
    cancelled(&rig, "pay", "api", Some(801));
    cancelled(&rig, "pay", "web", Some(802));
    gh(
        &rig,
        &[("cpd/pay/api", 801, "OPEN"), ("cpd/pay/web", 802, "OPEN")],
    );
    publish(&rig, "cpd/pay/api");
    publish(&rig, "cpd/pay/web");
    rig.repo.branch("cpd/pay/web");
    rig.universe.lock().unwrap().add_worktree(
        rig.repo.id(),
        &web,
        Head::Branch("cpd/pay/web".into()),
    );
    std::fs::write(web.join("f.txt"), "base\n").unwrap();
    std::fs::write(web.join("unpushed.txt"), "mine\n").unwrap();
    assert!(!api.exists() && !rig.branch_exists("cpd/pay/api"));

    run(&rig, "pay", "api").unwrap();
    run(&rig, "pay", "web").unwrap();

    assert!(
        api.exists() && rig.registered(&api),
        "api's worktree is back"
    );
    assert!(rig.branch_exists("cpd/pay/api"));
    assert!(
        web.join("unpushed.txt").exists() && rig.registered(&web),
        "web's registered worktree is kept"
    );
    assert_eq!(
        rows(
            &rig,
            "SELECT key, status, lead_worktree FROM workflows ORDER BY key"
        ),
        [
            format!("api|open|{}", api.display()),
            format!("web|open|{}", web.display())
        ]
    );
}

/// A legacy cancelled row (`pull_request` NULL) is matched to the open pull request by (repo, lead branch), only when exactly one
/// cancelled Workflow has that pair; with two, nothing is rebound.
#[test]
fn reopen_matches_a_legacy_cancelled_workflow_by_lead_branch_only_when_unique() {
    let rig_one = rig("deploy");
    cancelled(&rig_one, "pay", "api", None);
    stored_pull(&rig_one, 801, "open");
    gh(&rig_one, &[("cpd/pay/api", 801, "OPEN")]);
    publish(&rig_one, "cpd/pay/api");

    run(&rig_one, "pay", "api").unwrap();

    assert_eq!(
        rows(
            &rig_one,
            "SELECT w.status, w.pr_number, w.pull_request = r.id FROM workflows w, pull_requests r WHERE w.key = 'api' AND r.number = 801"
        ),
        ["open|801|1"]
    );

    let rig_two = rig("deploy");
    twins(&rig_two);
    stored_pull(&rig_two, 801, "open");
    gh(&rig_two, &[("cpd/pay", 801, "OPEN")]);
    publish(&rig_two, "cpd/pay");
    let before = snapshot(&rig_two);

    let refused = run(&rig_two, "pay", "api").unwrap_err();

    assert_eq!(refused.code(), ErrorCode::IllegalTransition);
    assert_eq!(snapshot(&rig_two), before, "nothing was rebound");
}

/// `(key, pull_request)` of every row `reopenable` returns, in its order.
fn listed(rig: &Rig) -> Vec<(String, Option<i64>)> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(super::reopen::reopenable)
        .unwrap()
        .into_iter()
        .map(|c| (c.key.as_str().to_owned(), c.pull_request))
        .collect()
}

#[test]
fn reopenable_lists_a_legacy_row_only_when_an_unowned_open_pull_request_exists() {
    // (a) no pull_requests row: nothing.
    let rig_a = rig("deploy");
    cancelled(&rig_a, "pay", "api", None);
    assert_eq!(listed(&rig_a), vec![], "no pull_requests row: nothing");

    // (b) an unowned open authored pull request: the legacy row.
    let rig_b = rig("deploy");
    cancelled(&rig_b, "pay", "api", None);
    stored_pull(&rig_b, 801, "open");
    assert_eq!(
        listed(&rig_b),
        vec![("api".to_owned(), None)],
        "an unowned open authored pull request: the legacy row"
    );

    // (c) the open pull request is linked to an `open` Workflow through `workflows.pull_request`: it is owned.
    let rig_c = rig("deploy");
    cancelled(&rig_c, "pay", "api", None);
    stored_pull(&rig_c, 801, "open");
    rig_c.sql(
        "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                pr_number, pr_url, pr_opened_at, pull_request)
           VALUES ('pay', 'web', '/web', 'pay', 'main', 'open', 'cpd/pay/web', '/wt/web', cpd_txn(), 't', 't',
                   801, 'https://github.com/acme/pay/pull/801', 't',
                   (SELECT id FROM pull_requests WHERE number = 801));",
    );
    assert_eq!(
        listed(&rig_c),
        vec![],
        "an owned open pull request: nothing"
    );

    // (d) an open pull request that is not authored by us: nothing.
    let rig_d = rig("deploy");
    cancelled(&rig_d, "pay", "api", None);
    stored_pull(&rig_d, 801, "open");
    rig_d.sql("UPDATE pull_requests SET authored = 0 WHERE number = 801;");
    assert_eq!(listed(&rig_d), vec![], "a not authored open row: nothing");

    // (e) a kept pull_request stored open is listed whatever else exists.
    let rig_e = rig("deploy");
    cancelled(&rig_e, "pay", "api", None);
    cancelled(&rig_e, "pay", "web", Some(802));
    rig_e.sql("UPDATE pull_requests SET state = 'open' WHERE number = 802;");
    let kept: i64 = rows(&rig_e, "SELECT id FROM pull_requests WHERE number = 802")[0]
        .parse()
        .unwrap();
    assert!(
        listed(&rig_e).contains(&("web".to_owned(), Some(kept))),
        "the kept row is listed: {:?}",
        listed(&rig_e)
    );
}
