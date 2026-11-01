//! Red tests: the working dot, from docs/commands.md §17 ("The Lead dot rule") and decision pro-d200 (superseding the
//! phase colours of hs-d622/hs-d624): a working Plan Lead's spinner shows what it works on (`Work`): planning,
//! dispatch or review (git). The stubs make these tests red until the implementation reads the Plan's pull requests
//! and Jobs.

use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::store::Store;

use crate::read::{self, Dot, Tree, Work};
use crate::tree::View;

fn session(id: &str, role: &str) -> String {
    format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('{id}', '{role}', 'ctx', NULL, '{id}', '/repo', 'O3', 'working',
                 '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
    )
}

const FP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// A store of one Context: five Plans, one per phase, each with a working Lead; a Plan with a failed Job and a
/// working Lead; a working fork of a Plan; a working Manager; and an archived working Lead. No tmux, git or gh.
fn fixture_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let t = "'2026-01-02T01:00:00.000Z'";
    let sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         {g} {p} {d} {y} {n} {f} {a} {fork} {mgr}
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at) VALUES
           ('pg', 'ctx', 'G', 'grounding', 'l-grounding', {t}, NULL, NULL),
           ('pp', 'ctx', 'P', 'plan', 'l-plan', {t}, NULL, NULL),
           ('pd', 'ctx', 'D', 'dispatch', 'l-dispatch', {t}, {t}, NULL),
           ('py', 'ctx', 'Y', 'deploy', 'l-deploy', {t}, {t}, NULL),
           ('pn', 'ctx', 'N', 'done', 'l-done', {t}, {t}, {t}),
           ('pf', 'ctx', 'F', 'dispatch', 'l-failed', {t}, {t}, NULL),
           ('pa', 'ctx', 'A', 'dispatch', 'l-arch', {t}, {t}, NULL);
         UPDATE sessions SET plan = 'pg' WHERE id = 'l-grounding';
         UPDATE sessions SET plan = 'pp' WHERE id = 'l-plan';
         UPDATE sessions SET plan = 'pd' WHERE id = 'l-dispatch';
         UPDATE sessions SET plan = 'py' WHERE id = 'l-deploy';
         UPDATE sessions SET plan = 'pn' WHERE id = 'l-done';
         UPDATE sessions SET plan = 'pf' WHERE id = 'l-failed';
         UPDATE sessions SET plan = 'pa' WHERE id = 'l-arch';
         UPDATE sessions SET plan = 'pd' WHERE id = 'l-fork';
         UPDATE sessions SET archived_at = '2026-01-05T00:00:00.000Z' WHERE id = 'l-arch';
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pf', 'work', '/repo', 'main', 'dispatch', 'cpd/pf/work', '/wt', cpd_txn(), {t}, {t});
         INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status,
                           appended, fingerprint, failed_cause, created_txn, created_at)
           VALUES ('pf', 'work', 'j1', 'J', 'S1', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', 'failed',
                   0, '{FP}', 'boom', cpd_txn(), {t});",
        g = session("l-grounding", "lead"),
        p = session("l-plan", "lead"),
        d = session("l-dispatch", "lead"),
        y = session("l-deploy", "lead"),
        n = session("l-done", "lead"),
        f = session("l-failed", "lead"),
        a = session("l-arch", "lead"),
        fork = session("l-fork", "lead"),
        mgr = session("mgr", "manager"),
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    (dir, store)
}

/// The dot of the Plan `plan`'s Lead.
fn lead_dot(tree: &Tree, plan: &str) -> Dot {
    tree.contexts
        .iter()
        .flat_map(|c| &c.plans)
        .find(|p| p.head.id.as_str() == plan)
        .and_then(|p| p.lead())
        .unwrap_or_else(|| panic!("the Lead of Plan {plan}"))
        .dot
}

/// The dot of the Session `id`, wherever the tree puts it.
fn session_dot(tree: &Tree, id: &str) -> Dot {
    let id = SessionId::parse(id).unwrap();
    tree.contexts
        .iter()
        .flat_map(|c| &c.plans)
        .flat_map(|p| &p.sessions)
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("Session {id}"))
        .dot
}

/// Sets the dot of every Session the tree draws under a Plan.
fn every_dot(tree: &mut Tree, dot: Dot) {
    for s in tree
        .contexts
        .iter_mut()
        .flat_map(|c| &mut c.plans)
        .flat_map(|p| &mut p.sessions)
    {
        s.dot = dot;
    }
}

/// A tree whose Sessions all carry `Dot::Working(Some(w))` is animated, and not when every such Session is archived.
#[test]
fn the_tree_animates_a_working_dot_of_every_work() {
    let (_dir, mut store) = fixture_store();
    let base = read::tree(&mut store).unwrap();
    for work in [Work::Planning, Work::Dispatch, Work::Review] {
        let mut tree = base.clone();
        every_dot(&mut tree, Dot::Working(Some(work)));
        assert!(View::new(tree, true).animated(), "{work:?}");
    }
    let mut tree = base;
    every_dot(&mut tree, Dot::Working(Some(Work::Dispatch)));
    for s in tree
        .contexts
        .iter_mut()
        .flat_map(|c| &mut c.plans)
        .flat_map(|p| &mut p.sessions)
    {
        s.archived_at = Some("2026-02-01T00:00:00.000Z".into());
    }
    for c in &mut tree.contexts {
        c.dot = Dot::Stopped;
    }
    assert!(
        !View::new(tree, true).animated(),
        "an archived Session working dot never turns"
    );
}

const T: &str = "'2026-01-02T01:00:00.000Z'";

/// The tree of a store with one Plan `px` in `phase`, its Lead `l-x` working, one Workflow `work` (status `dispatch`) and
/// `extra` SQL (more Workflows, Jobs, Attempts). No tmux, git or gh.
fn scenario(phase: &str, extra: &str) -> Tree {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let done = if phase == "done" { T } else { "NULL" };
    let locked = if phase == "grounding" || phase == "plan" {
        "NULL"
    } else {
        T
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         {lead}
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('px', 'ctx', 'X', '{phase}', 'l-x', {T}, {locked}, {done});
         UPDATE sessions SET plan = 'px' WHERE id = 'l-x';
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('px', 'work', '/repo', 'main', 'dispatch', 'cpd/px/work', '/wt', cpd_txn(), {T}, {T});
         {extra}",
        lead = session("l-x", "lead"),
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    read::tree(&mut store).unwrap()
}

/// A Workflow `key` of Plan `px`, `status` in the Workflow table, its pull request `mergeable` as given.
fn workflow(key: &str, number: u32, status: &str, mergeable: &str) -> String {
    let opened = if status == "open" {
        format!(
            "{number}, 'https://github.com/acme/repo/pull/{number}', {T}, (SELECT id FROM pull_requests WHERE number = {number}),"
        )
    } else {
        "NULL, NULL, NULL, NULL,".to_owned()
    };
    let pull = if status == "open" {
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, authored, created_at, changed_at, mergeable)
               VALUES ('acme/repo', {number}, 'https://github.com/acme/repo/pull/{number}', 'T', 0, 'open', 'none', 'passed', 1, {T}, {T}, '{mergeable}');"
        )
    } else {
        String::new()
    };
    format!(
        "{pull}
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at)
           VALUES ('px', '{key}', '/repo', 'main', '{status}', 'cpd/px/{key}', '/wt', {opened} cpd_txn(), {T}, {T});"
    )
}

/// A Job `key` of Workflow `pr` of kind `kind` and `status`.
fn job(workflow: &str, key: &str, kind: &str, status: &str) -> String {
    let fp = if status == "draft" {
        "NULL".to_owned()
    } else {
        format!("'{FP}'")
    };
    format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                          created_txn, created_at, kind)
         VALUES ('px', '{workflow}', '{key}', 'J', 'S1', 'g', 'c', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fp}, cpd_txn(), {T}, '{kind}');"
    )
}

/// An Attempt of Job `job` in `status` (`running` or `merging`).
fn attempt(job: &str, status: &str) -> String {
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, submitted_sha, submitted_at, started_at)
         VALUES ('px', '{job}-a1', '{job}', 1, 'S1', 'ladder', '{status}', 'cpd/px--{job}-a1', '/w/{job}', {sha}, {at}, {T});",
        sha = if status == "merging" { "'abc'" } else { "NULL" },
        at = if status == "merging" { T } else { "NULL" },
    )
}

/// `dots_tree_dispatch_with_open_pr_is_yellow`: in dispatch, an open Workflow (any of them, not only the first), a
/// conflicting one, or a queued or running `open`/`update` Job makes the Lead's dot yellow (`Work::Review`).
#[test]
fn dots_tree_dispatch_with_open_pr_is_yellow() {
    let review = Dot::Working(Some(Work::Review));
    assert_eq!(
        lead_dot(
            &scenario("dispatch", &workflow("api", 1, "open", "mergeable")),
            "px"
        ),
        review,
        "an open Workflow"
    );
    assert_eq!(
        lead_dot(
            &scenario("dispatch", &workflow("api", 1, "open", "conflicting")),
            "px"
        ),
        review,
        "a conflicting Workflow"
    );
    let second = format!(
        "{}{}",
        workflow("a-first", 1, "dispatch", "unknown"),
        workflow("b-second", 2, "open", "mergeable")
    );
    assert_eq!(
        lead_dot(&scenario("dispatch", &second), "px"),
        review,
        "only the second Workflow is open"
    );
    for kind in ["open", "update"] {
        for status in ["queued", "running"] {
            let extra = job("work", "j1", kind, status);
            assert_eq!(
                lead_dot(&scenario("dispatch", &extra), "px"),
                review,
                "a {status} {kind} Job"
            );
        }
    }
}

/// `dots_tree_dispatch_with_work_jobs_only_is_cyan`: running and queued work Jobs, no open Workflow: `Work::Dispatch`.
#[test]
fn dots_tree_dispatch_with_work_jobs_only_is_cyan() {
    let extra = format!(
        "{}{}",
        job("work", "j1", "work", "running"),
        job("work", "j2", "work", "queued")
    );
    assert_eq!(
        lead_dot(&scenario("dispatch", &extra), "px"),
        Dot::Working(Some(Work::Dispatch))
    );
    let conflicting = format!("{}{}", workflow("api", 1, "open", "conflicting"), extra);
    assert_eq!(
        lead_dot(&scenario("dispatch", &conflicting), "px"),
        Dot::Working(Some(Work::Review)),
        "an open Workflow turns it yellow"
    );
}

/// `dots_tree_grounding_plan_and_done_are_magenta`: never white for grounding nor green for done.
#[test]
fn dots_tree_grounding_plan_and_done_are_magenta() {
    for phase in ["grounding", "plan", "done"] {
        assert_eq!(
            lead_dot(&scenario(phase, ""), "px"),
            Dot::Working(Some(Work::Planning)),
            "{phase}"
        );
    }
}

/// `dots_tree_draft_job_beats_open_pr`: a draft Job (a new batch) is planning even while a Workflow is open.
#[test]
fn dots_tree_draft_job_beats_open_pr() {
    for phase in ["dispatch", "done"] {
        let extra = format!(
            "{}{}",
            workflow("api", 1, "open", "mergeable"),
            job("api", "d1", "work", "draft")
        );
        assert_eq!(
            lead_dot(&scenario(phase, &extra), "px"),
            Dot::Working(Some(Work::Planning)),
            "{phase}"
        );
    }
}

/// `dots_tree_landing_attempt_is_yellow`: a running Job whose Attempt is merging (landing) is `Work::Review`.
#[test]
fn dots_tree_landing_attempt_is_yellow() {
    let extra = format!(
        "{}{}",
        job("work", "j1", "work", "running"),
        attempt("j1", "merging")
    );
    assert_eq!(
        lead_dot(&scenario("dispatch", &extra), "px"),
        Dot::Working(Some(Work::Review))
    );
}

/// `dots_tree_session_without_plan_is_white`: a working fork and a working Manager have no Plan: `Working(None)`.
#[test]
fn dots_tree_session_without_plan_is_white() {
    let (_dir, mut store) = fixture_store();
    let tree = read::tree(&mut store).unwrap();
    assert_eq!(
        lead_dot(&tree, "pg"),
        Dot::Working(Some(Work::Planning)),
        "a Plan's Lead has a Work; a Session without a Plan has none"
    );
    assert_eq!(
        session_dot(&tree, "l-fork"),
        Dot::Working(None),
        "a working fork leads no Plan"
    );
    assert_eq!(
        tree.contexts[0].dot,
        Dot::Working(None),
        "the working Manager leads no Plan"
    );
    assert_eq!(lead_dot(&tree, "pf"), Dot::Fixing, "a failed Job stays red");
    assert_eq!(
        session_dot(&tree, "l-arch"),
        Dot::Working(None),
        "an archived working Lead keeps its glyph"
    );
}
