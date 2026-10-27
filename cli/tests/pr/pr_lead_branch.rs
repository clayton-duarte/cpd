//! The Lead Branch `<plan>/<key>` (contract §2.1, §3.8; commands.md `pr add`, `pr edit`, `pr start`): the name `pr add`
//! stores, git's own refusal over it (`branch_exists`), the start's branch and worktree, the legacy names left alone and
//! the adoption rule. `pr add`, `pr edit` and `pr start` in-process on `Fx::with_ports()` (the fake git route), the
//! adoption on the built binary with a fake `gh`.

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::PathBuf;

use common::fx::Fx;
use common::{Run, TestEnv, refused};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{Value, json};

const TESTS_TABLE: &str = "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n";

/// Plan `pay-1` in `plan`, repos `pay` and `web`, the `Fx` it derefs to.
struct Lb {
    fx: Fx,
    pay: PathBuf,
    web: PathBuf,
}

impl Deref for Lb {
    type Target = Fx;

    fn deref(&self) -> &Fx {
        &self.fx
    }
}

fn repo(fx: &Fx, name: &str) -> PathBuf {
    let repo = fx.keyed_repo(name).canonicalize().unwrap();
    fx.env.repo_policy("acme", name, TESTS_TABLE);
    repo
}

fn lb() -> Lb {
    lb_in("plan")
}

/// [`lb`] with Plan `pay-1` in `phase` (`plan`, or `done`).
fn lb_in(phase: &str) -> Lb {
    let fx = Fx::with_ports();
    let pay = repo(&fx, "pay");
    let web = repo(&fx, "web");
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    let (locked, done) = if phase == "done" { ("'t'", "'t'") } else { ("NULL", "NULL") };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', '{phase}', 'lead1', '1999-12-31T00:00:00.000Z', {locked}, {done});"
    ));
    Lb { fx, pay, web }
}

impl Lb {
    /// `pr add <key> --repo <repo> --title T --shape config --no-needs --plan pay-1`.
    fn add(&self, key: &str, repo: &PathBuf) -> Run {
        self.cpd(&["pr", "add", key, "--repo", repo.to_str().unwrap(), "--title", "T", "--shape", "config", "--no-needs", "--plan", "pay-1"])
    }

    fn start(&self, key: &str) -> Run {
        self.cpd(&["pr", "start", key, "--plan", "pay-1"])
    }

    fn dispatch(&self) {
        self.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay-1';");
    }

    fn writes(&self) -> (i64, i64) {
        (self.env.query("SELECT count(*) FROM txns"), self.env.query("SELECT count(*) FROM events"))
    }

    /// [`common::refused`] with exit 3, and nothing written.
    fn refuse(&self, run: &Run, code: &str) -> Value {
        let before = self.writes();
        let error = refused(run, code, 3);
        assert_eq!(self.writes(), before, "{code} wrote something");
        error
    }

    fn lead_branch(&self, key: &str) -> String {
        self.env.query(&format!("SELECT lead_branch FROM prs WHERE key = '{key}'"))
    }
}

#[test]
fn pr_add_names_the_lead_branch_plan_slash_key() {
    let fx = lb();
    let out = fx.add("api", &fx.pay).ok();
    assert_eq!(out["pr"]["lead_branch"], "pay-1/api");
    assert_eq!(fx.lead_branch("api"), "pay-1/api");
}

#[test]
fn pr_add_refuses_when_the_plan_slash_key_branch_exists() {
    let fx = lb();
    fx.git(&fx.web, &["branch", "pay-1/api"]);
    let error = fx.refuse(&fx.add("api", &fx.web), "branch_exists");
    assert_eq!(error["details"]["branch"], "pay-1/api");
}

#[test]
fn pr_add_refuses_when_the_plan_branch_exists() {
    let fx = lb();
    fx.git(&fx.web, &["branch", "pay-1"]);
    let error = fx.refuse(&fx.add("api", &fx.web), "branch_exists");
    assert_eq!(error["details"]["branch"], "pay-1");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM prs"), 0);
}

#[test]
fn pr_add_leaves_a_merged_legacy_lead_branch_alone() {
    let fx = lb_in("done");
    let origin = fx.env.root().join("repos/pay-origin.git");
    fx.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.local_origin(&fx.pay, &origin);
    fx.git(&fx.pay, &["push", "-q", "origin", "main"]);
    fx.git(&fx.pay, &["fetch", "-q", "origin"]);
    fx.git(&fx.pay, &["branch", "cpd/pay-1", "main"]);
    fx.git(&fx.pay, &["push", "-q", "origin", "cpd/pay-1"]);
    let sha = fx.git(&fx.pay, &["rev-parse", "refs/heads/cpd/pay-1"]);
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 1, 'u', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                          pr_number, pr_url, pr_opened_at, pull_request, merged_at)
           VALUES ('pay-1', 'old', '{}', 'main', 'merged', 'cpd/pay-1', '/w', cpd_txn(), 't', 't', 1, 'u', 't',
                   (SELECT id FROM pull_requests WHERE number = 1), 't');",
        fx.pay.display()
    ));

    let out = fx.add("web", &fx.pay).ok();

    assert_eq!(out["pr"]["lead_branch"], "pay-1/web");
    assert_eq!(fx.lead_branch("old"), "cpd/pay-1");
    assert_eq!(fx.git(&fx.pay, &["rev-parse", "refs/heads/cpd/pay-1"]), sha, "the legacy branch stays");
    assert_ne!(fx.git(&origin, &["branch", "--list", "cpd/pay-1"]), "", "and so does its remote branch");
    assert!(
        fx.calls("git").iter().all(|call| !call.args.iter().any(|a| a == "--delete")),
        "no `git push origin --delete` ran"
    );
}

#[test]
fn pr_start_creates_the_plan_slash_key_lead_branch_and_worktree() {
    let fx = lb();
    fx.add("api", &fx.pay).ok();
    fx.cpd(&["job", "cancel", "api-config", "--reason", "a stub", "--plan", "pay-1"]).ok();
    fx.drop_unreachable_origin(&fx.pay);
    fx.dispatch();

    let out = fx.start("api").ok();

    let worktree = fx.pay.parent().unwrap().join("pay.pay-1.api");
    assert_eq!(out["pr"]["lead_worktree"], worktree.display().to_string());
    assert_eq!(fx.git(&worktree, &["symbolic-ref", "--short", "HEAD"]), "pay-1/api");
}

#[test]
fn pr_start_refuses_when_the_plan_branch_exists() {
    let fx = lb();
    fx.add("api", &fx.pay).ok();
    fx.cpd(&["job", "cancel", "api-config", "--reason", "a stub", "--plan", "pay-1"]).ok();
    fx.drop_unreachable_origin(&fx.pay);
    fx.dispatch();
    fx.git(&fx.pay, &["branch", "pay-1"]);

    let error = fx.refuse(&fx.start("api"), "branch_exists");
    assert_eq!(error["details"]["branch"], "pay-1");
}

/// Guard (passes today): the legacy `cpd/<plan>/<key>` still refuses on its parent `cpd/<plan>`.
#[test]
fn pr_start_of_a_legacy_plan_key_branch_still_refuses_on_cpd_plan() {
    let fx = lb();
    fx.drop_unreachable_origin(&fx.pay);
    fx.env.sql(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('pay-1', 'api', '{}', 'main', 'waiting', 'cpd/pay-1/api', cpd_txn(), 't');",
        fx.pay.display()
    ));
    fx.dispatch();
    fx.git(&fx.pay, &["branch", "cpd/pay-1"]);

    let error = fx.refuse(&fx.start("api"), "branch_exists");
    assert_eq!(error["details"]["branch"], "cpd/pay-1");
}

#[test]
fn pr_edit_repo_refuses_when_the_plan_branch_exists() {
    let fx = lb();
    fx.add("api", &fx.pay).ok();
    fx.git(&fx.web, &["branch", "pay-1"]);
    let error = fx.refuse(&fx.cpd(&["pr", "edit", "api", "--repo", fx.web.to_str().unwrap(), "--plan", "pay-1"]), "branch_exists");
    assert_eq!(error["details"]["branch"], "pay-1");
}

// ---------- adoption ----------

struct Adopt {
    env: TestEnv,
    pay: PathBuf,
}

/// The env, repo `pay`, Plan `pay-1` and a fake `gh` answering `gh pr view` (`<root>/view.json`) and `gh api user`.
fn adopt_fx(head: &str) -> Adopt {
    let env = TestEnv::new();
    let pay = env.keyed_repo("pay").canonicalize().unwrap();
    fs::create_dir_all(pay.join(".config")).unwrap();
    fs::write(pay.join(".config/cpd.toml"), "[tests]\nbuild = \"true\"\nrun = \"true\"\n").unwrap();
    env.git(&pay, &["add", "-A"]);
    env.git(&pay, &["commit", "-q", "-m", "tests table"]);
    env.git(&pay, &["update-ref", "refs/remotes/origin/main", "main"]);
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let root = env.root().display().to_string();
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\"pr view\") cat '{root}/view.json';;\n\"api user\") cat '{root}/user';;\n*) echo \"unexpected gh $*\" >&2; exit 1;;\nesac"
    ));
    fs::write(env.root().join("user"), "bob\n").unwrap();
    let view = json!({"number": 7, "url": "https://github.com/acme/pay/pull/7", "state": "OPEN", "isCrossRepository": false,
                      "headRefName": head, "baseRefName": "main", "author": {"login": "bob"}});
    fs::write(env.root().join("view.json"), view.to_string()).unwrap();
    env.sql(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-pay-1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('pay-1', 'ctx', 'Plan', 'plan', 'lead-pay-1', '1999-12-31T00:00:00.000Z');",
    );
    Adopt { env, pay }
}

impl Adopt {
    fn add(&self, key: &str, extra: &[&str]) -> Run {
        let mut args = vec!["pr", "add", key, "--repo", self.pay.to_str().unwrap(), "--title", "T", "--shape", "config"];
        args.extend(extra);
        args.extend(["--plan", "pay-1"]);
        self.env.cpd(&args)
    }
}

#[test]
fn pr_adopt_refuses_a_head_branch_that_is_a_stored_lead_branch() {
    let fx = adopt_fx("pay-1/api");
    fx.add("api", &[]).ok();
    let error = refused(&fx.add("web", &["--adopt", "7"]), "adopt_cpd_branch", 3);
    assert_eq!(error["details"], json!({"number": 7, "head_ref": "pay-1/api"}));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM prs"), 1);
}

#[test]
fn pr_adopt_accepts_a_cpd_prefixed_head_branch_that_is_no_lead_branch() {
    let fx = adopt_fx("cpd/somebody-elses");
    let out = fx.add("web", &["--adopt", "7"]).ok();
    assert_eq!(out["pr"]["head_ref"], "cpd/somebody-elses");
}
