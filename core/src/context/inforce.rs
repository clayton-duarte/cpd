//! The records in force of a Context (contract §4.6 "Records in force", hs-d194 D2).
//!
//! Pure over records already read in one transaction: the digest cites exactly
//! these, and a first message is built from them.

use std::collections::HashSet;

use super::render::Indexed;
use crate::error::CpdError;
use crate::ids::{ContextId, RecordId, RecordKind};

/// The records of `records` (index order, as read) that are in force as of
/// `as_of`: those with `seq <= as_of` of kind `decision` or `note` that no decision with `seq <= as_of` names in
/// its `supersedes`. A `plan` or `brief` record is never in force. The result
/// keeps the order of `records`.
pub fn in_force(records: &[Indexed], as_of: i64) -> Vec<&Indexed> {
    in_force_without(records, as_of, &HashSet::new())
}

/// The records of `Context` that an approved retirement took out of force (contract §4.6 "Retirement", hs-d549).
pub fn retired(
    conn: &rusqlite::Connection,
    context: &ContextId,
) -> Result<HashSet<RecordId>, CpdError> {
    let mut stmt = conn.prepare(
        "SELECT record FROM record_retirements WHERE context = ?1 AND status = 'approved'",
    )?;
    let ids = stmt
        .query_map([context.as_str()], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.iter().map(|id| RecordId::parse(id)).collect()
}

/// [`in_force`] that also leaves out the `retired` records (read with [`retired`]).
pub fn in_force_without<'a>(
    records: &'a [Indexed],
    as_of: i64,
    retired: &HashSet<RecordId>,
) -> Vec<&'a Indexed> {
    let considered = || records.iter().filter(|(seq, _)| *seq <= as_of);
    let superseded: HashSet<_> = considered().flat_map(|(_, r)| r.supersedes()).collect();
    considered()
        .filter(|(_, r)| {
            matches!(r.kind(), RecordKind::Decision | RecordKind::Note)
                && !superseded.contains(&r.id)
                && !retired.contains(&r.id)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{By, Draft, PlanRef, Record};
    use crate::ids::{MachineName, PlanId, RecordId};

    fn id(s: &str) -> RecordId {
        RecordId::parse(s).unwrap()
    }

    fn record(s: &str, draft: Draft) -> Record {
        Record {
            id: id(s),
            at: "2026-10-18T05:53:00.000Z".into(),
            by: By {
                session: None,
                plan: None,
            },
            text: format!("text of {s}"),
            draft,
        }
    }

    fn decision(s: &str, supersedes: Option<&str>) -> Record {
        record(
            s,
            Draft::Decision {
                supersedes: supersedes.map(id).into_iter().collect(),
            },
        )
    }

    fn plan_ref() -> PlanRef {
        PlanRef {
            id: PlanId::parse("cross-repo").unwrap(),
            machine: MachineName::parse("hs").unwrap(),
            jira: None,
        }
    }

    fn ids(found: &[&Indexed]) -> Vec<String> {
        found.iter().map(|(_, r)| r.id.to_string()).collect()
    }

    /// Records numbered by position, from 1.
    fn indexed(records: Vec<Record>) -> Vec<Indexed> {
        records
            .into_iter()
            .enumerate()
            .map(|(i, r)| (i as i64 + 1, r))
            .collect()
    }

    #[test]
    fn a_retired_record_is_out_and_the_rest_stay() {
        let records = indexed(vec![
            decision("hs-d1", None),
            record("hs-n1", Draft::Note),
            record("hs-n2", Draft::Note),
        ]);
        let retired: HashSet<RecordId> = [id("hs-n1"), id("hs-d1")].into();
        assert_eq!(ids(&in_force_without(&records, 3, &retired)), ["hs-n2"]);
        assert_eq!(ids(&in_force(&records, 3)), ["hs-d1", "hs-n1", "hs-n2"]);
    }

    #[test]
    fn a_superseded_decision_is_out_and_its_successor_in() {
        let records = indexed(vec![
            decision("hs-d1", None),
            decision("hs-d2", Some("hs-d1")),
            decision("hs-d3", None),
        ]);
        assert_eq!(ids(&in_force(&records, 3)), ["hs-d2", "hs-d3"]);
    }

    #[test]
    fn a_note_or_a_decision_supersedes_a_note() {
        let note = |s: &str, over: Option<&str>| match over {
            Some(o) => record(
                s,
                Draft::NoteSupersedes {
                    supersedes: vec![id(o)],
                },
            ),
            None => record(s, Draft::Note),
        };
        let records = indexed(vec![
            note("hs-n1", None),
            note("hs-n2", Some("hs-n1")),
            note("hs-n3", None),
            decision("hs-d1", Some("hs-n3")),
        ]);
        assert_eq!(ids(&in_force(&records, 4)), ["hs-n2", "hs-d1"]);
        assert_eq!(
            ids(&in_force(&records, 3)),
            ["hs-n2", "hs-n3"],
            "a later supersede does not count"
        );
    }

    #[test]
    fn a_chain_of_supersedes_leaves_only_its_last_link() {
        let records = indexed(vec![
            decision("hs-d1", None),
            decision("hs-d2", Some("hs-d1")),
            decision("hs-d3", Some("hs-d2")),
        ]);
        assert_eq!(ids(&in_force(&records, 3)), ["hs-d3"]);
    }

    #[test]
    fn two_decisions_superseding_one_record_both_stay_and_the_old_one_goes() {
        let records = indexed(vec![
            decision("hs-d1", None),
            decision("hs-d2", Some("hs-d1")),
            decision("hs-d3", Some("hs-d1")),
        ]);
        assert_eq!(ids(&in_force(&records, 3)), ["hs-d2", "hs-d3"]);
    }

    #[test]
    fn as_of_a_seq_a_later_supersede_does_not_count() {
        let records = indexed(vec![
            decision("hs-d1", None),
            decision("hs-d2", Some("hs-d1")),
        ]);
        assert_eq!(ids(&in_force(&records, 1)), ["hs-d1"]);
        assert_eq!(ids(&in_force(&records, 2)), ["hs-d2"]);
        assert!(in_force(&records, 0).is_empty());
    }

    #[test]
    fn plan_and_brief_records_are_never_in_force() {
        let records = indexed(vec![
            record("hs-p1", Draft::Plan(plan_ref())),
            record("hs-b1", Draft::Brief(plan_ref())),
            record("hs-n1", Draft::Note),
        ]);
        assert_eq!(ids(&in_force(&records, 3)), ["hs-n1"]);
    }

    #[test]
    fn seqs_need_not_be_contiguous() {
        let records = vec![
            (10, decision("hs-d1", None)),
            (40, decision("hs-d2", Some("hs-d1"))),
        ];
        assert_eq!(ids(&in_force(&records, 39)), ["hs-d1"]);
        assert_eq!(ids(&in_force(&records, 40)), ["hs-d2"]);
    }
}
