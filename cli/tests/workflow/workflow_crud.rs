//! `pr add`, `pr edit` and `pr cancel` (commands.md §9.4–§9.6, contract §2.3,
//! §3.8, §5.2) and the plan fingerprint with the PRs: in-process on `Fx::with_ports()` (the fake git route, the fake
//! `gh`), against a temporary `$CPD_HOME`, and a Plan written straight into the database.

use crate::common;

use std::fs;
use std::ops::Deref;
use std::path::PathBuf;

use common::fx::Fx;
use common::{Run, TestEnv};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Reply};
use cpd_core::ids::PlanId;
use cpd_core::plan::plan_fingerprint;
use cpd_core::store::Store;
use serde_json::{Value, json};

/// The in-process env (fake ports, the git route) with its config, repos `pay` and `web`, and Plan `pay-1` in `phase`
/// with no PR (the Lead's session beside it). It derefs to the [`Fx`] (`cpd`, `git`, `env`, `calls`, ...).
pub(super) struct Crud {
    pub(super) fx: Fx,
    pub(super) pay: PathBuf,
    pub(super) web: PathBuf,
}

impl Deref for Crud {
    type Target = Fx;

    fn deref(&self) -> &Fx {
        &self.fx
    }
}

/// The repo policy file's `[test-commands]` (contract §7.7): what a `tdd` PR needs of its repo.
const TESTS_TABLE: &str = "[test-commands]\nbuild = \"true\"\nrun = \"true\"\n";

/// A keyed repo of the `Universe` whose policy file has `[test-commands]`.
fn keyed_repo_with_tests(fx: &Fx, name: &str) -> PathBuf {
    let repo = fx.keyed_repo(name).canonicalize().unwrap();
    fx.env.repo_policy("acme", name, TESTS_TABLE);
    repo
}

/// The `INSERT`s of Context `ctx`, the Lead and Plan `pay-1` in `phase`.
fn plan_sql(phase: &str) -> String {
    let locked = if matches!(phase, "grounding" | "plan") { "NULL" } else { "'2000-01-01T00:00:00.000Z'" };
    let done = if phase == "done" { "'2000-01-02T00:00:00.000Z'" } else { "NULL" };
    format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', '{phase}', 'lead1', '1999-12-31T00:00:00.000Z', {locked}, {done});"
    )
}

pub(super) fn crud(phase: &str) -> Crud {
    let fx = Fx::with_ports();
    // `pr add` of the default shape, `tdd`, needs the repo policy's `[test-commands]`.
    let pay = keyed_repo_with_tests(&fx, "pay");
    let web = keyed_repo_with_tests(&fx, "web");
    fx.env.write_config("version = 1\nmachine = \"t\"\n");
    fx.env.sql(&plan_sql(phase));
    Crud { fx, pay, web }
}

impl Crud {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    /// `cpd pr <args> --plan pay-1`; see [`Crud::pr_in`].
    pub(super) fn pr(&self, args: &[&str]) -> Run {
        self.pr_in("pay-1", args)
    }

    /// `cpd pr <args> --plan <plan>`. A `pr add` that added a PR is followed by the cancelling of the stub drafts of its
    /// shape (`job cancel`): these tests are about PRs, and a PR with a live Job is not one `pr cancel` ends and no
    /// Plan review takes. The template itself is tested with [`Crud::add_with_stubs`].
    fn pr_in(&self, plan: &str, args: &[&str]) -> Run {
        let run = self.add_with_stubs(plan, args);
        if args.first() == Some(&"add") && run.exit == 0 {
            let out = run.json();
            if out.get("already").is_none() {
                for job in out["workflow"]["jobs"].as_array().unwrap() {
                    let job = job.as_str().unwrap();
                    self.cpd(&["job", "cancel", job, "--reason", "a stub of the template", "--plan", plan]).ok();
                }
            }
        }
        run
    }

    /// `cpd pr <args> --plan <plan>`, as it is: a `pr add` leaves its stub drafts.
    fn add_with_stubs(&self, plan: &str, args: &[&str]) -> Run {
        let mut all = vec!["workflow"];
        all.extend(args);
        all.extend(["--plan", plan]);
        self.cpd(&all)
    }

    /// `pr add <key> --repo <repo> --title "Change <key>" <extra…>`.
    fn add(&self, key: &str, repo: &PathBuf, extra: &[&str]) -> Run {
        let title = format!("Change {key}");
        // `--shape config`: one stub draft, `<key>-config`, beside the Jobs these tests add by hand.
        let mut args = vec!["add", key, "--repo", repo.to_str().unwrap(), "--title", &title, "--shape", "config"];
        args.extend(extra);
        self.pr(&args)
    }

    /// A draft Job of PR `pr`.
    fn job(&self, key: &str, pr: &str, status: &str) {
        self.job_kind(key, pr, status, "work");
    }

    /// [`Crud::job`] of kind `kind` (a `test` Job is only ever a draft or cancelled here: it has no `tests`).
    fn job_kind(&self, key: &str, pr: &str, status: &str, kind: &str) {
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('pay-1', '{pr}', '{key}', '{kind}', 'Job {key}', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, iif('{status}' IN ('draft', 'cancelled'), NULL, printf('%064d', 1)),
                     iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');"
        ));
    }

    /// The counts of `txns` and `events`: unchanged by a refusal and by an `already`.
    fn writes(&self) -> (i64, i64) {
        (self.env.query("SELECT count(*) FROM txns"), self.env.query("SELECT count(*) FROM events"))
    }

    fn events(&self, kind: &str) -> Vec<Value> {
        self.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
    }

    fn fingerprint(&self) -> String {
        let plan = PlanId::parse("pay-1").unwrap();
        self.store().read(|conn| plan_fingerprint(conn, &plan)).unwrap()
    }

    fn needs(&self, pr: &str) -> String {
        self.env.query(&format!("SELECT coalesce(group_concat(needs), '') FROM (SELECT needs FROM workflow_needs WHERE workflow = '{pr}' ORDER BY needs)"))
    }

    /// A Plan `id` in Context `context` (made when new, owned by `acme`), in `phase`, with its Lead.
    fn plan(&self, id: &str, context: &str, phase: &str) {
        let locked = if matches!(phase, "grounding" | "plan") { "NULL" } else { "'2000-01-01T00:00:00.000Z'" };
        self.env.sql(&format!(
            "INSERT OR IGNORE INTO contexts (id, title, owner, created_at, created_on) VALUES ('{context}', 'Other', 'acme', 't', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead-{id}', 'lead', '{context}', '{id}', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', 'lead-{id}', '1999-12-31T00:00:00.000Z', {locked});"
        ));
    }

    /// A `merged` PR written straight into the database, with its pull request `number`.
    fn merged_pr(&self, plan: &str, key: &str, repo: &PathBuf, number: i64) {
        self.env.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', {number}, 'u{number}', 'T', 0, 'merged', 'none', 'none', 't', 't', 't');
             INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at,
                              pull_request, merged_at, created_txn, created_at)
               VALUES ('{plan}', '{key}', '{repo}', 'pay', 'main', 'merged', 'cpd/{plan}/{key}', '/w/{key}', 't', {number}, 'u{number}', 't',
                       (SELECT id FROM pull_requests WHERE number = {number}), 't', cpd_txn(), 't');",
            repo = repo.display()
        ));
    }

    /// `[base, base_pr, needs]` of PR `key` of Plan `plan` as `plan show` prints them.
    fn shown(&self, plan: &str, key: &str) -> Value {
        let shown = self.cpd(&["plan", "show", "--plan", plan]).ok();
        let pr = shown["workflows"].as_array().unwrap().iter().find(|p| p["key"] == key).unwrap().clone();
        json!([pr["base"], pr["base_pr"], pr["needs"]])
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
fn add_creates_a_waiting_workflow_with_its_event_and_no_git_write() {
    let fx = crud("grounding");
    let branches = fx.git(&fx.pay, &["branch", "--list"]);
    let out = fx.add("api", &fx.pay, &[]).ok();
    assert_eq!(
        out["workflow"],
        json!({"key": "api", "repo": fx.pay.display().to_string(), "name": "pay", "base": "main", "base_pr": null, "title": "Change api", "needs": [], "status": "waiting",
               "lead_branch": "pay-1/api", "appended": false, "shape": "config", "head_ref": null, "jira": null, "jobs": ["api-config", "open-api"]})
    );
    assert!(out.get("already").is_none());
    let out = fx.add("web", &fx.web, &["--base", "main", "--needs", "api"]).ok();
    assert_eq!(out["workflow"]["needs"], json!(["api"]));
    assert_eq!(out["workflow"]["lead_branch"], "pay-1/web");

    let row: (String, String, String, Option<String>, String) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, repo, lead_branch, lead_worktree, base FROM workflows WHERE key = 'web'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?)
        })
        .unwrap();
    assert_eq!(row, ("waiting".into(), fx.web.display().to_string(), "pay-1/web".into(), None, "main".into()));
    assert_eq!(fx.needs("web"), "api");
    assert_eq!(fx.git(&fx.pay, &["branch", "--list"]), branches, "no git write");

    let added = fx.events("workflow-added");
    assert_eq!(added.len(), 2);
    assert_eq!(added[1]["plan"], "pay-1");
    assert_eq!(
        added[1]["data"],
        json!({"workflow": "web", "repo": fx.web.display().to_string(), "base": "main", "title": "Change web", "head_ref": null, "author": null, "needs": ["api"], "appended": false})
    );
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'web'"), "Change web");
    let shown = fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
    let titles: Vec<&Value> = shown["workflows"].as_array().unwrap().iter().map(|p| &p["title"]).collect();
    assert_eq!(titles, [&json!("Change api"), &json!("Change web")], "plan show's PR objects carry the title");
}

#[test]
fn add_accepts_a_title_of_72_characters() {
    let fx = crud("grounding");
    let title = "é".repeat(72);
    let out = fx.pr(&["add", "api", "--repo", fx.pay.to_str().unwrap(), "--title", &title, "--shape", "config"]).ok();
    assert_eq!(out["workflow"]["title"], title.as_str());
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), title);
}

#[test]
fn add_stores_the_realpath_of_the_main_worktree_of_the_repo_given() {
    let fx = crud("plan");
    let linked = fx.env.root().join("linked");
    fx.git(&fx.pay, &["worktree", "add", "-q", "-b", "other", linked.to_str().unwrap()]);
    let out = fx.add("api", &linked.join("."), &[]).ok();
    assert_eq!(out["workflow"]["repo"], fx.pay.display().to_string());
}

#[test]
fn add_in_dispatch_appends_the_workflow() {
    let fx = crud("dispatch");
    // The PR waits for another PR: a PR that starts at once is refused its stub drafts (`draft_parked`).
    fx.locked_pr("base", &fx.pay, "dispatch");
    let out = fx.add("late", &fx.pay, &["--needs", "base"]).ok();
    assert_eq!(out["workflow"]["appended"], true);
    assert_eq!(out["workflow"]["status"], "waiting");
    assert_eq!(fx.events("workflow-added")[0]["data"]["appended"], true);
    // A rerun still says it was appended.
    assert_eq!(fx.add("late", &fx.pay, &["--needs", "base"]).ok()["workflow"]["appended"], true);
}

// ---------- pr add --shape: the template of stub drafts, and the stored shape (commands.md §9.4, contract §2.2 "Job kinds") ----------

impl Crud {
    /// `[key, kind, title, status, appended, has tier, has goal, needs]` of every Job of PR `pr`, in creation order.
    fn stubs(&self, pr: &str) -> Value {
        let shown = self.cpd(&["plan", "show", "--plan", "pay-1"]).ok();
        let jobs: Vec<Value> = shown["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|j| j["pr"] == pr)
            .map(|j| json!([j["key"], j["kind"], j["title"], j["status"], j["appended"], !j["tier"].is_null(), !j["goal"].is_null(), j["needs"]]))
            .collect();
        json!(jobs)
    }

    fn add_shape(&self, key: &str, shape: Option<&str>) -> Run {
        let mut args = vec!["add", key, "--repo", self.pay.to_str().unwrap(), "--title", "Add retries"];
        args.extend(shape.into_iter().flat_map(|s| ["--shape", s]));
        args.push("--no-needs");
        self.add_with_stubs("pay-1", &args)
    }
}

#[test]
fn add_lays_out_the_tdd_template_by_default_as_stub_drafts() {
    let fx = crud("plan");
    let out = fx.add_shape("api", None).ok();
    assert_eq!((out["workflow"]["shape"].clone(), out["workflow"]["jobs"].clone()), (json!("tdd"), json!(["api-docs", "api-tests", "api-impl", "open-api"])));
    assert_eq!(
        fx.stubs("api"),
        json!([
            ["api-docs", "docs", "Docs: Add retries", "draft", false, false, false, []],
            ["api-tests", "test", "Red tests: Add retries", "draft", false, false, false, ["api-docs"]],
            ["api-impl", "impl", "Implement: Add retries", "draft", false, false, false, ["api-tests"]],
            ["open-api", "open", "Open the pull request: Add retries", "draft", false, true, true, []],
        ])
    );
    assert_eq!(fx.env.query::<String>("SELECT shape FROM workflows WHERE key = 'api'"), "tdd");
    // No fingerprint, no `tests`: a stub, completed later by `job edit`.
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE fingerprint IS NOT NULL OR tests IS NOT NULL OR test_files IS NOT NULL"), 0);
    // One `job-added` per stub, after `workflow-added`, with the kind.
    let added = fx.events("job-added");
    let kinds: Vec<(Value, Value)> = added.iter().map(|e| (e["job"].clone(), e["data"]["kind"].clone())).collect();
    assert_eq!(kinds, [(json!("api-docs"), json!("docs")), (json!("api-tests"), json!("test")), (json!("api-impl"), json!("impl")), (json!("open-api"), json!("open"))]);
    assert_eq!(
        added[1]["data"],
        json!({"workflow": "api", "kind": "test", "status": "draft", "appended": false, "tier": null, "needs": ["api-docs"], "urgent": false})
    );
    let order: Vec<i64> = [fx.events("workflow-added")[0]["id"].as_i64().unwrap(), added[0]["id"].as_i64().unwrap()].to_vec();
    assert!(order[0] < order[1], "workflow-added comes first: {order:?}");
}

#[test]
fn add_lays_out_one_stub_for_docs_and_for_config() {
    let fx = crud("plan");
    let out = fx.add_shape("api", Some("docs")).ok();
    assert_eq!((out["workflow"]["shape"].clone(), out["workflow"]["jobs"].clone()), (json!("docs"), json!(["api-docs", "open-api"])));
    assert_eq!(
        fx.stubs("api"),
        json!([
            ["api-docs", "docs", "Docs: Add retries", "draft", false, false, false, []],
            ["open-api", "open", "Open the pull request: Add retries", "draft", false, true, true, []],
        ])
    );
    let out = fx.add_shape("web", Some("config")).ok();
    assert_eq!((out["workflow"]["shape"].clone(), out["workflow"]["jobs"].clone()), (json!("config"), json!(["web-config", "open-web"])));
    assert_eq!(
        fx.stubs("web"),
        json!([
            ["web-config", "work", "Config: Add retries", "draft", false, false, false, []],
            ["open-web", "open", "Open the pull request: Add retries", "draft", false, true, true, []],
        ])
    );
    assert_eq!(fx.env.query::<String>("SELECT group_concat(key || '=' || shape) FROM (SELECT key, shape FROM workflows ORDER BY key)"), "api=docs,web=config");
}

#[test]
fn add_lays_out_the_opener_stub_in_every_shape() {
    let fx = crud("plan");
    for (key, shape) in [("api", "tdd"), ("web", "docs"), ("cfg", "config")] {
        let out = fx.add_shape(key, Some(shape)).ok();
        let opener = format!("open-{key}");
        let jobs = out["workflow"]["jobs"].as_array().unwrap();
        assert_eq!(jobs.last().unwrap(), &json!(opener), "the opener is the last Job of the {shape} template");
        assert!(jobs.contains(&json!(opener)), "{shape} does not report the opener key in pr.jobs");
        // The inserted row: kind `open`, the PR's own, `draft`, no Dependencies.
        let row: (String, String, i64, String) = fx
            .store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT j.kind, j.workflow, (SELECT count(*) FROM dependencies d WHERE d.plan = j.plan AND d.job = j.key), j.status
                       FROM jobs j WHERE j.plan = 'pay-1' AND j.key = ?1",
                    [opener.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap();
        assert_eq!(row, ("open".into(), key.into(), 0, "draft".into()), "the {shape} opener row");
    }
}

#[test]
fn the_opener_stub_carries_the_fixed_spec_and_open_tier() {
    let fx = crud("plan");
    fx.add_shape("api", None).ok();
    let open_tier = fx.cpd(&["config"]).ok()["config"]["open_tier"].as_str().unwrap().to_owned();
    let row: (String, String, Option<String>, i64, String, String, String, String, String) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, kind, fingerprint, appended, goal, context, touches, acceptance, out_of_scope
                   FROM jobs WHERE plan = 'pay-1' AND key = 'open-api'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
            )?)
        })
        .unwrap();
    // A draft with no fingerprint; the fixed Spec is stored, the Tier is the config's `open_tier`.
    assert_eq!((row.0.as_str(), row.1.as_str(), row.2.is_some(), row.3), ("draft", "open", false, 0));
    let tier: String = fx.store().read(|c| Ok(c.query_row("SELECT tier FROM jobs WHERE plan = 'pay-1' AND key = 'open-api'", [], |r| r.get(0))?)).unwrap();
    assert_eq!(tier, open_tier, "the opener is at the config's open_tier");
    for (name, value) in [("goal", &row.4), ("context", &row.5), ("touches", &row.6), ("acceptance", &row.7), ("out_of_scope", &row.8)] {
        assert!(!value.trim().is_empty() && value != "[]" && value != "null", "the fixed Spec has no {name}: {value}");
    }
    // The fixed Spec is rendered from the PR: its goal names the base the opener works against.
    assert!(row.4.contains("fit its base main"), "the fixed goal does not name the PR's base: {}", row.4);
    let added = fx.events("job-added");
    let event = added.iter().find(|e| e["job"] == "open-api").unwrap_or_else(|| panic!("no job-added for the opener: {added:?}"));
    assert_eq!((&event["data"]["workflow"], &event["data"]["kind"], &event["data"]["status"]), (&json!("api"), &json!("open"), &json!("draft")));
}

#[test]
fn the_stubs_of_a_workflow_added_after_the_lock_are_appended_and_a_reopened_plan_goes_back_to_dispatch() {
    for phase in ["dispatch", "deploy"] {
        let fx = crud(phase);
        // The PR waits for another PR: a PR that starts at once is refused its stub drafts (`draft_parked`).
        fx.locked_pr("base", &fx.pay, "dispatch");
        let args = ["add", "late", "--repo", fx.pay.to_str().unwrap(), "--title", "Add retries", "--shape", "docs", "--needs", "base"];
        fx.add_with_stubs("pay-1", &args).ok();
        assert_eq!(
            fx.stubs("late"),
            json!([
                ["late-docs", "docs", "Docs: Add retries", "draft", true, false, false, []],
                ["open-late", "open", "Open the pull request: Add retries", "draft", true, true, true, []],
            ]),
            "{phase}"
        );
        assert_eq!(fx.events("job-added")[0]["data"]["appended"], true, "{phase}");
        assert_eq!(fx.env.query::<String>("SELECT phase FROM plans"), "dispatch", "{phase}");
    }
}

#[test]
fn a_shape_is_one_of_tdd_docs_and_config() {
    let fx = crud("plan");
    let writes = fx.writes();
    for shape in ["free", "other", ""] {
        let run = fx.add_shape("api", Some(shape));
        assert_eq!(run.error()["code"], "usage", "{shape:?}");
    }
    assert_eq!(fx.writes(), writes);
}

#[test]
fn add_refuses_a_tdd_workflow_in_a_repo_without_test_commands_and_reads_them_from_its_policy_file() {
    let fx = crud("plan");
    let bare = fx.keyed_repo("bare").canonicalize().unwrap();
    let add = |shape: Option<&str>| {
        let mut args = vec!["add", "api", "--repo", bare.to_str().unwrap(), "--title", "Add retries"];
        args.extend(shape.into_iter().flat_map(|s| ["--shape", s]));
        fx.add_with_stubs("pay-1", &args)
    };
    let writes = fx.writes();
    // `tdd`, by default or named, needs the table; the message names the ways out.
    for shape in [None, Some("tdd")] {
        let error = fx.refused(&add(shape), "tests_unconfigured", 3, writes);
        let message = error["message"].as_str().unwrap();
        assert!(message.contains("--shape docs") && message.contains("--shape config") && message.contains("[test-commands]"), "{message}");
    }
    // The table is read from the repo's policy file, not from its tree.
    fx.env.repo_policy("acme", "bare", TESTS_TABLE);
    assert_eq!(add(None).ok()["workflow"]["shape"], "tdd");
    // A repo that never had it is fine for a `docs` or a `config` PR.
    let plain = fx.keyed_repo("plain").canonicalize().unwrap();
    for (key, shape) in [("docs-pr", "docs"), ("cfg-pr", "config")] {
        let args = ["add", key, "--repo", plain.to_str().unwrap(), "--title", "No behaviour", "--shape", shape];
        assert_eq!(fx.add_with_stubs("pay-1", &args).ok()["workflow"]["shape"], shape);
    }
}

#[test]
fn add_refuses_a_test_commands_table_that_is_not_as_documented() {
    let fx = crud("plan");
    let bad = fx.keyed_repo("bad").canonicalize().unwrap();
    fx.env.repo_policy("acme", "bad", "[test-commands]\nbuild = \"true\"\n");
    let writes = fx.writes();
    let args = ["add", "api", "--repo", bad.to_str().unwrap(), "--title", "Add retries"];
    let error = fx.refused(&fx.add_with_stubs("pay-1", &args), "policy_invalid", 1, writes);
    assert!(error["message"].as_str().unwrap().contains("run"), "{error}");
}

#[test]
fn add_refuses_a_tdd_template_after_the_tests_table_before_the_job_keys() {
    // §9.4's order: `tests_unconfigured` precedes `job_key_exists`.
    let fx = crud("plan");
    let bare = fx.keyed_repo("bare").canonicalize().unwrap();
    fx.env.sql(
        "INSERT INTO workflows (plan, key, repo, base, title, status, lead_branch, created_txn, created_at) VALUES ('pay-1', 'other', '/x', 'main', 'T', 'waiting', 'cpd/pay-1/other', cpd_txn(), 't');
         INSERT INTO jobs (plan, workflow, key, title, status, appended, created_txn, created_at) VALUES ('pay-1', 'other', 'api-impl', 'T', 'draft', 0, cpd_txn(), 't')",
    );
    let writes = fx.writes();
    let args = ["add", "api", "--repo", bare.to_str().unwrap(), "--title", "Add retries"];
    fx.refused(&fx.add_with_stubs("pay-1", &args), "tests_unconfigured", 3, writes);
}

#[test]
fn add_refuses_a_template_whose_job_keys_the_plan_has_and_writes_nothing() {
    let fx = crud("plan");
    fx.add_shape("api", Some("config")).ok();
    // A Job `web-impl` of another PR is in the way of the tdd template of `web`.
    fx.env.sql(
        "INSERT INTO jobs (plan, workflow, key, title, status, appended, created_txn, created_at) VALUES ('pay-1', 'api', 'web-impl', 'T', 'draft', 0, cpd_txn(), 't')",
    );
    let writes = fx.writes();
    let error = fx.refused(&fx.add_shape("web", None), "job_key_exists", 3, writes);
    assert_eq!((error["details"]["pr"].clone(), error["details"]["jobs"].clone()), (json!("web"), json!(["web-impl"])));
    // The docs template of `web` does not lay out that key.
    fx.add_shape("web", Some("docs")).ok();
}

#[test]
fn the_shape_is_part_of_what_add_compares() {
    let fx = crud("plan");
    fx.add_shape("api", Some("docs")).ok();
    let writes = fx.writes();
    // The same flags, the shape included, are `already`: the default, `tdd`, is another shape.
    assert_eq!(fx.add_shape("api", Some("docs")).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
    let error = fx.refused(&fx.add_shape("api", None), "workflow_exists", 3, writes);
    assert_eq!(error["details"]["shape"], "docs");
    fx.refused(&fx.add_shape("api", Some("config")), "workflow_exists", 3, writes);
}

#[test]
fn add_again_with_the_resolved_flags_is_already_and_writes_nothing() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    let writes = fx.writes();
    let out = fx.add("web", &fx.web, &["--needs", "api"]).ok();
    assert_eq!((out["already"].clone(), out["workflow"]["key"].clone()), (json!(true), json!("web")));
    assert_eq!(out["workflow"]["lead_branch"], "pay-1/web");
    // The default base is `main`: saying so is the same.
    assert_eq!(fx.add("web", &fx.web, &["--base", "main", "--needs", "api"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
    // Other flags are `workflow_exists`, and the answer says what is stored.
    let error = fx.refused(&fx.add("web", &fx.web, &[]), "workflow_exists", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["api"]));
    // So is another title.
    let other = fx.pr(&["add", "web", "--repo", fx.web.to_str().unwrap(), "--title", "Another", "--needs", "api", "--shape", "config"]);
    let error = fx.refused(&other, "workflow_exists", 3, writes);
    assert_eq!(error["details"]["title"], "Change web");
    fx.refused(&fx.add("web", &fx.pay, &["--needs", "api"]), "workflow_exists", 3, writes);
}

#[test]
fn add_stores_the_name_parsed_from_origin_lowercased() {
    let fx = crud("grounding");
    fx.git(&fx.web, &["remote", "set-url", "origin", "git@github.com:ACME/Web.Site.git"]);
    fx.env.repo_policy("acme", "web.site", "");
    let out = fx.add("web", &fx.web, &[]).ok();
    assert_eq!(out["workflow"]["name"], "web.site");
    assert_eq!(fx.env.query::<String>("SELECT name FROM workflows WHERE plan = 'pay-1' AND key = 'web'"), "web.site");
}

#[test]
fn add_keys_the_repo_before_anything_is_written() {
    let fx = crud("grounding");

    // owner_unset: the Context has no owner (a Context made before the policy keys).
    fx.env.sql("UPDATE contexts SET owner = NULL WHERE id = 'ctx'");
    let writes = fx.writes();
    let error = fx.refused(&fx.add("api", &fx.pay, &[]), "owner_unset", 3, writes);
    assert_eq!(error["details"]["context"], "ctx");
    assert!(error["details"].get("next").is_none() && error["message"].as_str().unwrap().contains("cpd context owner <login> --context ctx"), "{error}");
    fx.env.sql("UPDATE contexts SET owner = 'acme' WHERE id = 'ctx'");
    let writes = fx.writes();

    // No origin: the repo is clayton-duarte's, so another owner's Context is owner_mismatch.
    fx.git(&fx.pay, &["remote", "remove", "origin"]);
    let error = fx.refused(&fx.add("api", &fx.pay, &[]), "owner_mismatch", 3, writes);
    assert_eq!((&error["details"]["owner"], &error["details"]["repo_owner"]), (&json!("acme"), &json!("clayton-duarte")));

    // repo_unkeyed (exit 1): a local path, another host, an SSH alias.
    for url in ["/tmp/somewhere/pay.git", "https://gitlab.com/acme/pay.git", "git@github-work:acme/pay.git"] {
        fx.git(&fx.pay, &["remote", "add", "origin", url]);
        let error = fx.refused(&fx.add("api", &fx.pay, &[]), "repo_unkeyed", 1, writes);
        assert_eq!(error["details"]["url"], url);
        fx.git(&fx.pay, &["remote", "remove", "origin"]);
    }

    // owner_mismatch (exit 3): another owner's repo.
    fx.git(&fx.pay, &["remote", "add", "origin", "https://github.com/other/pay.git"]);
    let error = fx.refused(&fx.add("api", &fx.pay, &[]), "owner_mismatch", 3, writes);
    assert_eq!((&error["details"]["owner"], &error["details"]["repo_owner"]), (&json!("acme"), &json!("other")));

    // owner_unconfirmed (exit 1): the Context's owner has no policy file.
    fx.git(&fx.pay, &["remote", "set-url", "origin", "https://github.com/acme/pay.git"]);
    fs::remove_file(fx.env.cpd_home().join("policy/acme/owner.toml")).unwrap();
    let error = fx.refused(&fx.add("api", &fx.pay, &[]), "owner_unconfirmed", 1, writes);
    assert_eq!(error["details"]["owner"], "acme");
    fs::write(fx.env.cpd_home().join("policy/acme/owner.toml"), "").unwrap();

    // With every refusal cleared (`remote remove` dropped the tracking ref), the same add succeeds.
    fx.git(&fx.pay, &["update-ref", "refs/remotes/origin/main", "main"]);
    fx.add("api", &fx.pay, &[]).ok();
}

#[test]
fn edit_repo_keys_the_new_repo_and_stores_its_name() {
    let fx = crud("grounding");
    fx.add("web", &fx.web, &[]).ok();
    let writes = fx.writes();
    fx.git(&fx.pay, &["remote", "set-url", "origin", "https://github.com/other/pay.git"]);
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap()]), "owner_mismatch", 3, writes);
    assert_eq!(error["details"]["repo_owner"], "other");
    fx.git(&fx.pay, &["remote", "set-url", "origin", "git@github-work:acme/pay.git"]);
    fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap()]), "repo_unkeyed", 1, writes);
    fx.git(&fx.pay, &["remote", "set-url", "origin", "https://github.com/acme/Payments.git"]);
    fx.env.repo_policy("acme", "payments", "");
    fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap()]).ok();
    assert_eq!(fx.env.query::<String>("SELECT name FROM workflows WHERE plan = 'pay-1' AND key = 'web'"), "payments");
}

#[test]
fn add_refusals_write_nothing() {
    let fx = crud("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("gone", &fx.web, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    let writes = fx.writes();

    // Usage (exit 2): a bad key, a repeated need, no --repo.
    fx.refused(&fx.add("Web", &fx.web, &[]), "invalid_workflow_id", 2, writes);
    fx.refused(&fx.add("web", &fx.web, &["--needs", "api,api"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["add", "web"]), "usage", 2, writes);
    // `--title` is required, 1 to 72 characters on one line.
    let web = fx.web.to_str().unwrap();
    fx.refused(&fx.pr(&["add", "web", "--repo", web]), "usage", 2, writes);
    for bad in ["", &"a".repeat(73), "one\ntwo"] {
        fx.refused(&fx.pr(&["add", "web", "--repo", web, "--title", bad]), "usage", 2, writes);
    }

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
    fx.refused(&fx.add("gone", &fx.pay, &[]), "workflow_exists", 3, writes);

    fx.git(&fx.web, &["branch", "pay-1/web"]);
    let error = fx.refused(&fx.add("web", &fx.web, &[]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "pay-1/web");
    fx.git(&fx.web, &["branch", "-D", "pay-1/web"]);
    fx.git(&fx.web, &["branch", "pay-1"]);
    let error = fx.refused(&fx.add("web", &fx.web, &[]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "pay-1");
    fx.git(&fx.web, &["branch", "-D", "pay-1"]);

    fx.refused(&fx.cpd(&["workflow", "add", "web", "--repo", web, "--title", "T"]), "plan_required", 2, writes);
    let run = fx.cpd(&["workflow", "add", "web", "--repo", web, "--title", "T", "--plan", "nope"]);
    fx.refused(&run, "plan_not_found", 4, writes);
}

#[test]
fn add_in_deploy_or_done_reopens_the_plan_in_dispatch() {
    for phase in ["deploy", "done"] {
        let fx = crud(phase);
        // The PR waits for another PR: a PR that starts at once is refused its stub drafts (`draft_parked`).
        fx.locked_pr("base", &fx.pay, "dispatch");
        let out = fx.add("fix", &fx.pay, &["--needs", "base"]).ok();
        assert_eq!((out["workflow"]["appended"].clone(), out["workflow"]["status"].clone()), (json!(true), json!("waiting")), "{phase}");
        let row: (String, Option<String>) = fx
            .store()
            .read(|c| Ok(c.query_row("SELECT phase, done_at FROM plans WHERE id = 'pay-1'", [], |r| Ok((r.get(0)?, r.get(1)?)))?))
            .unwrap();
        assert_eq!(row, ("dispatch".to_owned(), None), "{phase}");

        let moved = fx.events("plan-phase");
        assert_eq!(moved.len(), 1, "{phase}");
        assert_eq!(moved[0]["data"], json!({"from": phase, "to": "dispatch"}));
        let added = fx.events("workflow-added");
        assert_eq!(added.len(), 1);
        assert_eq!(added[0]["data"]["appended"], true);
        assert!(moved[0]["id"].as_i64() < added[0]["id"].as_i64(), "plan-phase comes before workflow-added");

        // A rerun is already and writes nothing.
        let writes = fx.writes();
        let again = fx.add("fix", &fx.pay, &["--needs", "base"]).ok();
        assert_eq!((again["already"].clone(), again["workflow"]["appended"].clone()), (json!(true), json!(true)));
        assert_eq!(fx.writes(), writes);
    }
}

#[test]
fn add_to_an_archived_plan_is_archived() {
    for phase in ["grounding", "dispatch", "done"] {
        let fx = crud(phase);
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
    fx: Crud,
    origin: PathBuf,
    lead: PathBuf,
    sha: String,
}

fn legacy(status: &str) -> Legacy {
    let fx = crud("done");
    let origin = fx.env.root().join("repos/pay-origin.git");
    fx.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.local_origin(&fx.pay, &origin);
    fx.git(&fx.pay, &["push", "-q", "origin", "main"]);
    fx.git(&fx.pay, &["fetch", "-q", "origin"]);
    let lead = fx.env.root().join("repos/pay.pay-1");
    fx.git(&fx.pay, &["worktree", "add", "-q", "-b", "cpd/pay-1", lead.to_str().unwrap(), "main"]);
    fx.git(&lead, &["config", "user.name", "cpd test"]);
    fx.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    fx.commit_file(&lead, "api.txt", "api\n", "api");
    fx.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay-1"]);
    let sha = fx.git(&lead, &["rev-parse", "HEAD"]);
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 1, 'u', 'T', 0, '{status}', 'none', 'none', 't', iif('{status}' = 'merged', 't', NULL), 't');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                            pr_number, pr_url, pr_opened_at, pull_request, merged_at)
           VALUES ('pay-1', 'old', '{}', 'main', '{status}', 'cpd/pay-1', '{}', cpd_txn(), 't', 't', 1, 'u', 't',
                   (SELECT id FROM pull_requests WHERE number = 1), iif('{status}' = 'merged', 't', NULL));",
        fx.pay.display(),
        lead.display()
    ));
    Legacy { fx, origin, lead, sha }
}

// ---------- pr edit ----------

#[test]
fn edit_changes_the_repo_the_base_and_the_needs() {
    let fx = crud("plan");
    fx.git(&fx.web, &["branch", "develop"]);
    fx.git(&fx.web, &["update-ref", "refs/remotes/origin/develop", "develop"]);
    for key in ["a", "b", "c"] {
        fx.add(key, &fx.pay, &[]).ok();
    }
    fx.add("web", &fx.pay, &["--needs", "a,b"]).ok();

    let out = fx.pr(&["edit", "web", "--repo", fx.web.to_str().unwrap(), "--base", "develop", "--needs", "b,c"]).ok();
    assert_eq!(out["workflow"], json!({"key": "web", "status": "waiting"}));
    assert_eq!(out["fields"], json!(["repo", "base", "needs"]));
    assert!(out.get("already").is_none());
    let (repo, base): (String, String) =
        fx.store().read(|c| Ok(c.query_row("SELECT repo, base FROM workflows WHERE key = 'web'", [], |r| Ok((r.get(0)?, r.get(1)?)))?)).unwrap();
    assert_eq!((repo, base), (fx.web.display().to_string(), "develop".to_owned()));
    assert_eq!(fx.needs("web"), "b,c");
    let edited = fx.events("workflow-edited");
    assert_eq!(edited.len(), 1);
    assert_eq!(edited[0]["plan"], "pay-1");
    assert_eq!(edited[0]["data"], json!({"workflow": "web", "fields": ["repo", "base", "needs"]}));

    // Only the fields whose value changed are listed; --repo keeps the stored base.
    let out = fx.pr(&["edit", "web", "--needs", "c", "--base", "develop"]).ok();
    assert_eq!(out["fields"], json!(["needs"]));
    let out = fx.pr(&["edit", "web", "--no-needs"]).ok();
    assert_eq!(out["fields"], json!(["needs"]));
    assert_eq!(fx.needs("web"), "");
    assert_eq!(fx.events("workflow-edited").last().unwrap()["data"]["fields"], json!(["needs"]));
    let out = fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap(), "--base", "main"]).ok();
    assert_eq!(out["fields"], json!(["repo", "base"]));
}

#[test]
fn edit_with_no_change_is_already_and_writes_nothing() {
    let fx = crud("grounding");
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
    assert!(fx.events("workflow-edited").is_empty());
}

#[test]
fn edit_refusals_write_nothing() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    fx.add("ui", &fx.web, &["--needs", "web"]).ok();
    fx.add("gone", &fx.web, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    let writes = fx.writes();

    fx.refused(&fx.pr(&["edit", "web"]), "nothing_to_edit", 2, writes);
    fx.refused(&fx.pr(&["edit", "web", "--needs", "api", "--no-needs"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["edit", "web", "--needs", "api,api"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["edit", "nope", "--no-needs"]), "workflow_not_found", 4, writes);
    fx.refused(&fx.pr(&["edit", "gone", "--no-needs"]), "illegal_transition", 3, writes);

    let not_a_repo = fx.env.root().join("not-a-repo");
    fs::create_dir_all(&not_a_repo).unwrap();
    fx.refused(&fx.pr(&["edit", "web", "--repo", not_a_repo.to_str().unwrap()]), "not_a_git_repo", 3, writes);
    let error = fx.refused(&fx.pr(&["edit", "web", "--base", "nope"]), "base_not_found", 4, writes);
    assert_eq!(error["details"]["base"], "nope");
    // --repo keeps the stored base, which must exist in the new repo.
    fx.git(&fx.web, &["branch", "develop"]);
    fx.git(&fx.web, &["update-ref", "refs/remotes/origin/develop", "develop"]);
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

    fx.git(&fx.pay, &["branch", "pay-1/web"]);
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap(), "--base", "main"]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "pay-1/web");
    fx.git(&fx.pay, &["branch", "-D", "pay-1/web"]);
    fx.git(&fx.pay, &["branch", "pay-1"]);
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.pay.to_str().unwrap(), "--base", "main"]), "branch_exists", 3, writes);
    assert_eq!(error["details"]["branch"], "pay-1");
}

#[test]
fn edit_and_cancel_of_a_workflow_that_is_not_waiting_are_illegal_transitions() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.env.sql("UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE key = 'api'");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "api", "--no-needs"]), "illegal_transition", 3, writes);
    assert_eq!(error["details"]["from"], "dispatch");
    fx.refused(&fx.pr(&["cancel", "api"]), "illegal_transition", 3, writes);
}

#[test]
fn edit_and_cancel_are_refused_once_the_plan_is_locked() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z'");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "api", "--base", "main"]), "workflow_locked", 3, writes);
    assert_eq!(error["details"]["phase"], "dispatch");
    fx.refused(&fx.pr(&["cancel", "nope"]), "workflow_not_found", 4, writes);
}

/// Plan `pay-1` in `plan` with PRs `a`, `c` (no needs), `b` (needs `a`) and `b2` (needs `a`, `c`), then locked.
fn restack_fixture() -> Crud {
    let fx = crud("plan");
    fx.add("a", &fx.pay, &["--no-needs"]).ok();
    fx.add("c", &fx.pay, &["--no-needs"]).ok();
    fx.add("b", &fx.pay, &["--needs", "a"]).ok();
    fx.add("b2", &fx.pay, &["--needs", "a,c"]).ok();
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z'");
    fx
}

#[test]
fn edit_after_the_lock_stacks_a_waiting_workflow_on_a_workflow_it_needs() {
    let fx = restack_fixture();
    let fingerprint = fx.fingerprint();
    let out = fx.pr(&["edit", "b", "--base", "a", "--no-needs"]).ok();
    assert_eq!(out["fields"], json!(["base", "needs"]));
    assert_eq!(fx.shown("pay-1", "b"), json!(["pay-1/a", "pay-1/a", []]));
    assert_eq!(fx.needs("b"), "");
    assert_eq!(fx.events("workflow-edited").last().unwrap()["data"], json!({"workflow": "b", "fields": ["base", "needs"]}));
    assert_eq!(fx.fingerprint(), fingerprint);

    let writes = fx.writes();
    let out = fx.pr(&["edit", "b", "--base", "a", "--no-needs"]).ok();
    assert_eq!(out["already"], json!(true));
    assert_eq!(fx.writes(), writes);

    let out = fx.pr(&["edit", "b2", "--base", "pay-1/a", "--needs", "c"]).ok();
    assert_eq!(out["fields"], json!(["base", "needs"]));
    assert_eq!(fx.needs("b2"), "c");
    assert_eq!(fx.shown("pay-1", "b2"), json!(["pay-1/a", "pay-1/a", ["c"]]));
}

#[test]
fn edit_after_the_lock_refuses_any_other_change_of_a_waiting_workflow() {
    let fx = restack_fixture();
    let web = fx.web.to_str().unwrap().to_owned();
    let cases: Vec<Vec<&str>> = vec![
        vec!["edit", "b", "--base", "main"],
        vec!["edit", "b", "--base", "a"],
        vec!["edit", "b", "--no-needs"],
        vec!["edit", "b", "--needs", "a"],
        vec!["edit", "b", "--repo", &web],
        vec!["edit", "b2", "--base", "a", "--no-needs"],
        vec!["edit", "b", "--base", "c", "--no-needs"],
        vec!["edit", "a", "--base", "main", "--no-needs"],
    ];
    for args in cases {
        fx.refused(&fx.pr(&args), "workflow_locked", 3, fx.writes());
    }
    fx.env.sql("UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't' WHERE key = 'b2'");
    fx.refused(&fx.pr(&["edit", "b2", "--base", "a", "--needs", "c"]), "workflow_locked", 3, fx.writes());
}

#[test]
fn a_workflow_restacked_after_the_lock_waits_for_its_base_workflow_to_open() {
    let fx = restack_fixture();
    fx.pr(&["edit", "b", "--base", "a", "--no-needs"]).ok();
    let error = fx.refused(&fx.pr(&["start", "b"]), "workflow_not_ready", 3, fx.writes());
    assert_eq!((&error["details"]["base_pr"], &error["details"]["needs"]), (&json!("pay-1/a"), &json!([])));
    assert_eq!(fx.env.query::<String>("SELECT status FROM workflows WHERE key = 'b'"), "waiting");
}

#[test]
fn edit_title_works_after_the_lock_in_any_status_and_keeps_the_review_and_the_lock() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.job("a", "api", "draft");
    fx.outline_for_review("pay-1");
    let id = fx.cpd(&["plan", "review", "--plan", "pay-1", "--title", "Review title"]).ok()["review"]["id"].as_str().unwrap().to_owned();
    fx.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay-1"]).ok();
    let fingerprint = fx.fingerprint();
    let current = |fx: &Crud| fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok()["review"]["current"].clone();

    // Before the lock, `waiting`: the title changes and the review stays current.
    let out = fx.pr(&["edit", "api", "--title", "Renamed api"]).ok();
    assert_eq!((out["workflow"].clone(), out["fields"].clone()), (json!({"key": "api", "status": "waiting"}), json!(["title"])));
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "Renamed api");
    assert_eq!(fx.events("workflow-edited").last().unwrap()["data"], json!({"workflow": "api", "fields": ["title"]}));
    assert_eq!(fx.fingerprint(), fingerprint, "the title is not in the plan fingerprint");
    assert_eq!(current(&fx), true);
    // `plan lock` starts the engine with `cpd watch --ensure` from `PATH`; a fake one that starts nothing keeps the
    // engine from writing `txns` beside the writes this test counts.
    fx.fake("cpd", Reply::ok("{\"ok\": true, \"started\": true, \"pane\": \"%9\"}\n"));
    // The lock starts the PR, which fetches `origin`: the keyed repo's GitHub origin is unreachable in tests.
    fx.drop_unreachable_origin(&fx.pay);
    fx.cpd(&["plan", "lock", "--plan", "pay-1"]).ok();

    // After the lock the other flags are refused, the title is not.
    fx.refused(&fx.pr(&["edit", "api", "--base", "main"]), "workflow_locked", 3, fx.writes());
    let out = fx.pr(&["edit", "api", "--title", "Locked and renamed"]).ok();
    assert_eq!(out["fields"], json!(["title"]));
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "Locked and renamed");
    assert_eq!(fx.env.query::<String>("SELECT phase FROM plans"), "dispatch");

    // Same value: already, and nothing written.
    let writes = fx.writes();
    let out = fx.pr(&["edit", "api", "--title", "Locked and renamed"]).ok();
    assert_eq!((out["already"].clone(), out["fields"].clone()), (json!(true), json!([])));
    assert_eq!(fx.writes(), writes);

    // Any status but the ended ones: a PR with no title yet (older than migration `pr-title`) gets one.
    fx.env.sql("UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't'");
    fx.env.sql("UPDATE workflows SET title = NULL");
    let out = fx.pr(&["edit", "api", "--title", "First title"]).ok();
    assert_eq!(out["fields"], json!(["title"]));
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "First title");
}

/// PR `api` is `open` with pull request #7, and `gh` is a fake that runs `gh`.
fn open_pr(fx: &Crud, gh: Reply) {
    fx.env.sql(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 7, 'https://github.com/acme/pay/pull/7', 'Change api', 0, 'open', 'none', 'none', 't', NULL, 't');
         UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't';
         UPDATE workflows SET status = 'open', pr_number = 7, pr_opened_at = 't',
                        pr_url = 'https://github.com/acme/pay/pull/7', pull_request = (SELECT id FROM pull_requests);",
    );
    fx.fake_gh(gh);
}

/// The arguments of every `gh` call so far.
fn gh_args(fx: &Crud) -> Vec<Vec<String>> {
    fx.calls("gh").into_iter().map(|call| call.args).collect()
}

#[test]
fn edit_title_of_an_open_pull_request_retitles_it_on_github_first() {
    // Added before the lock: after it a PR that starts at once is refused its stub drafts (`draft_parked`).
    let fx = crud("plan");
    fx.add("api", &fx.pay, &["--no-needs"]).ok();
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z'");
    open_pr(&fx, Reply::ok("{}\n"));

    let out = fx.pr(&["edit", "api", "--title", "Charge the card"]).ok();

    assert_eq!(out["fields"], json!(["title"]));
    assert_eq!(gh_args(&fx), [["api", "-X", "PATCH", "repos/acme/pay/pulls/7", "-f", "title=Charge the card"]]);
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "Charge the card");
    assert_eq!(fx.events("workflow-edited").last().unwrap()["data"], json!({"workflow": "api", "fields": ["title"]}));

    // The same title again: already, and gh is not called.
    let out = fx.pr(&["edit", "api", "--title", "Charge the card"]).ok();
    assert_eq!(out["already"], true);
    assert_eq!(fx.calls("gh").len(), 1);
}

#[test]
fn edit_title_of_an_open_pull_request_stores_nothing_when_github_fails() {
    // Added before the lock: after it a PR that starts at once is refused its stub drafts (`draft_parked`).
    let fx = crud("plan");
    fx.add("api", &fx.pay, &["--no-needs"]).ok();
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z'");
    open_pr(&fx, Reply::exit(1, "", "HTTP 422: nope\n"));
    let writes = fx.writes();

    let error = fx.refused(&fx.pr(&["edit", "api", "--title", "Charge the card"]), "gh_failed", 1, writes);

    assert_eq!(error["details"]["argv"], json!(["gh", "api", "-X", "PATCH", "repos/acme/pay/pulls/7", "-f", "title=Charge the card"]));
    assert!(error["details"]["stderr"].as_str().unwrap().contains("HTTP 422"), "{error}");
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "Change api");
}

#[test]
fn edit_title_of_a_workflow_without_an_open_pull_request_calls_no_gh() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.fake_gh(Reply::exit(1, "", "unexpected\n"));

    fx.pr(&["edit", "api", "--title", "Charge the card"]).ok();
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "Charge the card");

    // A dispatch PR has no pull request yet either.
    fx.env.sql("UPDATE workflows SET status = 'dispatch', lead_worktree = '/w', started_at = 't'");
    fx.pr(&["edit", "api", "--title", "Charge it"]).ok();
    assert!(fx.calls("gh").is_empty());
}

#[test]
fn edit_title_of_a_merged_or_cancelled_workflow_is_workflow_ended() {
    {
        let Legacy { fx, .. } = legacy("merged");
        let writes = fx.writes();
        let error = fx.refused(&fx.pr(&["edit", "old", "--title", "History"]), "workflow_ended", 3, writes);
        assert_eq!(error["details"]["status"], "merged");
        // With another flag it is the same refusal: `--title` is checked first.
        fx.refused(&fx.pr(&["edit", "old", "--title", "History", "--no-needs"]), "workflow_ended", 3, writes);
    }
    // The first env is gone (an `Fx` is one per process at a time) before the second.
    let fx = crud("plan");
    fx.add("gone", &fx.pay, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "gone", "--title", "History"]), "workflow_ended", 3, writes);
    assert_eq!(error["details"]["status"], "cancelled");
}

#[test]
fn edit_title_refuses_a_bad_title_and_writes_nothing() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    let writes = fx.writes();
    for bad in ["", &"a".repeat(73), "one\ntwo"] {
        fx.refused(&fx.pr(&["edit", "api", "--title", bad]), "usage", 2, writes);
    }
    assert_eq!(fx.env.query::<String>("SELECT title FROM workflows WHERE key = 'api'"), "Change api");
}

// ---------- pr cancel ----------

#[test]
fn cancel_removes_the_workflow_and_keeps_its_key() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    let out = fx.pr(&["cancel", "web"]).ok();
    assert_eq!(out["workflow"], json!({"key": "web", "status": "cancelled"}));
    assert!(out.get("already").is_none());
    assert_eq!(fx.env.query::<String>("SELECT status FROM workflows WHERE key = 'web'"), "cancelled");
    let cancelled = fx.events("workflow-cancelled");
    assert_eq!(cancelled.len(), 1);
    assert_eq!((cancelled[0]["plan"].clone(), cancelled[0]["data"].clone()), (json!("pay-1"), json!({"workflow": "web"})));

    // Idempotent.
    let writes = fx.writes();
    assert_eq!(fx.pr(&["cancel", "web"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
    // The key is kept: a new PR cannot take it, and nothing can need it.
    fx.refused(&fx.add("web", &fx.web, &[]), "workflow_exists", 3, writes);
    fx.refused(&fx.add("ui", &fx.web, &["--needs", "web"]), "needs_unavailable", 3, writes);
}

#[test]
fn cancel_is_refused_while_a_job_or_a_workflow_holds_the_workflow() {
    let fx = crud("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    // A `running` Job holds the PR; a draft one (and a cancelled one) does not.
    fx.job("a", "api", "queued");
    fx.env.sql("UPDATE jobs SET status = 'running' WHERE key = 'a'");
    fx.job("b", "api", "cancelled");
    fx.job("c", "api", "draft");
    let writes = fx.writes();

    let error = fx.refused(&fx.pr(&["cancel", "api"]), "workflow_in_use", 3, writes);
    assert_eq!((error["details"]["jobs"].clone(), error["details"]["workflows"].clone()), (json!(["a"]), json!(["web"])));
    fx.env.sql("UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'a'");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "api"]), "workflow_in_use", 3, writes);
    assert_eq!((error["details"]["jobs"].clone(), error["details"]["workflows"].clone()), (json!([]), json!(["web"])));
    fx.pr(&["edit", "web", "--no-needs"]).ok();
    fx.pr(&["cancel", "api"]).ok();
    assert_eq!(fx.env.query::<String>("SELECT status FROM workflows WHERE key = 'api'"), "cancelled");
}

impl Crud {
    /// A PR row as the lock and `pr open` leave it: `status` `dispatch` (a Lead worktree, no pull request),
    /// `open` or `merged` (with a pull request).
    fn locked_pr(&self, key: &str, repo: &PathBuf, status: &str) {
        self.locked_shaped_pr(key, repo, status, "free");
    }

    /// [`Fx::locked_pr`] with the PR's `shape` (written once, it is never changed).
    fn locked_shaped_pr(&self, key: &str, repo: &PathBuf, status: &str, shape: &str) {
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
            "INSERT INTO workflows (plan, key, repo, base, status, shape, lead_branch, lead_worktree, started_at, pr_number, pr_url, pr_opened_at,
                              pull_request, merged_at, created_txn, created_at)
               VALUES ('pay-1', '{key}', '{}', 'main', '{status}', '{shape}', 'cpd/pay-1/{key}', '/w/{key}', 't', {number}, {url}, {opened}, {pull}, {merged},
                       cpd_txn(), 't');",
            repo.display()
        ));
    }

    fn workflow_status(&self, key: &str) -> String {
        self.env.query(&format!("SELECT status FROM workflows WHERE key = '{key}'"))
    }

    fn phase(&self) -> String {
        self.env.query("SELECT phase FROM plans WHERE id = 'pay-1'")
    }
}

// ---------- pr cancel after the lock ----------

#[test]
fn cancel_that_completes_the_plan_supersedes_the_pending_reviews() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("gateci", &fx.web, "dispatch");
    fx.job("a", "gateci", "cancelled");
    insert_pending_reviews(&fx.env, "pay-1");

    let out = fx.pr(&["cancel", "gateci"]).ok();
    assert_eq!(out["workflow"], json!({"key": "gateci", "status": "cancelled"}));
    assert_eq!(out["phase"], "done");
    assert_reviews_superseded_before_done(&fx.env, "pay-1");
}

#[test]
fn a_locked_workflow_with_only_cancelled_jobs_is_cancelled_and_the_plan_reaches_done() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("gateci", &fx.web, "dispatch");
    fx.job("a", "gateci", "cancelled");
    assert_eq!(fx.phase(), "dispatch");

    let out = fx.pr(&["cancel", "gateci"]).ok();
    assert_eq!(out["workflow"], json!({"key": "gateci", "status": "cancelled"}));
    assert_eq!(out["phase"], "done");
    assert_eq!(fx.workflow_status("gateci"), "cancelled");
    assert_eq!(fx.phase(), "done");
    // The Locked Job is untouched, the Events are those of the cancel and the Plan's move.
    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE key = 'a'"), "cancelled");
    assert_eq!(fx.events("workflow-cancelled")[0]["data"], json!({"workflow": "gateci"}));
    let phases = fx.events("plan-phase");
    assert_eq!((phases.len(), phases[0]["data"]["to"].clone()), (1, json!("deploy")));
    let done = fx.events("plan-done");
    assert_eq!((done.len(), done[0]["data"]["workflows"][0]["key"].clone()), (1, json!("api")));

    // Idempotent.
    let writes = fx.writes();
    assert_eq!(fx.pr(&["cancel", "gateci"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
}

#[test]
fn a_locked_workflow_with_no_job_is_cancelled_and_the_plan_waits_for_the_others() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "dispatch");
    fx.locked_pr("web", &fx.web, "dispatch");
    let out = fx.pr(&["cancel", "web"]).ok();
    assert!(out.get("phase").is_none(), "{out}");
    assert_eq!((fx.workflow_status("web"), fx.phase()), ("cancelled".to_string(), "dispatch".to_string()));
    assert!(fx.events("plan-phase").is_empty());

    // A PR that is still `waiting` is cancelled too; the last one cancelled leaves the Plan in `dispatch`.
    fx.env.sql("UPDATE workflows SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE key = 'api'");
    fx.env.sql(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay-1', 'late', '/x', 'main', 'waiting', 'cpd/pay-1/late', cpd_txn(), 't')",
    );
    let out = fx.pr(&["cancel", "late"]).ok();
    assert!(out.get("phase").is_none(), "no PR is left that is not cancelled: {out}");
    assert_eq!(fx.phase(), "dispatch");
}

#[test]
fn a_locked_workflow_with_a_merged_or_live_job_or_a_workflow_that_needs_it_is_refused_and_a_merged_one_too() {
    let fx = crud("dispatch");
    for key in ["queued", "running", "merged"] {
        fx.locked_pr(key, &fx.pay, "dispatch");
        fx.job(&format!("{key}-old"), key, "cancelled");
        fx.job(&format!("{key}-job"), key, "queued");
    }
    fx.env.sql("UPDATE jobs SET status = 'running' WHERE key = 'running-job';
            UPDATE jobs SET status = 'running' WHERE key = 'merged-job';
            UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'merged-job'");
    let writes = fx.writes();
    for key in ["running", "merged"] {
        let error = fx.refused(&fx.pr(&["cancel", key]), "workflow_in_use", 3, writes);
        assert_eq!((error["details"]["jobs"].clone(), error["details"]["workflows"].clone()), (json!([format!("{key}-job")]), json!([])));
        assert_eq!(fx.workflow_status(key), "dispatch");
    }
    // A `queued` Job does not hold the PR: it is cancelled with it.
    fx.pr(&["cancel", "queued"]).ok();
    assert_eq!(fx.workflow_status("queued"), "cancelled");
    assert_eq!(fx.env.query::<String>("SELECT status || '/' || cancel_reason FROM jobs WHERE key = 'queued-job'"), "cancelled/pr cancelled");

    // A live PR that needs it holds it (`workflow_in_use`, `details.workflows`).
    fx.locked_pr("base", &fx.pay, "dispatch");
    fx.env.sql(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay-1', 'dependent', '{}', 'main', 'waiting', 'cpd/pay-1/dependent', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay-1', 'dependent', 'base')",
        fx.web.display()
    ));
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "base"]), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["workflows"], json!(["dependent"]));

    // A merged PR is refused.
    fx.locked_pr("opened", &fx.web, "merged");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "opened"]), "illegal_transition", 3, writes);
    assert_eq!(error["details"]["from"], "merged");
}

// ---------- pr cancel of an open PR whose pull request is closed (hs-d329) ----------

impl Crud {
    fn pull_state(&self, state: &str) {
        self.env.sql(&format!("UPDATE pull_requests SET state = '{state}', merged_at = iif('{state}' = 'merged', 't', NULL) WHERE number = 7"));
    }
}

#[test]
fn an_open_workflow_whose_pull_request_is_closed_is_cancelled_and_the_plan_reaches_done() {
    let fx = crud("deploy");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("web", &fx.web, "open");
    fx.pull_state("closed");
    // Its Jobs are `merged` or `cancelled` and are not checked.
    fx.job("a", "web", "cancelled");

    let out = fx.pr(&["cancel", "web"]).ok();
    assert_eq!(out["workflow"], json!({"key": "web", "status": "cancelled"}));
    assert_eq!(out["phase"], "done");
    assert_eq!(out["released"], json!({"number": 7, "url": "u", "lead_worktree": "/w/web", "lead_branch": "cpd/pay-1/web"}));
    assert_eq!((fx.workflow_status("web"), fx.phase()), ("cancelled".to_string(), "done".to_string()));
    // The table's checks: no Lead worktree, start time or pull request on a cancelled PR; the old values stay in `history`.
    assert_eq!(
        fx.env.query::<i64>(
            "SELECT count(*) FROM workflows WHERE key = 'web' AND lead_worktree IS NULL AND started_at IS NULL AND pr_number IS NULL
               AND pr_url IS NULL AND pr_opened_at IS NULL AND pull_request IS NULL"
        ),
        1
    );
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM history WHERE tbl = 'workflows' AND op = 'update' AND before LIKE '%\"/w/web\"%'"), 1);
    // The pull request's row stays for the poll to delete.
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM pull_requests WHERE number = 7"), 1);

    let cancelled = fx.events("workflow-cancelled");
    assert_eq!((cancelled.len(), cancelled[0]["data"].clone()), (1, json!({"workflow": "web"})));
    let text = cancelled[0]["text"].as_str().unwrap();
    for named in ["#7", "/w/web", "cpd/pay-1/web"] {
        assert!(text.contains(named), "{named} in {text}");
    }
    assert!(fx.events("plan-phase").is_empty());
    let done = fx.events("plan-done");
    assert_eq!((done.len(), done[0]["data"]["workflows"][0]["key"].clone()), (1, json!("api")));

    // Idempotent: a cancelled PR is `already`, in any phase.
    let writes = fx.writes();
    assert_eq!(fx.pr(&["cancel", "web"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
}

#[test]
fn cancelling_an_open_workflow_leaves_the_phase_while_another_workflow_remains_to_open_or_merge() {
    // `dispatch`: a PR is still to start.
    {
        let fx = crud("dispatch");
        fx.locked_pr("web", &fx.web, "open");
        fx.locked_pr("api", &fx.pay, "dispatch");
        fx.pull_state("closed");
        let out = fx.pr(&["cancel", "web"]).ok();
        assert!(out.get("phase").is_none(), "{out}");
        assert_eq!((fx.workflow_status("web"), fx.phase()), ("cancelled".to_string(), "dispatch".to_string()));
        assert!(fx.events("plan-phase").is_empty() && fx.events("plan-done").is_empty());
    }

    // `deploy`: another PR is still `open`.
    // The first Fx is dropped: the lock is process-wide.
    let fx = crud("deploy");
    fx.locked_pr("web", &fx.web, "open");
    fx.locked_pr("api", &fx.pay, "open");
    fx.pull_state("closed");
    let out = fx.pr(&["cancel", "web"]).ok();
    assert!(out.get("phase").is_none(), "{out}");
    assert_eq!((fx.workflow_status("web"), fx.phase()), ("cancelled".to_string(), "deploy".to_string()));

    // `deploy` with nothing merged: no PR is left, the Plan stays.
    fx.pr(&["cancel", "api"]).ok();
    assert_eq!(fx.phase(), "deploy");
    assert!(fx.events("plan-done").is_empty());
}

#[test]
fn an_open_workflow_whose_pull_request_is_open_or_merged_is_refused_workflow_not_closed() {
    let fx = crud("deploy");
    fx.locked_pr("web", &fx.web, "open");
    for state in ["open", "merged"] {
        fx.pull_state(state);
        let writes = fx.writes();
        let error = fx.refused(&fx.pr(&["cancel", "web"]), "pr_not_closed", 3, writes);
        assert_eq!(error["details"]["pr"], "web");
        assert_eq!((error["details"]["number"].clone(), error["details"]["url"].clone(), error["details"]["state"].clone()), (json!(7), json!("u"), json!(state)));
        assert_eq!((fx.workflow_status("web"), fx.phase()), ("open".to_string(), "deploy".to_string()));
    }
    // Closed, reopened, closed: the stored row decides.
    fx.pull_state("closed");
    fx.pr(&["cancel", "web"]).ok();
}

#[test]
fn an_open_workflow_that_another_workflow_needs_is_refused_workflow_in_use() {
    let fx = crud("dispatch");
    fx.locked_pr("base", &fx.pay, "open");
    fx.pull_state("closed");
    fx.env.sql(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay-1', 'dependent', '{}', 'main', 'waiting', 'cpd/pay-1/dependent', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay-1', 'dependent', 'base')",
        fx.web.display()
    ));
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "base"]), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["workflows"], json!(["dependent"]));
    assert_eq!(fx.workflow_status("base"), "open");
}

// ---------- the plan fingerprint ----------

#[test]
fn the_plan_fingerprint_changes_with_a_workflows_repo_base_needs_and_existence() {
    let fx = crud("plan");
    fx.git(&fx.web, &["branch", "develop"]);
    fx.git(&fx.web, &["update-ref", "refs/remotes/origin/develop", "develop"]);
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
fn the_plan_fingerprint_ignores_a_workflows_title() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &["--needs", "api"]).ok();
    let before = fx.fingerprint();
    fx.pr(&["edit", "web", "--title", "A different title"]).ok();
    fx.pr(&["edit", "api", "--title", "Another one"]).ok();
    assert_eq!(fx.fingerprint(), before);
}

#[test]
fn a_plan_review_goes_stale_when_a_workflow_changes() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.pay, &[]).ok();
    fx.job("a", "api", "draft");
    fx.outline_for_review("pay-1");
    let review = fx.cpd(&["plan", "review", "--plan", "pay-1", "--title", "Review title"]).ok();
    let id = review["review"]["id"].as_str().unwrap().to_owned();
    assert_eq!(review["review"]["fingerprint"], fx.fingerprint());
    assert_eq!(fx.cpd(&["plan", "render", "--plan", "pay-1"]).ok()["fingerprint"], fx.fingerprint());
    assert_eq!(fx.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay-1"]).ok()["current"], true);
    let current = |fx: &Crud| fx.cpd(&["plan", "show", "--plan", "pay-1"]).ok()["review"]["current"].clone();
    assert_eq!(current(&fx), true);

    for args in [&["edit", "web", "--repo", fx.web.to_str().unwrap()][..], &["edit", "web", "--needs", "api"], &["cancel", "web"]] {
        fx.pr(args).ok();
        assert_eq!(current(&fx), false, "{args:?}");
        let lock = fx.cpd(&["plan", "lock", "--plan", "pay-1"]);
        assert_eq!(lock.error()["code"], "plan_not_approved", "{lock:?}");
        // A new review of what the PRs are now, approved, is current again.
        fx.outline_for_review("pay-1");
        let id = fx.cpd(&["plan", "review", "--plan", "pay-1", "--title", "Review title"]).ok()["review"]["id"].as_str().unwrap().to_owned();
        fx.cpd(&["plan", "decide", &id, "--approve", "--plan", "pay-1"]).ok();
        assert_eq!(current(&fx), true, "{args:?}");
    }
}

// ---------- stacked PRs and needs of one Plan (contract §2.2, §3.8 "Stacked PRs"; commands.md §9.4, §9.5) ----------

#[test]
fn add_stacks_on_a_workflow_of_its_own_plan_and_a_branch_that_is_a_lead_branch_is_stacked_all_the_same() {
    let fx = crud("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    let out = fx.add("web", &fx.pay, &["--base", "pay-1/api", "--needs", "pay-1/api"]);
    let error = fx.refused(&out, "usage", 2, fx.writes());
    assert!(error["message"].as_str().unwrap().contains("api"), "{error}");
    let out = fx.add("web", &fx.pay, &["--base", "pay-1/api"]).ok();
    assert_eq!((out["workflow"]["base"].clone(), out["workflow"]["base_pr"].clone()), (json!("pay-1/api"), json!("pay-1/api")));
    // Its Lead Branch, given as a branch (it exists in the repo), is the derivation of §2.2 all the same: the refusals apply.
    fx.git(&fx.pay, &["branch", "pay-1/api"]);
    fx.git(&fx.pay, &["update-ref", "refs/remotes/origin/pay-1/api", "pay-1/api"]);
    let out = fx.add("ui", &fx.pay, &["--base", "pay-1/api"]).ok();
    assert_eq!(out["workflow"]["base_pr"], "pay-1/api");
    fx.pr(&["cancel", "ui"]).ok();
    fx.pr(&["cancel", "web"]).ok();
    fx.pr(&["cancel", "api"]).ok();
    fx.refused(&fx.add("late", &fx.pay, &["--base", "pay-1/api"]), "base_workflow_unavailable", 3, fx.writes());
}

#[test]
fn a_base_that_is_not_a_workflow_reference_is_a_branch() {
    let fx = crud("grounding");
    fx.plan("ledger-9", "ctx", "grounding");
    fx.git(&fx.pay, &["branch", "feature/x"]);
    fx.git(&fx.pay, &["update-ref", "refs/remotes/origin/feature/x", "feature/x"]);
    // `feature` is no Plan: a branch, which exists.
    let out = fx.add("a", &fx.pay, &["--base", "feature/x"]).ok();
    assert_eq!((out["workflow"]["base"].clone(), out["workflow"]["base_pr"].clone()), (json!("feature/x"), Value::Null));
    // A branch that does not exist is `base_not_found`, also when its first segment is not a Plan.
    let error = fx.refused(&fx.add("b", &fx.pay, &["--base", "feature/y"]), "base_not_found", 4, fx.writes());
    assert_eq!(error["details"]["base"], "feature/y");
    // A first segment that is another Plan is `workflow_other_plan`, not a branch; one that is the Plan itself, but no PR in it, `base_workflow_unknown`.
    let error = fx.refused(&fx.add("b", &fx.pay, &["--base", "ledger-9/ghost"]), "workflow_other_plan", 3, fx.writes());
    assert_eq!(error["details"]["plan"], "ledger-9");
    assert_eq!(error["details"]["reference"], "ledger-9/ghost");
    let error = fx.refused(&fx.add("b", &fx.pay, &["--base", "pay-1/ghost"]), "base_workflow_unknown", 3, fx.writes());
    assert_eq!(error["details"]["plan"], "pay-1");
    assert_eq!(error["details"]["pr"], "ghost");
    // A Lead Branch has two slashes and is never read as a reference.
    fx.refused(&fx.add("b", &fx.pay, &["--base", "cpd/ledger-9/api"]), "base_not_found", 4, fx.writes());
}

#[test]
fn add_refuses_a_bad_reference_and_writes_nothing() {
    let fx = crud("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &[]).ok();
    fx.add("gone", &fx.pay, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    fx.merged_pr("pay-1", "old", &fx.pay, 21);
    let writes = fx.writes();

    // A base PR that is cancelled or merged: name its base branch instead; a need that is cancelled.
    let error = fx.refused(&fx.add("x", &fx.pay, &["--base", "pay-1/gone"]), "base_workflow_unavailable", 3, writes);
    assert_eq!(error["details"]["pr"], "gone");
    let error = fx.refused(&fx.add("x", &fx.pay, &["--base", "pay-1/old"]), "base_workflow_unavailable", 3, writes);
    assert_eq!(error["details"]["pr"], "old");
    let error = fx.refused(&fx.add("x", &fx.pay, &["--needs", "gone"]), "needs_unavailable", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["gone"]));
    // A base PR of another repo.
    let error = fx.refused(&fx.add("x", &fx.pay, &["--base", "pay-1/web"]), "base_workflow_other_repo", 3, writes);
    assert_eq!(error["details"], json!({"pr": "web", "repo": fx.pay.display().to_string(), "base_repo": fx.web.display().to_string()}));
    // A need that is no PR.
    let error = fx.refused(&fx.add("x", &fx.pay, &["--needs", "ghost,api"]), "needs_unknown", 3, writes);
    assert_eq!(error["details"]["needs"], json!(["ghost"]));
    // The same PR twice, however it is written; a bad reference is a bad key.
    fx.refused(&fx.add("x", &fx.pay, &["--needs", "api,api"]), "usage", 2, writes);
    fx.refused(&fx.add("x", &fx.pay, &["--needs", "api,pay-1/api"]), "usage", 2, writes);
    fx.refused(&fx.add("x", &fx.pay, &["--needs", "pay-1/api/x"]), "invalid_workflow_id", 2, writes);
    // A merged PR is a need all the same.
    let out = fx.add("x", &fx.pay, &["--needs", "old"]).ok();
    assert_eq!(out["workflow"]["needs"], json!(["old"]));
}

#[test]
fn a_cycle_of_needs_and_base_workflows_is_refused() {
    let fx = crud("grounding");
    fx.add("x", &fx.pay, &[]).ok();
    // y is stacked on x: x only has to start before y; so x may not need y, which would wait for x to merge.
    fx.add("y", &fx.pay, &["--base", "pay-1/x"]).ok();
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "x", "--needs", "y"]), "dependency_cycle", 3, writes);
    assert_eq!(error["details"]["cycle"], json!(["x", "y", "x"]));
    // Directly or not.
    fx.add("z", &fx.pay, &["--needs", "y"]).ok();
    let error = fx.refused(&fx.pr(&["edit", "x", "--needs", "z"]), "dependency_cycle", 3, fx.writes());
    assert_eq!(error["details"]["cycle"], json!(["x", "z", "y", "x"]));
    // A PR cannot be stacked on itself.
    let error = fx.refused(&fx.pr(&["edit", "x", "--base", "pay-1/x"]), "dependency_cycle", 3, fx.writes());
    assert_eq!(error["details"]["cycle"], json!(["x", "x"]));
    // z needing x is no cycle: x is merged before z starts.
    fx.pr(&["edit", "z", "--needs", "x"]).ok();
    assert_eq!(fx.shown("pay-1", "z"), json!(["main", null, ["x"]]));
}

#[test]
fn edit_stacks_and_unstacks_and_a_workflow_something_is_stacked_on_keeps_its_repo() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("schema", &fx.web, &[]).ok();
    fx.add("web", &fx.pay, &[]).ok();

    let out = fx.pr(&["edit", "web", "--base", "pay-1/api", "--needs", "schema"]).ok();
    assert_eq!(out["fields"], json!(["base", "needs"]));
    assert_eq!(fx.shown("pay-1", "web"), json!(["pay-1/api", "pay-1/api", ["schema"]]));
    assert_eq!(fx.needs("web"), "schema");
    assert_eq!(fx.events("workflow-edited").last().unwrap()["data"]["fields"], json!(["base", "needs"]));
    // The same again is `already`; a branch unstacks it.
    assert_eq!(fx.pr(&["edit", "web", "--base", "pay-1/api"]).ok()["already"], true);
    let out = fx.pr(&["edit", "web", "--base", "main", "--no-needs"]).ok();
    assert_eq!(out["fields"], json!(["base", "needs"]));
    assert_eq!(fx.shown("pay-1", "web"), json!(["main", null, []]));

    // The refusals are `pr add`'s.
    fx.pr(&["edit", "web", "--base", "pay-1/api"]).ok();
    let writes = fx.writes();
    fx.refused(&fx.pr(&["edit", "web", "--base", "pay-1/ghost"]), "base_workflow_unknown", 3, writes);
    let error = fx.refused(&fx.pr(&["edit", "web", "--base", "pay-1/schema"]), "base_workflow_other_repo", 3, writes);
    assert_eq!(error["details"]["base_repo"], fx.web.display().to_string());
    fx.refused(&fx.pr(&["edit", "web", "--needs", "api"]), "usage", 2, writes);
    fx.refused(&fx.pr(&["edit", "web", "--needs", "ghost"]), "needs_unknown", 3, writes);
    // A new repo is checked against its base PR: a stacked PR cannot move away from it.
    fx.git(&fx.web, &["branch", "pay-1/api"]);
    fx.refused(&fx.pr(&["edit", "web", "--repo", fx.web.to_str().unwrap(), "--base", "pay-1/api"]), "base_workflow_other_repo", 3, writes);

    // A PR that another PR is stacked on keeps its repo.
    let error = fx.refused(&fx.pr(&["edit", "api", "--repo", fx.web.to_str().unwrap()]), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["workflows"], json!(["web"]));
    // Its other fields, and the stacked PR's repo (it stays stacked only in its repo), are not blocked.
    fx.pr(&["edit", "api", "--base", "main"]).ok();
}

#[test]
fn add_reads_a_bare_workflow_key_in_base_as_the_base_workflow() {
    let fx = crud("grounding");
    fx.add("api", &fx.pay, &[]).ok();
    let out = fx.add("web", &fx.pay, &["--base", "api"]).ok();
    let pr = &out["workflow"];
    assert_eq!((pr["base"].clone(), pr["base_pr"].clone(), pr["needs"].clone(), pr["status"].clone()), (json!("pay-1/api"), json!("pay-1/api"), json!([]), json!("waiting")), "{out}");
    assert_eq!(fx.add("web", &fx.pay, &["--base", "api"]).ok()["already"], true);
    let out = fx.add("ui", &fx.pay, &["--base", "pay-1/api"]).ok();
    assert_eq!(out["workflow"]["base"], "pay-1/api", "{out}");
    let error = fx.refused(&fx.add("x", &fx.pay, &["--base", "api", "--needs", "api"]), "usage", 2, fx.writes());
    assert!(error["message"].as_str().unwrap().contains("api"), "{error}");
}

#[test]
fn add_refuses_a_bare_base_workflow_that_is_cancelled_or_merged() {
    let fx = crud("grounding");
    fx.add("gone", &fx.pay, &[]).ok();
    fx.pr(&["cancel", "gone"]).ok();
    fx.merged_pr("pay-1", "old", &fx.pay, 21);
    let writes = fx.writes();
    let error = fx.refused(&fx.add("x", &fx.pay, &["--base", "gone"]), "base_workflow_unavailable", 3, writes);
    assert_eq!(error["details"]["pr"], "gone");
    let error = fx.refused(&fx.add("x", &fx.pay, &["--base", "old"]), "base_workflow_unavailable", 3, writes);
    assert_eq!(error["details"]["pr"], "old");
}

#[test]
fn a_bare_base_is_a_workflow_reference_only_for_a_workflow_of_the_plan_in_the_repo() {
    let fx = crud("grounding");
    fx.plan("ledger-9", "ctx", "grounding");
    fx.add("schema", &fx.web, &[]).ok();
    fx.merged_pr("ledger-9", "led", &fx.pay, 8);
    let writes = fx.writes();
    for bare in ["ghost", "schema", "led"] {
        let error = fx.refused(&fx.add("x", &fx.pay, &["--base", bare]), "base_not_found", 4, writes);
        assert_eq!(error["details"]["base"], bare);
    }
    // An existing branch with no PR of that key stays a branch.
    fx.git(&fx.pay, &["branch", "develop"]);
    fx.git(&fx.pay, &["update-ref", "refs/remotes/origin/develop", "develop"]);
    let out = fx.add("a", &fx.pay, &["--base", "develop"]).ok();
    assert_eq!((out["workflow"]["base"].clone(), out["workflow"]["base_pr"].clone()), (json!("develop"), Value::Null), "{out}");
    // A PR key of the Plan in the repo wins over the branch of the same name.
    fx.add("develop", &fx.pay, &[]).ok();
    let out = fx.add("b", &fx.pay, &["--base", "develop"]).ok();
    assert_eq!((out["workflow"]["base"].clone(), out["workflow"]["base_pr"].clone()), (json!("pay-1/develop"), json!("pay-1/develop")), "{out}");
}

#[test]
fn edit_reads_a_bare_workflow_key_in_base_as_the_base_workflow() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("schema", &fx.web, &[]).ok();
    fx.add("web", &fx.pay, &[]).ok();

    let out = fx.pr(&["edit", "web", "--base", "api", "--needs", "schema"]).ok();
    assert_eq!(out["fields"], json!(["base", "needs"]));
    assert_eq!(fx.shown("pay-1", "web"), json!(["pay-1/api", "pay-1/api", ["schema"]]));
    assert_eq!(fx.pr(&["edit", "web", "--base", "api"]).ok()["already"], true);
    fx.pr(&["edit", "web", "--base", "main", "--no-needs"]).ok();
    assert_eq!(fx.shown("pay-1", "web"), json!(["main", null, []]));

    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["edit", "web", "--base", "schema"]), "base_not_found", 4, writes);
    assert_eq!(error["details"]["base"], "schema");
    let error = fx.refused(&fx.pr(&["edit", "api", "--base", "api"]), "dependency_cycle", 3, writes);
    assert_eq!(error["details"]["cycle"], json!(["api", "api"]));
    let error = fx.refused(&fx.pr(&["edit", "web", "--repo", fx.web.to_str().unwrap(), "--base", "api"]), "base_not_found", 4, writes);
    assert_eq!(error["details"]["base"], "api");

    fx.pr(&["cancel", "api"]).ok();
    fx.refused(&fx.pr(&["edit", "web", "--base", "api"]), "base_workflow_unavailable", 3, fx.writes());
}

// ---------- pr cancel --superseded-by (hs-d301) ----------

impl Crud {
    /// A Job of PR `pr` that merged.
    fn merged_job(&self, key: &str, pr: &str) {
        self.job(key, pr, "queued");
        self.env.sql(&format!(
            "UPDATE jobs SET status = 'running' WHERE key = '{key}';
             UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = '{key}'"
        ));
    }
}

#[test]
fn a_dispatch_workflow_with_merged_jobs_is_cancelled_when_superseded_and_the_plan_reaches_done() {
    let fx = crud("dispatch");
    fx.plan("other-1", "ctx", "deploy");
    fx.merged_pr("other-1", "storefix", &fx.pay, 8);
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("busyfix", &fx.web, "dispatch");
    fx.job("old", "busyfix", "cancelled");
    fx.merged_job("done", "busyfix");

    // Without the flag it is still held by its merged Job.
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "busyfix"]), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["jobs"], json!(["done"]));

    let out = fx.pr(&["cancel", "busyfix", "--superseded-by", "other-1/storefix"]).ok();
    assert_eq!(out["workflow"], json!({"key": "busyfix", "status": "cancelled"}));
    assert_eq!(out["phase"], "done");
    assert_eq!((fx.workflow_status("busyfix"), fx.phase()), ("cancelled".to_string(), "done".to_string()));
    // The merged Job stays as it is.
    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE key = 'done'"), "merged");

    let cancelled = fx.events("workflow-cancelled");
    assert_eq!(cancelled.len(), 1);
    assert_eq!(cancelled[0]["data"], json!({"workflow": "busyfix", "superseded_by": "other-1/storefix"}));
    assert!(cancelled[0]["text"].as_str().unwrap().contains("other-1/storefix"));
    let done = fx.events("plan-done");
    assert_eq!((done.len(), done[0]["data"]["workflows"][0]["key"].clone()), (1, json!("api")));

    // Idempotent, with or without the flag.
    let writes = fx.writes();
    assert_eq!(fx.pr(&["cancel", "busyfix", "--superseded-by", "other-1/storefix"]).ok()["already"], true);
    assert_eq!(fx.pr(&["cancel", "busyfix"]).ok()["already"], true);
    assert_eq!(fx.writes(), writes);
}

#[test]
fn a_bare_superseding_key_is_a_workflow_of_the_plan_and_any_status_but_cancelled_will_do() {
    let fx = crud("dispatch");
    fx.locked_pr("busyfix", &fx.web, "dispatch");
    fx.locked_pr("storefix", &fx.pay, "dispatch");
    fx.merged_job("done", "busyfix");
    let out = fx.pr(&["cancel", "busyfix", "--superseded-by", "pay-1/storefix"]).ok();
    // The reference is recorded as the views print it: the bare key in the Plan itself.
    assert!(out.get("phase").is_none(), "{out}");
    assert_eq!(fx.events("workflow-cancelled")[0]["data"], json!({"workflow": "busyfix", "superseded_by": "storefix"}));
}

#[test]
fn superseding_is_refused_for_a_live_job_an_unknown_or_foreign_or_cancelled_or_own_workflow_and_a_workflow_that_is_not_dispatch() {
    let fx = crud("dispatch");
    fx.plan("other-1", "ctx", "deploy");
    fx.plan("alien-1", "alien", "deploy");
    fx.merged_pr("other-1", "storefix", &fx.pay, 8);
    fx.merged_pr("alien-1", "foreign", &fx.pay, 9);
    fx.locked_pr("busyfix", &fx.web, "dispatch");
    fx.locked_pr("dead", &fx.pay, "dispatch");
    fx.env.sql("UPDATE workflows SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE key = 'dead'");
    fx.merged_job("done", "busyfix");
    fx.job("live", "busyfix", "queued");
    fx.env.sql("UPDATE jobs SET status = 'running' WHERE key = 'live'");
    let by = |reference: &str| fx.pr(&["cancel", "busyfix", "--superseded-by", reference]);
    let writes = fx.writes();

    // A live Job holds the PR, the merged one does not.
    let error = fx.refused(&by("other-1/storefix"), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["jobs"], json!(["live"]));
    fx.env.sql("UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'live'");
    let writes = fx.writes();

    fx.refused(&by("other-1/nope"), "workflow_not_found", 4, writes);
    fx.refused(&by("nope"), "workflow_not_found", 4, writes);
    fx.refused(&by("nope-9/storefix"), "workflow_not_found", 4, writes);
    fx.refused(&by("alien-1/foreign"), "workflow_not_found", 4, writes);
    fx.refused(&by("dead"), "illegal_transition", 3, writes);
    fx.refused(&by("busyfix"), "usage", 2, writes);
    fx.refused(&by("pay-1/busyfix"), "usage", 2, writes);
    assert_eq!(fx.workflow_status("busyfix"), "dispatch");

    // The flag is for a `dispatch` PR only: an open PR is ended by its closed pull request.
    fx.locked_pr("web", &fx.web, "open");
    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "web", "--superseded-by", "other-1/storefix"]), "illegal_transition", 3, writes);
    assert_eq!(error["details"]["from"], "open");
}

#[test]
fn superseding_a_waiting_workflow_before_the_lock_is_refused() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.add("web", &fx.web, &[]).ok();
    let writes = fx.writes();
    fx.refused(&fx.pr(&["cancel", "web", "--superseded-by", "api"]), "illegal_transition", 3, writes);
    assert_eq!(fx.env.query::<String>("SELECT status FROM workflows WHERE key = 'web'"), "waiting");
}

/// Inserts pending reviews r1 and r2 of `plan` (fingerprints all `a` and all `b`).
fn insert_pending_reviews(env: &TestEnv, plan: &str) {
    env.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('{plan}', 'r1', 1, '{}', 'pending', 't');
         INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('{plan}', 'r2', 2, '{}', 'pending', 't');",
        "a".repeat(64),
        "b".repeat(64)
    ));
}

/// Asserts r1 and r2 of `plan` are superseded and their `plan-review` Events come, in order, before the one `plan-done`.
fn assert_reviews_superseded_before_done(env: &TestEnv, plan: &str) {
    assert_eq!(env.query::<String>(&format!("SELECT phase FROM plans WHERE id = '{plan}'")), "done");
    for (id, fingerprint) in [("r1", "a"), ("r2", "b")] {
        assert_eq!(
            env.query::<String>(&format!(
                "SELECT status || '|' || (decided_at IS NOT NULL) || '|' || (feedback IS NULL) || '|' || fingerprint FROM plan_reviews WHERE plan = '{plan}' AND id = '{id}'"
            )),
            format!("superseded|1|1|{}", fingerprint.repeat(64)),
            "{id}"
        );
    }
    let reviews: Vec<String> = env.read(|c| {
        let mut stmt = c.prepare(&format!("SELECT data FROM events WHERE plan = '{plan}' AND kind = 'plan-review' ORDER BY id"))?;
        stmt.query_map([], |r| r.get::<_, String>(0))?.collect()
    });
    let reviews: Vec<Value> = reviews.iter().map(|d| serde_json::from_str(d).unwrap()).collect();
    assert_eq!(
        reviews,
        [
            json!({"review": "r1", "fingerprint": "a".repeat(64), "status": "superseded"}),
            json!({"review": "r2", "fingerprint": "b".repeat(64), "status": "superseded"})
        ]
    );
    assert_eq!(env.query::<i64>(&format!("SELECT count(*) FROM events WHERE plan = '{plan}' AND kind = 'plan-done'")), 1);
    assert_eq!(
        env.query::<i64>(&format!(
            "SELECT (SELECT max(id) FROM events WHERE plan = '{plan}' AND kind = 'plan-review') < (SELECT id FROM events WHERE plan = '{plan}' AND kind = 'plan-done')"
        )),
        1
    );
}

// ---------- pr cancel cancels the draft, queued and failed Jobs of its PR (hs-d573, rule B2) ----------

impl Crud {
    /// A Job of PR `pr` that is `status`, of kind `kind`: a `failed` one has the cause `timed_out`, a `running` one has
    /// gone `queued` then `running`.
    fn job_of(&self, key: &str, pr: &str, kind: &str, status: &str) {
        match status {
            "running" | "failed" => {
                self.job_kind(key, pr, "queued", kind);
                self.env.sql(&format!("UPDATE jobs SET status = 'running' WHERE key = '{key}'"));
                if status == "failed" {
                    self.env.sql(&format!("UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = '{key}'"));
                }
            }
            _ => self.job_kind(key, pr, status, kind),
        }
    }

    /// `status/cancel_reason/failed_cause` of Job `key`.
    fn job_row(&self, key: &str) -> String {
        self.env.query(&format!("SELECT status || '/' || coalesce(cancel_reason, '-') || '/' || coalesce(failed_cause, '-') FROM jobs WHERE key = '{key}'"))
    }

    /// The `job` of every `job-cancelled` Event, sorted.
    fn cancelled_jobs(&self) -> Vec<String> {
        let mut jobs: Vec<String> = self.events("job-cancelled").iter().map(|e| e["job"].as_str().unwrap().to_owned()).collect();
        jobs.sort();
        jobs
    }
}

#[test]
fn a_dispatch_workflow_cancel_cancels_its_draft_queued_and_failed_jobs_with_the_workflow() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("gone", &fx.web, "dispatch");
    fx.job_of("d", "gone", "work", "draft");
    fx.job_of("q", "gone", "work", "queued");
    fx.job_of("f", "gone", "work", "failed");
    fx.job_of("old", "gone", "work", "cancelled");
    fx.job_of("op", "gone", "open", "queued");

    let out = fx.pr(&["cancel", "gone"]).ok();
    assert_eq!(out["workflow"], json!({"key": "gone", "status": "cancelled"}));
    for key in ["d", "q", "f", "op"] {
        assert_eq!(fx.job_row(key), "cancelled/pr cancelled/-", "{key}");
        assert_eq!(fx.env.query::<i64>(&format!("SELECT cancel_requested_at IS NOT NULL FROM jobs WHERE key = '{key}'")), 1, "{key}");
    }
    assert_eq!(fx.job_row("old"), "cancelled/x/-");
    assert_eq!(fx.cancelled_jobs(), ["d", "f", "op", "q"]);
    assert_eq!((fx.workflow_status("gone"), fx.phase()), ("cancelled".to_string(), "done".to_string()));
}

#[test]
fn a_tdd_workflow_in_draft_with_a_queued_open_job_is_cancelled_by_one_workflow_cancel() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_shaped_pr("gone", &fx.web, "dispatch", "tdd");
    fx.job_of("gone-docs", "gone", "docs", "cancelled");
    fx.job_of("gone-tests", "gone", "test", "draft");
    fx.job_of("gone-impl", "gone", "impl", "draft");
    fx.job_of("open-gone", "gone", "open", "queued");

    // `job cancel` by hand keeps the last `test` Job of a `tdd` PR (unchanged) ...
    let writes = fx.writes();
    let run = fx.cpd(&["job", "cancel", "gone-tests", "--reason", "No.", "--plan", "pay-1"]);
    fx.refused(&run, "shape_incomplete", 3, writes);
    assert_eq!(fx.job_row("gone-tests"), "draft/-/-");

    // ... and `pr cancel` takes it with the PR.
    fx.pr(&["cancel", "gone"]).ok();
    for key in ["gone-tests", "gone-impl", "open-gone"] {
        assert_eq!(fx.job_row(key), "cancelled/pr cancelled/-", "{key}");
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE status <> 'cancelled'"), 0);
    assert_eq!((fx.workflow_status("gone"), fx.phase()), ("cancelled".to_string(), "done".to_string()));
}

#[test]
fn a_waiting_tdd_workflow_with_its_template_stubs_is_cancelled_before_the_lock() {
    let fx = crud("plan");
    fx.add_with_stubs("pay-1", &["add", "gone", "--repo", fx.pay.to_str().unwrap(), "--title", "Change gone"]).ok();
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE workflow = 'gone' AND status = 'draft'"), 4);

    let out = fx.pr(&["cancel", "gone"]).ok();
    assert_eq!(out["workflow"], json!({"key": "gone", "status": "cancelled"}));
    assert_eq!(fx.workflow_status("gone"), "cancelled");
    for key in ["gone-docs", "gone-tests", "gone-impl", "open-gone"] {
        assert_eq!(fx.job_row(key), "cancelled/pr cancelled/-", "{key}");
    }
}

#[test]
fn a_running_job_holds_the_workflow_cancel_and_nothing_is_cancelled() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("gone", &fx.web, "dispatch");
    fx.job_of("d", "gone", "work", "draft");
    fx.job_of("q", "gone", "work", "queued");
    fx.job_of("run", "gone", "work", "running");
    let writes = fx.writes();

    let error = fx.refused(&fx.pr(&["cancel", "gone"]), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["jobs"], json!(["run"]));
    assert_eq!((fx.job_row("d"), fx.job_row("q"), fx.job_row("run")), ("draft/-/-".to_string(), "queued/-/-".to_string(), "running/-/-".to_string()));
    assert_eq!(fx.workflow_status("gone"), "dispatch");

    fx.env.sql("UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE key = 'run'");
    fx.pr(&["cancel", "gone"]).ok();
    assert_eq!((fx.job_row("d"), fx.job_row("q")), ("cancelled/pr cancelled/-".to_string(), "cancelled/pr cancelled/-".to_string()));
    assert_eq!(fx.workflow_status("gone"), "cancelled");
}

#[test]
fn a_merged_job_holds_the_workflow_cancel_unless_superseded_and_the_others_are_cancelled() {
    let fx = crud("dispatch");
    fx.plan("other-1", "ctx", "deploy");
    fx.merged_pr("other-1", "storefix", &fx.pay, 8);
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("busyfix", &fx.web, "dispatch");
    fx.merged_job("done", "busyfix");
    fx.job_of("d", "busyfix", "work", "draft");

    let writes = fx.writes();
    let error = fx.refused(&fx.pr(&["cancel", "busyfix"]), "workflow_in_use", 3, writes);
    assert_eq!(error["details"]["jobs"], json!(["done"]));
    assert_eq!((fx.job_row("d"), fx.workflow_status("busyfix")), ("draft/-/-".to_string(), "dispatch".to_string()));

    fx.pr(&["cancel", "busyfix", "--superseded-by", "other-1/storefix"]).ok();
    assert_eq!(fx.job_row("d"), "cancelled/pr cancelled/-");
    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE key = 'done'"), "merged");
    assert_eq!(fx.events("workflow-cancelled")[0]["data"], json!({"workflow": "busyfix", "superseded_by": "other-1/storefix"}));
    assert_eq!(fx.cancelled_jobs(), ["d"]);
}

#[test]
fn a_workflow_that_another_workflow_needs_still_refuses_and_none_of_its_jobs_is_cancelled() {
    let fx = crud("dispatch");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("base", &fx.pay, "dispatch");
    fx.job_of("d", "base", "work", "draft");
    fx.job_of("q", "base", "work", "queued");
    fx.env.sql(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay-1', 'dependent', '{}', 'main', 'waiting', 'cpd/pay-1/dependent', cpd_txn(), 't');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay-1', 'dependent', 'base')",
        fx.web.display()
    ));
    let writes = fx.writes();

    let error = fx.refused(&fx.pr(&["cancel", "base"]), "workflow_in_use", 3, writes);
    assert_eq!((error["details"]["workflows"].clone(), error["details"]["jobs"].clone()), (json!(["dependent"]), json!([])));
    assert_eq!((fx.job_row("d"), fx.job_row("q")), ("draft/-/-".to_string(), "queued/-/-".to_string()));

    fx.pr(&["cancel", "dependent"]).ok();
    fx.pr(&["cancel", "base"]).ok();
    assert_eq!((fx.job_row("d"), fx.job_row("q")), ("cancelled/pr cancelled/-".to_string(), "cancelled/pr cancelled/-".to_string()));
}

#[test]
fn an_open_workflow_with_a_closed_pull_request_has_its_draft_queued_and_failed_jobs_cancelled() {
    let fx = crud("deploy");
    fx.locked_pr("api", &fx.pay, "merged");
    fx.locked_pr("web", &fx.web, "open");
    fx.pull_state("closed");
    fx.job_of("d", "web", "work", "draft");
    fx.job_of("q", "web", "work", "queued");
    fx.job_of("f", "web", "work", "failed");

    fx.pr(&["cancel", "web"]).ok();
    for key in ["d", "q", "f"] {
        assert_eq!(fx.job_row(key), "cancelled/pr cancelled/-", "{key}");
    }
    assert_eq!(fx.cancelled_jobs(), ["d", "f", "q"]);
    assert_eq!(fx.workflow_status("web"), "cancelled");
}

#[test]
fn after_a_workflow_cancel_the_plan_can_be_archived() {
    let fx = crud("dispatch");
    fx.locked_shaped_pr("gone", &fx.web, "dispatch", "tdd");
    fx.job_of("gone-tests", "gone", "test", "draft");
    fx.job_of("gone-impl", "gone", "impl", "draft");
    fx.job_of("open-gone", "gone", "open", "queued");

    let archive = || fx.cpd(&["plan", "archive", "--plan", "pay-1"]);
    common::refused(&archive(), "archive_blocked", 3);
    fx.pr(&["cancel", "gone"]).ok();
    archive().ok();
    assert!(!fx.env.query::<String>("SELECT coalesce(archived_at, '') FROM plans WHERE id = 'pay-1'").is_empty());
}

#[test]
fn owner_is_in_use_while_a_plan_has_a_workflow_row() {
    let fx = crud("plan");
    fx.env.owner_policy("other", "");
    fx.add("api", &fx.pay, &[]).ok();
    let owner = |fx: &Crud| fx.cpd(&["context", "owner", "other", "--context", "ctx"]);

    let run = owner(&fx);
    let error = crate::common::refused(&run, "owner_in_use", 3);
    assert_eq!(error["details"]["context"], "ctx");
    assert_eq!(error["details"]["owner"], "acme");
    assert_eq!(error["details"]["prs"], json!(["pay-1/api"]));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM contexts WHERE owner = 'acme'"), 1);
    assert_eq!(fx.events("context-owner").len(), 0);

    let same = fx.cpd(&["context", "owner", "acme", "--context", "ctx"]).ok();
    assert_eq!(same["already"], true);

    // A cancelled row still blocks.
    fx.pr(&["cancel", "api"]).ok();
    crate::common::refused(&owner(&fx), "owner_in_use", 3);
    assert_eq!(fx.events("context-owner").len(), 0);
}

#[test]
fn a_null_owner_is_still_set_while_a_workflow_row_exists() {
    let fx = crud("plan");
    fx.add("api", &fx.pay, &[]).ok();
    fx.env.sql("UPDATE contexts SET owner = NULL WHERE id = 'ctx'");
    let out = fx.cpd(&["context", "owner", "acme", "--context", "ctx"]).ok();
    assert_eq!(out["context"], json!({"id": "ctx", "owner": "acme"}));
}
