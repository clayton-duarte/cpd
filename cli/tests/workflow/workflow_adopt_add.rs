//! `pr add --adopt` and the `workflows.head_ref` schema (commands.md §9.4, contract §2.3): the built binary against a
//! temporary `$CPD_HOME`, a keyed repo and the fake `gh` answering `gh pr view` and `gh api user`.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::{Run, TestEnv, refused};
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::{Value, json};

const TESTS_TABLE: &str = "[tests]\nbuild = \"true\"\nrun = \"true\"\n";

struct Fx {
    env: TestEnv,
    pay: PathBuf,
}

/// The env, repo `pay`, Plan `pay-1` in `plan`, and a fake `gh`: `gh pr view` prints `<root>/view.json`, `gh api user`
/// prints `<root>/user` (the User's login, `bob`).
fn fx() -> Fx {
    let env = TestEnv::new();
    let pay = env.keyed_repo("pay").canonicalize().unwrap();
    fs::create_dir_all(pay.join(".config")).unwrap();
    fs::write(pay.join(".config/cpd.toml"), TESTS_TABLE).unwrap();
    env.git(&pay, &["add", "-A"]);
    env.git(&pay, &["commit", "-q", "-m", "tests table"]);
    env.git(&pay, &["update-ref", "refs/remotes/origin/main", "main"]);
    env.git(&pay, &["update-ref", "refs/remotes/origin/release", "main"]);
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let root = env.root().display().to_string();
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\"pr view\") cat '{root}/view.json';;\n\"api user\") cat '{root}/user';;\n*) echo \"unexpected gh $*\" >&2; exit 1;;\nesac"
    ));
    fs::write(env.root().join("user"), "bob\n").unwrap();
    let fx = Fx { env, pay };
    fx.view(json!({}));
    fx.plan("pay-1");
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(
            &CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(),
            5000,
        )
        .unwrap()
    }

    /// What `gh pr view 7` answers: the default pull request with `over` merged in.
    fn view(&self, over: Value) {
        let mut view = json!({"number": 7, "url": "https://github.com/acme/pay/pull/7", "state": "OPEN", "isCrossRepository": false,
                              "headRefName": "feat/x", "baseRefName": "release", "author": {"login": "bob"}});
        for (k, v) in over.as_object().unwrap() {
            view[k] = v.clone();
        }
        fs::write(self.env.root().join("view.json"), view.to_string()).unwrap();
    }

    fn plan(&self, id: &str) {
        self.env.sql(&format!(
            "INSERT OR IGNORE INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{id}', 'lead', 'ctx', '{id}', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at)
               VALUES ('{id}', 'ctx', 'Plan {id}', 'plan', 'lead-{id}', '1999-12-31T00:00:00.000Z');"
        ));
    }

    /// `cpd pr <args> --plan <plan>`.
    fn pr_in(&self, plan: &str, args: &[&str]) -> Run {
        let mut all = vec!["workflow"];
        all.extend(args);
        all.extend(["--plan", plan]);
        self.env.cpd(&all)
    }

    fn pr(&self, args: &[&str]) -> Run {
        self.pr_in("pay-1", args)
    }

    /// `pr add <key> --repo pay --title T --shape config --adopt 7 <extra…>` in `plan`.
    fn adopt_in(&self, plan: &str, key: &str, extra: &[&str]) -> Run {
        let mut args = vec![
            "add",
            key,
            "--repo",
            self.pay.to_str().unwrap(),
            "--title",
            "T",
            "--shape",
            "config",
            "--adopt",
            "7",
        ];
        args.extend(extra);
        self.pr_in(plan, &args)
    }

    fn adopt(&self, key: &str, extra: &[&str]) -> Run {
        self.adopt_in("pay-1", key, extra)
    }

    fn writes(&self) -> (i64, i64, i64) {
        (
            self.env.query("SELECT count(*) FROM txns"),
            self.env.query("SELECT count(*) FROM events"),
            self.env.query("SELECT count(*) FROM workflows"),
        )
    }

    /// [`common::refused`] with exit 3, and nothing written.
    fn refuse(&self, run: &Run, code: &str) -> Value {
        let before = self.writes();
        let error = refused(run, code, 3);
        assert_eq!(self.writes(), before, "{code} wrote something");
        error
    }
}

#[test]
fn adopts_a_pull_request_of_the_user_without_asking() {
    let fx = fx();
    let out = fx.adopt("web", &[]).ok();
    assert_eq!(out["workflow"]["head_ref"], "feat/x");
    assert_eq!(out["workflow"]["base"], "release");
    assert_eq!(out["workflow"]["status"], "waiting");
    assert_eq!(out["workflow"]["lead_branch"], "pay-1/web");
    assert_eq!(
        fx.env
            .query::<String>("SELECT head_ref FROM workflows WHERE key = 'web'"),
        "feat/x"
    );
}

#[test]
fn refuses_a_pull_request_of_another_author_until_the_author_is_confirmed() {
    let fx = fx();
    fx.view(json!({"author": {"login": "alice"}}));
    let before = fx.writes();
    let error = refused(&fx.adopt("web", &[]), "adopt_foreign", 3);
    assert_eq!(error["details"]["author"], "alice");
    assert_eq!(error["details"]["user"], "bob");
    assert_eq!(error["details"]["confirm"], "--confirm-author alice");
    assert_eq!(fx.writes(), before);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM workflows"), 0);
}

#[test]
fn adopts_a_pull_request_of_another_author_once_confirmed() {
    let fx = fx();
    fx.view(json!({"author": {"login": "alice"}}));
    fx.refuse(
        &fx.adopt("web", &["--confirm-author", "bob"]),
        "adopt_foreign",
    );
    let out = fx.adopt("web", &["--confirm-author", "Alice"]).ok();
    assert_eq!(out["workflow"]["head_ref"], "feat/x");
    assert_eq!(
        fx.env
            .query::<String>("SELECT head_ref FROM workflows WHERE key = 'web'"),
        "feat/x"
    );
}

#[test]
fn refuses_a_pull_request_that_is_not_open() {
    let fx = fx();
    for state in ["CLOSED", "MERGED"] {
        fx.view(json!({"state": state}));
        let error = fx.refuse(&fx.adopt("web", &[]), "adopt_not_open");
        assert_eq!(error["details"]["state"], state);
        assert_eq!(error["details"]["number"], 7);
    }
}

#[test]
fn refuses_a_pull_request_from_a_fork() {
    let fx = fx();
    fx.view(json!({"isCrossRepository": true}));
    let error = fx.refuse(&fx.adopt("web", &[]), "adopt_fork");
    assert_eq!(error["details"]["number"], 7);
}

#[test]
fn refuses_a_head_branch_that_is_a_stored_lead_branch() {
    let fx = fx();
    fx.pr(&[
        "add",
        "api",
        "--repo",
        fx.pay.to_str().unwrap(),
        "--title",
        "T",
        "--shape",
        "config",
    ])
    .ok();
    fx.view(json!({"headRefName": "pay-1/api"}));
    let error = fx.refuse(&fx.adopt("web", &[]), "adopt_cpd_branch");
    assert_eq!(error["details"]["head_ref"], "pay-1/api");
}

#[test]
fn refuses_a_head_branch_another_live_pr_holds() {
    let fx = fx();
    fx.plan("pay-2");
    fx.adopt("web", &[]).ok();
    let error = fx.refuse(&fx.adopt("api", &[]), "adopt_taken");
    assert_eq!(
        error["details"],
        json!({"head_ref": "feat/x", "plan": "pay-1", "workflow": "web"})
    );
    let error = fx.refuse(&fx.adopt_in("pay-2", "api", &[]), "adopt_taken");
    assert_eq!(
        error["details"],
        json!({"head_ref": "feat/x", "plan": "pay-1", "workflow": "web"})
    );
    // Cancelled, the head is free again.
    fx.pr(&["cancel", "web"]).ok();
    fx.adopt_in("pay-2", "api", &[]).ok();
}

#[test]
fn the_same_adopt_twice_is_already() {
    let fx = fx();
    let first = fx.adopt("web", &[]).ok();
    let before = fx.writes();
    let again = fx.adopt("web", &[]).ok();
    assert_eq!(again["already"], true);
    assert_eq!(again["workflow"], first["workflow"]);
    assert_eq!(fx.writes(), before);
}

#[test]
fn adopt_and_base_exclude_each_other() {
    let fx = fx();
    let before = fx.writes();
    let error = refused(&fx.adopt("web", &["--base", "main"]), "usage", 2);
    let help = error["details"]["help"].as_str().unwrap();
    assert!(
        help.contains("cannot be used with") && help.contains("--adopt") && help.contains("--base"),
        "not a conflict: {help}"
    );
    assert_eq!(fx.writes(), before);
}

#[test]
fn pr_edit_refuses_repo_and_base_of_an_adopted_pr() {
    let fx = fx();
    fx.adopt("web", &[]).ok();
    let error = fx.refuse(&fx.pr(&["edit", "web", "--base", "x"]), "workflow_adopted");
    assert_eq!(
        error["details"],
        json!({"workflow": "web", "flag": "--base"})
    );
    let error = fx.refuse(
        &fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap()]),
        "workflow_adopted",
    );
    assert_eq!(
        error["details"],
        json!({"workflow": "web", "flag": "--repo"})
    );
    fx.pr(&["edit", "web", "--title", "Renamed"]).ok();
    assert_eq!(
        fx.env
            .query::<String>("SELECT title FROM workflows WHERE key = 'web'"),
        "Renamed"
    );
}

#[test]
fn the_event_and_plan_show_name_the_head_branch() {
    let fx = fx();
    fx.adopt("web", &[]).ok();
    fx.pr(&[
        "add",
        "plain",
        "--repo",
        fx.pay.to_str().unwrap(),
        "--title",
        "P",
        "--shape",
        "config",
    ])
    .ok();
    let added = fx
        .env
        .cpd(&["events", "list", "--kind", "workflow-added"])
        .ok()["events"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(added.len(), 2);
    assert_eq!(added[0]["data"]["head_ref"], "feat/x");
    assert_eq!(added[0]["data"]["author"], "bob");
    assert!(
        added[1]["data"]["head_ref"].is_null() && added[1]["data"].get("head_ref").is_some(),
        "{}",
        added[1]
    );
    assert!(
        added[1]["data"]["author"].is_null() && added[1]["data"].get("author").is_some(),
        "{}",
        added[1]
    );
    let shown = fx.env.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
    let workflows = shown["workflows"].as_array().unwrap();
    let head = |key: &str| {
        workflows
            .iter()
            .find(|p| p["key"] == key)
            .unwrap()
            .get("head_ref")
            .cloned()
    };
    assert_eq!(head("web"), Some(json!("feat/x")));
    assert_eq!(head("plain"), Some(Value::Null));
}

#[test]
fn the_head_branch_of_a_pr_cannot_change_and_is_unique_among_live_prs() {
    let fx = fx();
    fx.plan("pay-2");
    let insert = |plan: &str, key: &str, status: &str| {
        format!(
            "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, head_ref, created_txn, created_at)
             VALUES ('{plan}', '{key}', '{repo}', 'pay', 'main', '{status}', 'cpd/{plan}/{key}', 'feat/y', cpd_txn(), 't');",
            repo = fx.pay.display()
        )
    };
    let write = |sql: &str| fx.store().write("fixture", |tx| Ok(tx.execute_batch(sql)?));
    write(&insert("pay-1", "a", "waiting")).expect("insert a PR with a head_ref");
    let err = write("UPDATE workflows SET head_ref = 'feat/z' WHERE key = 'a'")
        .expect_err("head_ref is fixed");
    assert!(format!("{err:?}").contains("workflow_locked"), "{err:?}");
    let err = write(&insert("pay-2", "b", "waiting")).expect_err("a second live PR with the head");
    assert!(
        format!("{err:?}").to_lowercase().contains("unique"),
        "{err:?}"
    );
    // A cancelled row does not count.
    write(&insert("pay-2", "c", "cancelled")).expect("a cancelled row does not hold the head");
}
