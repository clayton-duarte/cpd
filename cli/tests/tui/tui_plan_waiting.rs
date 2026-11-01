//! The `plan` view draws the needs of a Job as edges and says nothing after its pill (commands.md §13.2, contract
//! P2, P7); the open Job's needs are the PR's other Jobs `cpd job show` lists (`shown_needs`).

use cpd_tui::palette::{MERGED, QUEUED};

use crate::tui_plan_cancelled::{draw, env, plan, rows};

/// The chart rows of the `api` box: those between its top and bottom borders, the borders and the padding cut off.
fn chart(screen: &[String]) -> Vec<String> {
    let top = screen
        .iter()
        .position(|l| l.trim_start().starts_with("╭─ ") && l.contains("api ·"))
        .unwrap_or_else(|| panic!("no box:\n{}", screen.join("\n")));
    screen[top + 1..]
        .iter()
        .take_while(|l| !l.trim_start().starts_with('╰'))
        .map(|l| {
            l.chars()
                .skip(2)
                .collect::<String>()
                .trim_end()
                .trim_end_matches('│')
                .trim_end()
                .to_owned()
        })
        .collect()
}

fn chart_of(jobs: &[crate::tui_plan_cancelled::FixtureJob]) -> Vec<String> {
    let fx = env();
    plan(&fx, &[("api", "api")], jobs);
    chart(&rows(&draw(&fx)))
}

#[test]
fn a_waiting_job_pill_is_not_followed_by_a_note() {
    let fx = env();
    plan(
        &fx,
        &[("api", "api")],
        &[
            ("api", "a", "queued", "work", &[]),
            ("api", "b", "queued", "work", &[]),
            ("api", "m", "merged", "work", &[]),
            ("api", "c", "queued", "work", &["a", "b", "m"]),
        ],
    );
    let screen = rows(&draw(&fx));
    assert!(
        screen.iter().all(|l| !l.contains("waiting")),
        "{}",
        screen.join("\n")
    );
    // every pill row ends at the closing cap of its last pill
    for row in chart(&screen).iter().filter(|r| r.contains('\u{e0b6}')) {
        assert!(
            row.ends_with('\u{e0b4}') || row.contains(['─', '┐', '┘', '┤', '┬', '┴', '├', '┼']),
            "{row:?}"
        );
    }
    let c = chart(&screen)
        .into_iter()
        .find(|r| r.contains(" c│"))
        .unwrap();
    assert!(
        c.ends_with("S3\u{e0b4}"),
        "the row of c is its pill, nothing after it: {c:?}"
    );
}

#[test]
fn the_open_job_is_drawn_with_needs_edges_from_its_prs_other_jobs() {
    // a and b are independent; the open Job is joined from both, as a Job `z` with the stored needs a and b is
    let open = chart_of(&[
        ("api", "a", "queued", "work", &[]),
        ("api", "b", "queued", "work", &[]),
        ("api", "open-api", "queued", "open", &[]),
    ]);
    let work = chart_of(&[
        ("api", "a", "queued", "work", &[]),
        ("api", "b", "queued", "work", &[]),
        ("api", "z", "queued", "work", &["a", "b"]),
    ]);
    assert_eq!(
        open.join("\n").replace("open-api│S3", "KEY│TIER"),
        work.join("\n").replace("z│S3", "KEY│TIER")
    );
    assert!(
        open.iter()
            .all(|r| !r.contains("waiting") && !r.contains(" on ")),
        "{open:?}"
    );
    assert_eq!(
        open,
        [
            format!(" \u{e0b6}{QUEUED} a│S3\u{e0b4}──╮"),
            format!("           ├──\u{e0b6}{QUEUED} open-api│S3\u{e0b4}"),
            format!(" \u{e0b6}{QUEUED} b│S3\u{e0b4}──╯"),
        ]
    );

    // a merged Job is among the edges, as `cpd job show` lists it
    let open = chart_of(&[
        ("api", "a", "queued", "work", &[]),
        ("api", "m", "merged", "work", &[]),
        ("api", "open-api", "queued", "open", &[]),
    ]);
    let work = chart_of(&[
        ("api", "a", "queued", "work", &[]),
        ("api", "m", "merged", "work", &[]),
        ("api", "z", "queued", "work", &["a", "m"]),
    ]);
    assert_eq!(
        open.join("\n").replace("open-api│S3", "KEY│TIER"),
        work.join("\n").replace("z│S3", "KEY│TIER")
    );
    assert_eq!(
        open,
        [
            format!(" \u{e0b6}{QUEUED} a│S3\u{e0b4}──╮"),
            format!("           ├──\u{e0b6}{QUEUED} open-api│S3\u{e0b4}"),
            format!(" \u{e0b6}{MERGED} m│S3\u{e0b4}──╯"),
        ]
    );
}

#[test]
fn ready_job_stays_queued() {
    let fx = env();
    plan(
        &fx,
        &[("api", "api")],
        &[("api", "a", "queued", "work", &[])],
    );
    let screen = rows(&draw(&fx));
    // `a` is ready to start now: its pill is the queued one, and nothing says it waits for anything
    let a = screen.iter().find(|l| l.contains(" a│")).expect("a's pill");
    assert!(a.contains(cpd_tui::palette::QUEUED), "{a}");
    assert!(
        screen.iter().all(|l| !l.contains("waiting")),
        "{}",
        screen.join("\n")
    );
}
