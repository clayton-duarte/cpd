//! The Job pill's text (commands.md §13.2 'Chart', contract §6.1.1): `<icon> <key>[│#<n>]│<tier>`, `#n` only from the second Attempt.

use cpd_core::config::Tier;

use crate::plan::flow::{layout, FlowJob, Live};
use crate::test_text::rows;

const CAP_L: &str = "\u{e0b6}";
const CAP_R: &str = "\u{e0b4}";

fn job(key: &str, attempt: usize) -> FlowJob {
    FlowJob { key: key.into(), status: Live::Queued, tier: Some(Tier::parse("S1").unwrap()), needs: vec![], attempt }
}

#[test]
fn a_pill_from_the_second_attempt_has_the_attempt_before_the_tier() {
    let icon = Live::Queued.icon(0).0;
    for (attempt, body) in [(0, "a│S1"), (1, "a│S1"), (2, "a│#2│S1"), (3, "a│#3│S1"), (12, "a│#12│S1")] {
        let c = layout(&[job("a", attempt)], 0);
        let r = c.pills[0].rect;
        let cells: String = (r.x..r.right()).map(|x| c.buf[(x, r.y)].symbol()).collect();
        assert_eq!(cells, format!("{CAP_L}{icon} {body}{CAP_R}"), "attempt {attempt}");
    }
}

#[test]
fn the_attempt_before_the_tier_keeps_the_tiers_right_aligned() {
    let c = layout(&[job("a", 1), job("a-longer-key", 2), job("b", 1)], 0);
    let want = "󰝦 a              │S1\n\n󰝦 a-longer-key│#2│S1\n\n󰝦 b              │S1";
    assert_eq!(rows(&c.buf).join("\n"), want);
}
