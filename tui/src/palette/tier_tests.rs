//! pro-d401: the curate-retire notice is white, not cyan.

use super::*;
use cpd_core::inbox::model::Kind;

#[test]
fn a_curate_retire_item_is_painted_white() {
    let span = item_span(Kind::CurateRetire, Urgency::White);
    assert_eq!(span.content, ITEM_RETIRE, "its own glyph");
    assert_eq!(
        span.style.fg,
        tier_span(Urgency::White).style.fg,
        "painted white, as the white tier is"
    );
}
