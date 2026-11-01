//! Red tests for audit-plan F1 in the Plan view's chart: a pill's key is cut in display columns, not `char`s.

use ratatui::text::Line;

use super::{FlowJob, Live, Tier, pill_parts};

#[test]
fn a_pill_key_in_wide_characters_is_cut_to_its_columns() {
    // Six `計` (U+8A08): six `char`s, twelve display columns. Cut to five columns it is two `計` and `…` (five
    // columns); counted in `char`s it keeps four `計` and `…` (nine columns) and overflows the key's budget.
    let job = FlowJob {
        key: "計計計計計計".to_owned(),
        status: Live::Queued,
        tier: Some(Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        attempt: 1,
    };
    let (left, _) = pill_parts(&job, 5, 0);
    let icon = job.status.icon(0).0;
    let key = left
        .strip_prefix(icon)
        .expect("the pill starts with its status icon")
        .trim_start_matches(' ');
    assert!(
        Line::from(key).width() <= 5,
        "a five-column key is not wider than five columns: {left:?}"
    );
}
