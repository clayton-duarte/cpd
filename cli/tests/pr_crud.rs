//! `pr add`, `pr edit` and `pr cancel` (commands.md §9.4–§9.6, contract §2.3,
//! §3.8, §5.2) and the plan fingerprint with the PRs: the built binary against
//! a temporary `$CPD_HOME`, temporary git repos, and a Plan written straight
//! into the database.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{Run, TestEnv, write_executable};
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::plan::plan_fingerprint;
use cpd_core::store::Store;
use serde_json::{Value, json};

/// The env with its config, repos `pay` and `web`, and Plan `pay-1` in `phase`
/// with no PR (the Lead's session beside it).
struct Fx {
    env: TestEnv,
    pay: PathBuf,
    web: PathBuf,
}

fn fx(phase: &str) -> Fx {
    let env = TestEnv::new();
    let pay = env.git_repo("pay").canonicalize().unwrap();
    let web = env.git_repo("web").canonicalize().unwrap();
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let locked = if matches!(phase, "grounding" | "plan") { "NULL" } else { "'2000-01-01T00:00:00.000Z'" };
    let done = if phase == "done" { "'2000-01-02T00:00:00.000Z'" } else { "NULL" };
    let fx = Fx { env, pay, web };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', '{phase}', 'lead1', '1999-12-31T00:00:00.000Z', {locked}, {done});"
    ));
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    /// `cpd pr <args> --plan pay-1`.
    fn pr(&self, args: &[&str]) -> Run {
        let mut all = vec!["pr"];
        all.extend(args);
        all.extend(["--plan", "pay-1"]);
        self.env.cpd(&all)
    }

    /// `pr add <key> --repo <repo> <extra…>`.
    fn add(&self, key: &str, repo: &PathBuf, extra: &[&str]) -> Run {
        let mut args = vec!["add", key, "--repo", repo.to_str().unwrap()];
        args.extend(extra);
        self.pr(&args)
    }

    /// A draft Job of PR `pr`.
    fn job(&self, key: &str, pr: &str, status: &str) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('pay-1', '{pr}', '{key}', 'Job {key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, iif('{status}' IN ('draft', 'cancelled'), NULL, printf('%064d', 1)),
                     iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');"
        ));
    }

    /// The counts of `txns` and `events`: unchanged by a refusal and by an `already`.
    fn writes(&self) -> (i64, i64) {
        (self.query("SELECT count(*) FROM txns"), self.query("SELECT count(*) FROM events"))
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.env.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
    }

    fn fingerprint(&self) -> String {
        let plan = PlanId::parse("pay-1").unwrap();
        self.store().read(|conn| plan_fingerprint(conn, &plan)).unwrap()
    }

    fn needs(&self, pr: &str) -> String {
        self.query(&format!("SELECT coalesce(group_concat(needs), '') FROM (SELECT needs FROM pr_needs WHERE pr = '{pr}' ORDER BY needs)"))
    }

    /// [`common::refused`], and nothing written.
    fn refused(&self, run: &Run, code: &str, exit: i32, writes: (i64, i64)) -> Value {
        let error = common::refused(run, code, exit);
        assert_eq!(self.writes(), writes, "{code} wrote something");
        error
    }
}

// ---------- pr add ----------

#[test]
fn add_creates_a_waiting_pr_with_its_event_and_no_git_write() {
    let fx = fx("grounding");
    let branches = fx.env.git(&fx.pay, &["branch", "--list"]);
    let out = fx.add("api", &fx.pay, &[]).ok();
    assert_eq!(
        out["pr"],
        json!({"key": "api", "repo": fx.pay.display().to_string(), "base": "main", "needs": [], "status": "waiting",
               "lead_branch": "cpd/pay-1/api", "appended": false})
    );
    assert!(out.get("already").is_none());
    let out = fx.add("web", &fx.web, &["--base", "main", "--needs", "api"]).ok();
    assert_eq!(out["pr"]["needs"], json!(["api"]));
    assert_eq!(out["pr"]["lead_branch"], "cpd/pay-1/web");

    let row: (String, String, String, Option<String>, String) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, repo, lead_branch, lead_worktree, base FROM prs WHERE key = 'web'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row, ("waiting".into(), fx.web.display().to_string(), "cpd/pay-1/web".into(), None, "main".into()));
    assert_eq!(fx.needs("web"), "api");
    assert_eq!(fx.env.git(&fx.pay, &["branch", "--list"]), branches, "no git write");

    let added = fx.events("pr-added");
    assert_eq!(added.len(), 2);
    assert_eq!(added[1]["plan"], "pay-1");
    assert_eq!(
        added[1]["data"],
        json!({"pr": "web", "repo": fx.web.display().to_string(), "base": "main", "needs": ["api"], "appended": false})
    );
}

#[test]
fn add_stores_the_realpath_of_the_main_worktree_of_the_repo_given() {
    let fx = fx("plan");
    let linked = fx.env.root().join("linked");
    fx.env.git(&fx.pay, &["worktree", "add", "-q", "-b", "other", linked.to_str().unwrap()]);
    let out = fx.add("api", &linked.join("."), &[]).ok();
    assert_eq!(out["pr"]["repo"], fx.pay.display().to_string());
}

#[test]
fn add_in_dispatch_appends_the_pr() {
    let fx = fx("dispatch");
    let out = fx.add("late", &fx.pay, &[]).ok();
    assert_eq!(out["pr"]["appended"], true);
    assert_eq!(out["pr"]["status"], "waiting");
    assert_eq!(fx.events("pr-added")[0]["data"]["appended"], true);
    // A rerun still says it was appended.
    assert_eq!(fx.add("late", &fx.pay, &[]).ok()["pr"]["appended"], true);
}

#[test]
fn add_again_with_the_resolved_flags_is_already_and_writes_nothing() {
    let fx = fx("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    let writes = fx.writes();
    let out = fx.add("web", &fx.web, &["--needs", "api"]).ok();
    assert_eq!((out["already"].clone(), out["pr"]["key"].clone()), (json!(true), json!("web")));
    assert_eq!(out["pr"]["lead_branch"], "cpd/pay-1/web");
    // The default base is `main`: saying so is the same.
    assert_eq!(fx.add("web", &fx.web, &["--base", "main", "--needs", "api"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
    // Other flags are `pr_exists`, and the answer says what is stored.
    let error = fx.refused(&fx.add("web", &fx.web, &[]), "pr_exists", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["api"]));
    fx.refused(&fx.add("web", &fx.pay, &["--needs", "api"]), "pr_exists", 3, writes);
}

#[test]
fn add_refusals_write_nothing() {
    let fx = fx("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("gone", &fx.web, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    let writes = fx.writes();

    // Usage (exit 2): a bad key, a repeated need, no --repo.
    fx.refused(&fx.add("Web", &fx.web, &[]), "invalid_pr_id", 2, writes);
    fx.refused(&fx.add("web", &fx.web, &["--needs", "api,api"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["add", "web"]), "usage", 2, writes);

    let not_a_repo = fx.env.root().join("not-a-repo");
    fs::create_dir_all(&not_a_repo).unwrap();
    fx.refused(&fx.add("web", &not_a_repo, &[]), "not_a_git_repo", 3, writes);
    let error = fx.refused(&fx.add("web", &fx.web, &["--base", "nope"]), "base_not_found", 4, writes);
    assert_eq!(error["details"]["base"], "nope");

    let error = fx.refused(&fx.add("web", &fx.web, &["--needs", "api,ghost,spook"]), "needs_unknown", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["ghost", "spook"]));
    fx.refused(&fx.add("web", &fx.web, &["--needs", "web"]), "needs_unknown", 3, writes);
    let error = fx.refused(&fx.add("web", &fx.web, &["--needs", "gone"]), "needs_unavailable", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["gone"]));
    // A cancelled PR keeps its key.
    fx.refused(&fx.add("gone", &fx.pay, &[]), "pr_exists", 3, writes);

    fx.env.git(&fx.web, &["branch", "cpd/pay-1/web"]);
    let error = fx.refused(&fx.add("web", &fx.web, &[]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "cpd/pay-1/web");
    fx.env.git(&fx.web, &["branch", "-D", "cpd/pay-1/web"]);
    fx.env.git(&fx.web, &["branch", "cpd/pay-1"]);
    let error = fx.refused(&fx.add("web", &fx.web, &[]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "cpd/pay-1");

    fx.refused(&fx.env.cpd(&["pr", "add", "web", "--repo", fx.web.to_str().unwrap()]), "plan_required", 2, writes);
    let run = fx.env.cpd(&["pr", "add", "web", "--repo", fx.web.to_str().unwrap(), "--plan", "nope"]);
    fx.refused(&run, "plan_not_found", 4, writes);
}

#[test]
fn add_in_deploy_or_done_reopens_the_plan_in_dispatch() {
    for phase in ["deploy", "done"] {
        let fx = fx(phase);
        let out = fx.add("fix", &fx.pay, &[]).ok();
        assert_eq!((out["pr"]["appended"].clone(), out["pr"]["status"].clone()), (json!(true), json!("waiting")), "{phase}");
        let row: (String, Option<String>) = fx
            .store()
            .read(|c| Ok(c.query_row("SELECT phase, done_at FROM plans WHERE id = 'pay-1'", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!(row, ("dispatch".to_owned(), None), "{phase}");

        let moved = fx.events("plan-phase");
        assert_eq!(moved.len(), 1, "{phase}");
        assert_eq!(moved[0]["data"], json!({"from": phase, "to": "dispatch"}));
        let added = fx.events("pr-added");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0]["data"]["appended"], true);
        assert!(moved[0]["id"].as_i64() < added[0]["id"].as_i64(), "plan-phase comes before pr-added");

        // A rerun is already and writes nothing.
        let writes = fx.writes();
        let again = fx.add("fix", &fx.pay, &[]).ok();
        assert_eq!((again["already"].clone(), again["pr"]["appended"].clone()), (json!(true), json!(true)));
        assert_eq!(fx.writes(), writes);
    }
}

#[test]
fn add_to_an_archived_plan_is_archived() {
    for phase in ["grounding", "dispatch", "done"] {
        let fx = fx(phase);
        fx.env.sql("UPDATE plans SET archived_at = '2001-01-01T00:00:00.000Z' WHERE id = 'pay-1'");
        let writes = fx.writes();
        let error = fx.refused(&fx.add("api", &fx.pay, &[]), "archived", 3, writes);
        // `refused` parses `details.next` with the real command tree; it names this Plan.
        let next = error["details"]["next"].as_str().unwrap();
        assert_eq!(next, "cpd plan unarchive --plan pay-1");
    }
}

/// Plan `pay-1` in `done` with one PR `old` on the legacy Lead Branch `cpd/pay-1`: the repo `pay`
/// has a bare `origin` with that branch, and a second worktree has it checked out.
struct Legacy {
    fx: Fx,
    origin: PathBuf,
    lead: PathBuf,
    sha: String,
}

fn legacy(status: &str) -> Legacy {
    let fx = fx("done");
    let origin = fx.env.root().join("repos/pay-origin.git");
    fx.env.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.env.git(&fx.pay, &["remote", "add", "origin", origin.to_str().unwrap()]);
    fx.env.git(&fx.pay, &["push", "-q", "origin", "main"]);
    fx.env.git(&fx.pay, &["fetch", "-q", "origin"]);
    let lead = fx.env.root().join("repos/pay.pay-1");
    fx.env.git(&fx.pay, &["worktree", "add", "-q", "-b", "cpd/pay-1", lead.to_str().unwrap(), "main"]);
    fx.env.git(&lead, &["config", "user.name", "cpd test"]);
    fx.env.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    fs::write(lead.join("api.txt"), "api\n").unwrap();
    fx.env.git(&lead, &["add", "-A"]);
    fx.env.git(&lead, &["commit", "-q", "-m", "api"]);
    fx.env.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay-1"]);
    let sha = fx.env.git(&lead, &["rev-parse", "HEAD"]);
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 1, 'u', 'T', 0, '{status}', 'none', 'none', 't', iif('{status}' = 'merged', 't', NULL), 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                            pr_number, pr_url, pr_opened_at, pull_request, merged_at)
           VALUES ('pay-1', 'old', '{}', 'main', '{status}', 'cpd/pay-1', '{}', cpd_txn(), 't', 't', 1, 'u', 't',
                   (SELECT id FROM pull_requests WHERE number = 1), iif('{status}' = 'merged', 't', NULL));",
        fx.pay.display(),
        lead.display()
    ));
    Legacy { fx, origin, lead, sha }
}

#[test]
fn add_moves_the_legacy_lead_branch_of_a_merged_pr() {
    let Legacy { fx, origin, lead, sha } = legacy("merged");
    let out = fx.add("fix", &fx.pay, &[]).ok();
    assert_eq!(out["pr"]["lead_branch"], "cpd/pay-1/fix");

    assert_eq!(fx.env.git(&fx.pay, &["rev-parse", "refs/heads/cpd/pay-1/old"]), sha);
    assert_eq!(fx.env.git(&fx.pay, &["branch", "--list", "cpd/pay-1"]), "", "gone locally");
    assert_eq!(fx.env.git(&origin, &["branch", "--list", "cpd/pay-1*"]), "", "gone on origin");
    assert_eq!(fx.env.git(&lead, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay-1/old", "the worktree follows");
    let branches: Vec<String> =
        fx.query::<String>("SELECT group_concat(key || '=' || lead_branch) FROM (SELECT * FROM prs ORDER BY key)").split(',').map(str::to_owned).collect();
    assert_eq!(branches, ["fix=cpd/pay-1/fix", "old=cpd/pay-1/old"]);
    assert_eq!(fx.query::<String>("SELECT phase FROM plans"), "dispatch");

    // A second PR finds nothing to move.
    fx.add("more", &fx.pay, &[]).ok();
}

#[test]
fn add_completes_a_legacy_move_that_stopped_after_the_rename() {
    let Legacy { fx, origin, sha, .. } = legacy("merged");
    fx.env.git(&fx.pay, &["branch", "-m", "cpd/pay-1", "cpd/pay-1/old"]);
    assert_ne!(fx.env.git(&origin, &["branch", "--list", "cpd/pay-1"]), "");

    fx.add("fix", &fx.pay, &[]).ok();
    assert_eq!(fx.env.git(&origin, &["branch", "--list", "cpd/pay-1*"]), "");
    assert_eq!(fx.env.git(&fx.pay, &["rev-parse", "refs/heads/cpd/pay-1/old"]), sha);
    assert_eq!(fx.query::<String>("SELECT lead_branch FROM prs WHERE key = 'old'"), "cpd/pay-1/old");
}

#[test]
fn add_refuses_push_failed_when_origin_will_not_delete_and_writes_nothing() {
    let Legacy { fx, origin, .. } = legacy("merged");
    let hook = origin.join("hooks/pre-receive");
    write_executable(&hook, "#!/bin/sh\necho no >&2\nexit 1\n");
    let writes = fx.writes();
    let error = fx.refused(&fx.add("fix", &fx.pay, &[]), "push_failed", 1, writes);
    assert_eq!(error["details"]["branch"], "cpd/pay-1");
    assert_eq!(error["details"]["repo"], fx.pay.display().to_string());
    assert!(error["details"]["stderr"].as_str().is_some_and(|s| s.contains("no")), "{error}");
    assert_eq!(fx.query::<String>("SELECT lead_branch FROM prs WHERE key = 'old'"), "cpd/pay-1");
    assert_eq!(fx.query::<String>("SELECT phase FROM plans"), "done");
}

#[test]
fn add_keeps_refusing_the_legacy_lead_branch_of_a_pr_that_is_not_merged() {
    let Legacy { fx, origin, lead, .. } = legacy("open");
    let writes = fx.writes();
    let error = fx.refused(&fx.add("fix", &fx.pay, &[]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "cpd/pay-1");
    assert_eq!(fx.env.git(&lead, &["symbolic-ref", "--short", "HEAD"]), "cpd/pay-1");
    assert_ne!(fx.env.git(&origin, &["branch", "--list", "cpd/pay-1"]), "");
}

// ---------- pr edit ----------

#[test]
fn edit_changes_the_repo_the_base_and_the_needs() {
    let fx = fx("plan");
    fx.env.git(&fx.web, &["branch", "develop"]);
    for key in ["a", "b", "c"] {
        fx.add(key, &fx.pay, &[]).ok();
    }
    fx.add("web", &fx.pay, &["--needs", "a,b"]).ok();

    let out = fx.pr(&["edit", "web", "--repo", fx.web.to_str().unwrap(), "--base", "develop", "--needs", "b,c"]).ok();
    assert_eq!(out["pr"], json!({"key": "web", "status": "waiting"}));
    assert_eq!(out["fields"], json!(["repo", "base", "needs"]));
    assert!(out.get("already").is_none());
    let (repo, base): (String, String) =
        fx.store().read(|c| Ok(c.query_row("SELECT repo, base FROM prs WHERE key = 'web'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!((repo, base), (fx.web.display().to_string(), "develop".to_owned()));
    assert_eq!(fx.needs("web"), "b,c");
    let edited = fx.events("pr-edited");
    assert_eq!(edited.len(), 1);
    assert_eq!(edited[0]["plan"], "pay-1");
    assert_eq!(edited[0]["data"], json!({"pr": "web", "fields": ["repo", "base", "needs"]}));

    // Only the fields whose value changed are listed; --repo keeps the stored base.
    let out = fx.pr(&["edit", "web", "--needs", "c", "--base", "develop"]).ok();
    assert_eq!(out["fields"], json!(["needs"]));
    let out = fx.pr(&["edit", "web", "--no-needs"]).ok();
    assert_eq!(out["fields"], json!(["needs"]));
    assert_eq!(fx.needs("web"), "");
    assert_eq!(fx.events("pr-edited").last().unwrap()["data"]["fields"], json!(["needs"]));
    let out = fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap(), "--base", "main"]).ok();
    assert_eq!(out["fields"], json!(["repo", "base"]));
}

#[test]
fn edit_with_no_change_is_already_and_writes_nothing() {
    let fx = fx("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    let writes = fx.writes();
    for args in [
        &["edit", "web", "--needs", "api"][..],
        &["edit", "web", "--base", "main"],
        &["edit", "web", "--repo", fx.web.to_str().unwrap()],
        &["edit", "api", "--no-needs"],
    ] {
        let out = fx.pr(args).ok();
        assert_eq!((out["already"].clone(), out["fields"].clone()), (json!(true), json!([])), "{args:?}");
        assert_eq!(fx.writes(), writes, "{args:?}");
    }
    assert!(fx.events("pr-edited").is_empty());
}

#[test]
fn edit_refusals_write_nothing() {
    let fx = fx("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    fx.add("ui", &fx.web, &["--needs", "web"]).ok();
    fx.add("gone", &fx.web, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    let writes = fx.writes();

    fx.refused(&fx.pr(&["edit", "web"]), "nothing_to_edit", 2, writes);
    fx.refused(&fx.pr(&["edit", "web", "--needs", "api", "--no-needs"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["edit", "web", "--needs", "api,api"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["edit", "nope", "--no-needs"]), "pr_not_found", 4, writes);
    fx.refused(&fx.pr(&["edit", "gone", "--no-needs"]), "illegal_transition", 3, writes);

    let not_a_repo = fx.env.root().join("not-a-repo");
    fs::create_dir_all(&not_a_repo).unwrap();
    fx.refused(&fx.pr(&["edit", "web", "--repo", not_a_repo.to_str().unwrap()]), "not_a_git_repo", 3, writes);
    let error = fx.refused(&fx.pr(&["edit", "web", "--base", "nope"]), "base_not_found", 4, writes);
    assert_eq!(error["details"]["base"], "nope");
    // --repo keeps the stored base, which must exist in the new repo.
    fx.env.git(&fx.web, &["branch", "develop"]);
    fx.pr(&["edit", "web", "--base", "develop"]).ok();
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap()]), "base_not_found", 4, writes);
    assert_eq!(error["details"]["base"], "develop");

    let error = fx.refused(&fx.pr(&["edit", "web", "--needs", "api,ghost"]), "needs_unknown", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["ghost"]));
    fx.refused(&fx.pr(&["edit", "web", "--needs", "web"]), "needs_unknown", 3, writes);
    let error = fx.refused(&fx.pr(&["edit", "web", "--needs", "gone"]), "needs_unavailable", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["gone"]));
    let error = fx.refused(&fx.pr(&["edit", "api", "--needs", "ui"]), "dependency_cycle", 3, writes);
    let cycle = error["details"]["cycle"].as_array().unwrap();
    assert_eq!((cycle.first(), cycle.len()), (cycle.last(), 4), "{cycle:?}");

    fx.env.git(&fx.pay, &["branch", "cpd/pay-1/web"]);
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap(), "--base", "main"]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "cpd/pay-1/web");
    fx.env.git(&fx.pay, &["branch", "-D", "cpd/pay-1/web"]);
    fx.env.git(&fx.pay, &["branch", "cpd/pay-1"]);
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap(), "--base", "main"]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "cpd/pay-1");
}

#[test]
fn edit_and_cancel_of_a_pr_that_is_not_waiting_are_illegal_transitions() {
    let fx = fx("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.env.sql("UPDATE prs SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE key = 'api'");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "api", "--no-needs"]), "illegal_transition", 3, writes);
    assert_eq!(error["details"]["from"], "dispatch");
    fx.refused(&fx.pr(&["cancel", "api"]), "illegal_transition", 3, writes);
}

#[test]
fn edit_and_cancel_are_refused_once_the_plan_is_locked() {
    let fx = fx("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z'");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "api", "--base", "main"]), "pr_locked", 3, writes);
    assert_eq!(error["details"]["phase"], "dispatch");
    fx.refused(&fx.pr(&["cancel", "nope"]), "pr_not_found", 4, writes);
}

// ---------- pr cancel ----------

#[test]
fn cancel_removes_the_pr_and_keeps_its_key() {
    let fx = fx("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    let out = fx.pr(&["cancel", "web"]).ok();
    assert_eq!(out["pr"], json!({"key": "web", "status": "cancelled"}));
    assert!(out.get("already").is_none());
    assert_eq!(fx.query::<String>("SELECT status FROM prs WHERE key = 'web'"), "cancelled");
    let cancelled = fx.events("pr-cancelled");
    assert_eq!(cancelled.len(), 1);
    assert_eq!((cancelled[0]["plan"].clone(), cancelled[0]["data"].clone()), (json!("pay-1"), json!({"pr": "web"})));

    // Idempotent.
    let writes = fx.writes();
    assert_eq!(fx.pr(&["cancel", "web"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
    // The key is kept: a new PR cannot take it, and nothing can need it.
    fx.refused(&fx.add("web", &fx.web, &[]), "pr_exists", 3, writes);
    fx.refused(&fx.add("ui", &fx.web, &["--needs", "web"]), "needs_unavailable", 3, writes);
}

#[test]
fn cancel_is_refused_while_a_job_or_a_pr_holds_the_pr() {
    let fx = fx("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    fx.job("a", "api", "draft");
    fx.job("b", "api", "cancelled");
    let writes = fx.writes();

    let error = fx.refused(&fx.pr(&["cancel", "api"]), "pr_in_use", 3, writes);
    assert_eq!((error["details"]["jobs"].clone(), error["details"]["prs"].clone()), (json!(["a"]), json!(["web"])));
    fx.env.sql("UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'a'");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "api"]), "pr_in_use", 3, writes);
    assert_eq!((error["details"]["jobs"].clone(), error["details"]["prs"].clone()), (json!([]), json!(["web"])));
    fx.pr(&["edit", "web", "--no-needs"]).ok();
    fx.pr(&["cancel", "api"]).ok();
    assert_eq!(fx.query::<String>("SELECT status FROM prs WHERE key = 'api'"), "cancelled");
}

impl Fx {
    /// A PR row as the lock and `pr open` leave it: `status` `dispatch` (a Lead worktree, no pull request),
    /// `open` or `merged` (with a pull request).
    fn locked_pr(&self, key: &str, repo: &PathBuf, status: &str) {
        let (number, url, opened, pull, merged) = match status {
            "dispatch" => ("NULL", "NULL", "NULL", "NULL", "NULL"),
            _ => ("7", "'u'", "'t'", "(SELECT id FROM pull_requests WHERE number = 7)", if status == "merged" { "'t'" } else { "NULL" }),
        };
        if status != "dispatch" {
            self.env.sql(&format!(
                "INSERT OR IGNORE INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                   VALUES ('acme/pay', 7, 'u', 'T', 0, '{status}', 'none', 'none', 't', {merged}, 't');"
            ));
        }
        self.env.sql(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at,
                              pull_request, merged_at, created_txn, created_at)
               VALUES ('pay-1', '{key}', '{}', 'main', '{status}', 'cpd/pay-1/{key}', '/w/{key}', 't', {number}, {url}, {opened}, {pull}, {merged},
                       cpd_txn(), 't');",
            repo.display()
        ));
    }

    fn pr_status(&self, key: &str) -> String {
        self.query(&format!("SELECT status FROM prs WHERE key = '{key}'"))
    }

    fn phase(&self) -> String {
        self.query("SELECT phase FROM plans WHERE id = 'pay-1'")
    }
}

// ---------- pr cancel after the lock ----------

#[test]
fn a_locked_pr_with_only_cancelled_jobs_is_cancelled_and_the_plan_reaches_done() {
    let fx = fx("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("gateci", &fx.web, "dispatch");
    fx.job("a", "gateci", "cancelled");
    assert_eq!(fx.phase(), "dispatch");

    let out = fx.pr(&["cancel", "gateci"]).ok();
    assert_eq!(out["pr"], json!({"key": "gateci", "status": "cancelled"}));
    assert_eq!(out["phase"], "done");
    assert_eq!(fx.pr_status("gateci"), "cancelled");
    assert_eq!(fx.phase(), "done");
    // The Locked Job is untouched, the Events are those of the cancel and the Plan's move.
    assert_eq!(fx.query::<String>("SELECT status FROM jobs WHERE key = 'a'"), "cancelled");
    assert_eq!(fx.events("pr-cancelled")[0]["data"], json!({"pr": "gateci"}));
    let phases = fx.events("plan-phase");
    assert_eq!((phases.len(), phases[0]["data"]["to"].clone()), (1, json!("deploy")));
    let done = fx.events("plan-done");
    assert_eq!((done.len(), done[0]["data"]["prs"][0]["key"].clone()), (1, json!("api")));

    // Idempotent.
    let writes = fx.writes();
    assert_eq!(fx.pr(&["cancel", "gateci"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
}

#[test]
fn a_locked_pr_with_no_job_is_cancelled_and_the_plan_waits_for_the_others() {
    let fx = fx("dispatch");
    fx.locked_pr("api", &fx.pay, "dispatch");
    fx.locked_pr("web", &fx.web, "dispatch");
    let out = fx.pr(&["cancel", "web"]).ok();
    assert!(out.get("phase").is_none(), "{out}");
    assert_eq!((fx.pr_status("web"), fx.phase()), ("cancelled".to_string(), "dispatch".to_string()));
    assert!(fx.events("plan-phase").is_empty());

    // A PR that is still `waiting` is cancelled too; the last one cancelled leaves the Plan in `dispatch`.
    fx.env.sql("UPDATE prs SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE key = 'api'");
    fx.env.sql(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay-1', 'late', '/x', 'main', 'waiting', 'cpd/pay-1/late', cpd_txn(), 't')",
    );
    let out = fx.pr(&["cancel", "late"]).ok();
    assert!(out.get("phase").is_none(), "no PR is left that is not cancelled: {out}");
    assert_eq!(fx.phase(), "dispatch");
}

#[test]
fn a_locked_pr_with_a_merged_or_live_job_or_a_pr_that_needs_it_is_refused_and_an_open_one_too() {
    let fx = fx("dispatch");
    for key in ["queued", "running", "merged"] {
        fx.locked_pr(key, &fx.pay, "dispatch");
        fx.job(&format!("{key}-old"), key, "cancelled");
        fx.job(&format!("{key}-job"), key, "queued");
    }
    fx.env.sql("UPDATE jobs SET status = 'running' WHERE key = 'running-job';
            UPDATE jobs SET status = 'running' WHERE key = 'merged-job';
            UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'merged-job'");
    let writes = fx.writes();
    for key in ["queued", "running", "merged"] {
        let error = fx.refused(&fx.pr(&["cancel", key]), "pr_in_use", 3, writes);
        assert_eq!((error["details"]["jobs"].clone(), error["details"]["prs"].clone()), (json!([format!("{key}-job")]), json!([])));
        assert_eq!(fx.pr_status(key), "dispatch");
    }

    // A live PR that needs it holds it (`pr_in_use`, `details.prs`).
    fx.locked_pr("base", &fx.pay, "dispatch");
    fx.env.sql(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay-1', 'dependent', '{}', 'main', 'waiting', 'cpd/pay-1/dependent', cpd_txn(), 't');
         INSERT INTO pr_needs (plan, pr, needs) VALUES ('pay-1', 'dependent', 'base')",
        fx.web.display()
    ));
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "base"]), "pr_in_use", 3, writes);
    assert_eq!(error["details"]["prs"], json!(["dependent"]));

    // An open pull request is out of scope, a merged one too.
    fx.locked_pr("opened", &fx.web, "open");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "opened"]), "illegal_transition", 3, writes);
    assert_eq!(error["details"]["from"], "open");
    fx.env.sql("UPDATE prs SET status = 'merged', merged_at = 't' WHERE key = 'opened'");
    let writes = fx.writes();
    fx.refused(&fx.pr(&["cancel", "opened"]), "illegal_transition", 3, writes);
}

// ---------- the plan fingerprint ----------

#[test]
fn the_plan_fingerprint_changes_with_a_prs_repo_base_needs_and_existence() {
    let fx = fx("plan");
    fx.env.git(&fx.web, &["branch", "develop"]);
    let none = fx.fingerprint();
    assert_eq!(fx.fingerprint(), none, "the same PRs give the same fingerprint");

    fx.add("api", &fx.pay, &[]).ok();
    assert_eq!(fx.fingerprint(), fx.fingerprint());
    let api = fx.fingerprint();
    fx.add("web", &fx.pay, &[]).ok();
    let mut states = vec![none.clone(), api.clone(), fx.fingerprint()];
    for args in [
        &["edit", "web", "--repo", fx.web.to_str().unwrap()][..],
        &["edit", "web", "--base", "develop"],
        &["edit", "web", "--needs", "api"],
    ] {
        fx.pr(args).ok();
        let next = fx.fingerprint();
        assert!(!states.contains(&next), "{args:?} did not change the fingerprint");
        states.push(next);
    }
    // Removing the need gives back the PR as the base edit left it.
    fx.pr(&["edit", "web", "--no-needs"]).ok();
    assert_eq!(fx.fingerprint(), states[4]);

    // A cancelled PR is out of it: the fingerprint is what it was without the PR.
    fx.pr(&["cancel", "web"]).ok();
    assert_eq!(fx.fingerprint(), api);
    fx.pr(&["cancel", "api"]).ok();
    assert_eq!(fx.fingerprint(), none);
}

#[test]
fn a_plan_review_goes_stale_when_a_pr_changes() {
    let fx = fx("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.pay, &[]).ok();
    fx.job("a", "api", "draft");
    let review = fx.env.cpd(&["plan", "review", "--plan", "pay-1"]).ok();
    let id = review["review"]["id"].as_str().unwrap().to_owned();
    assert_eq!(review["review"]["fingerprint"], fx.fingerprint());
    assert_eq!(fx.env.cpd(&["plan", "render", "--plan", "pay-1"]).ok()["fingerprint"], fx.fingerprint());
    assert_eq!(fx.env.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay-1"]).ok()["current"], true);
    let current = |fx: &Fx| fx.env.cpd(&["plan", "show", "--plan", "pay-1"]).ok()["review"]["current"].clone();
    assert_eq!(current(&fx), true);

    for args in [&["edit", "web", "--repo", fx.web.to_str().unwrap()][..], &["edit", "web", "--needs", "api"], &["cancel", "web"]] {
        fx.pr(args).ok();
        assert_eq!(current(&fx), false, "{args:?}");
        let lock = fx.env.cpd(&["plan", "lock", "--plan", "pay-1"]);
        assert_eq!(lock.error()["code"], "plan_not_approved", "{lock:?}");
        // A new review of what the PRs are now, approved, is current again.
        let id = fx.env.cpd(&["plan", "review", "--plan", "pay-1"]).ok()["review"]["id"].as_str().unwrap().to_owned();
        fx.env.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay-1"]).ok();
        assert_eq!(current(&fx), true, "{args:?}");
    }
}
