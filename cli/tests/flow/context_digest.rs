//! The cpd-rendered Context digest, `context show --digest` and `--topic`, and the first message (contract §4.6 "The
//! Context digest", "Consumers"; §8.2; commands.md §3.3; hs-d549, hs-d551). The tests that start no pane run in
//! this process (`Fx`), the others as the built binary. Curations are written through `curation::propose`, as the
//! distiller will.

use crate::common;

use std::fs;

use common::fx::Fx;
use common::{Run, SLEEP_SECONDS, TestEnv, WATCH_OK};
use cpd_core::context::curation::{Curation, Proposal, Reason, propose};
use cpd_core::home::CpdHome;
use cpd_core::ids::{ContextId, PlanId};
use serde_json::Value;

const CTX: &str = "ctx";

/// Machine `m1`, Context `ctx` and a fake `cpd` for the panes (`plan new`).
fn fx() -> TestEnv {
    let env = TestEnv::new();
    env.write_tmux_config("");
    env.fake_cpd_panes(WATCH_OK);
    env.cpd(&["context", "new", CTX, "--title", "Payments", "--owner", "acme"]).ok();
    env
}

/// Machine `m1` and Context `ctx`, the command tree in this process: for the tests that start no pane (`plan new` does).
fn fast() -> Fx {
    let fx = Fx::new();
    fx.env.write_tmux_config("");
    fx.cpd(&["context", "new", CTX, "--title", "Payments", "--owner", "acme"]).ok();
    fx
}

/// `cpd <args>` in this process (`Fx`) or as the binary (`TestEnv`): the same call, the same [`Run`].
trait Cpd {
    fn cpd(&self, args: &[&str]) -> Run;
}

impl Cpd for Fx {
    fn cpd(&self, args: &[&str]) -> Run {
        Fx::cpd(self, args)
    }
}

impl Cpd for TestEnv {
    fn cpd(&self, args: &[&str]) -> Run {
        TestEnv::cpd(self, args)
    }
}

/// Inserts a record of `kind` written `minutes` minutes ago (the CLI cannot backdate a record).
fn seed(env: &TestEnv, id: &str, kind: &str, minutes: u32, text: &str) {
    env.sql(&format!(
        "INSERT INTO context_records (context, id, kind, at, machine, text)
         VALUES ('{CTX}', '{id}', '{kind}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{minutes} minutes'), 'm1', '{text}')"
    ));
}

/// Inserts every record of `rows` (id, kind, minutes, text) in ONE transaction and one [`TestEnv::store`]: the same
/// rows as [`seed`] one at a time, at a fraction of the cost (each `Store::open` migrates and checks the schema).
fn seed_all(env: &TestEnv, rows: &[(String, &str, u32, &str)]) {
    let values: Vec<String> = rows
        .iter()
        .map(|(id, kind, minutes, text)| {
            format!(
                "('{CTX}', '{id}', '{kind}', strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-{minutes} minutes'), 'm1', '{text}')"
            )
        })
        .collect();
    env.sql(&format!("INSERT INTO context_records (context, id, kind, at, machine, text) VALUES {}", values.join(", ")));
}

const DAY: u32 = 24 * 60;

fn curate(env: &TestEnv, rows: &[(&str, &str, &str)]) {
    let curations: Vec<Curation> = rows
        .iter()
        .map(|(id, topic, short)| Curation { record: id.parse().unwrap(), topic: (*topic).into(), short: (*short).into(), doc_paths: None })
        .collect();
    propose(&mut env.store(), &ContextId::parse(CTX).unwrap(), &curations, &[]).unwrap();
}

fn digest(env: &impl Cpd) -> Value {
    env.cpd(&["context", "show", "--context", CTX, "--digest"]).ok()
}

fn topic(env: &impl Cpd, t: &str) -> Value {
    env.cpd(&["context", "show", "--context", CTX, "--topic", t]).ok()
}

fn markdown(out: &Value) -> String {
    out["markdown"].as_str().unwrap_or_else(|| panic!("a markdown in {out}")).to_owned()
}

fn lead_message(env: &TestEnv, plan: &str) -> String {
    let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
    fs::read_to_string(home.lead_message(&PlanId::parse(plan).unwrap())).unwrap()
}

fn new_plan_with_brief(env: &TestEnv, id: &str, brief: &str) {
    let file = env.root().join(format!("{id}-brief.md"));
    fs::write(&file, brief).unwrap();
    env.cpd(&["plan", "new", id, "--context", CTX, "--title", &format!("Plan {id}"), "--brief-file", file.to_str().unwrap()]).ok();
}

#[test]
fn the_digest_holds_recent_records_open_questions_and_brief_citations() {
    let env = fx();
    seed(&env, "m1-n1", "note", 30 * DAY, "Old and uncited.");
    seed(&env, "m1-n2", "note", 30 * DAY, "Old but a brief cites it.");
    seed(&env, "m1-q1", "question", 30 * DAY, "Old but still open?");
    seed(&env, "m1-n3", "note", 60, "Written an hour ago.");
    seed(&env, "m1-n4", "note", 60, "Recent but not curated.");
    curate(
        &env,
        &[
            ("m1-n1", "ledger", "old uncited fact"),
            ("m1-n2", "ledger", "old cited fact"),
            ("m1-q1", "retries", "old open question"),
            ("m1-n3", "retries", "recent fact"),
        ],
    );
    new_plan_with_brief(&env, "pay", "Retry the payouts. Rests on m1-n2.\n");

    let out = digest(&env);
    let md = markdown(&out);
    for line in ["- [m1-n2] old cited fact", "- [m1-q1] old open question", "- [m1-n3] recent fact"] {
        assert!(md.lines().any(|l| l == line), "{line:?} in\n{md}");
    }
    assert!(!md.contains("m1-n1]") && !md.contains("old uncited fact"), "an old uncited record is not relevant: {md}");
    assert!(!md.contains("m1-n4") && !md.contains("not curated"), "an uncurated record is never in the digest: {md}");
    assert_eq!(out["digest"]["records"], 3, "{out}");
    assert_eq!(out["digest"]["bytes"], md.len(), "{out}");

    // A wider window takes the old record in.
    env.write_config("version = 1\nmachine = \"m1\"\ncontext_recent_days = 60\ncontext_curation_min_records = 1\n");
    assert!(markdown(&digest(&env)).contains("- [m1-n1] old uncited fact"));
}

#[test]
fn the_digest_groups_short_forms_by_topic_and_lists_every_topic() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 300, "a");
    seed(&fx.env, "m1-n2", "note", 180, "b");
    seed(&fx.env, "m1-n3", "note", 60, "c");
    seed(&fx.env, "m1-n4", "note", 120, "d");
    seed(&fx.env, "m1-n5", "note", 40 * DAY, "e");
    seed(&fx.env, "m1-n6", "note", 40 * DAY, "f");
    curate(
        &fx.env,
        &[
            ("m1-n1", "engine", "engine one"),
            ("m1-n2", "store", "store two"),
            ("m1-n3", "engine", "engine three"),
            ("m1-n4", "inbox", "inbox four"),
            ("m1-n5", "engine", "engine old"),
            ("m1-n6", "archive", "archive old"),
        ],
    );

    let out = digest(&fx);
    let md = markdown(&out);
    let (sections, topics) = md.split_once("## Topics\n").unwrap_or_else(|| panic!("a ## Topics section in\n{md}"));
    // topics by their newest relevant record, newest first; lines oldest first within a topic
    assert_eq!(
        sections.trim_end(),
        "## engine\n- [m1-n1] engine one\n- [m1-n3] engine three\n\n## inbox\n- [m1-n4] inbox four\n\n## store\n- [m1-n2] store two",
        "{md}"
    );
    // every topic of the curated records in force, with its count of records in force (the old ones too)
    let lines: Vec<&str> = topics.lines().filter(|l| !l.is_empty()).collect();
    for expected in ["- engine (3)", "- inbox (1)", "- store (1)", "- archive (1)"] {
        assert!(lines.contains(&expected), "{expected:?} in {lines:?}");
    }
    assert_eq!(lines.len(), 4, "{lines:?}");
    assert_eq!(out["digest"]["records"], 4, "{out}");
}

#[test]
fn an_over_limit_digest_drops_the_oldest_and_points_to_topic() {
    let fx = fast();
    // 400 recent records of one topic, each line 130+ bytes: far over 32 768 bytes. Record n is n minutes old... n=1 newest.
    let seeded: Vec<(String, &str, u32, &str)> = (1..=400u32).map(|n| (format!("m1-n{n}"), "note", n, "x")).collect();
    seed_all(&fx.env, &seeded);
    let shorts: Vec<String> = (1..=400u32).map(|n| format!("{n:03} {}", "s".repeat(100))).collect();
    let mut rows = Vec::new();
    for n in 1..=400usize {
        rows.push((format!("m1-n{n}"), shorts[n - 1].clone()));
    }
    let rows: Vec<(&str, &str, &str)> = rows.iter().map(|(id, s)| (id.as_str(), "alpha", s.as_str())).collect();
    curate(&fx.env, &rows);

    let out = digest(&fx);
    let md = markdown(&out);
    assert!(md.len() <= 32_768, "{} bytes", md.len());
    assert_eq!(out["digest"]["bytes"], md.len(), "{out}");
    assert!(md.contains("- [m1-n1] 001 "), "the newest line stays");
    assert!(!md.contains("- [m1-n400] "), "the oldest line is dropped first");
    let kept = md.lines().filter(|l| l.starts_with("- [m1-n")).count();
    assert!(kept > 100 && kept < 400, "{kept} lines kept");
    let pointer = format!("and {} older: cpd context show --context {CTX} --topic alpha", 400 - kept);
    assert!(md.lines().any(|l| l == pointer), "{pointer:?} in the digest's tail: {}", &md[md.len().saturating_sub(300)..]);
    assert!(md.contains("## Topics\n- alpha (400)"), "the topic list is kept whole");
    // idempotent: the same records give the same text
    assert_eq!(markdown(&digest(&fx)), md);
}

#[test]
fn first_message_lists_uncurated_records_raw_at_most_20() {
    let env = fx();
    env.cpd(&["context", "note", "FULL TEXT of the curated one.", "--context", CTX]).ok();
    for n in 2..=25 {
        env.cpd(&["context", "note", &format!("raw-{n:02}"), "--context", CTX]).ok();
    }
    curate(&env, &[("m1-n1", "ledger", "curated short form")]);
    new_plan_with_brief(&env, "pay", "Pay out.\n");

    let message = lead_message(&env, "pay");
    let at = |needle: &str| message.find(needle).unwrap_or_else(|| panic!("{needle:?} in\n{message}"));
    assert!(at("- [m1-n1] curated short form") < at("raw-02"), "the digest, then the raw records");
    assert!(!message.contains("FULL TEXT"), "a curated record is a short form, not raw: {message}");
    for n in 2..=21 {
        assert!(message.contains(&format!("raw-{n:02}")), "raw-{n:02} in {message}");
    }
    for n in 22..=25 {
        assert!(!message.contains(&format!("raw-{n:02}")), "raw-{n:02} is past the 20: {message}");
    }
    assert!(message.contains("and 4 more: cpd context show --context ctx"), "{message}");
    let pointer = message.rfind("cpd context show --context ctx --since").unwrap_or_else(|| panic!("the pointer line in {message}"));
    assert!(pointer > at("raw-21"), "the pointer line comes last: {message}");
}

#[test]
fn show_topic_prints_every_curated_record_of_the_topic() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 40 * DAY, "a");
    seed(&fx.env, "m1-n2", "note", 30 * DAY, "b");
    seed(&fx.env, "m1-n3", "note", 20 * DAY, "c");
    seed(&fx.env, "m1-n4", "note", 10 * DAY, "d");
    seed(&fx.env, "m1-n5", "note", 5 * DAY, "e");
    curate(&fx.env, &[("m1-n1", "engine", "first"), ("m1-n2", "store", "other topic"), ("m1-n3", "engine", "second"), ("m1-n4", "engine", "superseded one")]);
    // m1-n5 is uncurated; a later note supersedes m1-n4
    fx.cpd(&["context", "note", "Newer.", "--supersedes", "m1-n4", "--context", CTX]).ok();

    let out = topic(&fx, "engine");
    assert_eq!(out["topic"], "engine", "{out}");
    assert_eq!(out["records"], serde_json::json!([{"id": "m1-n1", "short": "first"}, {"id": "m1-n3", "short": "second"}]), "{out}");
    assert_eq!(markdown(&out), "- [m1-n1] first\n- [m1-n3] second", "oldest first, as the docs say");
}

#[test]
fn a_retired_record_is_in_no_digest() {
    let env = fx();
    seed(&env, "m1-n1", "note", 60, "a");
    seed(&env, "m1-n2", "note", 50, "b");
    curate(&env, &[("m1-n1", "engine", "retired short"), ("m1-n2", "engine", "kept short")]);
    propose(
        &mut env.store(),
        &ContextId::parse(CTX).unwrap(),
        &[],
        &[Proposal { record: "m1-n1".parse().unwrap(), reason: Reason::Duplicate, target: "m1-n2".into() }],
    )
    .unwrap();
    // pending: still in force, so still in the digest
    assert!(markdown(&digest(&env)).contains("m1-n1"), "a pending proposal changes nothing");

    env.cpd(&["context", "retire", "--context", CTX, "--approve", "m1-n1"]).ok();
    new_plan_with_brief(&env, "pay", "Cites the retired m1-n1.\n");

    let md = markdown(&digest(&env));
    assert!(md.contains("- [m1-n2] kept short"), "{md}");
    assert!(!md.contains("m1-n1") && !md.contains("retired short"), "{md}");
    assert!(md.contains("- engine (1)"), "the count is of records in force: {md}");
    let by_topic = topic(&env, "engine");
    assert_eq!(markdown(&by_topic), "- [m1-n2] kept short", "{by_topic}");
    let message = lead_message(&env, "pay");
    assert!(!message.contains("retired short"), "{message}");
}

/// A fake `pi` that creates its session file and sleeps; the cockpit is the one `plan new` made.
fn manager_cockpit(env: &TestEnv) {
    env.fake_cpd_views();
    env.fake_pi(&format!(
        "id=''; prev=''\nfor a in \"$@\"; do [ \"$prev\" = --session-id ] && id=$a; prev=$a; done\n\
         if [ -n \"$id\" ]; then mkdir -p \"$HOME/.pi/agent/sessions/g\"; \
         printf '{{\"type\":\"session\"}}\\n' > \"$HOME/.pi/agent/sessions/g/2026_$id.jsonl\"; fi\n\
         exec sleep {SLEEP_SECONDS}"
    ));
}

/// hs-d558 FX5: a topic no curated record has ever used is refused, with the topics in use.
#[test]
fn show_topic_refuses_an_unknown_topic_with_the_topics_in_use() {
    let fx = fast();
    seed(&fx.env, "m1-n1", "note", 60, "a");
    seed(&fx.env, "m1-n2", "note", 50, "b");
    curate(&fx.env, &[("m1-n1", "engine", "one"), ("m1-n2", "store", "two")]);

    let run = fx.cpd(&["context", "show", "--context", CTX, "--topic", "nothing"]);
    let err = run.error();
    assert_eq!(run.exit, 4, "{err}");
    assert_eq!(err["code"], "topic_not_found", "{err}");
    assert_eq!(err["details"], serde_json::json!({"context": CTX, "topic": "nothing", "topics": ["engine", "store"]}), "{err}");
    // a topic in use is still answered
    assert_eq!(markdown(&topic(&fx, "engine")), "- [m1-n1] one");
}

/// hs-d558 FX6: the pointer line names the Context.
#[test]
fn the_pointer_line_names_the_context() {
    let fx = fast();
    let seeded: Vec<(String, &str, u32, &str)> = (1..=400u32).map(|n| (format!("m1-n{n}"), "note", n, "x")).collect();
    seed_all(&fx.env, &seeded);
    let shorts: Vec<String> = (1..=400u32).map(|n| format!("{n:03} {}", "s".repeat(100))).collect();
    let rows: Vec<(String, &str)> = (1..=400usize).map(|n| (format!("m1-n{n}"), shorts[n - 1].as_str())).collect();
    let rows: Vec<(&str, &str, &str)> = rows.iter().map(|(id, s)| (id.as_str(), "alpha", *s)).collect();
    curate(&fx.env, &rows);

    let md = markdown(&digest(&fx));
    let kept = md.lines().filter(|l| l.starts_with("- [m1-n")).count();
    let pointer = format!("and {} older: cpd context show --context {CTX} --topic alpha", 400 - kept);
    assert!(md.lines().any(|l| l == pointer), "{pointer:?} in the digest's tail: {}", &md[md.len().saturating_sub(300)..]);
}

/// hs-d558 FX6: even when the topic list alone is over 32 768 bytes, the digest is not.
#[test]
fn the_digest_never_exceeds_32_kb_even_when_the_topic_list_is_long() {
    let fx = fast();
    // 900 topics of 40 bytes: the list is `- <topic> (1)` lines, about 50 bytes each, 45 kB.
    let seeded: Vec<(String, &str, u32, &str)> = (1..=900u32).map(|n| (format!("m1-n{n}"), "note", n, "x")).collect();
    seed_all(&fx.env, &seeded);
    let topics: Vec<String> = (1..=900u32).map(|n| format!("topic-{n:04}-{}", "a".repeat(26))).collect();
    let rows: Vec<(String, &str)> = (1..=900usize).map(|n| (format!("m1-n{n}"), topics[n - 1].as_str())).collect();
    let rows: Vec<(&str, &str, &str)> = rows.iter().map(|(id, t)| (id.as_str(), *t, "short")).collect();
    curate(&fx.env, &rows);

    let out = digest(&fx);
    let md = markdown(&out);
    assert!(md.len() <= 32_768, "{} bytes", md.len());
    assert_eq!(out["digest"]["bytes"], md.len(), "{out}");
    assert!(md.contains("## Topics\n- "), "the topic list starts: {}", &md[..200.min(md.len())]);
    assert_eq!(md.lines().last(), Some("and more topics: cpd context show --context ctx --digest"), "{}", &md[md.len().saturating_sub(300)..]);
    // the lines kept are whole, dropped from the end
    assert!(md.contains("- topic-0001-"), "the first topic line stays");
    assert!(!md.contains("- topic-0900-"), "the last topic line is dropped");
}

/// hs-d558 FX7: the Plan's own brief is in its Lead's message once, in full; another Plan's brief is short.
#[test]
fn the_first_message_does_not_list_the_plans_own_brief_again() {
    let env = fx();
    new_plan_with_brief(&env, "old", "OTHER-FIRST-LINE\nOTHER-SECOND-LINE\n");
    new_plan_with_brief(&env, "pay", "OWN-FIRST-LINE\nOWN-SECOND-LINE\n");

    let message = lead_message(&env, "pay");
    assert_eq!(message.matches("OWN-FIRST-LINE").count(), 1, "the own brief once: {message}");
    assert_eq!(message.matches("OWN-SECOND-LINE").count(), 1, "the own brief once, in full: {message}");
    assert!(message.contains("OTHER-FIRST-LINE"), "another brief is listed: {message}");
    assert!(!message.contains("OTHER-SECOND-LINE"), "another brief is short, its first line: {message}");
}

/// hs-d558 FX7: a Manager's first message holds the digest and the uncurated records, at most 20; a brief is short.
#[test]
fn a_managers_first_message_holds_the_digest_and_the_uncurated_records() {
    let env = fx();
    env.cpd(&["context", "note", "FULL TEXT of the curated one.", "--context", CTX]).ok();
    for n in 2..=25 {
        env.cpd(&["context", "note", &format!("raw-{n:02}"), "--context", CTX]).ok();
    }
    curate(&env, &[("m1-n1", "ledger", "curated short form")]);
    new_plan_with_brief(&env, "pay", "BRIEF-FIRST-LINE\nBRIEF-SECOND-LINE\n");
    manager_cockpit(&env);
    env.cpd(&["context", "manager", "--context", CTX]).ok();

    let path = env.cpd_home().join("run/ctx/manager-message.md");
    let message = fs::read_to_string(&path).unwrap();
    assert!(message.starts_with("You are the Manager of Context ctx \"Payments\"."), "{message}");
    assert!(message.contains("- [m1-n1] curated short form"), "the digest: {message}");
    assert!(!message.contains("FULL TEXT"), "{message}");
    assert!(message.contains("raw-02"), "the uncurated records: {message}");
    assert!(message.contains("BRIEF-FIRST-LINE"), "{message}");
    assert!(!message.contains("BRIEF-SECOND-LINE"), "a brief is short, as `context show` renders it: {message}");
    let listed = message.lines().filter(|l| l.starts_with("- [m1-") && l.contains("raw-")).count();
    assert!(listed <= 20, "{listed} raw records in\n{message}");
}
