//! The `plan` view (commands.md §13.2, §17), the centre-top pane of the cockpit, on a private tmux server: the flow
//! chart of the Plan of the Lead in the chat slot, and the view's refresh from Events.
//!
//! The store is prepared through the store API (`Store::write`), as the other areas' tests do, because Plans, Jobs
//! and Attempts belong to their areas; the view runs as `cpd tui view plan` in a pane, and its screen is read with
//! `tmux capture-pane`.

use crate::common;

use std::fs;
use std::time::{Duration, Instant};

use common::{CPD, SLEEP_SECONDS, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_tui::palette::{DRAFT, FAILED, MERGED, MERGING, QUEUED, SPINNER};
use serde_json::json;

/// The expected chart of the store `cpd_jobs()` makes: the fixture of the cockpit of branch `cpd/cpd-ui`
/// (`tui/tests/fixtures/cpd-chart.txt`), kept beside the view.
const CPD_CHART: &str = include_str!("../../../tui/src/plan/fixtures/cpd-chart.txt");

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

fn tmux_out(env: &TestEnv, args: &[&str]) -> String {
    let out = env.tmux(args);
    assert!(out.status.success(), "tmux {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

/// Starts `cpd tui view plan` alone in a 300 × 40 window of the private server, with `PLANNOTATOR_URL_HOST=review.test`
/// (the review host's override, contract §6.5); returns its pane.
fn start_view(env: &TestEnv) -> String {
    tmux_out(env, &["new-session", "-d", "-s", "view", "-x", "300", "-y", "40", "-P", "-F", "#{pane_id}", "-e", "PLANNOTATOR_URL_HOST=review.test", CPD, "tui", "view", "plan"])
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

/// The chart rows inside the one PR box of a screen: below one row of padding and the box's top border, indented by two columns of padding and the box's left border; the box's right border is left out. A running pill spins on the clock (`SPINNER`): its frame is whatever the screen was captured at, so every frame reads as the first.
fn chart_rows(screen: &[String], rows: usize) -> String {
    screen[2..2 + rows]
        .iter()
        .map(|l| {
            let inside: String = l.chars().skip(3).map(|c| if SPINNER[1..].iter().any(|g| g.starts_with(c)) { SPINNER[0].chars().next().unwrap() } else { c }).collect();
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
    env.sql(&plan_with("dispatch", None, CPD_JOBS));
    let pane = start_view(&env);
    let rows = CPD_CHART.lines().count();
    // the screen is captured whenever it is: wait for a frame drawn to the box's right border on every row (a capture may fall in the middle of the first frame, which the view writes in pieces)
    let screen = screen_when(&env, &pane, "the chart", |s| {
        s.len() > 2 + rows && s[2].contains("contract-core") && s[2..2 + rows].iter().all(|l| l.ends_with('│') && l.chars().count() == s[1].chars().count()) && s.iter().any(|l| l.ends_with('╯'))
    });
    assert_eq!(chart_rows(&screen, rows), CPD_CHART.trim_end(), "{}", screen.join("\n"));
    // a Plan of one PR is one box, titled on its top border
    assert!(screen[1].starts_with("  ╭ repo · repo 󱦰 main · dispatch ─"), "{:?}", screen[1]);
    assert_eq!(screen.iter().filter(|l| l.trim().starts_with("╭ ") && l.contains(" · ") && l.trim_end().ends_with('╮')).count(), 1, "{}", screen.join("\n"));
    assert!(screen.iter().all(|l| !l.contains(['┌', '┐', '└', '┘'])), "the corners are rounded:\n{}", screen.join("\n"));
}

#[test]
fn a_plan_of_two_prs_is_two_titled_boxes_one_below_the_other_linked_by_an_arrow() {
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
            INSERT INTO pr_needs (plan, pr, needs_plan, needs) VALUES ('ship', 'web', 'ship', 'api');"#;
    s += &job_row_in("ship", "api", "a", "merged", "O3");
    s += &job_row_in("ship", "api", "b", "merged", "S3");
    s += &need_row("ship", "b", "a");
    s += &job_row_in("ship", "web", "c", "queued", "S3");
    env.sql(&s);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the boxes", |s| s.len() > 8 && s[8].ends_with('╯'));
    // web below api, the arrow from api's bottom border into web's top border
    let (merged_row, queued_row) = (
        format!("  │ \u{e0b6}{MERGED} a│O3\u{e0b4}─────\u{e0b6}{MERGED} b│S3\u{e0b4}                │"),
        format!("  │ \u{e0b6}{QUEUED} c│S3\u{e0b4}                      │"),
    );
    let want = [
        "  ╭ api · api-server 󱦰 main · #12 open 󰏌 ╮",
        merged_row.as_str(),
        "  ╰──────────────────┬───────────────────╯",
        "                  ╭──╯",
        "                  󱦳",
        "  ╭ web · web 󱦰 develop · waiting ╮",
        queued_row.as_str(),
        "  ╰───────────────────────────────╯",
    ];
    assert_eq!(screen[1..9], want, "{}", screen.join("\n"));
    // the selection crosses from one box down into the one below it
    tmux_out(&env, &["send-keys", "-t", &pane, "Right", "Right", "Down", "Enter"]);
    let screen = screen_when(&env, &pane, "the note", |s| s.iter().any(|l| l.contains("has not started yet")));
    assert!(screen.iter().any(|l| l.trim() == "c has not started yet"), "{}", screen.join("\n"));
}

#[test]
fn a_plan_with_no_pr_yet_says_so() {
    let env = env();
    env.sql(&bare_plan("grounding"));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the message", |s| s.iter().any(|l| l.contains("no PR yet")));
    assert!(screen.iter().any(|l| l.trim() == "no PR yet: cpd pr add"), "{}", screen.join("\n"));
    assert!(!screen.iter().any(|l| l.contains('╭') || l.contains("No Plan")), "{}", screen.join("\n"));
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
    env.sql(&plan_with("dispatch", None, jobs));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.len() > 13 && s[2].contains("draft"));
    // a column is as wide as its widest pill (the failed pill, `failed│S3│a3`); the tier, and `aN` from the second Attempt, sit at its right
    let want = format!(
        "
 \u{e0b6}{DRAFT} draft    │S3\u{e0b4}

 \u{e0b6}{QUEUED} queued   │S3\u{e0b4}

 \u{e0b6}{} running  │S3\u{e0b4}

 \u{e0b6}{MERGING} merging  │S3\u{e0b4}

 \u{e0b6}{MERGED} merged   │S3\u{e0b4}

 \u{e0b6}{FAILED} failed│S3│a3\u{e0b4}",
        SPINNER[0]
    );
    assert_eq!(chart_rows(&screen, 11), want.strip_prefix('\n').unwrap(), "{}", screen.join("\n"));
    assert!(screen.iter().all(|l| !l.contains("gone")), "a cancelled Job is not drawn:\n{}", screen.join("\n"));
}

#[test]
fn a_plan_without_jobs_says_no_plan_yet() {
    let env = env();
    env.sql(&plan_with("dispatch", None, &[]));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the message", |s| s.iter().any(|l| l.contains("no Plan yet")));
    assert!(screen.iter().any(|l| l.trim() == "no Plan yet"), "{}", screen.join("\n"));
}

#[test]
fn escalations_and_a_decided_plan_review_are_not_drawn() {
    let env = env();
    let mut s = plan_with("dispatch", None, &[("a", "running", "S3", &[], 1)]);
    s += &format!("INSERT INTO escalations (plan, id, attempt, number, question, status, answer, asker_pid, asked_at, answered_at, expired_at)
          VALUES ('ship', 'a-a1-q1', 'a-a1', 1, 'Which?', 'open', NULL, 1, '2026-01-03T00:20:00.000Z', NULL, NULL);
          INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
          VALUES ('ship', 'r1', 1, '{FP}', 'approved', '2026-01-02T01:00:00.000Z', '2026-01-02T02:00:00.000Z');");
    env.sql(&s);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.len() > 2 && s[2].contains(" a│"));
    assert!(screen.iter().all(|l| !l.contains("Escalation") && !l.contains("review")), "{}", screen.join("\n"));
}

/// A Plan of one PR `repo` with its pull request #7 and one Job, and a Plan review `r<number>` in `status`.
fn plan_under_review(number: u32, status: &str) -> String {
    let decided = if status == "pending" { "NULL" } else { "'2026-01-02T02:00:00.000Z'" };
    plan_with("dispatch", Some((7, "none")), &[("a", "queued", "S3", &[], 0)])
        + &format!(
            "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at, decided_at)
             VALUES ('ship', 'r{number}', {number}, '{FP}', '{status}', '2026-01-02T01:00:00.000Z', {decided});"
        )
}

fn write_port(env: &TestEnv, text: &str) {
    let dir = env.cpd_home().join("run").join("ship");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("plannotator-port"), text).unwrap();
}

/// The screen with its escapes (`capture-pane -e`); tmux 3.4 keeps the OSC 8 links in it.
fn styled_screen(env: &TestEnv, pane: &str) -> String {
    tmux_out(env, &["capture-pane", "-p", "-e", "-t", pane])
}

/// The visible text the OSC 8 links to `url` cover in `styled`, SGR sequences dropped.
fn linked_text(styled: &str, url: &str) -> String {
    let (mut out, mut inside, mut rest) = (String::new(), false, styled);
    while let Some(at) = rest.find('\x1b') {
        if inside {
            out.push_str(&rest[..at]);
        }
        rest = &rest[at..];
        if let Some(osc) = rest.strip_prefix("\x1b]8;;") {
            let end = osc.find("\x1b\\").expect("an OSC 8 ends in ST");
            inside = &osc[..end] == url;
            rest = &osc[end + 2..];
        } else {
            let end = rest.find('m').expect("an SGR ends in m");
            rest = &rest[end + 1..];
        }
    }
    out
}

#[test]
fn a_pull_request_box_title_ends_in_a_link_to_it_and_a_pending_review_is_a_row_above_the_boxes() {
    let env = env();
    env.sql(&plan_under_review(2, "pending"));
    write_port(&env, "19433\n");
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the row and the box", |s| s.len() > 3 && s[3].contains(" a│"));
    // the row is the pane's first, indented by the margin; the boxes are one row lower than without it
    assert_eq!(screen[0], "   Plan review r2 pending 󰏌", "{}", screen.join("\n"));
    assert_eq!(screen[1], "");
    assert_eq!(screen[2], "  ╭ repo · repo 󱦰 main · #7 open 󰏌 ╮");
    let styled = styled_screen(&env, &pane);
    assert!(styled.contains("]8;;https://github.com/acme/repo/pull/7"), "{styled:?}");
    assert!(styled.contains("]8;;http://review.test:19433"), "{styled:?}");
    // each link wraps its whole text, from `#` or `Plan` through `󰏌`, and nothing else
    for (url, text) in [("https://github.com/acme/repo/pull/7", "#7 open 󰏌"), ("http://review.test:19433", "Plan review r2 pending 󰏌")] {
        assert_eq!(linked_text(&styled, url), text, "{url}: {styled:?}");
    }
}

#[test]
fn a_conflict_shows_in_the_box_title_and_clears_with_no_event() {
    let env = env();
    env.sql(&plan_with("deploy", Some((7, "none")), &[("a", "merged", "S3", &[], 1)]));
    let pane = start_view(&env);
    let title = |s: &[String], word: &str| s.iter().any(|l| l.contains(&format!("#7 {word} 󰏌")));
    screen_when(&env, &pane, "the open title", |s| title(s, "open"));

    // the poll stored a conflict: no Event is appended for the view to wake on, so only the change of the row shows it
    // (the poll sets `changed_at` with every change, contract §4.4)
    env.sql("UPDATE pull_requests SET mergeable = 'conflicting', changed_at = '2026-02-01T00:00:01.000Z' WHERE number = 7;");
    screen_when(&env, &pane, "conflict", |s| title(s, "conflict"));

    // `unknown` is not a conflict
    env.sql("UPDATE pull_requests SET mergeable = 'unknown', changed_at = '2026-02-01T00:00:02.000Z' WHERE number = 7;");
    screen_when(&env, &pane, "open again", |s| title(s, "open"));
}

/// A failed snapshot keeps the chart on screen and shows the error in the row below it, for longer than one poll; the
/// view takes the snapshot again at the next refresh, and the error goes when it succeeds (tui-W4). The read is made to
/// fail by renaming the table it reads.
#[test]
fn a_failed_snapshot_keeps_the_chart_and_shows_the_error_until_a_snapshot_succeeds() {
    let env = env();
    env.sql(&plan_with("deploy", Some((7, "none")), &[("a", "merged", "S3", &[], 1)]));
    let pane = start_view(&env);
    let title = |s: &[String]| s.iter().any(|l| l.contains("#7 open 󰏌"));
    screen_when(&env, &pane, "the chart", title);
    let mut store = env.store();
    // not through `Store`, which refuses a changed schema
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("ALTER TABLE jobs RENAME TO jobs_gone;");
    // the snapshot is taken because an Event of the Plan came (the store was opened before the rename)
    store
        .write("fixture", |tx| {
            tx.execute_batch("INSERT INTO events (txn, at, kind, context, plan, text, data)
                 VALUES (cpd_txn(), '2026-02-01T00:00:00.000Z', 'plan-phase', 'ctx', 'ship', 'fixture', '{}');")?;
            Ok::<(), CpdError>(())
        })
        .unwrap();
    screen_when(&env, &pane, "the error", |s| s.iter().any(|l| l.contains("no such table")));
    // time-claim: the poll is 50 ms: an error cleared by the next poll's success (the old behaviour) would be gone
    // within this second
    std::thread::sleep(Duration::from_secs(1));
    let screen = screen(&env, &pane);
    assert!(screen.iter().any(|l| l.contains("no such table")) && title(&screen), "{screen:?}");
    raw("ALTER TABLE jobs_gone RENAME TO jobs;");
    screen_when(&env, &pane, "the error to go", |s| !s.iter().any(|l| l.contains("no such table")) && title(s));
}

/// A follow that fails is shown below the chart, for longer than one snapshot (tui-G4): the highest Event id fell below
/// the cursor, which is what a replaced database looks like. The chart stays.
#[test]
fn a_follow_that_fails_is_shown_below_the_chart() {
    let env = env();
    env.sql(&plan_with("deploy", Some((7, "none")), &[("a", "merged", "S3", &[], 1)]));
    env.sql(&event("plan-phase", "ctx", Some("ship"), None, "{}"));
    let pane = start_view(&env);
    let title = |s: &[String]| s.iter().any(|l| l.contains("#7 open 󰏌"));
    screen_when(&env, &pane, "the chart", title);
    // not through `Store`, which keeps the Events append-only
    let raw = |text: &str| rusqlite::Connection::open(env.cpd_home().join("cpd.db")).unwrap().execute_batch(text).unwrap();
    raw("DROP TRIGGER events_no_delete; DELETE FROM events WHERE id > (SELECT min(id) FROM events);");
    screen_when(&env, &pane, "the error", |s| s.iter().any(|l| l.contains("Event cursor")));
    // time-claim: a snapshot is retaken within 3 s (the refresh): the error stays
    std::thread::sleep(Duration::from_secs(3));
    let screen = screen(&env, &pane);
    assert!(screen.iter().any(|l| l.contains("Event cursor")) && title(&screen), "{screen:?}");
}

#[test]
fn without_a_port_file_the_pending_review_row_has_no_arrow() {
    let env = env();
    env.sql(&plan_under_review(1, "pending"));
    let pane = start_view(&env);
    let first = screen_when(&env, &pane, "the row", |s| s.len() > 3 && s[3].contains(" a│"));
    assert_eq!(first[0], "   Plan review r1 pending", "{}", first.join("\n"));
    assert!(!styled_screen(&env, &pane).contains("localhost"));
    // a port file that holds no port is no port file
    write_port(&env, "not a port\n");
    // The same write that appends the Event stores a conflict in the box title: when the title shows it, the view has
    // read the store (and the port file, written before) again.
    env.sql(&format!(
        "{}UPDATE pull_requests SET mergeable = 'conflicting' WHERE number = 7;",
        event("plan-review", "ctx", Some("ship"), None, "{}")
    ));
    let after = screen_when(&env, &pane, "the re-read", |s| s.iter().any(|l| l.contains("#7 conflict 󰏌")));
    assert_eq!(after[0], "   Plan review r1 pending");
}

#[test]
fn with_no_pending_review_there_is_no_row_and_the_boxes_start_at_the_top() {
    let env = env();
    env.sql(&plan_under_review(1, "rejected"));
    write_port(&env, "19433\n");
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the box", |s| s.len() > 2 && s[2].contains(" a│"));
    assert_eq!(screen[0], "", "{}", screen.join("\n"));
    assert_eq!(screen[1], "  ╭ repo · repo 󱦰 main · #7 open 󰏌 ╮");
    assert!(screen.iter().all(|l| !l.contains("review")), "{}", screen.join("\n"));
    assert!(!styled_screen(&env, &pane).contains("localhost"));
}

#[test]
fn no_lead_in_the_chat_slot_says_so() {
    let env = env();
    env.sql(&(context("ctx") + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z")));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the message", |s| s.iter().any(|l| l.contains("no Lead")));
    // in the message row, the pane's last row, indented by the margin; nothing above it
    assert_eq!(screen.iter().filter(|l| !l.is_empty()).collect::<Vec<_>>(), [&"   no Lead session in the centre".to_owned()], "{}", screen.join("\n"));
    let styled = tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]);
    assert!(styled.contains("2m no Lead session in the centre"), "{styled}");
}

#[test]
fn the_first_rows_of_the_pane_are_the_top_of_the_first_box_with_no_tab() {
    let env = env();
    env.sql(&plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]));
    // a second Lead of the Context with a pane: it used to be a tab
    env.sql(&(lead_row("fork-ship", "ctx", "ship", "2026-01-02T00:00:09.000Z") + &event(
        "session-moved", "ctx", Some("ship"), Some("fork-ship"), r#"{"pane":"%2","window":"@2","place":"parked"}"#)));
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the box", |s| s.len() > 3 && s[2].contains(" a│"));
    // one row of margin, then the box's top border, its one chart row and its bottom border
    assert_eq!(screen[0], "", "{}", screen.join("\n"));
    assert_eq!(screen[1], "  ╭ repo · repo 󱦰 main · dispatch ╮");
    assert!(screen[2].starts_with(&format!("  │ \u{e0b6}{QUEUED} a│S3\u{e0b4}")), "{:?}", screen[2]);
    assert!(screen[3].starts_with("  ╰─"), "{:?}", screen[3]);
    assert!(screen.iter().all(|l| !l.contains('\u{F0142}' /* md-chevron_right */) && !l.contains("name of") && !l.starts_with('─')), "{}", screen.join("\n"));
}

#[test]
fn the_view_follows_events_and_switches_plan_when_the_chat_slot_changes() {
    let env = env();
    let mut s = plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]);
    s += &plan_row("side", "ctx", "dispatch", "lead-side", "2026-01-02T00:00:05.000Z", None);
    s += &lead_row("lead-side", "ctx", "side", "2026-01-02T00:00:05.000Z");
    s += &job_row("side", "other", "queued", "O3");
    env.sql(&s);
    let pane = start_view(&env);
    screen_when(&env, &pane, "Job a", |s| s.len() > 2 && s[2].contains(" a│"));

    // a Job is appended to the shown Plan: its Event makes the view take a new snapshot
    env.sql(
        &(job_row("ship", "b", "queued", "S3")
            + &need_row("ship", "b", "a")
            + &event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":["a"]}"#)),
    );
    let screen = screen_when(&env, &pane, "Job b", |s| s.len() > 2 && s[2].contains(" b│"));
    assert_eq!(chart_rows(&screen, 1), format!(" \u{e0b6}{QUEUED} a│S3\u{e0b4}─────\u{e0b6}{QUEUED} b│S3\u{e0b4}"));

    // the other Lead moves into the chat slot: the view shows its Plan
    env.sql(&in_chat("ctx", "side", "lead-side"));
    let screen = screen_when(&env, &pane, "the other Plan", |s| s.len() > 2 && s[2].contains("other"));
    assert_eq!(chart_rows(&screen, 1), format!(" \u{e0b6}{QUEUED} other│O3\u{e0b4}"));
    assert!(!screen.iter().any(|l| l.contains(&format!("{QUEUED} a│"))), "{}", screen.join("\n"));
}

#[test]
fn each_plan_keeps_its_scroll_offset_while_the_view_runs_and_its_selection_is_reset() {
    let env = env();
    let keys: Vec<String> = (10..60).map(|n| format!("j{n}")).collect();
    let jobs: Vec<(&str, &str, &str, &[&str], u32)> = keys.iter().map(|k| (k.as_str(), "queued", "S3", &[][..], 0)).collect();
    let mut s = plan_with("dispatch", None, &jobs);
    s += &plan_row("side", "ctx", "dispatch", "lead-side", "2026-01-02T00:00:05.000Z", None);
    s += &lead_row("lead-side", "ctx", "side", "2026-01-02T00:00:05.000Z");
    s += &job_row("side", "other", "queued", "O3");
    env.sql(&s);
    let selected = |styled: &str| styled.contains("\x1b[1;7m") || styled.contains("\x1b[7;1m") || styled.contains("\x1b[7m");
    let pane = start_view(&env);
    let top = screen_when(&env, &pane, "the top of the chart", |s| s.len() > 2 && s[2].contains(" j10│"));

    // arrow keys select the pills one by one, and the chart scrolls to keep the selection visible
    let mut down = vec!["send-keys", "-t", &pane];
    down.extend(std::iter::repeat_n("j", 45));
    tmux_out(&env, &down);
    // the view takes the keys one by one: wait for the last one (the 45th pill from j10, j54, selected), then for a screen that holds still
    wait_until("the last pill selected", || {
        tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]).lines().any(|l| l.contains(" j54│") && selected(l))
    });
    let mut scrolled = screen(&env, &pane);
    wait_until("the chart to hold still", || {
        // time-claim: the chart holds still when two screens 300 ms apart are the same; no signal from the view that it is done.
        std::thread::sleep(Duration::from_millis(300));
        let now = screen(&env, &pane);
        std::mem::replace(&mut scrolled, now) == scrolled
    });
    assert!(scrolled.iter().all(|l| !l.contains(" j10│")), "{}", scrolled.join("\n"));
    assert!(selected(&tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane])), "no pill is selected before the switch");

    // another Plan starts at its corner; coming back shows the chart where it was left, with nothing selected
    env.sql(&in_chat("ctx", "side", "lead-side"));
    screen_when(&env, &pane, "the other Plan", |s| s.len() > 2 && s[2].contains("other"));
    env.sql(&in_chat("ctx", "ship", "lead-ship"));
    // a frame reaches the pane in more than one write (the view's stdout flushes every 1024 bytes), so a capture can
    // show the first rows of the chart and nothing below: wait for the whole frame, and compare the last capture
    let back = common::wait_some("the chart where it was left", common::WAIT, || {
        let now = screen(&env, &pane);
        if now == scrolled { Ok(now) } else { Err(now.join("\n")) }
    });
    assert_ne!(back, top);
    assert!(!selected(&tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane])), "a pill is selected");
}

/// A Plan `ship` of 50 independent Jobs, `j10` to `j59`: a chart taller than the pane.
fn tall_plan() -> String {
    let keys: Vec<String> = (10..60).map(|n| format!("j{n}")).collect();
    let jobs: Vec<(&str, &str, &str, &[&str], u32)> = keys.iter().map(|k| (k.as_str(), "queued", "S3", &[][..], 0)).collect();
    plan_with("dispatch", None, &jobs)
}

fn scroll_file(env: &TestEnv) -> std::path::PathBuf {
    env.cpd_home().join("run").join("tui").join("plan-scroll.json")
}

#[test]
fn the_scroll_offset_survives_a_restart_of_the_view_and_the_selection_does_not() {
    let env = env();
    env.sql(&tall_plan());
    let selected = |styled: &str| styled.contains("\x1b[1;7m") || styled.contains("\x1b[7;1m") || styled.contains("\x1b[7m");
    let pane = start_view(&env);
    let top = screen_when(&env, &pane, "the top of the chart", |s| s.len() > 2 && s[2].contains(" j10│"));

    // select down the chart: it scrolls to keep the selection visible
    let mut down = vec!["send-keys", "-t", &pane];
    down.extend(std::iter::repeat_n("j", 45));
    tmux_out(&env, &down);
    wait_until("the last pill selected", || {
        tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane]).lines().any(|l| l.contains(" j54│") && selected(l))
    });
    let mut scrolled = screen(&env, &pane);
    wait_until("the chart to hold still", || {
        // time-claim: the chart has settled when two captures 300 ms apart are the same; stillness has no Event to
        // wait for.
        std::thread::sleep(Duration::from_millis(300));
        let now = screen(&env, &pane);
        std::mem::replace(&mut scrolled, now) == scrolled
    });
    assert!(scrolled.iter().all(|l| !l.contains(" j10│")), "{}", scrolled.join("\n"));

    // the offset is written (at most once a second): wait for the file to hold, then restart the view
    let offset = |env: &TestEnv| -> Option<serde_json::Value> {
        let text = fs::read_to_string(scroll_file(env)).ok()?;
        serde_json::from_str::<serde_json::Value>(&text).ok()?.get("ship").cloned()
    };
    wait_until("the offset in the file", || offset(&env).is_some_and(|o| o[1].as_u64().unwrap_or(0) > 0));
    // time-claim: the view writes the file at most once a second (SAVE_EVERY), so 1.5 s after the first write any
    // write still due for the last scroll has happened; the final offset has no signal of its own.
    std::thread::sleep(Duration::from_millis(1500));
    let written = offset(&env).unwrap();
    tmux_out(&env, &["respawn-pane", "-k", "-t", &pane, CPD, "tui", "view", "plan"]);

    // frames can reach the pane in two writes (hs-n206): poll until the whole screen is there
    let back = screen_when(&env, &pane, "the offset restored", |s| s == scrolled.as_slice());
    assert_ne!(back, top);
    assert!(!selected(&tmux_out(&env, &["capture-pane", "-p", "-e", "-t", &pane])), "a pill is selected");
    assert_eq!(offset(&env).unwrap(), written, "the restart did not move the offset");
    let leftovers: Vec<_> = fs::read_dir(scroll_file(&env).parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(leftovers, ["plan-scroll.json"], "no temporary file is left");
}

#[test]
fn an_invalid_scroll_file_starts_the_view_at_the_top_with_a_note_and_the_next_write_replaces_it() {
    let env = env();
    env.sql(&tall_plan());
    fs::create_dir_all(scroll_file(&env).parent().unwrap()).unwrap();
    fs::write(scroll_file(&env), "{not json").unwrap();
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the note", |s| s.iter().any(|l| l.contains("scroll positions reset: ")));
    assert!(screen[2].contains(" j10│"), "{}", screen.join("\n"));
    assert_eq!(fs::read_to_string(scroll_file(&env)).unwrap(), "{not json", "nothing is written until an offset changes");

    // scroll down the chart: the file becomes a valid one
    let mut down = vec!["send-keys", "-t", &pane];
    down.extend(std::iter::repeat_n("j", 45));
    tmux_out(&env, &down);
    wait_until("the file replaced", || {
        fs::read_to_string(scroll_file(&env)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).is_some_and(|v| v["ship"][1].as_u64().unwrap_or(0) > 0)
    });
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_dead}"]).trim(), "0");
}

#[test]
fn the_view_starts_no_subprocess_and_asks_gh_nothing() {
    let env = env();
    env.sql(&plan_with("deploy", Some((7, "failed")), &[("a", "running", "S3", &[], 1)]));
    let pane = start_view(&env);
    let pid = tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_pid}"]).trim().to_owned();
    // let it draw, follow an Event and take a new snapshot
    screen_when(&env, &pane, "the chart", |s| s.len() > 2 && s[2].contains(" a│"));
    // The Event comes with its Job: when the new pill shows, the view has followed the Event and read the store again.
    env.sql(&format!(
        "{}{}",
        job_row("ship", "b", "queued", "S3"),
        event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":[]}"#)
    ));
    screen_when(&env, &pane, "the new Job", |s| s.iter().any(|l| l.contains(" b│")));
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
    env.sql(&plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1), ("b", "queued", "S3", &["a"], 0)]));
    let (plan, chat) = centre(&env);
    screen_when(&env, &plan, "the chart", |s| s.len() > 2 && s[2].contains(" a│"));

    let workers = || tmux_out(&env, &["list-panes", "-t", "=cpd:main", "-F", "#{@cpd_view} #{@cpd_job} #{@cpd_attempt}"]);
    // nothing is selected until the User asks: an arrow selects the first pill, Enter opens its Worker
    tmux_out(&env, &["send-keys", "-t", &plan, "Right"]);
    tmux_out(&env, &["send-keys", "-t", &plan, "Enter"]);
    wait_until("the Worker pane", || workers().contains("worker a a-a1"));
    // The pane exists before the fake `cpd` has recorded its call: wait for the call.
    let started = ["tui", "view", "worker", "a-a1", "--plan", "ship"].map(str::to_owned).to_vec();
    wait_until("the Worker view to be started", || env.calls("cpd").contains(&started));
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

/// Opening a Worker runs several tmux calls (`toggle_worker`): the view keeps following Events meanwhile (tui-W7). The
/// `tmux` first on `PATH` takes 4 s to answer `split-window`, the call that makes the pane.
#[test]
fn the_chart_keeps_following_events_while_a_worker_pane_is_being_opened() {
    let env = env();
    env.sql(&plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1)]));
    let (plan, _chat) = centre(&env);
    screen_when(&env, &plan, "the chart", |s| s.len() > 2 && s[2].contains(" a│"));
    let real = std::env::var("PATH")
        .unwrap()
        .split(':')
        .map(|dir| std::path::Path::new(dir).join("tmux"))
        .find(|p| p.is_file())
        .expect("tmux on PATH");
    env.fake("tmux", &format!(r#"case " $* " in *" split-window "*) sleep 4;; esac; exec {} "$@""#, real.display()));
    tmux_out(&env, &["send-keys", "-t", &plan, "Right"]);
    tmux_out(&env, &["send-keys", "-t", &plan, "Enter"]);
    wait_until("the split-window call", || env.calls("tmux").iter().any(|c| c.iter().any(|a| a == "split-window")));
    // time-claim: the chart follows the new Job while the popup's split-window (4 s) is still running: within 3 s
    let opening = Instant::now();
    env.sql(
        &(job_row("ship", "b", "queued", "S3")
            + &need_row("ship", "b", "a")
            + &event("job-added", "ctx", Some("ship"), None, r#"{"status":"queued","appended":true,"tier":"S3","needs":["a"]}"#)),
    );
    screen_when(&env, &plan, "Job b", |s| s.len() > 2 && s[2].contains(" b│"));
    // time-claim: within 3 s, while the popup's split-window (4 s) still runs
    assert!(opening.elapsed() < Duration::from_secs(3), "the chart followed at once: {:?}", opening.elapsed());
}

#[test]
fn another_job_retargets_the_worker_pane_in_place() {
    let env = env();
    env.sql(&plan_with("dispatch", None, &[("a", "merged", "O3", &[], 1), ("b", "running", "S3", &["a"], 1)]));
    let (plan, _) = centre(&env);
    screen_when(&env, &plan, "the chart", |s| s.len() > 2 && s[2].contains(" a│"));
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
    // The pane exists before the fake `cpd` has recorded its call: wait for the call.
    let started = ["tui", "view", "worker", "b-a1", "--plan", "ship"].map(str::to_owned).to_vec();
    wait_until("the Worker view to be started", || env.calls("cpd").contains(&started));
}

// ---------- Pulse (a Manager in the chat slot; User, hs-d327) ----------

/// An Event of `plan` in `ctx` that happened `minutes` minutes ago by the clock (the view reads the real time).
fn event_ago(kind: &str, plan: &str, minutes: u32) -> String {
    format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
         VALUES (cpd_txn(), strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{minutes} minutes'), '{kind}', 'ctx', '{plan}', NULL, 'fixture {kind}', '{{}}');"
    )
}

/// The bars of a row of a styled screen with their foreground: the number of the 256-colour palette tmux captures
/// (`1` red, `2` green, `3` yellow, `4` blue, `5` magenta, `8` dark grey, `15` bright white), or `default`.
fn bars_with_colours(styled_row: &str) -> Vec<(char, String)> {
    let (mut out, mut colour, mut rest) = (Vec::new(), "default".to_owned(), styled_row);
    while let Some(c) = rest.chars().next() {
        if let Some(seq) = rest.strip_prefix("\x1b[") {
            let end = seq.find('m').expect("an SGR ends in m");
            let params: Vec<&str> = seq[..end].split(';').collect();
            match params.as_slice() {
                ["38", "5", n] => colour = (*n).to_owned(),
                ["" | "0" | "39"] => colour = "default".to_owned(),
                _ => {}
            }
            rest = &seq[end + 1..];
        } else {
            if ('▁'..='█').contains(&c) {
                out.push((c, colour.clone()));
            }
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

#[test]
fn pulse_rows_have_no_icon_and_bars_take_the_colour_of_their_kind_with_a_grey_low_bar_for_zero() {
    let env = env();
    let mut s = context("ctx") + &lead_row("g-ctx", "ctx", "", "2026-01-01T00:00:00.000Z");
    s += &lead_row("mgr", "ctx", "", "2026-01-01T00:00:01.000Z").replace("'lead'", "'manager'");
    s += &plan_row("p1", "ctx", "dispatch", "lead-p1", "2026-01-02T00:00:00.000Z", None);
    s += &lead_row("lead-p1", "ctx", "p1", "2026-01-02T00:00:00.000Z");
    // five slots, ten minutes apart (two buckets), oldest first; each with a lower-precedence kind beside the winner
    for (minutes, kinds) in [
        (52, &["job-added"][..]),
        (42, &["pr-opened", "job-merged"]),
        (32, &["error", "plan-phase"]),
        (22, &["plan-phase", "job-merged", "job-added"]),
        (12, &["escalation", "job-merged"]),
        (2, &["job-merged"]),
    ] {
        for kind in kinds {
            s += &event_ago(kind, "p1", minutes);
        }
    }
    s += &event("session-moved", "ctx", None, Some("mgr"), r#"{"pane":"%1","window":"@1","place":"chat"}"#);
    env.sql(&s);
    let pane = start_view(&env);

    let screen = screen_when(&env, &pane, "the Pulse row", |s| s.get(1).is_some_and(|l| l.contains("p1") && l.contains('▁')));
    let row = &screen[1];
    assert!(row.starts_with("  p1  "), "the id first, no dot: {row:?}");
    assert!(!row.contains('󰗠') && !row.contains('󰪥') && !row.contains('󰝦'), "no icon: {row:?}");
    let age = row.chars().rev().take(9).collect::<String>();
    assert!(age.trim().chars().next().is_some(), "the age cell stays: {row:?}");
    // the pane is 300 wide (start_view); 2 columns of margin on each side
    let n = 300 - 4 - ("p1".len() + 2 + 2 + 9);
    assert_eq!(row.chars().filter(|c| ('▁'..='█').contains(c)).count(), n, "the sparkline takes the width the icons freed: {row:?}");

    let styled = styled_screen(&env, &pane);
    let bars = bars_with_colours(styled.lines().nth(1).expect("the first row"));
    assert_eq!(bars.len(), n);
    let coloured: Vec<&str> = bars.iter().filter(|(_, c)| c != "8").map(|(_, c)| c.as_str()).collect();
    assert_eq!(coloured, ["15", "3", "1", "5", "4", "2"], "white, yellow, red, magenta, blue, green: {bars:?}");
    assert!(bars.iter().filter(|(_, c)| c == "8").all(|(g, _)| *g == '▁'), "an empty slot is a grey ▁: {bars:?}");
    assert!(bars.iter().any(|(_, c)| c == "8"), "{bars:?}");
}

#[test]
fn q_does_not_quit_the_view() {
    let env = env();
    env.sql(&plan_with("dispatch", None, &[("a", "queued", "S3", &[], 0)]));
    let pane = start_view(&env);
    screen_when(&env, &pane, "the chart", |s| s.len() > 2 && s[2].contains(" a│"));
    // The view handles keys in order: once `j` has selected the pill, `q` before it has been handled.
    tmux_out(&env, &["send-keys", "-t", &pane, "q", "j"]);
    wait_until("the pill selected", || {
        let styled = styled_screen(&env, &pane);
        styled.contains("\x1b[1;7m") || styled.contains("\x1b[7;1m") || styled.contains("\x1b[7m")
    });
    assert_eq!(tmux_out(&env, &["display", "-p", "-t", &pane, "#{pane_dead}"]).trim(), "0");
    assert!(screen(&env, &pane)[2].contains(" a│"));
}

#[test]
fn without_a_terminal_it_is_refused_before_anything_is_read() {
    let env = env();
    let run = env.cpd(&["tui", "view", "plan"]);
    assert_eq!((run.exit, run.error()["code"].clone()), (3, json!("not_a_terminal")), "{run:?}");
}
