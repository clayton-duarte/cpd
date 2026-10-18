//! The `plan` view (commands.md §13.2, §17), the centre-top pane of the cockpit, on a private tmux server: the flow
//! chart of the Plan of the Lead in the chat slot, its tabs, and the view's refresh from Events.
//!
//! The store is prepared through the store API (`Store::write`), as the other areas' tests do, because Plans, Jobs
//! and Attempts belong to their areas; the view runs as `cpd tui view plan` in a pane, and its screen is read with
//! `tmux capture-pane`.

mod common;

use std::fs;
use std::time::{Duration, Instant};

use common::{CPD, SLEEP_SECONDS, TestEnv};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::Store;
use serde_json::json;

/// The expected chart of the store `cpd_jobs()` makes: the fixture of the cockpit of branch `cpd/cpd-ui`
/// (`tui/tests/fixtures/cpd-chart.txt`), kept beside the view.
const CPD_CHART: &str = include_str!("../../tui/src/plan/fixtures/cpd-chart.txt");

/// The 27 Jobs of the cockpit's `cpd-jobs.json` fixture (cpd 1's `workflow show` of cpd-rust's `cpd` Workflow), as
/// cpd 2 stores them: `(key, status, tier, needs, Attempts)`. cpd 1's `closed` is `merged`, and its `in_review` is a
/// `running` Job whose Attempt is `merging` (here `merging`).
const CPD_JOBS: &[(&str, &str, &str, &[&str], u32)] = &[
    ("contract-core", "merged", "O3", &[], 2),
    ("contract-commands", "merged", "O3", &["contract-core"], 1),
    ("review-in-tmux", "merged", "S3", &[], 1),
    ("contract-amend", "merged", "S3", &[], 1),
    ("rs-workspace", "merging", "O3", &[], 1),
    ("rs-store", "queued", "O3", &["rs-workspace"], 0),
    ("rs-events", "queued", "S3", &["rs-store"], 0),
    ("rs-config", "queued", "S3", &["rs-workspace"], 0),
    ("rs-git", "queued", "S3", &["rs-config"], 0),
    ("rs-host", "queued", "O3", &["rs-config"], 0),
    ("rs-context", "queued", "S3", &["rs-events"], 0),
    ("rs-session", "queued", "O3", &["rs-events", "rs-host"], 0),
    ("rs-plan", "queued", "O3", &["rs-context", "rs-git", "rs-session"], 0),
    ("rs-job", "queued", "S3", &["rs-plan"], 0),
    ("rs-merge", "queued", "O3", &["rs-events", "rs-git"], 0),
    ("rs-attempt-cmds", "queued", "S3", &["rs-git", "rs-job"], 0),
    ("rs-ask", "queued", "S3", &["rs-job"], 0),
    ("rs-review", "queued", "S3", &["rs-host", "rs-job"], 0),
    ("rs-spec-metrics", "queued", "S3", &["rs-job"], 0),
    ("rs-driver", "queued", "O3", &["rs-attempt-cmds", "rs-merge", "rs-review", "rs-session", "rs-spec-metrics"], 0),
    ("rs-pr", "queued", "S3", &["rs-merge", "rs-plan"], 0),
    ("rs-watch", "queued", "O3", &["rs-ask", "rs-context", "rs-driver", "rs-pr"], 0),
    ("rs-doctor", "queued", "S3", &["rs-config", "rs-git", "rs-host", "rs-store"], 0),
    ("pi-adapter-v2", "queued", "S3", &["rs-events", "rs-plan", "rs-session"], 0),
    ("skill-v2", "queued", "S3", &["pi-adapter-v2", "rs-watch"], 0),
    ("gate-at-pr", "running", "S3", &[], 1),
    ("contract-gate-pr", "running", "O3", &["contract-amend"], 1),
];

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

// ---------- the environment ----------

/// An env whose config has `concurrency = 4` and the private tmux server, with a fake `gh` and `pi` that record any call.
fn env() -> TestEnv {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\nconcurrency = 4\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n\
             [events]\npoll_ms = 50\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake_gh("exit 1");
    env.fake_pi("exit 1");
    env
}

fn open(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn sql(env: &TestEnv, text: &str) {
    open(env)
        .write("fixture", |tx| {
            tx.execute_batch(text)?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
}

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Waits until `done` holds, at most 30 s.
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Starts `cpd tui view plan` alone in a 260 × 40 window of the private server; returns its pane.
fn start_view(env: &TestEnv) -> String {
    tmux_out(env, &["new-session", "-d", "-s", "view", "-x", "260", "-y", "40", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "plan"])
        .trim()
        .to_owned()
}

fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

/// Waits until the screen satisfies `ok`, and returns it.
fn screen_when(env: &TestEnv, pane: &str, what: &str, ok: impl Fn(&[String]) -> bool) -> Vec<String> {
    let mut last = Vec::new();
    wait_until(what, || {
        last = screen(env, pane);
        ok(&last)
    });
    last
}

/// The chart rows inside the one PR box of a screen: below the two rows of the tabs, one of padding and the box's top
/// border, indented by two columns of padding and the box's left border; the box's right border is left out.
fn chart_rows(screen: &[String], rows: usize) -> String {
    screen[4..4 + rows]
        .iter()
        .map(|l| {
            let inside: String = l.chars().skip(3).collect();
            let inside = inside.trim_end();
            inside.strip_suffix('│').unwrap_or_else(|| panic!("no right border: {l:?}")).trim_end().to_owned()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ---------- rows ----------

fn context(id: &str) -> String {
    format!(
        "INSERT INTO contexts (id, title, jira, created_at, created_on)
         VALUES ('{id}', 'Title of {id}', NULL, '2026-01-01T00:00:00.000Z', 'm1');"
    )
}

/// A Plan in `phase` (`dispatch` or `deploy`) with its Lead; `pr` is `(number, checks)`.
fn plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str, pr: Option<(i64, &str)>) -> String {
    let (row, number, url, opened, pull_request) = match pr {
        Some((n, checks)) => (
            format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/repo', {n}, 'https://github.com/acme/repo/pull/{n}', 'PR {n}', 0, 'open', 'none', '{checks}',
                         '2026-01-05T00:00:00.000Z', NULL, '2026-01-06T00:00:00.000Z');"
            ),
            n.to_string(),
            format!("'https://github.com/acme/repo/pull/{n}'"),
            "'2026-01-05T00:00:00.000Z'",
            format!("(SELECT id FROM pull_requests WHERE number = {n})"),
        ),
        None => (String::new(), "NULL".into(), "NULL".into(), "NULL", "NULL".into()),
    };
    let status = if pr.is_some() { "open" } else { "dispatch" };
    bare_plan_row(id, context, phase, lead, created)
        + &format!(
            "{row}
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at)
         VALUES ('{id}', 'repo', '/repo', 'main', '{status}', 'cpd/{id}', '/wt', {number}, {url}, {opened}, {pull_request},
                 cpd_txn(), '{created}', '2026-01-02T00:00:00.000Z');"
        )
}

/// A Plan in `phase` with its Lead, and no PR.
fn bare_plan_row(id: &str, context: &str, phase: &str, lead: &str, created: &str) -> String {
    let locked = if matches!(phase, "grounding" | "plan") { "NULL" } else { "'2026-01-02T00:00:00.000Z'" };
    format!(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
         VALUES ('{id}', '{context}', 'Plan {id}', '{phase}', '{lead}', '{created}', {locked});"
    )
}

/// One Context `ctx` and one Plan `ship` in `phase`, with no PR, whose Lead `lead-ship` is in the chat slot.
fn bare_plan(phase: &str) -> String {
    context("ctx")
        + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z")
        + &bare_plan_row("ship", "ctx", phase, "lead-ship", "2026-01-02T00:00:00.000Z")
        + &lead_row("lead-ship", "ctx", "ship", "2026-01-02T00:00:00.000Z")
        + &in_chat("ctx", "ship", "lead-ship")
}

/// A Lead; `plan` is empty for a Context's grounded Lead.
fn lead_row(id: &str, context: &str, plan: &str, created: &str) -> String {
    let plan = if plan.is_empty() { "NULL".to_owned() } else { format!("'{plan}'") };
    format!(
        "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
         VALUES ('{id}', 'lead', '{context}', {plan}, NULL, 'name of {id}', '/sessions/{id}.jsonl', '/repo', 'O3',
                 NULL, 'idle', NULL, '2026-01-03T00:00:00.000Z', '{created}');"
    )
}

fn job_row(plan: &str, key: &str, status: &str, tier: &str) -> String {
    job_row_in(plan, "repo", key, status, tier)
}

/// A Job of the PR `pr`.
fn job_row_in(plan: &str, pr: &str, key: &str, status: &str, tier: &str) -> String {
    // `merging` is a `running` Job whose Attempt is `merging`.
    let status = if status == "merging" { "running" } else { status };
    let fingerprint = if status == "draft" { "NULL".to_owned() } else { format!("'{FP}'") };
    let merged = if status == "merged" { "'abc123'" } else { "NULL" };
    let failed = if status == "failed" { "'crashed'" } else { "NULL" };
    let (reason, requested) =
        if status == "cancelled" { ("'not needed'", "'2026-01-04T00:00:00.000Z'") } else { ("NULL", "NULL") };
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           created_txn, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_at)
         VALUES ('{plan}', '{pr}', '{key}', 'Job {key}', '{tier}', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, {fingerprint},
                 cpd_txn(), {reason}, {requested}, {failed}, {merged}, '2026-01-02T00:00:00.000Z');"
    )
}

fn need_row(plan: &str, job: &str, needs: &str) -> String {
    format!("INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{job}', '{needs}');")
}

/// The Attempts of one Job: `count` of them, the last one in the Job's status, the earlier ones ended `crashed`.
fn attempt_rows(plan: &str, job: &str, status: &str, tier: &str, count: u32) -> String {
    let mut out = String::new();
    for number in 1..=count {
        let last = number == count;
        let (state, outcome, cause, merged, ended) = match (last, status) {
            (false, _) => ("ended", "'failed'", "'crashed'", "NULL", "'2026-01-03T01:00:00.000Z'"),
            (true, "merged") => ("ended", "'merged'", "'merged'", "'abc123'", "'2026-01-03T01:00:00.000Z'"),
            (true, "failed") => ("ended", "'failed'", "'crashed'", "NULL", "'2026-01-03T01:00:00.000Z'"),
            (true, "merging") => ("merging", "NULL", "NULL", "NULL", "NULL"),
            (true, _) => ("running", "NULL", "NULL", "NULL", "NULL"),
        };
        let (sha, at) = if state == "merging" { ("'def456'", "'2026-01-03T00:30:00.000Z'") } else { ("NULL", "NULL") };
        let origin = if number == 1 { "ladder" } else { "retry" };
        out += &format!(
            "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, worker_session,
                                   submitted_sha, submitted_at, merged_sha, started_at, ended_at)
             VALUES ('{plan}', '{job}-a{number}', '{job}', {number}, '{tier}', '{origin}', '{state}', {outcome}, {cause},
                     'cpd/{plan}--{job}-a{number}', '/w/{job}-a{number}', NULL, {sha}, {at}, {merged},
                     '2026-01-03T00:{number:02}:00.000Z', {ended});"
        );
    }
    out
}

fn event(kind: &str, context: &str, plan: Option<&str>, session: Option<&str>, data: &str) -> String {
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', '{kind}', '{context}', {}, {}, 'fixture {kind}', '{data}');",
        q(plan),
        q(session)
    )
}

/// `session-moved` to the chat slot: the Lead is the one the view shows.
fn in_chat(context: &str, plan: &str, session: &str) -> String {
    event("session-moved", context, Some(plan), Some(session), r#"{"pane":"%1","window":"@1","place":"chat"}"#)
}

/// One Context `ctx`, one Plan `ship` in `phase` whose Lead `lead-ship` is in the chat slot, and `jobs`
/// as `(key, status, tier, needs, Attempts)`; no Attempt for the statuses that have none.
fn plan_with(phase: &str, pr: Option<(i64, &str)>, jobs: &[(&str, &str, &str, &[&str], u32)]) -> String {
    let mut s = context("ctx");
    s += &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z");
    s += &plan_row("ship", "ctx", phase, "lead-ship", "2026-01-02T00:00:00.000Z", pr);
    s += &lead_row("lead-ship", "ctx", "ship", "2026-01-02T00:00:00.000Z");
    for (key, status, tier, _, _) in jobs {
        s += &job_row("ship", key, status, tier);
    }
    for (key, _, _, needs, _) in jobs {
        for need in *needs {
            s += &need_row("ship", key, need);
        }
    }
    for (key, status, tier, _, attempts) in jobs {
        s += &attempt_rows("ship", key, status, tier, *attempts);
    }
    s + &in_chat("ctx", "ship", "lead-ship")
}

// ---------- tests ----------

#[test]
fn the_chart_of_a_store_equivalent_to_the_cockpits_fixture_matches_it() {
    let env = env();
    sql(&env, &plan_with("dispatch", None, CPD_JOBS));
    let pane = start_view(&env);
    let rows = CPD_CHART.lines().count();
    let screen = screen_when(&env, &pane, "the chart", |s| s.len() > 4 + rows && s[4].contains("contract-core"));
    assert_eq!(chart_rows(&screen, rows), CPD_CHART.trim_end(), "{}", screen.join("\n"));
    // a Plan of one PR is one box, titled on its top border
    assert!(screen[3].starts_with("  ┌ repo · repo → main · dispatch ─"), "{:?}", screen[3]);
    assert_eq!(screen.iter().filter(|l| l.contains('┌')).count(), 1, "{}", screen.join("\n"));
}

#[test]
fn a_plan_of_two_prs_is_two_titled_boxes_linked_by_an_arrow() {
    let env = env();
    // `api` has its open pull request #12; `web` needs it and waits
    let mut s = bare_plan("dispatch");
    s += r#"INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
            VALUES ('acme/api-server', 12, 'https://github.com/acme/api-server/pull/12', 'api', 0, 'open', 'none', 'none',
                    '2026-01-05T00:00:00.000Z', NULL, '2026-01-05T00:00:00.000Z');
            INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                          created_txn, created_at, started_at)
            VALUES ('ship', 'api', '/src/api-server', 'main', 'open', 'cpd/ship/api', '/wt/api', 12, 'https://github.com/acme/api-server/pull/12',
                    '2026-01-05T00:00:00.000Z', (SELECT id FROM pull_requests WHERE number = 12), cpd_txn(), '2026-01-02T00:00:00.000Z',
                    '2026-01-02T00:00:00.000Z');
            INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
            VALUES ('ship', 'web', '/src/web', 'develop', 'waiting', 'cpd/ship/web', cpd_txn(), '2026-01-02T00:00:00.000Z');
            INSERT INTO pr_needs (plan, pr, needs) VALUES ('ship', 'web', 'api');"#;
    s += &job_row_in("ship", "api", "a", "merged", "O3");
    s += &job_row_in("ship", "api", "b", "merged", "S3");
    s += &need_row("ship", "b", "a");
    s += &job_row_in("ship", "web", "c", "queued", "S3");
    sql(&env, &s);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the boxes", |s| s.len() > 6 && s[5].ends_with('┘'));
    let want = [
        "  ┌ api · api-server → main · #12 open ┐     ┌ web · web → develop · waiting ┐",
        "  │  ✓ a O3 ───── ✓ b S3               ├────▶│  ○ c S3                       │",
        "  └────────────────────────────────────┘     └───────────────────────────────┘",
    ];
    assert_eq!(screen[3..6], want, "{}", screen.join("\n"));
    // the selection crosses from one box into the next
    tmux_out(&env, &["send-keys", "-t", &pane, "Right", "Right", "Right", "Enter"]);
    let screen = screen_when(&env, &pane, "the note", |s| s.iter().any(|l| l.contains("has not started yet")));
    assert!(screen.iter().any(|l| l.trim() == "c has not started yet"), "{}", screen.join("\n"));
}

#[test]
fn a_plan_with_no_pr_yet_says_so() {
    let env = env();
    sql(&env, &bare_plan("grounding"));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the message", |s| s.iter().any(|l| l.contains("no PR yet")));
    assert!(screen.iter().any(|l| l.trim() == "no PR yet: cpd pr add"), "{}", screen.join("\n"));
    assert!(!screen.iter().any(|l| l.contains('┌') || l.contains("No Plan")), "{}", screen.join("\n"));
    // dim
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    assert!(styled.contains("2mno PR yet: cpd pr add"), "{styled}");
}

#[test]
fn each_job_status_has_its_icon_and_a_cancelled_job_is_not_drawn() {
    let env = env();
    let jobs: &[(&str, &str, &str, &[&str], u32)] = &[
        ("draft", "draft", "S3", &[], 0),
        ("queued", "queued", "S3", &[], 0),
        ("running", "running", "S3", &[], 1),
        ("merging", "merging", "S3", &[], 1),
        ("merged", "merged", "S3", &[], 1),
        ("failed", "failed", "S3", &[], 3),
        ("gone", "cancelled", "S3", &[], 0),
    ];
    sql(&env, &plan_with("dispatch", None, jobs));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.len() > 15 && s[4].contains("draft"));
    // a column is as wide as its widest pill (`✗ failed S3 a3`); the tier, and `aN` from the second Attempt, sit at its right
    let want = "
  ◌ draft     S3

  ○ queued    S3

  ◐ running   S3

  ⇥ merging   S3

  ✓ merged    S3

  ✗ failed S3 a3";
    assert_eq!(chart_rows(&screen, 11), want.strip_prefix('\n').unwrap(), "{}", screen.join("\n"));
    assert!(screen.iter().all(|l| !l.contains("gone")), "a cancelled Job is not drawn:\n{}", screen.join("\n"));
}

/// `session-moved` to a parked pane: the Lead has a pane in the cockpit, but not in the chat slot.
fn parked(context: &str, plan: &str, session: &str) -> String {
    event("session-moved", context, Some(plan), Some(session), r#"{"pane":"%2","window":"@2","place":"parked"}"#)
}

#[test]
fn the_top_row_is_the_context_id_and_one_tab_per_lead_with_a_pane_and_nothing_at_the_right() {
    let env = env();
    let jobs: &[(&str, &str, &str, &[&str], u32)] =
        &[("a", "merged", "O3", &[], 1), ("b", "running", "S3", &["a"], 1), ("c", "running", "S3", &["a"], 2)];
    let mut s = plan_with("deploy", Some((7, "failed")), jobs);
    // a side fork with a pane, one without a pane (no tab), and one whose name is blank (the Plan id labels it)
    s += &lead_row("fork-ship", "ctx", "ship", "2026-01-02T00:00:09.000Z");
    s += &parked("ctx", "ship", "fork-ship");
    s += &lead_row("no-pane", "ctx", "ship", "2026-01-02T00:00:10.000Z");
    s += &lead_row("bare", "ctx", "ship", "2026-01-02T00:00:11.000Z");
    s += "UPDATE sessions SET name = '  ' WHERE id = 'bare';";
    s += &parked("ctx", "ship", "bare");
    sql(&env, &s);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the top row", |s| s.first().is_some_and(|l| l.contains('▸')));
    let (top, line) = (&screen[0], &screen[1]);
    assert_eq!(top, " ctx ▸  name of lead-ship   name of fork-ship   ship", "{top:?}");
    assert!(line.starts_with('─') && line.chars().all(|c| c == '─'), "{line:?}");
    // the active tab is reversed and bold: the Lead in the chat slot
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    let first = styled.lines().next().unwrap();
    assert!(first.contains("1;7m name of lead-ship "), "the active tab is not reversed and bold: {first:?}");
    let after_active = first.split("name of lead-ship ").nth(1).unwrap();
    assert!(!after_active.contains("7m"), "another tab is reversed: {first:?}");
}

#[test]
fn a_plan_without_jobs_says_no_plan_yet() {
    let env = env();
    sql(&env, &plan_with("dispatch", None, &[]));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the message", |s| s.iter().any(|l| l.contains("no Plan yet")));
    assert!(screen.iter().any(|l| l.trim() == "no Plan yet"), "{}", screen.join("\n"));
}

#[test]
fn escalations_and_the_plan_review_are_not_drawn() {
    let env = env();
    let mut s = plan_with("dispatch", None, &[("a", "running", "S3", &[], 1)]);
    s += &format!("INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at)
          VALUES ('ship', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', NULL, 1, '2026-01-03T00:20:00.000Z', NULL, NULL);
          INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at)
          VALUES ('ship', 'r1', 1, '{FP}', 'pending', '2026-01-02T01:00:00.000Z');");
    sql(&env, &s);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.len() > 4 && s[4].contains(" a "));
    assert!(screen.iter().all(|l| !l.contains("Escalation") && !l.contains("review")), "{}", screen.join("\n"));
}

#[test]
fn no_lead_in_the_chat_slot_says_so() {
    let env = env();
    sql(&env, &(context("ctx") + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z")));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the message", |s| s.first().is_some_and(|l| l.contains("no Lead")));
    assert_eq!(screen[0], " no Lead session in the centre");
}

#[test]
fn the_view_follows_events_and_switches_plan_when_the_chat_slot_changes() {
    let env = env();
    let mut s = plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]);
    s += &plan_row("side", "ctx", "dispatch", "lead-side", "2026-01-02T00:00:05.000Z", None);
    s += &lead_row("lead-side", "ctx", "side", "2026-01-02T00:00:05.000Z");
    s += &job_row("side", "other", "queued", "O3");
    sql(&env, &s);
    let pane = start_view(&env);
    screen_when(&env, &pane, "Job a", |s| s.len() > 4 && s[4].contains(" a "));

    // a Job is appended to the shown Plan: its Event makes the view take a new snapshot
    sql(
        &env,
        &(job_row("ship", "b", "queued", "S3")
            + &need_row("ship", "b", "a")
            + &event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":["a"]}"#)),
    );
    let screen = screen_when(&env, &pane, "Job b", |s| s.len() > 4 && s[4].contains(" b "));
    assert_eq!(chart_rows(&screen, 1), "  ○ a S3 ───── ○ b S3");

    // the other Lead moves into the chat slot: the view shows its Plan
    sql(&env, &in_chat("ctx", "side", "lead-side"));
    let screen = screen_when(&env, &pane, "the other Plan", |s| s.len() > 4 && s[4].contains("other"));
    assert_eq!(chart_rows(&screen, 1), "  ○ other O3");
    assert!(!screen.iter().any(|l| l.contains("○ a ")), "{}", screen.join("\n"));
}

#[test]
fn the_view_starts_no_subprocess_and_asks_gh_nothing() {
    let env = env();
    sql(&env, &plan_with("deploy", Some((7, "failed")), &[("a", "running", "S3", &[], 1)]));
    let pane = start_view(&env);
    let pid = tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_pid}"]).trim().to_owned();
    // let it draw, follow an Event and take a new snapshot
    screen_when(&env, &pane, "the chart", |s| s.len() > 4 && s[4].contains(" a "));
    sql(&env, &event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":[]}"#));
    std::thread::sleep(Duration::from_millis(1500));
    // the pane's program is the view itself, and it has no child process
    let children = env.command("ps").args(["-o", "pid=,args=", "--ppid", &pid]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&children.stdout).trim(), "", "children of the view");
    for fake in ["gh", "pi"] {
        assert_eq!(env.calls(fake), Vec::<Vec<String>>::new(), "the view ran {fake}");
    }
}

/// The cockpit's centre on the private server: the plan view above the chat slot; returns `(plan pane, chat pane)`.
fn centre(env: &TestEnv) -> (String, String) {
    env.fake(
        "cpd",
        &format!("if [ \"$1\" = tui ] && [ \"$3\" = worker ]; then exec sleep {SLEEP_SECONDS}; fi\nexec '{CPD}' \"$@\""),
    );
    let plan = tmux_out(
        env,
        &["new-session", "-d", "-s", "cpd", "-n", "main", "-x", "200", "-y", "40", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "plan"],
    )
    .trim()
    .to_owned();
    tmux_out(env, &["set", "-p", "-t", &plan, "@cpd_view", "plan"]);
    let chat = tmux_out(env, &["split-window", "-d", "-v", "-t", &plan, "-P", "-F", "#{pane_id}", "sleep", "3600"]).trim().to_owned();
    tmux_out(env, &["set", "-p", "-t", &chat, "@cpd_view", "chat"]);
    (plan, chat)
}

#[test]
fn a_selected_pill_opens_its_worker_beside_the_chat_and_a_second_enter_closes_it() {
    let env = env();
    sql(&env, &plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1), ("b", "queued", "S3", &["a"], 0)]));
    let (plan, chat) = centre(&env);
    screen_when(&env, &plan, "the chart", |s| s.len() > 4 && s[4].contains(" a "));

    let workers = || tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_view} #{@cpd_job} #{@cpd_attempt}"]);
    // nothing is selected until the User asks: an arrow selects the first pill, Enter opens its Worker
    tmux_out(&env, &["send-keys", "-t", &plan, "Right"]);
    tmux_out(&env, &["send-keys", "-t", &plan, "Enter"]);
    wait_until("the Worker pane", || workers().contains("worker a a-a1"));
    let calls = env.calls("cpd");
    assert!(calls.contains(&["tui", "view", "worker", "a-a1", "--plan", "ship"].map(str::to_owned).to_vec()), "{calls:?}");
    // beside the chat, without taking the focus: the plan view keeps it
    let layout = tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_id} #{@cpd_view} #{pane_active} #{pane_top} #{pane_width}"]);
    let row = |view: &str| layout.lines().find(|l| l.split(' ').nth(1) == Some(view)).unwrap().split(' ').map(str::to_owned).collect::<Vec<_>>();
    let (worker, chat_row) = (row("worker"), row("chat"));
    assert_eq!((worker[2].as_str(), worker[3].as_str()), ("0", chat_row[3].as_str()), "{layout}");
    assert_eq!(row("plan")[2], "1", "{layout}");
    assert!(chat.starts_with('%') && layout.contains(&chat), "{layout}");
    tmux_out(&env, &["send-keys", "-t", &plan, "Enter"]);
    wait_until("the Worker pane to close", || !workers().contains("worker"));
}

#[test]
fn another_job_retargets_the_worker_pane_in_place() {
    let env = env();
    sql(&env, &plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1), ("b", "running", "S3", &["a"], 1)]));
    let (plan, _) = centre(&env);
    screen_when(&env, &plan, "the chart", |s| s.len() > 4 && s[4].contains(" a "));
    let workers = || tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{pane_id} #{@cpd_view} #{@cpd_job} #{@cpd_attempt}"]);
    tmux_out(&env, &["send-keys", "-t", &plan, "Right", "Enter"]);
    wait_until("the Worker pane of a", || workers().contains("worker a a-a1"));
    let pane = workers().lines().find(|l| l.contains(" worker ")).unwrap().split(' ').next().unwrap().to_owned();
    // the next Job's pill: the same pane shows it
    tmux_out(&env, &["send-keys", "-t", &plan, "Right", "Enter"]);
    wait_until("the Worker pane of b", || workers().contains("worker b b-a1"));
    let after = workers();
    assert_eq!(after.lines().filter(|l| l.contains(" worker ")).count(), 1, "{after}");
    assert!(after.contains(&format!("{pane} worker b b-a1")), "retargeted in place: {after}");
    let calls = env.calls("cpd");
    assert!(calls.contains(&["tui", "view", "worker", "b-a1", "--plan", "ship"].map(str::to_owned).to_vec()), "{calls:?}");
}

#[test]
fn q_does_not_quit_the_view() {
    let env = env();
    sql(&env, &plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]));
    let pane = start_view(&env);
    screen_when(&env, &pane, "the chart", |s| s.len() > 4 && s[4].contains(" a "));
    tmux_out(&env, &["send-keys", "-t", &pane, "q"]);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_dead}"]).trim(), "0");
    assert!(screen(&env, &pane)[4].contains(" a "));
}

#[test]
fn without_a_terminal_it_is_refused_before_anything_is_read() {
    let env = env();
    let run = env.cpd(&["tui", "view", "plan"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (3, json!("not_a_terminal")), "{run:?}");
}
