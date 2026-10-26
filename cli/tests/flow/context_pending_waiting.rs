//! `cpd context pending` shows work that cannot start yet as `waiting`, with its blockers (commands.md §3.11,
//! hs-d521, hs-d523): `waiting_on` in JSON, `waiting on <keys>` in the markdown. The stored status does not change.
//! The JSON test runs in this process (`Fx`); the markdown one stays on the binary: `--markdown` prints on the
//! process's own stdout, which `Fx` cannot read back.

use crate::common::TestEnv;
use crate::common::fx::Fx;
use crate::plan_render_cancelled::{env, job, job_of_kind, pr};
use serde_json::json;

fn fixture() -> TestEnv {
    let env = env();
    pr(&env, "api", "dispatch", "main", &[]);
    job(&env, "api", "a", "queued", &[]);
    job(&env, "api", "m", "merged", &[]);
    job(&env, "api", "c", "queued", &["a", "m"]);
    job_of_kind(&env, "api", "open-api", "open", "queued", &[]);
    // a PR that needs `api`, which has not merged
    pr(&env, "web", "waiting", "main", &["api"]);
    env
}

#[test]
fn waiting_job_lists_its_blockers() {
    let fx = Fx::with(fixture());
    let out = fx.cpd(&["context", "pending", "--context", "ctx"]).ok();
    let prs = &out["plans"][0]["prs"];
    let api = prs.as_array().unwrap().iter().find(|p| p["key"] == "api").unwrap();
    let jobs = api["jobs"].as_array().unwrap();
    let find = |key: &str| jobs.iter().find(|j| j["key"] == key).unwrap_or_else(|| panic!("{key}: {out}"));
    // `c` cannot start until `a` merges (`m` is merged); `a` is ready, and stays queued
    assert_eq!((&find("c")["status"], &find("c")["waiting_on"]), (&json!("waiting"), &json!(["a"])), "{out}");
    assert_eq!(find("a")["status"], "queued", "{out}");
    assert!(find("a").get("waiting_on").is_none(), "{out}");
    // the `open` Job waits for the other Jobs of its PR that are not merged
    assert_eq!((&find("open-api")["status"], &find("open-api")["waiting_on"]), (&json!("waiting"), &json!(["a", "c"])), "{out}");
    // a PR waits for the PRs it needs
    let web = prs.as_array().unwrap().iter().find(|p| p["key"] == "web").unwrap();
    assert_eq!(web["waiting_on"], json!(["api"]), "{out}");
    // the stored status stays
    assert_eq!(fx.env.query::<String>("SELECT status FROM jobs WHERE key = 'c'"), "queued");
}

#[test]
fn markdown_shows_waiting_on_blockers() {
    let env = fixture();
    let run = env.cpd(&["context", "pending", "--context", "ctx", "--markdown"]);
    assert_eq!(run.exit, 0, "{run:?}");
    let line = |prefix: &str| run.stdout.lines().find(|l| l.starts_with(prefix)).unwrap_or_else(|| panic!("{prefix}:\n{}", run.stdout));
    assert!(line("- c ").starts_with("- c waiting on a: Job c"), "{}", run.stdout);
    assert!(line("- open-api ").starts_with("- open-api waiting on a, c: "), "{}", run.stdout);
    assert!(line("- a ").starts_with("- a queued: "), "{}", run.stdout);
    let web = run.stdout.split("### PR web").nth(1).unwrap_or_else(|| panic!("{}", run.stdout));
    assert!(web.lines().take(4).any(|l| l == "waiting on api"), "{}", run.stdout);
}
