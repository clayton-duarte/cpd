//! Red tests of `context::reads` (decision pro-d10 item 5): `log_reads_for`, `mark_used_for`, `search_reads`.

use super::reads::{Channel, Read, log_reads_for, mark_used_for, search_reads};
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId, SessionId};
use crate::store::Store;

type Row = (String, Option<i64>, Option<f64>, String, Option<i64>);

struct Fx {
    _dir: tempfile::TempDir,
    store: Store,
}

fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c1', 'C1', 't', 'm1');
                 INSERT INTO contexts (id, title, created_at, created_on) VALUES ('c2', 'C2', 't', 'm1');
                 INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
                   ('c1', 'm1-d1', 'decision', 't', 'm1', 'x'),
                   ('c1', 'm1-d2', 'decision', 't', 'm1', 'x'),
                   ('c1', 'm1-n3', 'note', 't', 'm1', 'x'),
                   ('c2', 'm1-d9', 'decision', 't', 'm1', 'x');",
            )?;
            Ok(())
        })
        .unwrap();
    Fx { _dir: dir, store }
}

impl Fx {
    fn rows(&mut self, sql: &str) -> Vec<Row> {
        self.store
            .read(|c| {
                let mut st = c.prepare(sql)?;
                let rows = st
                    .query_map([], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// `(record, rank, score, read_at, used)` of the rows of `session`, `channel`.
    fn of(&mut self, session: &str, channel: &str) -> Vec<Row> {
        self.rows(&format!(
            "SELECT record, rank, score, read_at, used FROM context_reads
             WHERE session = '{session}' AND channel = '{channel}' ORDER BY record"
        ))
    }

    fn used(&mut self, session: &str) -> Vec<(String, Option<i64>)> {
        self.rows(&format!(
            "SELECT record, rank, score, channel, used FROM context_reads
             WHERE session = '{session}' ORDER BY record, channel"
        ))
        .into_iter()
        .map(|(r, _, _, _, u)| (r, u))
        .collect()
    }
}

fn ctx(s: &str) -> ContextId {
    ContextId::parse(s).unwrap()
}
fn sess(s: &str) -> SessionId {
    SessionId::parse(s).unwrap()
}
fn rid(s: &str) -> RecordId {
    RecordId::parse(s).unwrap()
}
fn read(id: &str, rank: Option<u32>, score: Option<f64>) -> Read {
    Read {
        record: rid(id),
        rank,
        score,
    }
}

#[test]
fn log_reads_for_inserts_rows_with_rank_score_and_time() {
    let mut fx = fixture();
    let n = log_reads_for(
        &mut fx.store,
        &ctx("c1"),
        &sess("s1"),
        Channel::Search,
        &[
            read("m1-d1", Some(1), Some(2.5)),
            read("m1-d2", Some(2), Some(1.5)),
        ],
        "2026-01-01T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(n, 2);
    let rows = fx.of("s1", "search");
    assert_eq!(
        rows,
        vec![
            (
                "m1-d1".into(),
                Some(1),
                Some(2.5),
                "2026-01-01T00:00:00.000Z".into(),
                None
            ),
            (
                "m1-d2".into(),
                Some(2),
                Some(1.5),
                "2026-01-01T00:00:00.000Z".into(),
                None
            ),
        ]
    );
    assert_eq!(Channel::Shadow.as_str(), "shadow");
}

#[test]
fn log_reads_for_ignores_a_repeat_and_keeps_the_first_read() {
    let mut fx = fixture();
    let c = ctx("c1");
    let s = sess("s1");
    let first = log_reads_for(
        &mut fx.store,
        &c,
        &s,
        Channel::Show,
        &[read("m1-d1", Some(1), Some(9.0))],
        "2026-01-01T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(first, 1);
    let again = log_reads_for(
        &mut fx.store,
        &c,
        &s,
        Channel::Show,
        &[read("m1-d1", Some(5), Some(0.5)), read("m1-d2", None, None)],
        "2026-02-02T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(again, 1, "only the new record is inserted");
    let rows = fx.of("s1", "show");
    assert_eq!(
        rows[0],
        (
            "m1-d1".into(),
            Some(1),
            Some(9.0),
            "2026-01-01T00:00:00.000Z".into(),
            None
        )
    );
    assert_eq!(rows[1].1, None);
    let same = log_reads_for(
        &mut fx.store,
        &c,
        &s,
        Channel::Show,
        &[read("m1-d1", Some(7), None)],
        "2026-03-03T00:00:00.000Z",
    )
    .unwrap();
    assert_eq!(same, 0);
}

#[test]
fn log_reads_for_keeps_channels_and_sessions_apart() {
    let mut fx = fixture();
    let c = ctx("c1");
    let r = [read("m1-d1", None, None)];
    for (s, ch) in [
        ("s1", Channel::Show),
        ("s1", Channel::Pushed),
        ("s2", Channel::Show),
    ] {
        assert_eq!(
            log_reads_for(&mut fx.store, &c, &sess(s), ch, &r, "t").unwrap(),
            1
        );
    }
    let n: i64 = fx
        .store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM context_reads", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 3);
}

#[test]
fn log_reads_for_refuses_a_record_of_another_context() {
    let mut fx = fixture();
    let res = log_reads_for(
        &mut fx.store,
        &ctx("c1"),
        &sess("s1"),
        Channel::Show,
        &[read("m1-d1", None, None), read("m1-d9", None, None)],
        "t",
    );
    assert!(res.is_err());
    let n: i64 = fx
        .store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM context_reads", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 0, "one transaction: nothing is kept");
}

fn seed_reads(fx: &mut Fx) {
    let c = ctx("c1");
    for s in ["s1", "s2"] {
        for ch in [Channel::Show, Channel::Search] {
            log_reads_for(
                &mut fx.store,
                &c,
                &sess(s),
                ch,
                &[
                    read("m1-d1", None, None),
                    read("m1-d2", None, None),
                    read("m1-n3", None, None),
                ],
                "t",
            )
            .unwrap();
        }
    }
}

#[test]
fn mark_used_for_sets_used_for_cited_and_read_records_only() {
    let mut fx = fixture();
    seed_reads(&mut fx);
    let n = mark_used_for(
        &mut fx.store,
        &ctx("c1"),
        &sess("s1"),
        "we follow m1-d1, and m1-d77 never read; also m1-d9 of another context",
        &[],
    )
    .unwrap();
    assert_eq!(n, 2, "both channel rows of m1-d1");
    assert_eq!(
        fx.used("s1"),
        vec![
            ("m1-d1".into(), Some(1)),
            ("m1-d1".into(), Some(1)),
            ("m1-d2".into(), None),
            ("m1-d2".into(), None),
            ("m1-n3".into(), None),
            ("m1-n3".into(), None),
        ]
    );
    assert_eq!(
        mark_used_for(&mut fx.store, &ctx("c1"), &sess("s1"), "no ids here", &[]).unwrap(),
        0
    );
    // never lowers used 1
    mark_used_for(&mut fx.store, &ctx("c1"), &sess("s1"), "m1-d2", &[]).unwrap();
    assert!(
        fx.used("s1")
            .iter()
            .filter(|(r, _)| r == "m1-d1")
            .all(|(_, u)| *u == Some(1))
    );
}

#[test]
fn mark_used_for_skips_excluded_ids_and_other_sessions() {
    let mut fx = fixture();
    seed_reads(&mut fx);
    let n = mark_used_for(
        &mut fx.store,
        &ctx("c1"),
        &sess("s1"),
        "m1-d1 m1-d2 m1-n3",
        &[rid("m1-d1")],
    )
    .unwrap();
    assert_eq!(n, 4);
    let s1 = fx.used("s1");
    assert!(
        s1.iter()
            .filter(|(r, _)| r == "m1-d1")
            .all(|(_, u)| u.is_none())
    );
    assert!(
        s1.iter()
            .filter(|(r, _)| r != "m1-d1")
            .all(|(_, u)| *u == Some(1))
    );
    assert!(fx.used("s2").iter().all(|(_, u)| u.is_none()));
}

#[test]
fn search_reads_ranks_from_one_in_page_order() {
    let reads = search_reads(&[
        (rid("m1-d2"), 3.0),
        (rid("m1-d1"), 2.0),
        (rid("m1-n3"), 1.0),
    ]);
    assert_eq!(
        reads,
        vec![
            read("m1-d2", Some(1), Some(3.0)),
            read("m1-d1", Some(2), Some(2.0)),
            read("m1-n3", Some(3), Some(1.0)),
        ]
    );
    assert!(search_reads(&[]).is_empty());
}
