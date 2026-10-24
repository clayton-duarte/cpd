//! The `plan` view shows cancelled Jobs and PRs (commands.md §13.2, hs-d518): a cancelled Job keeps its place in
//! its PR's chart, muted (the palette's `Meaning::Nothing` grey), with its status `cancelled` and its reason; the
//! needs on it are not drawn; a cancelled PR gets its box, muted, after the live ones, with its Jobs.
//!
//! The view runs as `cpd tui view plan` on a private tmux server, as in `tui_plan`; the store is prepared with SQL,
//! and the Jobs and PRs are cancelled with `job cancel` and `pr cancel`. The fixture helpers are shared with
//! `tui_plan_waiting`.

use crate::common;

use std::fs;

use common::{CPD, TestEnv, wait_until};
use cpd_tui::palette::CANCELLED;

pub(crate) const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// An env whose config has the private tmux server, with a fake `gh` and `pi` that record any call.
pub(crate) fn env() -> TestEnv {
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

/// Starts `cpd tui view plan` alone in a 300 × 40 window of the private server; returns its pane.
pub(crate) fn start_view(env: &TestEnv) -> String {
    tmux_out(env, &["new-session", "-d", "-s", "view", "-x", "300", "-y", "40", "-P", "-F", "#{pane_id}", CPD, "tui", "view", "plan"]).trim().to_owned()
}

pub(crate) fn screen(env: &TestEnv, pane: &str) -> Vec<String> {
    tmux_out(env, &["capture-pane", "-p", "-t", pane]).lines().map(|l| l.trim_end().to_owned()).collect()
}

/// Waits until the screen satisfies `ok`, and returns it.
pub(crate) fn screen_when(env: &TestEnv, pane: &str, what: &str, ok: impl Fn(&[String]) -> bool) -> Vec<String> {
    let mut last = Vec::new();
    wait_until(what, || {
        last = screen(env, pane);
        ok(&last)
    });
    last
}

/// The screen with its escape sequences (`capture-pane -e`).
pub(crate) fn styled_screen(env: &TestEnv, pane: &str) -> String {
    tmux_out(env, &["capture-pane", "-p", "-e", "-t", pane])
}

/// Every visible character of a styled row with its foreground: the number of the 256-colour palette tmux captures
/// (`8` dark grey, the colour of `Meaning::Nothing`), or `default`.
pub(crate) fn chars_with_colours(styled_row: &str) -> Vec<(char, String)> {
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
            out.push((c, colour.clone()));
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The colours of `word` where it first appears in the styled screen's rows: `None` when no row has it.
pub(crate) fn colours_of(styled: &str, word: &str) -> Option<Vec<String>> {
    for row in styled.lines() {
        let chars = chars_with_colours(row);
        let text: String = chars.iter().map(|(c, _)| *c).collect();
        if let Some(at) = text.find(word) {
            let start = text[..at].chars().count();
            return Some(chars[start..start + word.chars().count()].iter().map(|(_, c)| c.clone()).collect());
        }
    }
    None
}

/// A fixture Job: `(pr, key, status, kind, needs)`; `status` is `queued`, `merged` or `cancelled` (written `queued`, then cancelled).
pub(crate) type FixtureJob<'a> = (&'a str, &'a str, &'a str, &'a str, &'a [&'a str]);

/// Context `ctx`, Plan `ship` in `dispatch` whose Lead is in the chat slot, the started PRs `prs` (`(key, repo dir)`) and
/// `jobs`; then every `cancelled` Job is cancelled with `job cancel --reason "<key> is not needed"`.
pub(crate) fn plan(env: &TestEnv, prs: &[(&str, &str)], jobs: &[FixtureJob]) {
    let mut s = String::from(
        "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'Title', NULL, '2026-01-01T00:00:00.000Z', 'm1');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('ship', 'ctx', 'Plan ship', 'dispatch', 'lead-ship', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');",
    );
    for (key, dir) in prs {
        s += &format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
             VALUES ('ship', '{key}', '/{dir}', 'main', 'dispatch', 'cpd/ship/{key}', '/wt/{key}', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
        );
    }
    for (pr, key, status, kind, _) in jobs {
        let merged = if *status == "merged" { "'abc123'" } else { "NULL" };
        let status = if *status == "cancelled" { "queued" } else { status };
        s += &format!(
            "INSERT INTO jobs (plan, pr, key, kind, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                               created_txn, merged_sha, created_at)
             VALUES ('ship', '{pr}', '{key}', '{kind}', 'Job {key}', 'S3', 'Goal.', 'Why.', '[\"x\"]', '[\"y\"]', '[]', '{status}', 0, '{FP}',
                     cpd_txn(), {merged}, '2026-01-02T00:00:00.000Z');"
        );
    }
    for (_, key, _, _, needs) in jobs {
        for need in *needs {
            s += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('ship', '{key}', '{need}');");
        }
    }
    // the Lead in the chat slot
    s += "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
          VALUES ('lead-ship', 'lead', 'ctx', 'ship', NULL, 'name', '/sessions/lead-ship.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                  '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
          INSERT INTO events (txn, at, kind, context, plan, session, text, data)
          VALUES (cpd_txn(), '2026-01-03T00:00:00.000Z', 'session-moved', 'ctx', 'ship', 'lead-ship', 'fixture',
                  '{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}');";
    env.sql(&s);
    for (_, key, status, _, _) in jobs {
        if *status == "cancelled" {
            env.cpd(&["job", "cancel", key, "--reason", &format!("{key} is not needed"), "--plan", "ship"]).ok();
        }
    }
}

/// The rows of the box titled with `title` (its title row to its bottom border), the box's borders cut off.
fn box_inside(screen: &[String], title: &str) -> Vec<String> {
    let top = screen.iter().position(|l| l.trim_start().starts_with("╭─ ") && l.contains(title)).unwrap_or_else(|| panic!("no box {title}:\n{}", screen.join("\n")));
    screen[top + 1..]
        .iter()
        .take_while(|l| !l.trim_start().starts_with('╰'))
        .map(|l| l.chars().skip(3).collect::<String>().trim_end().trim_end_matches('│').to_owned())
        .collect()
}

#[test]
fn a_cancelled_job_pill_is_colour_8_and_nothing_follows_it() {
    let env = env();
    plan(&env, &[("api", "api")], &[("api", "a", "cancelled", "work", &[])]);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.iter().any(|l| l.contains(" a│")));
    let text = screen.join("\n");
    // the whole chart row is the pill, to its closing cap: no status word, no reason
    let row = screen.iter().find(|l| l.contains(" a│")).unwrap();
    let inside: String = row.chars().skip(2).collect::<String>().trim_end().trim_end_matches('│').trim_end().to_owned();
    assert_eq!(inside, format!(" \u{e0b6}{CANCELLED} a│S3\u{e0b4}"), "{text}");
    assert!(!text.contains("cancelled") && !text.contains("not needed"), "{text}");
    let styled = styled_screen(&env, &pane);
    // the pill is colour 8, inverted (contract §6.1.1: a cancelled Job's pill): no colour 0 anywhere
    let colours = colours_of(&styled, "a│S3").unwrap_or_else(|| panic!("the pill is not on the styled screen:\n{styled}"));
    assert!(colours.iter().all(|c| c == "8"), "{colours:?}");
}

#[test]
fn cancelled_pr_box_is_drawn_muted_after_live_ones() {
    let env = env();
    // `aaa` sorts first by key: it is drawn after `web` only because it is cancelled
    plan(&env, &[("aaa", "aaa-repo"), ("web", "web-repo")], &[("aaa", "o1", "cancelled", "work", &[]), ("web", "w", "queued", "work", &[])]);
    env.cpd(&["pr", "cancel", "aaa", "--plan", "ship"]).ok();
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the boxes", |s| s.iter().any(|l| l.contains("web ·")));
    let text = screen.join("\n");
    let row_of = |needle: &str| screen.iter().position(|l| l.trim_start().starts_with("╭─ ") && l.contains(needle));
    let (web, aaa) = (row_of("web ·").expect("the live box"), row_of("aaa ·").unwrap_or_else(|| panic!("the cancelled PR has its box:\n{text}")));
    assert!(web < aaa, "the cancelled box is below the live one:\n{text}");
    assert!(screen[aaa].contains("cancelled"), "its title says so: {}", screen[aaa]);
    assert!(box_inside(&screen, "aaa ·").iter().any(|l| l.contains("o1")), "with its Jobs:\n{text}");
    let styled = styled_screen(&env, &pane);
    let colours = colours_of(&styled, "cancelled").expect("cancelled on the styled screen");
    assert!(colours.iter().all(|c| c == "8"), "the status is muted: {colours:?}");
    let title = colours_of(&styled, "aaa ·").expect("the title");
    assert!(title.iter().filter(|c| *c == "8").count() >= 3, "the cancelled PR's key is muted: {title:?}");
}

#[test]
fn needs_on_a_cancelled_job_are_not_drawn() {
    let env = env();
    // `b` needs `gone`, which is cancelled: the need counts for nothing
    plan(&env, &[("api", "api")], &[("api", "gone", "cancelled", "work", &[]), ("api", "b", "queued", "work", &["gone"])]);
    let pane = start_view(&env);
    let screen = screen_when(&env, &pane, "the chart", |s| s.iter().any(|l| l.contains(" b│")));
    let text = screen.join("\n");
    assert!(text.contains("gone"), "the cancelled Job is drawn:\n{text}");
    let inside = box_inside(&screen, "api ·").join("\n");
    assert!(
        !inside.contains(['─', '┬', '┴', '├', '┤', '┼', '╭', '╮', '╰', '╯', '󱦳']),
        "no arrow or link between the pills:\n{text}"
    );
}
