//! Bare `context show` is bounded to the records in force and `--all` renders the full history (commands.md §3.3;
//! Context records pro-d3 and pro-d1). The tests run the command tree in this process (`Fx`).

use crate::common;

use crate::context_on_demand::{CTX, curate, fast};
use common::TestEnv;
use common::fx::Fx;
use serde_json::Value;

/// Inserts a record of `kind` written `minutes` minutes ago; `extra` names the `supersedes` or `question` column.
fn seed_with(
    env: &TestEnv,
    id: &str,
    kind: &str,
    minutes: u32,
    text: &str,
    extra: Option<(&str, &str)>,
) {
    let (column, value) = extra.map_or((String::new(), String::new()), |(c, v)| {
        (format!(", {c}"), format!(", '{v}'"))
    });
    env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text{column})
         VALUES ('{CTX}', '{id}', '{kind}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{minutes} minutes'), 'm1', '{text}'{value})"
    ));
}

fn seed(env: &TestEnv, id: &str, kind: &str, minutes: u32, text: &str) {
    seed_with(env, id, kind, minutes, text, None);
}

/// A Plan `plan` of `CTX` with a `plan` record `m1-p<n>` and a `brief` record `m1-b<n>`; done when `done`.
fn seed_plan(env: &TestEnv, n: u32, plan: &str, done: bool) {
    let (phase, done_at) = if done {
        ("done", "'t'")
    } else {
        ("dispatch", "NULL")
    };
    env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-{plan}', 'lead', '{CTX}', '{plan}', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('{plan}', '{CTX}', 'The {plan} plan', '{phase}', 'lead-{plan}', 't', 't', {done_at});
         INSERT INTO context_records (context, id, kind, at, machine, text, plan_id, plan_machine)
           VALUES ('{CTX}', 'm1-p{n}', 'plan', '2026-10-14T21:12:00.000Z', 'm1', 'The {plan} plan', '{plan}', 'm1'),
                  ('{CTX}', 'm1-b{n}', 'brief', '2026-10-14T21:12:00.000Z', 'm1', 'Brief of {plan}', '{plan}', 'm1');"
    ));
}

fn show(fx: &Fx, extra: &[&str]) -> Value {
    let mut args = vec!["context", "show", "--context", CTX];
    args.extend_from_slice(extra);
    fx.cpd(&args).ok()
}

fn markdown(out: &Value) -> String {
    out["markdown"]
        .as_str()
        .unwrap_or_else(|| panic!("a markdown in {out}"))
        .to_owned()
}

fn pos(md: &str, needle: &str) -> usize {
    md.find(needle)
        .unwrap_or_else(|| panic!("{needle:?} in {md}"))
}

fn ids(out: &Value) -> Vec<String> {
    out["records"]
        .as_array()
        .unwrap_or_else(|| panic!("records in {out}"))
        .iter()
        .map(|r| r["id"].as_str().unwrap().to_owned())
        .collect()
}

/// One of each kind in force, plus a superseded decision, an answered question, a superseded note and a done Plan.
fn history(fx: &Fx) {
    let env = &fx.env;
    seed(env, "m1-d1", "decision", 100, "Old decision");
    seed_with(
        env,
        "m1-d2",
        "decision",
        99,
        "Current decision",
        Some(("supersedes", "m1-d1")),
    );
    seed(env, "m1-q1", "question", 98, "Answered question text");
    seed_with(
        env,
        "m1-a1",
        "answer",
        97,
        "The given answer",
        Some(("question", "m1-q1")),
    );
    seed(env, "m1-q2", "question", 96, "Open question text");
    seed(env, "m1-n1", "note", 95, "Superseded note text");
    seed_with(
        env,
        "m1-n2",
        "note",
        94,
        "Current note text",
        Some(("supersedes", "m1-n1")),
    );
    seed_plan(env, 1, "finished", true);
    seed_plan(env, 2, "running", false);
}

#[test]
fn bare_show_orders_open_questions_decisions_and_notes_before_plans() {
    let fx = fast();
    history(&fx);
    let md = markdown(&show(&fx, &[]));
    let order: Vec<usize> = ["## Open questions", "## Decisions", "## Notes", "## Plans"]
        .iter()
        .map(|h| pos(&md, h))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{md}");
    assert!(md.starts_with("# Payments\n"), "{md}");
    assert!(
        md.contains("Open question text") && md.contains("Current decision"),
        "{md}"
    );
}

#[test]
fn bare_show_collapses_history_to_count_lines() {
    let fx = fast();
    history(&fx);
    let out = show(&fx, &[]);
    let md = markdown(&out);
    for gone in [
        "Old decision",
        "Answered question text",
        "The given answer",
        "Superseded note text",
        "finished",
        "Brief of finished",
    ] {
        assert!(!md.contains(gone), "{gone} in {md}");
    }
    let shown = ids(&out);
    for gone in ["m1-d1", "m1-q1", "m1-a1", "m1-n1", "m1-p1", "m1-b1"] {
        assert!(!shown.contains(&gone.to_owned()), "{gone} in {shown:?}");
    }
    let history = &md[pos(&md, "## History")..];
    assert!(
        history
            .lines()
            .next()
            .unwrap()
            .contains("cpd context show --all"),
        "{md}"
    );
    for line in [
        "- 1 superseded decisions",
        "- 1 answered questions",
        "- 1 superseded notes",
        "- 1 done plans",
    ] {
        assert!(history.contains(line), "{line} in {md}");
    }
}

#[test]
fn bare_show_caps_notes_to_the_newest_forty_with_a_footer() {
    let fx = fast();
    for n in 1..=45u32 {
        let at = 1000 - n;
        seed(
            &fx.env,
            &format!("m1-n{n}"),
            "note",
            at,
            &format!("Note number {n}x"),
        );
        seed(
            &fx.env,
            &format!("m1-d{n}"),
            "decision",
            at,
            &format!("Decision number {n}x"),
        );
        seed(
            &fx.env,
            &format!("m1-q{n}"),
            "question",
            at,
            &format!("Question number {n}x"),
        );
    }
    let md = markdown(&show(&fx, &[]));
    for n in 1..=5 {
        assert!(
            !md.contains(&format!("Note number {n}x")),
            "note {n} in {md}"
        );
    }
    for n in 6..=45 {
        assert!(
            md.contains(&format!("Note number {n}x")),
            "note {n} in {md}"
        );
    }
    for n in 1..=45 {
        assert!(
            md.contains(&format!("Decision number {n}x")),
            "decision {n} in {md}"
        );
        assert!(
            md.contains(&format!("Question number {n}x")),
            "question {n} in {md}"
        );
    }
    let footer = md
        .lines()
        .find(|l| l.contains("5 older notes omitted"))
        .unwrap_or_else(|| panic!("a footer in {md}"));
    for named in ["--topic", "search", "--all"] {
        assert!(footer.contains(named), "{named} in {footer}");
    }
    assert!(
        pos(&md, "Note number 6x") < pos(&md, "Note number 45x"),
        "seq order: {md}"
    );
}

#[test]
fn bare_show_stays_under_fifty_kb_on_a_large_context() {
    let fx = fast();
    let long = "x".repeat(400);
    let mut sql = String::new();
    for n in 1..=601 {
        let sup = if n > 1 {
            format!("'m1-d{}'", n - 1)
        } else {
            "NULL".to_owned()
        };
        sql.push_str(&format!(
            "INSERT INTO context_records (context, id, kind, at, machine, text, supersedes)
             VALUES ('{CTX}', 'm1-d{n}', 'decision', '2026-10-14T21:12:00.000Z', 'm1', 'D{n} {long}', {sup});"
        ));
    }
    for n in 1..=200 {
        sql.push_str(&format!(
            "INSERT INTO context_records (context, id, kind, at, machine, text)
               VALUES ('{CTX}', 'm1-q{n}', 'question', '2026-10-14T21:12:00.000Z', 'm1', 'Q{n} {long}');
             INSERT INTO context_records (context, id, kind, at, machine, text, question)
               VALUES ('{CTX}', 'm1-a{n}', 'answer', '2026-10-14T21:12:00.000Z', 'm1', 'A{n} {long}', 'm1-q{n}');"
        ));
    }
    fx.env.sql(&sql);
    for n in 1..=100 {
        seed_plan(&fx.env, n, &format!("done{n}"), true);
    }
    let out = show(&fx, &[]);
    let md = markdown(&out);
    assert!(md.len() < 50 * 1024, "markdown is {} bytes", md.len());
    let whole = out.to_string();
    assert!(whole.len() < 50 * 1024, "json is {} bytes", whole.len());
}

#[test]
fn bare_show_json_counts_what_it_omits() {
    let fx = fast();
    history(&fx);
    for n in 3..=44 {
        seed(
            &fx.env,
            &format!("m1-n{n}"),
            "note",
            50,
            &format!("Bulk note {n}"),
        );
    }
    // notes in force: m1-n2 and m1-n3..m1-n44 = 43; the newest 40 are shown, 3 are older
    let out = show(&fx, &[]);
    assert_eq!(
        out["omitted"],
        serde_json::json!({"superseded_decisions": 1, "answered_questions": 1, "superseded_notes": 1, "older_notes": 3, "done_plans": 1}),
        "{out}"
    );
    let shown = ids(&out);
    for kept in ["m1-d2", "m1-q2", "m1-p2", "m1-b2", "m1-n44"] {
        assert!(shown.contains(&kept.to_owned()), "{kept} in {shown:?}");
    }
    for gone in [
        "m1-d1", "m1-q1", "m1-a1", "m1-n1", "m1-p1", "m1-b1", "m1-n2", "m1-n3", "m1-n4",
    ] {
        assert!(!shown.contains(&gone.to_owned()), "{gone} in {shown:?}");
    }
    let seqs: Vec<i64> = out["records"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["seq"].as_i64().unwrap())
        .collect();
    assert!(seqs.windows(2).all(|w| w[0] < w[1]), "seq order: {seqs:?}");
}

#[test]
fn bare_show_lists_curated_topics_with_counts() {
    let fx = fast();
    let none = markdown(&show(&fx, &[]));
    assert!(!none.contains("## Topics"), "{none}");

    for n in 1..=4u32 {
        seed(
            &fx.env,
            &format!("m1-n{n}"),
            "note",
            60 - n,
            &format!("Note {n}"),
        );
    }
    curate(
        &fx.env,
        &[
            ("m1-n1", "engine", "one"),
            ("m1-n2", "engine", "two"),
            ("m1-n3", "engine", "three"),
            ("m1-n4", "store", "four"),
        ],
    );
    let md = markdown(&show(&fx, &[]));
    assert!(md.contains("## Topics\nengine (3), store (1)"), "{md}");
}

#[test]
fn show_all_renders_the_full_history_in_the_old_order() {
    let fx = fast();
    history(&fx);
    let out = show(&fx, &["--all"]);
    let md = markdown(&out);
    let order: Vec<usize> = [
        "## Plans",
        "## Decisions",
        "## Superseded decisions",
        "## Open questions",
        "## Answered questions",
        "## Notes",
    ]
    .iter()
    .map(|h| pos(&md, h))
    .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{md}");
    for present in [
        "finished",
        "Old decision",
        "Answered question text",
        "The given answer",
        "Superseded note text",
    ] {
        assert!(md.contains(present), "{present} in {md}");
    }
    assert!(!md.contains("## History"), "{md}");
    assert!(out.get("omitted").is_none(), "{out}");
    assert_eq!(
        ids(&out),
        [
            "m1-d1", "m1-d2", "m1-q1", "m1-a1", "m1-q2", "m1-n1", "m1-n2", "m1-p1", "m1-b1",
            "m1-p2", "m1-b2"
        ],
        "{out}"
    );
}

#[test]
fn show_all_is_exclusive_with_since_record_and_topic() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "Note");
    curate(&fx.env, &[("m1-n1", "engine", "one")]);
    // `--all` alone is accepted, so the refusals below come from the conflicts, not from an unknown flag
    show(&fx, &["--all"]);
    for extra in [
        ["--since", "0"],
        ["--record", "m1-n1"],
        ["--topic", "engine"],
    ] {
        let mut args = vec!["context", "show", "--context", CTX, "--all"];
        args.extend_from_slice(&extra);
        let run = fx.cpd(&args);
        assert_eq!(run.exit, 2, "{extra:?}: {run:?}");
        assert_eq!(run.error()["code"], "usage", "{extra:?}: {run:?}");
    }
}
