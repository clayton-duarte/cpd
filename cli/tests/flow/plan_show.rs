//! `plan show`, `plan list` and `plan render` with PRs (commands.md §4.2, §4.3,
//! §4.5, R34): the command tree in this process (`Fx`) against a temporary `$CPD_HOME` with a Plan
//! written straight into the database.

use crate::common;

use std::fs;

use common::fx::Fx;
use cpd_core::home::CpdHome;
use serde_json::{Value, json};

/// The env with its config and Plan `pay-1` in `phase` (the Lead's session beside it), no PR.
pub(super) fn fx(phase: &str) -> Fx {
    let fx = Fx::new();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL"
    } else {
        "'2000-01-01T00:00:00.000Z'"
    };
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay-1', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay-1', 'ctx', 'Pay the bills', '{phase}', 'lead1', '1999-12-31T00:00:00.000Z', {locked});"
    ));
    fx
}

/// The fixtures of Plan `pay-1` on an [`Fx`].
pub(super) trait Pay {
    fn pr(&self, key: &str, status: &str, needs: &[&str]);
    fn job(&self, key: &str, pr: &str, status: &str, needs: &[&str]);
    fn show(&self) -> Value;
    fn list(&self) -> Value;
    fn render(&self) -> (Value, String);
}

impl Pay for Fx {
    /// A PR of `pay-1` in `status`, with the columns that status requires.
    fn pr(&self, key: &str, status: &str, needs: &[&str]) {
        let (worktree, started, pull) = match status {
            "waiting" | "cancelled" => ("NULL".to_owned(), "NULL", "NULL, NULL, NULL, NULL"),
            "dispatch" => (
                format!("'/r/pay.pay-1.{key}'"),
                "'2000-01-01T00:00:01.000Z'",
                "NULL, NULL, NULL, NULL",
            ),
            "open" => (
                format!("'/r/pay.pay-1.{key}'"),
                "'2000-01-01T00:00:01.000Z'",
                "7, 'https://github.com/acme/api/pull/7', '2000-01-01T00:00:02.000Z', (SELECT id FROM pull_requests WHERE number = 7)",
            ),
            other => panic!("no fixture for {other}"),
        };
        let row = if status == "open" {
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at,
                                        mergeable, merge_state)
             VALUES ('acme/api', 7, 'https://github.com/acme/api/pull/7', 'Add api', 0, 'open', 'approved', 'passed',
                     '2000-01-01T00:00:02.000Z', NULL, '2000-01-01T00:00:03.000Z', 'conflicting', 'dirty');"
        } else {
            ""
        };
        let needs: String = needs.iter().map(|n| format!("INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay-1', '{key}', '{n}');")).collect();
        self.env.sql(&format!(
            "{row}
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                              created_txn, created_at, started_at)
             VALUES ('pay-1', '{key}', '/r/{key}', 'main', '{status}', 'cpd/pay-1/{key}', {worktree}, {pull}, cpd_txn(), 't', {started});
             {needs}"
        ));
    }

    /// A draft (or `cancelled`) Job of PR `pr`.
    fn job(&self, key: &str, pr: &str, status: &str, needs: &[&str]) {
        let needs: String = needs
            .iter()
            .map(|n| {
                format!(
                    "INSERT INTO dependencies (plan, job, needs) VALUES ('pay-1', '{key}', '{n}');"
                )
            })
            .collect();
        self.env.sql(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                               cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('pay-1', '{pr}', '{key}', 'Job {key}', 'S2', 'Goal of {key}.', 'Context of {key}.', '[\"src/{key}.rs\"]', '[\"cargo test\"]', '[]',
                     '{status}', 0, iif('{status}' = 'cancelled', 'x', NULL), iif('{status}' = 'cancelled', 't', NULL), cpd_txn(), 't');
             {needs}"
        ));
    }

    fn show(&self) -> Value {
        self.cpd(&["plan", "show", "--plan", "pay-1"]).ok()
    }

    fn list(&self) -> Value {
        self.cpd(&["plan", "list"]).ok()
    }

    /// `plan render`'s output and the file it wrote.
    fn render(&self) -> (Value, String) {
        let out = self.cpd(&["plan", "render", "--plan", "pay-1"]).ok();
        let text = fs::read_to_string(out["path"].as_str().unwrap()).unwrap();
        (out, text)
    }
}

/// The pull request row `Fx::pr` writes for an `open` PR, as `plan show` and `plan list` print it.
fn pull_request() -> Value {
    json!({"repo": "acme/api", "number": 7, "url": "https://github.com/acme/api/pull/7", "title": "Add api", "draft": false,
           "state": "open", "review_decision": "approved", "checks": "passed", "mergeable": "conflicting", "merge_state": "dirty",
           "created_at": "2000-01-01T00:00:02.000Z", "merged_at": null})
}

fn keys(values: &Value) -> Vec<&str> {
    values
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["key"].as_str().unwrap())
        .collect()
}

/// The position of `needle` in `text`, which must have it.
fn at(text: &str, needle: &str) -> usize {
    text.find(needle)
        .unwrap_or_else(|| panic!("no {needle:?} in {text}"))
}

// ---------- no PR ----------

#[test]
fn a_plan_without_a_pr_shows_lists_and_renders() {
    let fx = fx("grounding");
    let show = fx.show();
    assert_eq!(
        (&show["plan"]["id"], &show["workflows"], &show["jobs"]),
        (&json!("pay-1"), &json!([]), &json!([]))
    );
    assert_eq!(fx.list()["plans"][0]["workflows"], json!([]));
    let (out, text) = fx.render();
    assert_eq!(
        (&out["workflows"], &out["draft_jobs"]),
        (&json!(0), &json!(0))
    );
    assert_eq!(
        text,
        "# Plan: Pay the bills\n\nContext: `ctx` · Phase: `grounding`\n\nNo Workflows yet.\n"
    );
}

// ---------- one PR ----------

fn one_pr() -> Fx {
    let fx = fx("dispatch");
    fx.pr("pay", "dispatch", &[]);
    fx.job("a", "pay", "draft", &[]);
    fx.job("b", "pay", "draft", &["a"]);
    fx.job("gone", "pay", "cancelled", &[]);
    fx
}

#[test]
fn show_with_one_pr_lists_its_fields_and_its_jobs() {
    let fx = one_pr();
    let show = fx.show();
    let plan = show["plan"].as_object().unwrap();
    assert_eq!(
        plan.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "archived_at",
            "context",
            "created_at",
            "done_at",
            "id",
            "jira",
            "lead_session",
            "locked_at",
            "outline",
            "phase",
            "title"
        ]
    );
    assert_eq!(
        (&plan["phase"], &plan["lead_session"], &plan["jira"]),
        (&json!("dispatch"), &json!("lead1"), &Value::Null)
    );

    let prs = show["workflows"].as_array().unwrap();
    assert_eq!(prs.len(), 1);
    assert_eq!(
        prs[0],
        json!({"key": "pay", "repo": "/r/pay", "base": "main", "base_workflow": null, "needs": [], "status": "dispatch", "closed_unmerged": false,
               "lead_branch": "cpd/pay-1/pay", "lead_worktree": "/r/pay.pay-1.pay",
               "pr": null, "pull_request": null, "all_merged_at": null, "start_failed_at": null,
               "created_at": "t", "started_at": "2000-01-01T00:00:01.000Z", "merged_at": null, "title": null, "shape": "free",
               "head_ref": null, "jira": null, "feedback_open": null, "jobs": ["a", "b", "gone"]})
    );
    assert_eq!(keys(&show["jobs"]), ["a", "b", "gone"]);
    let jobs = show["jobs"].as_array().unwrap();
    assert_eq!(
        (&jobs[1]["pr"], &jobs[1]["needs"]),
        (&json!("pay"), &json!(["a"]))
    );
    assert_eq!(
        (&jobs[0]["pr"], &jobs[0]["attempts"]),
        (&json!("pay"), &json!([]))
    );
    assert!(
        jobs.iter()
            .all(|job| !job.as_object().unwrap().contains_key("review")),
        "a Job has no review key: {jobs:?}"
    );
    assert_eq!(
        show["counts"],
        json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 1})
    );
}

#[test]
fn list_with_one_pr_has_the_dispatch_columns() {
    let fx = one_pr();
    let list = fx.list();
    let plan = &list["plans"][0];
    assert_eq!(
        (
            &plan["id"],
            &plan["context"],
            &plan["title"],
            &plan["phase"]
        ),
        (
            &json!("pay-1"),
            &json!("ctx"),
            &json!("Pay the bills"),
            &json!("dispatch")
        )
    );
    assert_eq!(
        plan["counts"],
        json!({"draft": 2, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 1})
    );
    assert_eq!(
        plan["workflows"],
        json!([{"key": "pay", "repo": "/r/pay", "status": "dispatch", "pr": null, "pull_request": null}])
    );
}

#[test]
fn render_with_one_pr_keeps_the_jobs_table_and_graph() {
    let fx = one_pr();
    let (out, text) = fx.render();
    assert_eq!(
        (&out["draft_jobs"], &out["workflows"]),
        (&json!(2), &json!(1))
    );
    assert!(
        text.starts_with("# Plan: Pay the bills\n\nContext: `ctx` · Phase: `dispatch`\n"),
        "{text}"
    );
    assert!(
        text.contains("| `pay` | `/r/pay` | `main` | none |"),
        "{text}"
    );
    assert!(
        !text.contains("graph LR"),
        "one PR has no graph of PRs: {text}"
    );
    assert!(
        text.contains("\n## Workflow pay\n\nRepo: `/r/pay` · Base: `main` · Needs: none\n"),
        "{text}"
    );
    assert!(
        text.contains("| `b` | Job b | work | S2 | draft | `a` | `src/b.rs` |"),
        "{text}"
    );
    assert!(
        text.contains("```mermaid\ngraph TD\n  j1[\"a\"]\n  j2[\"b\"]\n  j1 --> j2\n```"),
        "{text}"
    );
    assert!(text.contains("\n### `a`: Job a\n"), "{text}");
    assert!(text.contains("\n#### Goal\n\nGoal of b.\n"), "{text}");
    // a cancelled Job is shown with its status, and drawn in no graph (hs-d518)
    assert!(
        text.contains("| `gone` |") && !text.contains("[\"gone\"]"),
        "{text}"
    );
    assert_eq!(fx.render().1, text, "idempotent");
}

// ---------- two PRs ----------

/// `z-api` (open, with a pull request) and `a-web` (waiting, needs `z-api`),
/// and a `cancelled` PR `old` with a Job: the needs order is not the key order.
fn two_prs() -> Fx {
    let fx = fx("dispatch");
    fx.pr("z-api", "open", &[]);
    fx.pr("a-web", "waiting", &["z-api"]);
    fx.pr("old", "cancelled", &[]);
    fx.job("api-1", "z-api", "draft", &[]);
    fx.job("api-2", "z-api", "draft", &["api-1"]);
    fx.job("web-1", "a-web", "draft", &[]);
    fx.job("old-1", "old", "cancelled", &[]);
    fx
}

#[test]
fn show_with_two_prs_lists_every_pr_in_key_order_with_its_jobs() {
    let fx = two_prs();
    let show = fx.show();
    let prs = &show["workflows"];
    assert_eq!(
        keys(prs),
        ["a-web", "old", "z-api"],
        "cancelled PRs included, key order"
    );
    let by_key = |key: &str| {
        prs.as_array()
            .unwrap()
            .iter()
            .find(|p| p["key"] == key)
            .unwrap()
    };
    let (web, api, old) = (by_key("a-web"), by_key("z-api"), by_key("old"));
    assert_eq!(
        (
            &web["status"],
            &web["needs"],
            &web["lead_worktree"],
            &web["started_at"],
            &web["jobs"]
        ),
        (
            &json!("waiting"),
            &json!(["z-api"]),
            &Value::Null,
            &Value::Null,
            &json!(["web-1"])
        )
    );
    assert_eq!(
        (&api["status"], &api["needs"], &api["jobs"]),
        (&json!("open"), &json!([]), &json!(["api-1", "api-2"]))
    );
    assert_eq!(
        api["pr"],
        json!({"number": 7, "url": "https://github.com/acme/api/pull/7", "opened_at": "2000-01-01T00:00:02.000Z"})
    );
    assert_eq!(api["pull_request"], pull_request());
    assert_eq!(
        (
            &api["closed_unmerged"],
            &web["closed_unmerged"],
            &old["closed_unmerged"]
        ),
        (&json!(false), &json!(false), &json!(false))
    );
    assert_eq!(
        (&web["pull_request"], &old["pull_request"]),
        (&Value::Null, &Value::Null)
    );
    assert_eq!(
        (&old["status"], &old["jobs"]),
        (&json!("cancelled"), &json!(["old-1"]))
    );
    assert_eq!(
        keys(&show["jobs"]),
        ["api-1", "api-2", "web-1", "old-1"],
        "Jobs in seq order"
    );
    let job_pr: Vec<&str> = show["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["pr"].as_str().unwrap())
        .collect();
    assert_eq!(job_pr, ["z-api", "z-api", "a-web", "old"]);
}

#[test]
fn list_with_two_prs_has_each_prs_columns_in_key_order() {
    let fx = two_prs();
    let plan = fx.list()["plans"][0].clone();
    assert_eq!(
        plan["workflows"],
        json!([
            {"key": "a-web", "repo": "/r/a-web", "status": "waiting", "pr": null, "pull_request": null},
            {"key": "old", "repo": "/r/old", "status": "cancelled", "pr": null, "pull_request": null},
            {"key": "z-api", "repo": "/r/z-api", "status": "open",
             "pr": {"number": 7, "url": "https://github.com/acme/api/pull/7", "opened_at": "2000-01-01T00:00:02.000Z"},
             "pull_request": pull_request()},
        ])
    );
    assert_eq!(
        plan["counts"],
        json!({"draft": 3, "queued": 0, "running": 0, "merged": 0, "failed": 0, "cancelled": 1})
    );
}

#[test]
fn render_with_two_prs_has_a_section_per_pr_in_needs_order() {
    let fx = two_prs();
    let (out, text) = fx.render();
    assert_eq!(
        (&out["draft_jobs"], &out["workflows"]),
        (&json!(3), &json!(2)),
        "the cancelled PR is not counted"
    );
    assert!(
        text.contains("| `a-web` | `/r/a-web` | `main` | `z-api` |"),
        "{text}"
    );
    assert!(
        text.contains("| `z-api` | `/r/z-api` | `main` | none |"),
        "{text}"
    );
    // the cancelled PR and its Job are shown, after the live PRs, and in no graph (hs-d518)
    assert!(
        text.contains("| `old` | `/r/old` | `main` | none | cancelled |")
            && text.contains("| `old-1` |"),
        "{text}"
    );
    assert!(
        text.find("| `z-api` |").unwrap() < text.find("| `old` |").unwrap(),
        "{text}"
    );
    assert!(!text.contains("[\"old\"]"), "{text}");
    assert!(
        text.contains("```mermaid\ngraph LR\n  p1[\"z-api\"]\n  p2[\"a-web\"]\n  p1 --> p2\n```"),
        "{text}"
    );

    let (api, web) = (
        at(&text, "\n## Workflow z-api\n"),
        at(&text, "\n## Workflow a-web\n"),
    );
    assert!(api < web, "the needed PR comes first: {text}");
    assert!(
        text.contains("\n## Workflow a-web\n\nRepo: `/r/a-web` · Base: `main` · Needs: `z-api`\n"),
        "{text}"
    );
    // Each Job is under its PR only, with that PR's graph.
    for job in ["api-1", "api-2"] {
        let section = at(&text, &format!("\n### `{job}`: Job {job}\n"));
        assert!(api < section && section < web, "{job}: {text}");
    }
    assert!(web < at(&text, "\n### `web-1`: Job web-1\n"), "{text}");
    assert_eq!(text.matches("### `web-1`").count(), 1, "{text}");
    assert!(
        text.contains("```mermaid\ngraph TD\n  j1[\"api-1\"]\n  j2[\"api-2\"]\n  j1 --> j2\n```"),
        "{text}"
    );
    assert!(
        text.contains("```mermaid\ngraph TD\n  j1[\"web-1\"]\n```"),
        "{text}"
    );
    assert_eq!(fx.render().1, text, "idempotent");
}

#[test]
fn render_ties_prs_by_key() {
    let fx = fx("plan");
    for key in ["web", "api", "db"] {
        fx.pr(key, "waiting", &[]);
        fx.job(&format!("{key}-1"), key, "draft", &[]);
    }
    let (_, text) = fx.render();
    let order: Vec<usize> = ["api", "db", "web"]
        .iter()
        .map(|k| at(&text, &format!("\n## Workflow {k}\n")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{text}");
}

#[test]
fn a_pr_without_jobs_says_so_and_the_fingerprint_covers_the_prs() {
    let fx = fx("plan");
    fx.pr("api", "waiting", &[]);
    fx.job("api-1", "api", "draft", &[]);
    fx.pr("web", "dispatch", &["api"]);
    let (out, text) = fx.render();
    assert!(
        text.contains(
            "\n## Workflow web\n\nRepo: `/r/web` · Base: `main` · Needs: `api`\n\nNo Jobs yet.\n"
        ),
        "{text}"
    );
    let before = out["fingerprint"].clone();
    fx.env
        .sql("UPDATE workflows SET base = 'develop' WHERE key = 'web'");
    assert_ne!(fx.render().0["fingerprint"], before);
}

/// A PR that has not started has no Job yet, and shows the `open` Job cpd will add (commands.md §4.5, hs-d497).
#[test]
fn a_waiting_pr_with_no_jobs_shows_the_open_job_placeholder_in_no_fingerprint() {
    let fx = fx("plan");
    fx.pr("web", "waiting", &[]);
    let (out, text) = fx.render();
    assert!(!text.contains("No Jobs yet."), "{text}");
    assert!(text.contains("\n## Workflow web\n\nRepo: `/r/web` · Base: `main` · Needs: none\n\n### Jobs\n\n| Key | Title | Kind | Tier | Status | Needs | Touches |\n"), "{text}");
    assert!(text.contains("| `open-web` | Open the pull request: web | open | H3 | — | every other Job of the Workflow | — |"), "{text}");
    assert!(
        text.contains("\n### `open-web`: Open the pull request: web\n"),
        "{text}"
    );
    // `worker_tier` is the config key; the placeholder is no row, so the fingerprint is that of a PR with no Job.
    fx.env
        .write_config("version = 1\nmachine = \"t\"\nworker_tier = \"S3\"\n");
    let (again, text) = fx.render();
    assert!(text.contains("| open | S3 | — |"), "{text}");
    assert_eq!(again["fingerprint"], out["fingerprint"]);
}

// ---------- closed unmerged ----------

#[test]
fn a_closed_unmerged_pr_is_flagged_in_plan_show() {
    let fx = fx("dispatch");
    fx.pr("api", "open", &[]);
    fx.pr("web", "waiting", &[]);
    fx.env
        .sql("UPDATE pull_requests SET state = 'closed' WHERE number = 7");
    let show = fx.show();
    let flags: Vec<(&str, &Value)> = show["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["key"].as_str().unwrap(), &p["closed_unmerged"]))
        .collect();
    assert_eq!(flags, [("api", &json!(true)), ("web", &json!(false))]);
}

#[test]
fn a_pr_with_an_open_pull_request_is_not_flagged_in_plan_show() {
    let fx = fx("dispatch");
    fx.pr("api", "open", &[]);
    let show = fx.show();
    assert_eq!(show["workflows"][0]["closed_unmerged"], json!(false));
    assert_eq!(show["workflows"][0]["pull_request"]["state"], json!("open"));
}
