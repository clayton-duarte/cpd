//! Red tests of the retirement store layer: approvals written with a proposal, the sweep's decisions and the pending
//! list (contract §4.6 "Retirement"; commands.md §3.13a). A tempdir Store, no ports.

use super::{
    Approval, Curation, PendingPair, Proposal, Reason, Sweep, decide_pending, pending_pairs,
    propose_rejecting,
};
use crate::context::distill::State;
use crate::context::{By, Draft, Record};
use crate::error::ErrorCode;
use crate::home::CpdHome;
use crate::ids::{ContextId, RecordId};
use crate::store::Store;

const DECIDER: &str = "kev-4b+gemini-3.8-flash";

struct Fx {
    _dir: tempfile::TempDir,
    store: Store,
}

fn id(s: &str) -> RecordId {
    RecordId::parse(s).unwrap()
}

fn context() -> ContextId {
    ContextId::parse("ctx").unwrap()
}

/// Notes hs-n1..hs-n4 and a decision hs-d1, in that seq order.
fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'C', '2026-11-01T00:00:00.000Z', 'hs');",
            )?;
            Ok(())
        })
        .unwrap();
    let mut f = Fx { _dir: dir, store };
    for rid in ["hs-n1", "hs-n2", "hs-n3", "hs-n4", "hs-d1"] {
        let draft = if rid == "hs-d1" {
            Draft::Decision { supersedes: vec![] }
        } else {
            Draft::Note
        };
        let record = Record {
            id: id(rid),
            at: "2026-11-01T00:00:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: format!("text of {rid}"),
            draft,
        };
        f.store
            .write("fixture", |tx| {
                record.insert(tx, &context())?;
                Ok(())
            })
            .unwrap();
    }
    f
}

impl Fx {
    fn count(&mut self, sql: &str) -> i64 {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    fn row(&mut self, record: &str) -> (String, Option<String>, Option<String>, String) {
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, decided_at, decided_by, proposed_at FROM record_retirements WHERE record = ?1",
                    [record],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap()
    }

    fn retired_events(&mut self) -> i64 {
        self.count("SELECT count(*) FROM events WHERE kind = 'records-retired'")
    }

    fn in_force(&mut self) -> std::collections::HashSet<String> {
        let ctx = context();
        self.store
            .read(|c| State::read(c, &ctx))
            .unwrap()
            .in_force_ids()
    }

    /// Inserts a retirement row directly: `status` 'pending', 'approved' or 'rejected'.
    fn seed(&mut self, record: &str, reason: &str, target: &str, status: &str, proposed_at: &str) {
        let decided = (status != "pending").then_some("2026-11-02T00:00:00.000Z");
        let by = (status != "pending").then_some("user");
        self.store
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at, decided_at, decided_by)
                     VALUES ('ctx', ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![record, reason, target, status, proposed_at, decided, by],
                )?;
                Ok(())
            })
            .unwrap();
    }
}

fn proposal(record: &str, reason: Reason, target: &str, approval: Approval) -> Proposal {
    Proposal {
        record: id(record),
        reason,
        target: target.into(),
        approval,
    }
}

fn propose(f: &mut Fx, proposals: &[Proposal]) -> Result<usize, crate::error::CpdError> {
    let curations: Vec<Curation> = proposals
        .iter()
        .map(|p| Curation {
            record: p.record.clone(),
            topic: "topic".into(),
            short: "short".into(),
        })
        .collect();
    propose_rejecting(&mut f.store, &context(), &curations, proposals, 0)
}

#[test]
fn an_auto_approved_proposal_is_written_approved_with_its_decider() {
    let mut f = fixture();
    propose(
        &mut f,
        &[proposal(
            "hs-n2",
            Reason::Duplicate,
            "hs-n1",
            Approval::Models,
        )],
    )
    .unwrap();
    let (status, at, by, proposed) = f.row("hs-n2");
    assert_eq!(status, "approved");
    assert_eq!(at.as_deref(), Some(proposed.as_str()));
    assert_eq!(by.as_deref(), Some(DECIDER));
    assert_eq!(Approval::Models.decided_by(), Some(DECIDER));
    assert_eq!(Approval::Pending.decided_by(), None);
}

#[test]
fn a_git_grep_proposal_is_written_approved_by_git_grep() {
    let mut f = fixture();
    propose(
        &mut f,
        &[proposal(
            "hs-n2",
            Reason::Documented,
            "docs/a.md",
            Approval::GitGrep,
        )],
    )
    .unwrap();
    let (status, at, by, proposed) = f.row("hs-n2");
    assert_eq!(status, "approved");
    assert_eq!(at.as_deref(), Some(proposed.as_str()));
    assert_eq!(by.as_deref(), Some("git-grep"));
    assert_eq!(Approval::GitGrep.decided_by(), Some("git-grep"));
}

#[test]
fn a_pending_proposal_keeps_status_pending_and_no_decision() {
    let mut f = fixture();
    propose(
        &mut f,
        &[proposal(
            "hs-n2",
            Reason::Duplicate,
            "hs-n1",
            Approval::Pending,
        )],
    )
    .unwrap();
    let (status, at, by, _) = f.row("hs-n2");
    assert_eq!((status.as_str(), at, by), ("pending", None, None));
    assert_eq!(Approval::Pending.decided_by(), None);
}

#[test]
fn auto_approved_proposals_append_one_records_retired_event() {
    let mut f = fixture();
    propose(
        &mut f,
        &[
            proposal("hs-n2", Reason::Duplicate, "hs-n1", Approval::Models),
            proposal("hs-n3", Reason::Superseded, "hs-d1", Approval::Models),
            proposal("hs-n4", Reason::Duplicate, "hs-n1", Approval::Pending),
        ],
    )
    .unwrap();
    assert_eq!(f.retired_events(), 1);
    assert_eq!(
        f.count("SELECT count(*) FROM events WHERE kind = 'context-curated'"),
        1
    );
    let (seq_curated, seq_retired) = (
        f.count("SELECT max(seq) FROM events WHERE kind = 'context-curated'"),
        f.count("SELECT max(seq) FROM events WHERE kind = 'records-retired'"),
    );
    assert!(
        seq_retired > seq_curated,
        "records-retired follows context-curated"
    );
    let payload = f.count("SELECT count(*) FROM events WHERE kind = 'records-retired' AND payload LIKE '%hs-n2%' AND payload LIKE '%hs-n3%' AND payload NOT LIKE '%hs-n4%'");
    assert_eq!(payload, 1);
}

#[test]
fn pending_only_proposals_append_no_records_retired_event() {
    let mut f = fixture();
    propose(
        &mut f,
        &[proposal(
            "hs-n2",
            Reason::Duplicate,
            "hs-n1",
            Approval::Pending,
        )],
    )
    .unwrap();
    assert_eq!(
        f.count("SELECT count(*) FROM events WHERE kind = 'context-curated'"),
        1
    );
    assert_eq!(f.retired_events(), 0);
    assert_eq!(f.row("hs-n2").0, "pending");
}

#[test]
fn a_proposal_with_an_existing_row_of_any_status_writes_nothing_new() {
    for status in ["pending", "approved", "rejected"] {
        let mut f = fixture();
        f.seed(
            "hs-n2",
            "duplicate",
            "hs-n1",
            status,
            "2026-11-01T01:00:00.000Z",
        );
        let before = f.row("hs-n2");
        propose(
            &mut f,
            &[proposal(
                "hs-n2",
                Reason::Duplicate,
                "hs-n1",
                Approval::Models,
            )],
        )
        .unwrap();
        assert_eq!(
            f.count("SELECT count(*) FROM record_retirements"),
            1,
            "{status}"
        );
        assert_eq!(f.row("hs-n2"), before, "{status}");
        assert_eq!(f.retired_events(), 0, "{status}");
    }
}

#[test]
fn a_mismatched_approval_and_reason_is_an_internal_error_and_writes_nothing() {
    for (reason, target, approval) in [
        (Reason::Documented, "docs/a.md", Approval::Models),
        (Reason::Duplicate, "hs-n1", Approval::GitGrep),
        (Reason::Superseded, "hs-d1", Approval::GitGrep),
    ] {
        let mut f = fixture();
        let err = propose(&mut f, &[proposal("hs-n2", reason, target, approval)]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::Internal);
        assert_eq!(f.count("SELECT count(*) FROM record_retirements"), 0);
        assert_eq!(
            f.count(
                "SELECT count(*) FROM events WHERE kind IN ('context-curated', 'records-retired')"
            ),
            0
        );
        assert_eq!(f.count("SELECT count(*) FROM record_curations"), 0);
    }
}

#[test]
fn an_approved_retirement_leaves_force() {
    let mut f = fixture();
    assert!(f.in_force().contains("hs-n2"));
    propose(
        &mut f,
        &[proposal(
            "hs-n2",
            Reason::Duplicate,
            "hs-n1",
            Approval::Models,
        )],
    )
    .unwrap();
    let force = f.in_force();
    assert!(!force.contains("hs-n2"));
    assert!(force.contains("hs-n1"));
}

#[test]
fn decide_pending_approves_and_rejects_with_the_decider_and_one_event() {
    let mut f = fixture();
    f.seed(
        "hs-n2",
        "duplicate",
        "hs-n1",
        "pending",
        "2026-11-01T01:00:00.000Z",
    );
    f.seed(
        "hs-n3",
        "superseded",
        "hs-d1",
        "pending",
        "2026-11-01T02:00:00.000Z",
    );
    let sweep = [
        Sweep {
            record: id("hs-n2"),
            approve: true,
        },
        Sweep {
            record: id("hs-n3"),
            approve: false,
        },
    ];
    assert_eq!(
        decide_pending(&mut f.store, &context(), &sweep).unwrap(),
        (1, 1)
    );
    let (status, at, by, _) = f.row("hs-n2");
    assert_eq!(status, "approved");
    assert!(at.is_some());
    assert_eq!(by.as_deref(), Some(DECIDER));
    let (status, at, by, _) = f.row("hs-n3");
    assert_eq!(status, "rejected");
    assert!(at.is_some());
    assert_eq!(by.as_deref(), Some(DECIDER));
    assert_eq!(f.retired_events(), 1);
    assert!(!f.in_force().contains("hs-n2"));
    assert!(f.in_force().contains("hs-n3"));
}

#[test]
fn decide_pending_skips_a_row_no_longer_pending_or_whose_target_left_force() {
    let mut f = fixture();
    f.seed(
        "hs-n2",
        "duplicate",
        "hs-n1",
        "approved",
        "2026-11-01T01:00:00.000Z",
    );
    // hs-n3 duplicates hs-n2, which the approved row above retired.
    f.seed(
        "hs-n3",
        "duplicate",
        "hs-n2",
        "pending",
        "2026-11-01T02:00:00.000Z",
    );
    f.seed(
        "hs-n4",
        "documented",
        "docs/a.md",
        "pending",
        "2026-11-01T03:00:00.000Z",
    );
    let sweep = [
        Sweep {
            record: id("hs-n2"),
            approve: false,
        },
        Sweep {
            record: id("hs-n3"),
            approve: true,
        },
        Sweep {
            record: id("hs-n4"),
            approve: true,
        },
    ];
    assert_eq!(
        decide_pending(&mut f.store, &context(), &sweep).unwrap(),
        (0, 0)
    );
    assert_eq!(f.row("hs-n2").0, "approved");
    assert_eq!(f.row("hs-n3").0, "pending");
    assert_eq!(f.row("hs-n4").0, "pending");
    assert_eq!(f.retired_events(), 0);
    // A rejection needs neither record nor target in force.
    let reject = [Sweep {
        record: id("hs-n3"),
        approve: false,
    }];
    assert_eq!(
        decide_pending(&mut f.store, &context(), &reject).unwrap(),
        (0, 1)
    );
    assert_eq!(f.row("hs-n3").0, "rejected");
    assert_eq!(f.retired_events(), 1);
}

#[test]
fn pending_pairs_lists_the_oldest_pending_duplicate_and_superseded_rows_only() {
    let mut f = fixture();
    f.seed(
        "hs-n4",
        "duplicate",
        "hs-n1",
        "pending",
        "2026-11-01T01:00:00.000Z",
    );
    f.seed(
        "hs-n3",
        "superseded",
        "hs-d1",
        "pending",
        "2026-11-01T01:00:00.000Z",
    );
    f.seed(
        "hs-n2",
        "duplicate",
        "hs-n1",
        "pending",
        "2026-11-01T00:30:00.000Z",
    );
    f.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO record_retirements (context, record, reason, target, status, proposed_at) VALUES ('ctx', 'hs-d1', 'documented', 'docs/a.md', 'pending', '2026-11-01T00:00:00.000Z')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    f.seed(
        "hs-n1",
        "duplicate",
        "hs-n2",
        "approved",
        "2026-11-01T00:00:00.000Z",
    );
    let pairs = pending_pairs(&mut f.store, &context(), 2).unwrap();
    let got: Vec<(String, String, Reason)> = pairs
        .into_iter()
        .map(
            |PendingPair {
                 record,
                 target,
                 reason,
             }| (record.to_string(), target.to_string(), reason),
        )
        .collect();
    assert_eq!(
        got,
        vec![
            ("hs-n2".to_string(), "hs-n1".to_string(), Reason::Duplicate),
            ("hs-n3".to_string(), "hs-d1".to_string(), Reason::Superseded),
        ]
    );
    assert_eq!(
        pending_pairs(&mut f.store, &context(), 10).unwrap().len(),
        3
    );
}
