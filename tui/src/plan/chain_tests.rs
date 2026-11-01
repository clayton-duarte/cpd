//! A Job chained after the merged `open` Job is drawn in a column right of it (commands.md §5.1).

use super::flow::{FlowJob, Live, layout};
use crate::test_text::rows;
use cpd_core::config::Tier;

fn job(key: &str, status: Live, needs: &[&str]) -> FlowJob {
    FlowJob {
        key: key.into(),
        status,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: needs.iter().map(|s| s.to_string()).collect(),
        attempt: 1,
    }
}

#[test]
fn an_open_job_then_a_fix_and_a_second_fix_are_drawn_in_columns_left_to_right() {
    let c = layout(
        &[
            job("open", Live::Merged, &[]),
            job("fix1", Live::Merged, &["open"]),
            job("fix2", Live::Queued, &["fix1"]),
        ],
        0,
    );
    let text = rows(&c.buf).join("\n");
    let (merged, queued) = (Live::Merged.icon(0).0, Live::Queued.icon(0).0);
    let (l, r) = ("\u{e0b6}", "\u{e0b4}");
    assert_eq!(
        text,
        format!("{l}{merged} open│S1{r}─────{l}{merged} fix1│S1{r}─────{l}{queued} fix2│S1{r}")
    );
}
