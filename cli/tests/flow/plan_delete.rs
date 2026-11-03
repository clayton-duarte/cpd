//! `plan delete` (commands.md §4.14; contract §2.2 "Deleted", hs-d527), run in-process (`Fx::with_ports`) against a temporary
//! `$CPD_HOME`, the fake tmux and git routes, and a bare repo of the fake git `Universe` that stands in for `origin`. The Plan's rows that only the
//! engine reaches are written straight into the database, as `plan_cancel.rs` does.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};

use common::fx::Fx;
use common::refused;
use serde_json::Value;

// ---------- fixtures ----------

/// What a Plan's world is made of, on disk and in the database.
pub struct World {
    pub plan: String,
    pub lead: String,
    pub repo: PathBuf,
    pub bare: PathBuf,
    pub lead_worktree: PathBuf,
    pub attempt_worktree: PathBuf,
    pub run_dir: PathBuf,
}

impl World {
    pub fn lead_branch(&self) -> String {
        format!("{}/api", self.plan)
    }

    pub fn attempt_branch(&self) -> String {
        format!("cpd/{}--m-a1", self.plan)
    }
}

/// The config, the fakes and the Context `ctx` (owner `acme`).
pub fn base(fx: &Fx) {
    fx.env.write_tmux_config("");
    fx.cpd(&[
        "context", "new", "ctx", "--title", "Payments", "--owner", "acme",
    ])
    .ok();
}

pub fn fixture(fx: &Fx, sql: &str) {
    let mut store = fx.env.store();
    common::sql(&mut store, sql);
}

fn branches(fx: &Fx, repo: &Path) -> Vec<String> {
    fx.git(
        repo,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )
    .lines()
    .map(str::to_owned)
    .collect()
}

pub fn local_branches(fx: &Fx, w: &World) -> Vec<String> {
    branches(fx, &w.repo)
}

pub fn remote_branches(fx: &Fx, w: &World) -> Vec<String> {
    branches(fx, &w.bare)
}

/// Plan `plan` of Context `ctx`, finished: its one PR `api` is `merged` (pull request `acme/<plan>#11`), its one Job `m` is
/// merged with its Attempt `m-a1` ended, a Lead worktree and an Attempt worktree exist with their local branches, the
/// Lead Branch is also on the local bare `origin`, and `run/<plan>/` holds files. The Lead is `stopped`, not archived.
pub fn world(fx: &Fx, plan: &str) -> World {
    world_ending(fx, plan, true)
}

/// [`world`] whose PR ends `merged` or, when `merged` is false, `open` with its pull request open (a PR never goes back from `merged`).
fn world_ending(fx: &Fx, plan: &str, merged: bool) -> World {
    let lead = fx
        .cpd(&[
            "plan",
            "new",
            plan,
            "--context",
            "ctx",
            "--title",
            &format!("Plan {plan}"),
        ])
        .ok()["lead"]["session"]
        .as_str()
        .unwrap()
        .to_owned();
    let repo = fx.keyed_repo(plan);
    let bare = fx.env.root().join("origins").join(format!("{plan}.git"));
    fs::create_dir_all(&bare).unwrap();
    fx.git(&bare, &["init", "-q", "--bare", "-b", "main"]);
    fx.local_origin(&repo, &bare);
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    fx.cpd(&[
        "workflow",
        "add",
        "api",
        "--repo",
        repo.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--plan",
        plan,
    ])
    .ok();
    fx.cpd(&[
        "job",
        "cancel",
        "api-config",
        "--reason",
        "a stub of the template",
        "--plan",
        plan,
    ])
    .ok();

    let (lead_worktree, attempt_worktree) = (
        fx.env.root().join("wt").join(format!("{plan}-api")),
        fx.env.root().join("wt").join(format!("{plan}-m-a1")),
    );
    let w = World {
        plan: plan.to_owned(),
        lead,
        run_dir: fx.env.cpd_home().join("run").join(plan),
        repo,
        bare,
        lead_worktree,
        attempt_worktree,
    };
    fx.git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &w.lead_branch(),
            w.lead_worktree.to_str().unwrap(),
            "main",
        ],
    );
    fx.git(&w.repo, &["push", "-q", "origin", &w.lead_branch()]);
    fx.git(
        &w.repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            &w.attempt_branch(),
            w.attempt_worktree.to_str().unwrap(),
            "main",
        ],
    );
    fs::create_dir_all(w.run_dir.join("specs")).unwrap();
    fs::write(w.run_dir.join("summary.md"), "summary\n").unwrap();
    fs::write(w.run_dir.join("specs/m-a1.md"), "spec\n").unwrap();

    let fingerprint = "f".repeat(64);
    fixture(
        fx,
        &format!(
            "UPDATE plans SET phase = 'plan' WHERE id = '{plan}';
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
               VALUES ('{plan}', 'api', 'm', 'Job m', 'S2', 'Goal.', 'Context.', '[\"src/m.rs\"]', '[\"cargo test\"]', '[]', 'draft', 0, cpd_txn(), 't');
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = '{plan}';
             UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE plan = '{plan}' AND key = 'm';
             UPDATE jobs SET status = 'cancelled', cancel_reason = 'a stub of the template', cancel_requested_at = 't' WHERE plan = '{plan}' AND status = 'draft';
             UPDATE workflows SET status = 'dispatch', lead_worktree = '{lead_wt}', started_at = 't' WHERE plan = '{plan}' AND key = 'api';
             UPDATE jobs SET status = 'running' WHERE plan = '{plan}' AND key = 'm';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, merged_sha, branch, worktree, started_at, ended_at)
               VALUES ('{plan}', 'm-a1', 'm', 1, 'S2', 'ladder', 'ended', 'merged', 'merged', 'c0ffee', '{branch}', '{attempt_wt}', 't', 't');
             UPDATE jobs SET status = 'merged', merged_sha = 'c0ffee' WHERE plan = '{plan}' AND key = 'm';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/{plan}', 11, 'https://github.com/acme/{plan}/pull/11', 'T', 0, '{state}', 'none', 'none', 't', {merged_at}, 't');
             UPDATE workflows SET status = 'open', pr_number = 11, pr_url = 'https://github.com/acme/{plan}/pull/11', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{plan}' AND number = 11)
              WHERE plan = '{plan}' AND key = 'api';
             {finish}",
            finish = if merged { format!("UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = '{plan}' AND key = 'api';") } else { String::new() },
            state = if merged { "merged" } else { "open" },
            merged_at = if merged { "'t'" } else { "NULL" },
            lead_wt = w.lead_worktree.display(),
            attempt_wt = w.attempt_worktree.display(),
            branch = w.attempt_branch(),
        ),
    );
    w
}

/// [`world`] whose PR is still `open`, with its pull request open.
pub fn world_with_open_pr(fx: &Fx, plan: &str) -> World {
    world_ending(fx, plan, false)
}

/// `plan archive`, after the fixture walked to what the archive rule wants: the Plan `done` (the Plans of
/// [`world`] have merged Workflows already).
pub fn archive(fx: &Fx, plan: &str) {
    fx.env.sql(&format!(
        "UPDATE plans SET phase = 'plan' WHERE id = '{plan}' AND phase = 'grounding';
         UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = '{plan}' AND phase = 'plan';
         UPDATE plans SET phase = 'deploy' WHERE id = '{plan}' AND phase = 'dispatch';
         UPDATE plans SET phase = 'done', done_at = 't' WHERE id = '{plan}' AND phase = 'deploy'"
    ));
    fx.cpd(&["plan", "archive", "--plan", plan]).ok();
}

pub fn count(fx: &Fx, sql: &str) -> i64 {
    fx.env.query(sql)
}

/// The rows the Plan owns, in the tables the delete empties.
pub fn plan_rows(fx: &Fx, plan: &str) -> i64 {
    // the Lead's row and the `plan-deleted` Event stay (commands.md §4.14): they are not what the delete empties
    ["jobs", "attempts", "workflows"]
        .iter()
        .map(|t| {
            count(
                fx,
                &format!("SELECT count(*) FROM {t} WHERE plan = '{plan}'"),
            )
        })
        .sum::<i64>()
        + count(
            fx,
            &format!(
                "SELECT count(*) FROM events WHERE plan = '{plan}' AND kind <> 'plan-deleted'"
            ),
        )
        + count(
            fx,
            &format!(
                "SELECT count(*) FROM sessions WHERE plan = '{plan}' AND id NOT IN (SELECT lead_session FROM plans)"
            ),
        )
}

pub fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

// ---------- tests ----------

#[test]
fn without_yes_lists_what_goes_and_deletes_nothing() {
    let fx = Fx::with_ports();
    base(&fx);
    let w = world(&fx, "pay");
    archive(&fx, "pay");
    let before = plan_rows(&fx, "pay");
    let events_before = count(&fx, "SELECT count(*) FROM events");

    let out = fx.cpd(&["plan", "delete", "pay"]).ok();
    assert_eq!(out["preview"], true, "{out}");
    assert_eq!(
        out["jobs"], 3,
        "the Job `m`, the cancelled stub `api-config` and the opener stub `open-api` {out}"
    );
    assert_eq!(out["attempts"], 1, "{out}");
    assert_eq!(out["sessions"], 1, "the Lead {out}");
    assert_eq!(
        out["worktrees"], 2,
        "the Lead worktree and the Attempt worktree {out}"
    );
    assert!(out["disk_freed_mb"].is_number(), "{out}");

    assert_eq!(plan_rows(&fx, "pay"), before, "nothing deleted");
    assert_eq!(
        count(&fx, "SELECT count(*) FROM events"),
        events_before,
        "no Event written"
    );
    assert!(w.lead_worktree.exists() && w.attempt_worktree.exists() && w.run_dir.exists());
    assert!(local_branches(&fx, &w).contains(&w.attempt_branch()));
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM plans WHERE id = 'pay' AND deleted_at IS NULL"),
        1
    );
}

#[test]
fn refuses_a_plan_that_is_not_archived_or_asleep() {
    let fx = Fx::with_ports();
    base(&fx);
    let w = world(&fx, "pay");

    let error = refused(
        &fx.cpd(&["plan", "delete", "pay", "--yes"]),
        "archive_blocked",
        3,
    );
    assert!(
        error["details"]["blockers"]
            .as_array()
            .is_some_and(|b| !b.is_empty()),
        "{error}"
    );
    refused(&fx.cpd(&["plan", "delete", "pay"]), "archive_blocked", 3);
    refused(
        &fx.cpd(&["plan", "delete", "nope", "--yes"]),
        "plan_not_found",
        4,
    );

    assert!(w.lead_worktree.exists() && w.run_dir.exists());
    assert!(plan_rows(&fx, "pay") > 0);
}

#[test]
fn refuses_with_live_work_or_an_unmerged_pr() {
    let fx = Fx::with_ports();
    base(&fx);
    let busy = world(&fx, "busy");
    archive(&fx, "busy");
    // a Session working after the archive: nothing running is a precondition of the delete itself
    fixture(
        &fx,
        &format!(
            "UPDATE sessions SET state = 'working' WHERE id = '{}'",
            busy.lead
        ),
    );
    let error = refused(
        &fx.cpd(&["plan", "delete", "busy", "--yes"]),
        "archive_blocked",
        3,
    );
    assert!(
        error["details"]["blockers"]
            .as_array()
            .is_some_and(|b| !b.is_empty()),
        "{error}"
    );

    let open = world_with_open_pr(&fx, "openpr");
    // A Plan whose Workflow is still open is never archived by `plan archive`: the row is set, as an older cpd wrote it.
    fixture(
        &fx,
        "UPDATE plans SET archived_at = 't' WHERE id = 'openpr'",
    );
    let error = refused(
        &fx.cpd(&["plan", "delete", "openpr", "--yes"]),
        "archive_blocked",
        3,
    );
    assert!(
        error["details"]["blockers"]
            .as_array()
            .is_some_and(|b| !b.is_empty()),
        "{error}"
    );

    assert!(busy.run_dir.exists() && open.run_dir.exists() && open.lead_worktree.exists());
    assert!(plan_rows(&fx, "busy") > 0 && plan_rows(&fx, "openpr") > 0);
}

#[test]
fn deletes_rows_files_and_worktrees_of_an_archived_plan() {
    let fx = Fx::with_ports();
    base(&fx);
    let w = world(&fx, "pay");
    let other = world(&fx, "other");
    archive(&fx, "pay");

    let out = fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();
    assert_eq!(
        (out["preview"].clone(), out["already"].clone()),
        (Value::Bool(false), Value::Bool(false)),
        "{out}"
    );

    assert_eq!(
        plan_rows(&fx, "pay"),
        0,
        "the Plan's Jobs, Attempts, PRs, Events and Sessions are gone"
    );
    assert_eq!(
        count(&fx, "SELECT count(*) FROM escalations WHERE plan = 'pay'"),
        0
    );
    assert_ne!(
        fx.env
            .query::<Option<String>>("SELECT deleted_at FROM plans WHERE id = 'pay'"),
        None,
        "the Plan row is marked, never deleted"
    );
    assert!(!w.run_dir.exists(), "$CPD_HOME/run/pay/");
    assert!(
        !w.lead_worktree.exists() && !w.attempt_worktree.exists(),
        "the worktrees"
    );
    let local = local_branches(&fx, &w);
    assert!(
        !local.contains(&w.lead_branch()) && !local.contains(&w.attempt_branch()),
        "the local branches: {local:?}"
    );

    let deleted = events(&fx, "plan-deleted");
    assert_eq!(deleted.len(), 1, "{deleted:?}");
    assert_eq!(deleted[0]["data"]["attempts"], 1);

    // another Plan is untouched
    assert!(plan_rows(&fx, "other") > 0 && other.run_dir.exists() && other.lead_worktree.exists());
    assert!(local_branches(&fx, &other).contains(&other.attempt_branch()));

    // idempotent: a rerun writes nothing
    let events_before = count(&fx, "SELECT count(*) FROM events");
    let again = fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();
    assert_eq!(again["already"], true, "{again}");
    assert_eq!(count(&fx, "SELECT count(*) FROM events"), events_before);
}

#[test]
fn deletes_the_plan_of_an_asleep_lead() {
    let fx = Fx::with_ports();
    base(&fx);
    let w = world(&fx, "pay");
    // asleep is a timestamp of the Session (contract §2.2), not a state: the Plan is not archived
    fixture(
        &fx,
        &format!(
            "UPDATE sessions SET asleep_at = 't' WHERE id = '{}'",
            w.lead
        ),
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM plans WHERE id = 'pay' AND archived_at IS NULL"),
        1
    );

    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(plan_rows(&fx, "pay"), 0);
    assert!(!w.run_dir.exists() && !w.lead_worktree.exists());
}

#[test]
fn keeps_context_records_and_remote_branches() {
    let fx = Fx::with_ports();
    base(&fx);
    fx.cpd(&["context", "note", "Mind the ledger.", "--context", "ctx"])
        .ok();
    let w = world(&fx, "pay");
    archive(&fx, "pay");
    let records = count(
        &fx,
        "SELECT count(*) FROM context_records WHERE context = 'ctx'",
    );
    assert!(records > 0);
    assert!(remote_branches(&fx, &w).contains(&w.lead_branch()));

    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(
        count(
            &fx,
            "SELECT count(*) FROM context_records WHERE context = 'ctx'"
        ),
        records,
        "Context records stay"
    );
    assert_eq!(
        fx.env
            .query::<i64>("SELECT count(*) FROM contexts WHERE id = 'ctx' AND deleted_at IS NULL"),
        1,
        "the Context stays"
    );
    assert!(
        remote_branches(&fx, &w).contains(&w.lead_branch()),
        "the remote branch is left alone"
    );
    assert!(
        fx.calls("gh")
            .iter()
            .all(|c| c.args.first().map(String::as_str) != Some("pr")
                || c.args.get(1).map(String::as_str) != Some("close")),
        "GitHub is left alone"
    );
}

#[test]
fn tamper_check_stays_clean_after_a_delete() {
    let fx = Fx::with_ports();
    base(&fx);
    world(&fx, "pay");
    let other = world(&fx, "other");
    archive(&fx, "pay");

    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    // every DELETE is in `history` (the tamper check's record of the deleted ids), and the check finds nothing
    assert!(
        count(
            &fx,
            "SELECT count(*) FROM history WHERE tbl = 'jobs' AND op = 'delete'"
        ) > 0
    );
    assert_eq!(fx.env.store().tamper().unwrap(), Vec::<Value>::new());
    // the guard is not weakened for any other write: another Plan's rows cannot be deleted outside cpd
    let raw = rusqlite::Connection::open(fx.env.cpd_home().join("cpd.db")).unwrap();
    assert!(
        raw.execute("DELETE FROM jobs WHERE plan = 'other'", [])
            .is_err(),
        "a raw delete is still refused"
    );
    assert!(
        raw.execute("DELETE FROM events WHERE plan = 'other'", [])
            .is_err(),
        "Events stay append-only"
    );
    assert!(plan_rows(&fx, "other") > 0 && other.run_dir.exists());
    // the store still writes, and the deleted id is never reused
    fx.cpd(&["context", "note", "After the delete.", "--context", "ctx"])
        .ok();
    let again = fx.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Again"]);
    assert_ne!(again.exit, 0, "a deleted id is never reused: {again:?}");
}

#[test]
fn deleting_a_plan_deletes_its_pr_posts() {
    let fx = Fx::with_ports();
    base(&fx);
    world(&fx, "pay");
    world(&fx, "other");
    let post = |plan: &str| {
        format!(
            "INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, reaction, reacted_at)
             VALUES ('{plan}', 'api', 'C02JU6C2Z1D', '1793040197.490209', 'https://missionlane.slack.com/archives/C02JU6C2Z1D/p1793040197490209', 't', NULL, NULL)"
        )
    };
    fixture(&fx, &format!("{}; {};", post("pay"), post("other")));
    archive(&fx, "pay");

    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(
        count(&fx, "SELECT count(*) FROM pr_posts WHERE plan = 'pay'"),
        0
    );
    assert_eq!(
        count(&fx, "SELECT count(*) FROM pr_posts WHERE plan = 'other'"),
        1,
        "another Plan's post stays"
    );
    assert_eq!(
        plan_rows(&fx, "pay"),
        0,
        "the Plan's other rows go as before"
    );
}

#[test]
fn deleting_a_plan_deletes_its_jira_transitions() {
    let fx = Fx::with_ports();
    base(&fx);
    world(&fx, "pay");
    world(&fx, "other");
    let row = |plan: &str| {
        format!(
            "INSERT INTO jira_transitions (plan, pr, target, queued_at) VALUES ('{plan}', 'api', 'in_review', 't')"
        )
    };
    fixture(&fx, &format!("{}; {};", row("pay"), row("other")));
    archive(&fx, "pay");

    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(
        count(
            &fx,
            "SELECT count(*) FROM jira_transitions WHERE plan = 'pay'"
        ),
        0
    );
    assert_eq!(
        count(
            &fx,
            "SELECT count(*) FROM jira_transitions WHERE plan = 'other'"
        ),
        1,
        "another Plan's row stays"
    );
}

#[test]
fn deleting_a_plan_deletes_its_job_addenda() {
    let fx = Fx::with_ports();
    base(&fx);
    world(&fx, "pay");
    world(&fx, "other");
    // `world` gives each Plan the Job `m` and its Attempt `m-a1`; `pay`'s row is unaddressed, `other`'s is addressed to an Attempt and a Session
    let columns = "(plan, job, number, attempt, session, text, created_at)";
    let unaddressed = format!(
        "INSERT INTO job_addenda {columns} VALUES ('pay', 'm', 1, NULL, NULL, 'a note', 't')"
    );
    let addressed = format!(
        "INSERT INTO job_addenda {columns} VALUES ('other', 'm', 1, 'm-a1', (SELECT lead_session FROM plans WHERE id = 'other'), 'a note', 't')"
    );
    fixture(&fx, &format!("{unaddressed}; {addressed};"));
    assert_eq!(count(&fx, "SELECT count(*) FROM job_addenda"), 2);
    archive(&fx, "pay");

    fx.cpd(&["plan", "delete", "pay", "--yes"]).ok();

    assert_eq!(
        count(&fx, "SELECT count(*) FROM job_addenda WHERE plan = 'pay'"),
        0
    );
    assert_eq!(
        count(&fx, "SELECT count(*) FROM job_addenda WHERE plan = 'other'"),
        1,
        "another Plan's row stays"
    );
    assert_eq!(
        plan_rows(&fx, "pay"),
        0,
        "the Plan's other rows go as before"
    );
}
