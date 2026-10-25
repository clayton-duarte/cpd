//! `plan cancel` (commands.md §4.12, E1), run as the built binary against a temporary `$CPD_HOME`,
//! a private tmux server, a fake `pi`-less cockpit (a fake `cpd` for the panes) and a fake `gh`.
//! Jobs, Attempts and PR rows that only the engine reaches are written straight into the database.

use crate::common;

use std::fs;

use common::{TestEnv, WATCH_OK, refused, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};

// ---------- fixtures ----------

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

/// Where the fake `session run <id>` writes its environment: it exists once the pane runs.
fn wait_for_pane(env: &TestEnv, session: &str) {
    let path = env.root().join(format!("pane-{session}.env"));
    wait_until("the pane's session run", || path.exists() && fs::read_to_string(&path).is_ok_and(|t| t.contains("CPD_SESSION")));
}

/// Plan `pay` in phase `dispatch` with its Lead and a side fork (both with a live pane), three PRs
/// and Jobs in every state:
///
/// | PR | status | its pull request | Jobs |
/// |---|---|---|---|
/// | `api` | `open` | `acme/api#11`, open | `d` draft, `q` queued, `f` failed, `r` running (Attempt `r-a1`), `m` merged |
/// | `web` | `open` | `acme/web#22`, merged by the poller | |
/// | `ui` | `waiting` | none | |
///
/// `gh` is a fake that records its calls and says every pull request is open.
fn setup(env: &TestEnv) -> (String, String) {
    let config = format!("version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n[tmux]\nsocket = \"{}\"\n", env.tmux_socket());
    fs::write(env.cpd_home().join("config.toml"), config).unwrap();
    env.fake_cpd_panes(WATCH_OK);
    env.fake_gh("case \"$1 $2\" in\n\"pr view\") echo '{\"state\":\"OPEN\"}';;\nesac\nexit 0");
    env.cpd(&["context", "new", "ctx", "--title", "Payments", "--owner", "acme"]).ok();
    let lead = env.cpd(&["plan", "new", "pay", "--context", "ctx", "--title", "Plan pay"]).ok()["lead"]["session"].as_str().unwrap().to_owned();
    for key in ["api", "web", "ui"] {
        let repo = env.keyed_repo(key);
        env.cpd(&["pr", "add", key, "--repo", repo.to_str().unwrap(), "--title", "Change the PR", "--shape", "config", "--plan", "pay"]).ok();
        // The stub the shape lays out is cancelled, as a Lead cancels a stub it does not need: these Jobs are the table's.
        env.cpd(&["job", "cancel", &format!("{key}-config"), "--reason", "a stub of the template", "--plan", "pay"]).ok();
    }
    wait_for_pane(env, &lead);
    let fork = env.cpd(&["session", "fork", &lead]).ok()["session"]["id"].as_str().unwrap().to_owned();
    wait_for_pane(env, &fork);

    // The Jobs and PRs reach their states the way the engine moves them, one legal step at a time.
    let pull_request = |repo: &str, number: i64, state: &str| {
        let merged = if state == "merged" { "'t'" } else { "NULL" };
        format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'T', 0, '{state}', 'none', 'none', 't', {merged}, 't');"
        )
    };
    let start_pr = |key: &str| {
        format!("UPDATE prs SET status = 'dispatch', lead_worktree = '/wt/{key}', started_at = 't' WHERE plan = 'pay' AND key = '{key}';")
    };
    let open_pr = |key: &str, number: i64, status: &str| {
        let merged = if status == "merged" { ", merged_at = 't'" } else { "" };
        format!(
            "UPDATE prs SET status = 'open', pr_number = {number}, pr_url = 'https://github.com/acme/{key}/pull/{number}', pr_opened_at = 't',
                    pull_request = (SELECT id FROM pull_requests WHERE repo = 'acme/{key}' AND number = {number})
              WHERE plan = 'pay' AND key = '{key}';
             UPDATE prs SET status = '{status}'{merged} WHERE plan = 'pay' AND key = '{key}' AND status <> '{status}';"
        )
    };
    let draft = |key: &str| {
        format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', 'api', '{key}', 'Job {key}', 'S2', 'Goal.', 'Context.', '[\"src/{key}.rs\"]', '[\"cargo test\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        )
    };
    let attempt = |key: &str, ended: Option<(&str, &str)>| {
        let (status, outcome, cause, ended_at) = match ended {
            Some((outcome, cause)) => ("ended", format!("'{outcome}'"), format!("'{cause}'"), "'t'"),
            None => ("running", "NULL".to_owned(), "NULL".to_owned(), "NULL"),
        };
        format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
             VALUES ('pay', '{key}-a1', '{key}', 1, 'S2', 'ladder', '{status}', {outcome}, {cause}, 'cpd/pay--{key}-a1', '/w', 't', {ended_at});"
        )
    };
    let fingerprint = "f".repeat(64);
    fixture(
        env,
        &format!(
            "UPDATE plans SET phase = 'plan' WHERE id = 'pay';
             {d}{q}{f}{r}{m}
             UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'pay';
             UPDATE jobs SET status = 'queued', fingerprint = '{fingerprint}' WHERE key <> 'd' AND status = 'draft';
             {start_api}{start_web}
             UPDATE jobs SET status = 'running' WHERE key IN ('f', 'r', 'm');
             {attempt_f}{attempt_r}
             UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE key = 'f';
             UPDATE jobs SET status = 'merged', merged_sha = 'c0ffee' WHERE key = 'm';
             {api}{web}{open_api}{open_web}
             INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
               VALUES ('pay', 'r-a1-q1', 'r-a1', 1, 'Which?', 'open', 1, 't');",
            d = draft("d"),
            q = draft("q"),
            f = draft("f"),
            r = draft("r"),
            m = draft("m"),
            start_api = start_pr("api"),
            start_web = start_pr("web"),
            attempt_f = attempt("f", Some(("failed", "timed_out"))),
            attempt_r = attempt("r", None),
            api = pull_request("api", 11, "open"),
            web = pull_request("web", 22, "merged"),
            open_api = open_pr("api", 11, "open"),
            open_web = open_pr("web", 22, "merged"),
        ),
    );
    (lead, fork)
}

// ---------- the command ----------

// ---------- a `tdd` PR's last test and impl Jobs do not stop a plan cancel (hs-d573, rule B2) ----------

// ---------- refusals ----------

#[test]
fn an_unknown_plan_is_plan_not_found() {
    let env = TestEnv::new();
    setup(&env);
    refused(&env.cpd(&["plan", "cancel", "--plan", "nope"]), "plan_not_found", 4);
}

