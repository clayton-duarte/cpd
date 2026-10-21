//! The `plan` view shows work that cannot start yet as `waiting on <keys>` (commands.md §13.2, hs-d521, hs-d523);
//! `queued` stays for work that is ready to start now.

use crate::tui_plan_cancelled::{env, plan, screen_when, start_view};

#[test]
fn job_with_unmerged_needs_shows_waiting_on_them() {
    let env = env();
    plan(
        &env,
        &[("api", "api")],
        &[
            ("api", "a", "queued", "work", &[]),
            ("api", "b", "queued", "work", &[]),
            ("api", "m", "merged", "work", &[]),
            ("api", "c", "queued", "work", &["a", "b", "m"]),
        ],
    );
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.iter().any(|l| l.contains(" c│")));
    // the merged need blocks nothing
    assert!(screen.iter().any(|l| l.contains("waiting on a, b") && !l.contains("waiting on a, b, m")), "{}", screen.join("\n"));
}

#[test]
fn open_job_shows_waiting_on_its_prs_other_jobs() {
    let env = env();
    plan(
        &env,
        &[("api", "api")],
        &[("api", "a", "queued", "work", &[]), ("api", "m", "merged", "work", &[]), ("api", "open-api", "queued", "open", &[])],
    );
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.iter().any(|l| l.contains("open-api│")));
    assert!(screen.iter().any(|l| l.contains("waiting on a") && !l.contains("waiting on a, m")), "{}", screen.join("\n"));
}

#[test]
fn ready_job_stays_queued() {
    let env = env();
    plan(&env, &[("api", "api")], &[("api", "a", "queued", "work", &[])]);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.iter().any(|l| l.contains(" a│")));
    // `a` is ready to start now: its pill is the queued one, and nothing says it waits for anything
    let a = screen.iter().find(|l| l.contains(" a│")).expect("a's pill");
    assert!(a.contains(cpd_tui::palette::QUEUED), "{a}");
    assert!(screen.iter().all(|l| !l.contains("waiting")), "{}", screen.join("\n"));
}
