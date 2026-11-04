//! Record dates in `cpd context show` and in the distiller's `records.md` are the UTC date, labelled `UTC`.

use super::*;
use crate::context::distill::records_md;
use crate::context::record::{By, Draft};
use crate::ids::RecordId;

fn record() -> Record {
    Record {
        id: RecordId::parse("pro-d41").unwrap(),
        at: "2026-10-26T03:03:34.352Z".into(),
        by: By {
            session: None,
            plan: None,
        },
        text: "Use local time.".into(),
        draft: Draft::Decision { supersedes: vec![] },
    }
}

#[test]
fn context_show_dates_are_labelled_utc() {
    let md = markdown("Building", None, &[(1, record())], 0, &Repos::new());
    assert!(md.contains("(2026-10-26 UTC)"), "{md}");
    let one = single("Building", None, &record());
    assert!(
        one.contains("## pro-d41 (decision, 2026-10-26 UTC)"),
        "{one}"
    );
}

#[test]
fn distill_records_dates_are_labelled_utc() {
    let indexed = (1, record());
    let md = records_md(&[&indexed]);
    assert!(md.contains("(2026-10-26 UTC)"), "{md}");
}
