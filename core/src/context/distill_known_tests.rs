//! `known_md` (contract §4.6 step 3): pure over an in-memory `State`, nothing mocked. `State`'s fields are
//! private to `distill`, so a `State` is read with `State::read` from an in-memory SQLite holding the three
//! tables it reads (hand-made, columns as in the store's migrations).
//!
//! Every curated record `hs-n<N>` has the topic `tp<N:03>`, the short form `short zq<N>x` and a text holding the
//! rare token `zq<N>x`; a batch record repeats exactly one such token, so its only lexical neighbour is that record.

use std::collections::BTreeSet;

use rusqlite::Connection;

use super::distill::State;
use super::distill_known::{KNOWN_FULL_MAX, KNOWN_K, known_md};
use super::render::Indexed;
use super::{By, Draft, Record};
use crate::ids::{ContextId, RecordId};

const CTX: &str = "ctx";

fn id(n: u32) -> String {
    format!("hs-n{n}")
}

fn topic(n: u32) -> String {
    format!("tp{n:03}")
}

fn line(n: u32) -> String {
    format!("[hs-n{n}] {} | short zq{n}x\n", topic(n))
}

fn note(n: u32, text: &str) -> Record {
    Record {
        id: RecordId::parse(&id(n)).unwrap(),
        at: "2026-10-01T10:00:00.000Z".into(),
        by: By {
            session: None,
            plan: None,
        },
        text: text.into(),
        draft: Draft::Note,
    }
}

/// The records of a Context: `curated` curated notes (1..=curated), then the uncurated notes `(n, text)` in
/// order; `retired` are the numbers of curated records an approved retirement took out of force.
fn state(curated: u32, uncurated: &[(u32, String)], retired: &[u32]) -> State {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE context_records (seq INTEGER PRIMARY KEY AUTOINCREMENT, context TEXT NOT NULL, id TEXT NOT NULL,
           kind TEXT NOT NULL, at TEXT NOT NULL, machine TEXT NOT NULL, by_session TEXT, by_plan TEXT, text TEXT NOT NULL,
           supersedes TEXT, question TEXT, plan_id TEXT, plan_machine TEXT, plan_jira TEXT, UNIQUE (context, id));
         CREATE TABLE record_curations (context TEXT NOT NULL, record TEXT NOT NULL, topic TEXT NOT NULL, short TEXT NOT NULL);
         CREATE TABLE record_retirements (context TEXT NOT NULL, record TEXT NOT NULL, status TEXT NOT NULL);",
    )
    .unwrap();
    let ctx = ContextId::parse(CTX).unwrap();
    for n in 1..=curated {
        note(n, &format!("Record {n} about zq{n}x."))
            .insert(&conn, &ctx)
            .unwrap();
        conn.execute(
            "INSERT INTO record_curations (context, record, topic, short) VALUES (?1, ?2, ?3, ?4)",
            (CTX, id(n), topic(n), format!("short zq{n}x")),
        )
        .unwrap();
    }
    for (n, text) in uncurated {
        note(*n, text).insert(&conn, &ctx).unwrap();
    }
    for n in retired {
        conn.execute(
            "INSERT INTO record_retirements (context, record, status) VALUES (?1, ?2, 'approved')",
            (CTX, id(*n)),
        )
        .unwrap();
    }
    State::read(&conn, &ctx).unwrap()
}

/// `known.md` for the first `size` uncurated records of `state`.
fn known(state: &State, size: usize) -> String {
    let uncurated = state.uncurated();
    known_md(state, &uncurated[..size]).unwrap()
}

/// The `known.md` lines that name a record.
fn record_lines(text: &str) -> Vec<&str> {
    text.lines().filter(|l| l.starts_with('[')).collect()
}

fn header(numbers: impl IntoIterator<Item = u32>) -> String {
    let topics: BTreeSet<String> = numbers.into_iter().map(topic).collect();
    let mut out: String = topics.iter().map(|t| format!("{t}\n")).collect();
    out.push('\n');
    out
}

/// A batch record naming the rare tokens of the curated records `tokens`.
fn batch_note(n: u32, tokens: &[u32]) -> (u32, String) {
    let words: Vec<String> = tokens.iter().map(|t| format!("zq{t}x")).collect();
    (n, format!("Batch {n}: {}", words.join(" ")))
}

#[test]
fn known_md_lists_every_curated_record_at_80_or_fewer() {
    assert_eq!(KNOWN_FULL_MAX, 80);
    let s = state(80, &[batch_note(900, &[5])], &[]);
    let expected = format!(
        "{}{}",
        header(1..=80),
        (1..=80).map(line).collect::<String>()
    );
    assert_eq!(known(&s, 1), expected);
}

#[test]
fn known_md_narrows_above_80_to_neighbours_and_forced() {
    assert_eq!(KNOWN_K, 8);
    // Each batch record names one token; the batch cites 50 before 5, the output is oldest first.
    let s = state(100, &[batch_note(900, &[50]), batch_note(901, &[5])], &[]);
    let expected = format!("{}{}{}", header(1..=100), line(5), line(50));
    assert_eq!(known(&s, 2), expected);
}

#[test]
fn known_md_keeps_every_topic_above_80() {
    let s = state(120, &[batch_note(900, &[7])], &[]);
    let out = known(&s, 1);
    assert!(out.starts_with(&header(1..=120)), "{out}");
    // A topic with no listed record is still there.
    assert!(out.lines().any(|l| l == "tp100"), "{out}");
    assert_eq!(record_lines(&out), [line(7).trim_end()]);
}

#[test]
fn known_md_forces_ids_cited_in_batch_text_and_supersedes() {
    // 90 curated records; the cited and the superseded ids are named by no rare token, so they never rank.
    // hs-n3 is cited in the text; hs-n95 is not curated; hs-n1000 is uncurated.
    let uncurated = [
        (
            900,
            "Batch 900: zq50x, see hs-n3 and hs-n1000 and hs-n95.".to_owned(),
        ),
        (1000, "Another uncurated record.".to_owned()),
    ];
    let s = state(90, &uncurated, &[]);
    let cited = known(&s, 1);
    assert_eq!(
        record_lines(&cited),
        [line(3).trim_end(), line(50).trim_end()],
        "{cited}"
    );

    // The supersedes target: a batch record that is not part of the State (a superseded record would
    // otherwise be out of force), a decision `hs-d1` superseding the curated hs-n4.
    let decision: Indexed = (
        10_000,
        Record {
            id: RecordId::parse("hs-d1").unwrap(),
            at: "2026-10-02T10:00:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: "Replaces the old one, zq60x.".into(),
            draft: Draft::Decision {
                supersedes: vec![RecordId::parse(&id(4)).unwrap()],
            },
        },
    );
    let s = state(90, &[], &[]);
    let out = known_md(&s, &[&decision]).unwrap();
    assert_eq!(
        record_lines(&out),
        [line(4).trim_end(), line(60).trim_end()],
        "{out}"
    );
}

#[test]
fn known_md_ignores_retired_and_uncurated_records() {
    // The batch names the tokens of the retired 7 and 9, of the curated 20, and of the uncurated hs-n901.
    let uncurated = [
        batch_note(900, &[7, 9, 20]),
        (901, "Uncurated zq901x.".to_owned()),
        (902, "Batch 902: zq901x and hs-n901 and hs-n7".to_owned()),
    ];
    let s = state(90, &uncurated, &[7, 9]);
    let out = known(&s, 2);
    assert_eq!(record_lines(&out), [line(20).trim_end()], "{out}");
    assert!(!out.contains("hs-n7]") && !out.contains("hs-n9]") && !out.contains("hs-n901]"));
    assert!(!out.lines().any(|l| l == "tp007" || l == "tp009"), "{out}");
}

#[test]
fn known_md_is_bounded_by_batch_size_times_k_plus_forced() {
    let uncurated: Vec<(u32, String)> = (0..60)
        .map(|k| batch_note(1000 + k, &[1 + k * 6]))
        .collect();
    let s = state(400, &uncurated, &[]);
    let out = known(&s, 60);
    let lines = record_lines(&out).len();
    let forced = 0;
    assert!(lines <= 60 * KNOWN_K + forced, "{lines} lines");
    // One neighbour each, every one of them once: exactly the 60 named records.
    let expected: Vec<String> = (0..60).map(|k| line(1 + k * 6)).collect();
    assert_eq!(out, format!("{}{}", header(1..=400), expected.concat()));
}

#[test]
fn known_md_fails_loud_when_the_fts_query_errors() {
    // `lexical_rank` quotes every term of an alphanumeric run, so no text of a record makes its FTS5 query fail
    // from outside: the limitation is named here. The nearest test: a text full of FTS5 syntax must not make
    // `known_md` fall back to the full list or panic; it ranks by its alphanumeric terms only.
    let hostile = (
        900,
        "\" OR NOT ( * : ^ NEAR(zq5x) zq5x AND - \"unterminated".to_owned(),
    );
    let s = state(100, &[hostile], &[]);
    let out = known_md(&s, &s.uncurated()[..1]).expect("the quoted terms are a valid query");
    assert_eq!(record_lines(&out), [line(5).trim_end()], "{out}");
}
