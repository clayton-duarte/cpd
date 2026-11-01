//! E8, a repeated ask (contract §3.7 "A repeated ask"): a Job that repeats a question nobody answers is ONE message, ever,
//! to the live Manager of its Context. Each test prepares a store and ticks `inbox::run`.

use super::inbox;
use super::inbox_tests::{fx, manager, messages, open_items, tick};
use super::testkit::{Fx, ago};

/// Job `key` of Plan `pay`, queued.
fn job(fx: &mut Fx, key: &str) {
    fx.add_job(key, &[], "");
}

/// Job `key` of Plan `pay` in `status`.
fn job_in(fx: &mut Fx, key: &str, status: &str) {
    let then = match status {
        "failed" => "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'JOB';
                     UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE plan = 'pay' AND key = 'JOB'",
        "queued" => "",
        "running" => "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'JOB'",
        "merged" => "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'JOB';
                     UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'pay' AND key = 'JOB'",
        "cancelled" => "UPDATE jobs SET status = 'cancelled', cancel_reason = 'r', cancel_requested_at = 't' WHERE plan = 'pay' AND key = 'JOB'",
        other => panic!("{other}"),
    };
    fx.add_job(key, &[], &then.replace("JOB", key));
}

/// An ended Attempt `<job>-a<number>` of `job` in `plan`.
fn attempt(fx: &mut Fx, plan: &str, job: &str, number: u32) {
    fx.fixture(&format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, started_at, ended_at)
           VALUES ('{plan}', '{job}-a{number}', '{job}', {number}, 'S2', 'ladder', 'ended', 'failed', 'no_submit',
                   'cpd/{plan}--{job}-a{number}', '/w/{plan}-{job}-a{number}', 't', 't');"
    ));
}

/// Escalation `<attempt>-q<number>` of `plan`, asked `minutes` ago, in `status`.
fn ask(fx: &mut Fx, plan: &str, attempt: &str, number: u32, question: &str, status: &str, minutes: i64) {
    let (cols, vals) = match status {
        "expired" => (", expired_at", format!(", '{}'", ago(0))),
        "answered" => (", answer, answered_at", format!(", 'No.', '{}'", ago(0))),
        _ => ("", String::new()),
    };
    fx.fixture(&format!(
        "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at{cols})
           VALUES ('{plan}', '{attempt}-q{number}', '{attempt}', {number}, '{}', '{status}', 1, '{}'{vals});",
        question.replace('\'', "''"),
        ago(minutes)
    ));
}

/// Job `key` of `pay` with one Attempt `a1` and the `questions` as expired Escalations asked `minutes` ago, `minutes - 1`...
fn expired(fx: &mut Fx, key: &str, questions: &[&str], minutes: i64) {
    attempt(fx, "pay", key, 1);
    for (i, q) in questions.iter().enumerate() {
        ask(fx, "pay", &format!("{key}-a1"), i as u32 + 1, q, "expired", minutes - i as i64);
    }
}

fn text(plan: &str, job: &str, n: usize, a: usize, m: i64, q: &str) -> String {
    format!(
        "Job {plan}/{job} asked the same question {n} times over {a} Attempt(s), none answered, the first {m} min ago: \"{q}\". \
         Nudge its Lead (cpd session send {plan} \"<text>\" --unblocks \"<reason>\"), or ask the User."
    )
}

fn only(fx: &mut Fx, plan: &str, job: &str, n: usize, a: usize, m: i64, q: &str) {
    assert_eq!(messages(fx), [("mgr1".to_owned(), None, true, text(plan, job, n, a, m, q))]);
}

/// `prefix` padded with `k` to `n` characters, then `tail`.
fn pad(prefix: &str, n: usize, tail: &str) -> String {
    let mut s = prefix.to_owned();
    while s.chars().count() < n {
        s.push('k');
    }
    s + tail
}

fn told(fx: &mut Fx, job: &str) -> usize {
    let head = format!("Job pay/{job} asked ");
    messages(fx).iter().filter(|m| m.3.starts_with(&head)).count()
}

#[test]
fn three_unanswered_equal_asks_of_one_job_are_one_message_to_its_manager() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    attempt(&mut fx, "pay", "api", 1);
    attempt(&mut fx, "pay", "api", 2);
    ask(&mut fx, "pay", "api-a1", 1, "Which schema?", "expired", 45);
    ask(&mut fx, "pay", "api-a1", 2, "Which schema?", "expired", 40);
    ask(&mut fx, "pay", "api-a2", 1, "Which schema?", "expired", 30);

    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 2, 45, "Which schema?");
    assert_eq!(open_items(&mut fx), []);
}

#[test]
fn two_equal_asks_send_nothing() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    expired(&mut fx, "api", &["Which schema?", "Which schema?"], 20);
    tick(&mut fx);
    assert_eq!(messages(&mut fx), []);

    ask(&mut fx, "pay", "api-a1", 3, "Which schema?", "expired", 10);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}

#[test]
fn answered_asks_do_not_count() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    expired(&mut fx, "api", &["Which schema?", "Which schema?"], 20);
    ask(&mut fx, "pay", "api-a1", 3, "Which schema?", "answered", 10);
    tick(&mut fx);
    assert_eq!(messages(&mut fx), []);

    ask(&mut fx, "pay", "api-a1", 4, "Which schema?", "expired", 5);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}

#[test]
fn an_open_ask_counts_with_the_expired_ones() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    expired(&mut fx, "api", &["Which schema?", "Which schema?"], 20);
    ask(&mut fx, "pay", "api-a1", 3, "Which schema?", "open", 10);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}

#[test]
fn the_key_is_the_first_sixty_characters_with_digits_masked() {
    let mut fx = fx();
    manager(&mut fx);
    let long = pad("long ", 60, "");
    let uni = pad("caf\u{e9} ", 59, "");
    for j in ["tail", "digit", "count", "char", "bytes", "uni"] {
        job(&mut fx, j);
    }
    expired(&mut fx, "tail", &[&format!("{long}aaa"), &format!("{long}bbb"), &format!("{long}ccc")], 30);
    expired(&mut fx, "digit", &["round 1", "round 2", "round 3"], 30);
    expired(&mut fx, "count", &["round 9", "round 10", "round 11"], 30);
    expired(&mut fx, "char", &["round x", "round y", "round z"], 30);
    // The 60th character differs: a cut of 60 bytes would drop it (the é takes two) and make the keys equal.
    expired(&mut fx, "bytes", &[&format!("{uni}x"), &format!("{uni}y"), &format!("{uni}z")], 30);
    expired(&mut fx, "uni", &[&format!("{uni}x{uni}"), &format!("{uni}x{uni}"), &format!("{uni}x")], 30);

    tick(&mut fx);
    let all = messages(&mut fx);
    assert_eq!(told(&mut fx, "tail"), 1, "{all:?}");
    assert_eq!(told(&mut fx, "digit"), 1, "{all:?}");
    assert_eq!(told(&mut fx, "count"), 0, "{all:?}");
    assert_eq!(told(&mut fx, "char"), 0, "{all:?}");
    assert_eq!(told(&mut fx, "bytes"), 0, "{all:?}");
    assert_eq!(told(&mut fx, "uni"), 1, "{all:?}");
    let q: String = format!("{uni}x{uni}").chars().take(60).collect();
    assert!(all.iter().any(|m| m.3 == text("pay", "uni", 3, 1, 30, &q)), "{all:?}");
    assert_eq!(all.len(), 3, "{all:?}");
}

/// A second Plan `shop` with its own Lead and a queued Job `api` (explore kind, no Workflow).
fn shop(fx: &mut Fx) {
    fx.fixture(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead2', 'lead', 'ctx', 'shop', 'Lead2', '/', 'O3', 'working', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('shop', 'ctx', 'Shop', 'dispatch', 'lead2', 't', 't');
         INSERT INTO jobs (plan, pr, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at)
           VALUES ('shop', NULL, 'api', 'scout', 'Title', 'H2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'queued', 0,
                   hex(zeroblob(32)), cpd_txn(), 't');",
    );
}

#[test]
fn equal_ids_and_job_keys_in_two_plans_are_never_one_group() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    shop(&mut fx);
    expired(&mut fx, "api", &["Which schema?", "Which schema?"], 20);
    attempt(&mut fx, "shop", "api", 1);
    ask(&mut fx, "shop", "api-a1", 1, "Which schema?", "expired", 20);
    ask(&mut fx, "shop", "api-a1", 2, "Which schema?", "expired", 19);
    tick(&mut fx);
    assert_eq!(messages(&mut fx), []);

    ask(&mut fx, "pay", "api-a1", 3, "Which schema?", "expired", 10);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}

#[test]
fn a_merged_or_cancelled_job_sends_nothing_and_a_live_one_does() {
    for status in ["merged", "cancelled"] {
        let mut fx = fx();
        manager(&mut fx);
        job_in(&mut fx, "api", status);
        expired(&mut fx, "api", &["Which schema?"; 3], 20);
        tick(&mut fx);
        assert_eq!(messages(&mut fx), [], "{status}");
    }
    for status in ["queued", "running", "failed"] {
        let mut fx = fx();
        manager(&mut fx);
        job_in(&mut fx, "api", status);
        expired(&mut fx, "api", &["Which schema?"; 3], 20);
        tick(&mut fx);
        only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
    }
}

#[test]
fn with_no_live_manager_nothing_is_written_and_a_later_manager_is_told() {
    let mut fx = fx();
    job(&mut fx, "api");
    expired(&mut fx, "api", &["Which schema?"; 3], 20);
    tick(&mut fx);
    assert_eq!(messages(&mut fx), []);
    assert_eq!(fx.count("session-message"), 0);

    manager(&mut fx);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}

#[test]
fn only_the_manager_of_the_jobs_context_is_told() {
    let mut fx = fx();
    fx.fixture(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('other', 'Other', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr2', 'manager', 'other', 'Mgr2', '/', 'O3', 'idle', 't', 't');",
    );
    job(&mut fx, "api");
    expired(&mut fx, "api", &["Which schema?"; 3], 20);
    tick(&mut fx);
    assert_eq!(messages(&mut fx), []);

    manager(&mut fx);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}

#[test]
fn a_job_with_two_repeated_questions_is_one_message_naming_the_larger_group() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    attempt(&mut fx, "pay", "api", 1);
    for i in 1..=4 {
        ask(&mut fx, "pay", "api-a1", i, "Which schema?", "expired", 30 - i as i64);
    }
    for i in 5..=7 {
        ask(&mut fx, "pay", "api-a1", i, "Which port?", "expired", 40 - i as i64);
    }
    tick(&mut fx);
    only(&mut fx, "pay", "api", 4, 1, 29, "Which schema?");

    // A tie: the group asked first is named.
    let mut fx = self::fx();
    manager(&mut fx);
    job(&mut fx, "api");
    attempt(&mut fx, "pay", "api", 1);
    for i in 1..=3 {
        ask(&mut fx, "pay", "api-a1", i, "Which schema?", "expired", 30 - i as i64);
    }
    for i in 4..=6 {
        ask(&mut fx, "pay", "api-a1", i, "Which port?", "expired", 50 - i as i64);
    }
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 46, "Which port?");
}

#[test]
fn an_explore_job_that_repeats_a_question_is_told_too() {
    let mut fx = fx();
    manager(&mut fx);
    fx.fixture(
        "INSERT INTO jobs (plan, pr, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended,
                           fingerprint, created_txn, created_at)
         VALUES ('pay', NULL, 'scout1', 'scout', 'Title', 'H2', 'Goal.', 'Because.', '[\"a\"]', '[\"ok\"]', '[]', 'queued', 0,
                 hex(zeroblob(32)), cpd_txn(), 't');",
    );
    expired(&mut fx, "scout1", &["Which schema?"; 3], 20);
    tick(&mut fx);
    only(&mut fx, "pay", "scout1", 3, 1, 20, "Which schema?");
}

#[test]
fn a_later_tick_a_restarted_engine_and_a_fourth_ask_send_nothing_more() {
    let mut fx = fx();
    manager(&mut fx);
    job(&mut fx, "api");
    expired(&mut fx, "api", &["Which schema?"; 3], 20);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");

    tick(&mut fx);
    let mut again = fx.restarted();
    inbox::run(&mut again);
    ask(&mut fx, "pay", "api-a1", 4, "Which schema?", "expired", 5);
    tick(&mut fx);
    only(&mut fx, "pay", "api", 3, 1, 20, "Which schema?");
}
