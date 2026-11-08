//! Red test for pinning the selected Manager's Context first: a live Context whose Manager is in the chat slot
//! (`manager_place == Some(Place::Chat)`) is ordered before every other live Context, whatever its urgency tier.
//! The impl Job adds the rule to `rows::project_items`; this fails while the live Contexts are ordered by
//! `dot.order_key(&created_at)` alone.

use std::collections::HashSet;

use super::fixture::{context, managed, tree};
use super::rows;
use crate::read::{Dot, Place, Tree};

/// The Context title rows of the drawn project tree, in their drawn order, as their ids. A Context with no rows
/// closes with a toggle row whose path starts with `╰`; the spacer between two blocks starts with a space. Neither
/// is a title row.
fn context_order(t: &Tree) -> Vec<String> {
    rows::items(t, &HashSet::new(), 44, 0)
        .into_iter()
        .map(|r| r.path)
        .filter(|p| p.len() == 1 && !p[0].starts_with('╰') && !p[0].starts_with(' '))
        .map(|p| p[0].clone())
        .collect()
}

#[test]
fn pins_selected_manager_first() {
    // Live Contexts in four distinct urgency tiers; the selected Manager's Context sits in the last tier, so today's
    // tier ordering (red, yellow, magenta, then the idle one) leaves it last.
    let red = managed(context("red", vec![]), Dot::Red);
    let yellow = managed(context("yellow", vec![]), Dot::Yellow);
    let magenta = managed(context("magenta", vec![]), Dot::Magenta);
    let mut selected = managed(context("selected", vec![]), Dot::Idle);
    selected.manager_place = Some(Place::Chat);

    let t = tree(vec![red, yellow, magenta, selected]);
    let order = context_order(&t);

    assert_eq!(
        order.first().map(String::as_str),
        Some("selected"),
        "the selected Manager's Context is pinned first, above the red one: {order:?}"
    );
    // The rest keep the tier order (red, yellow, magenta).
    assert_eq!(
        order.get(1..).map(|rest| rest.to_vec()),
        Some(vec![
            "red".to_owned(),
            "yellow".to_owned(),
            "magenta".to_owned(),
        ]),
        "the unselected Contexts keep the tier order: {order:?}"
    );
}
