//! Red tests for archived Manager rows (commands.md §17; hs-d630, hs-d633): an archived Context's
//! archived Manager is a muted row inside its collapsible archived section, after the archived Plans and
//! forks, named `<context>-manager`. The impl Job wires the rows up; these fail while the stub does not.

use std::collections::HashSet;

use cpd_core::ids::ContextId;
use cpd_core::session::{Role, State};
use ratatui::text::Line;

use super::fixture::{WHEN, archived, session};
use super::rows;
use crate::palette;
use crate::read::Place;

/// The plain text of a row's line, styles ignored.
fn text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn an_archived_manager_is_a_muted_row_in_its_contexts_archived_section() {
    let mut tree = archived();
    // The manager test builds its own archived Manager: the shared `fixture::archived()` stays manager-free,
    // so the frozen scroll golden is untouched.
    let mut manager = session("m-zeta", "zeta-manager", State::Idle, None, Place::None);
    manager.role = Role::Manager;
    manager.context = ContextId::parse("zeta").unwrap();
    manager.archived_at = Some(WHEN.into());
    tree.contexts
        .iter_mut()
        .find(|c| c.id.as_str() == "zeta")
        .unwrap()
        .archived_managers
        .push(manager);
    let path = vec!["zeta".to_owned(), "m-zeta".to_owned()];
    // `zeta` unfolded: its title row and its closing row are both open.
    let mut opened = HashSet::new();
    opened.insert(vec!["zeta".to_owned()]);
    opened.insert(vec![rows::toggle_id("zeta")]);
    opened.insert(vec![rows::ARCHIVE.to_owned()]);

    let manager = tree
        .contexts
        .iter()
        .find(|c| c.id.as_str() == "zeta")
        .and_then(|c| {
            c.archived_managers
                .iter()
                .find(|s| s.id.as_str() == "m-zeta")
        })
        .expect("the archived Context `zeta` has an archived Manager `m-zeta`");
    let dot = palette::dot_span(manager.dot, 0, true);

    let flat = rows::items(&tree, &opened, 44, 0);
    let drawn = flat.iter().find(|r| r.path == path).unwrap_or_else(|| {
        panic!(
            "no row for zeta's archived Manager: {:?}",
            flat.iter().map(|r| text(&r.line)).collect::<Vec<_>>()
        )
    });
    assert_eq!(
        text(&drawn.line),
        format!("├─{} zeta-manager", dot.content),
        "{:?}",
        flat.iter().map(|r| text(&r.line)).collect::<Vec<_>>()
    );
    let shown_dot = drawn
        .line
        .spans
        .iter()
        .find(|s| s.content == dot.content)
        .expect("the row's dot");
    assert_eq!(
        shown_dot.style,
        palette::archived_text(),
        "the archived Manager's dot is muted (colour 8)"
    );

    assert!(
        matches!(rows::row(&tree, &path), Some(rows::Row::Manager(c, s)) if c.id.as_str() == "zeta" && s.id.as_str() == "m-zeta"),
        "the archived Manager row resolves to its Context's archived Manager"
    );
}
