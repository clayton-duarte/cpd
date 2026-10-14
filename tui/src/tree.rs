//! Pure parts of the session tree: which rows are visible, which row a click lands on, and the state icons.

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Context,
    Plan(usize),
}

/// The visible rows, top to bottom: the Context row, then one row per Plan unless the Context is collapsed.
pub fn visible_rows(plans: usize, collapsed: bool) -> Vec<Row> {
    let mut rows = vec![Row::Context];
    if !collapsed {
        rows.extend((0..plans).map(Row::Plan));
    }
    rows
}

/// The index into `visible_rows` of a click on screen row `y`, when the tree's first row is at `top`.
pub fn row_at(plans: usize, collapsed: bool, top: u16, y: u16) -> Option<usize> {
    let i = y.checked_sub(top)? as usize;
    (i < visible_rows(plans, collapsed).len()).then_some(i)
}

/// The icon and colour of a Plan's chat pane `@agent_state`. Anything not listed is unknown.
pub fn state_icon(state: &str) -> (&'static str, Color) {
    match state {
        "working" => ("●", Color::Yellow),
        "blocked" => ("◆", Color::Magenta),
        "idle" => ("○", Color::Green),
        "in-herdr" => ("⏸", Color::DarkGray),
        "stopped" => ("■", Color::Red),
        _ => ("?", Color::DarkGray),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expanded_lists_the_context_then_every_plan() {
        assert_eq!(visible_rows(2, false), vec![Row::Context, Row::Plan(0), Row::Plan(1)]);
    }

    #[test]
    fn collapsed_hides_the_plans() {
        assert_eq!(visible_rows(2, true), vec![Row::Context]);
    }

    #[test]
    fn a_click_lands_on_the_row_under_it_when_expanded() {
        let rows = visible_rows(2, false);
        assert_eq!(row_at(2, false, 0, 0).map(|i| rows[i]), Some(Row::Context));
        assert_eq!(row_at(2, false, 0, 1).map(|i| rows[i]), Some(Row::Plan(0)));
        assert_eq!(row_at(2, false, 0, 2).map(|i| rows[i]), Some(Row::Plan(1)));
        assert_eq!(row_at(2, false, 0, 3), None);
    }

    #[test]
    fn a_click_lands_on_the_row_under_it_when_collapsed() {
        let rows = visible_rows(2, true);
        assert_eq!(row_at(2, true, 0, 0).map(|i| rows[i]), Some(Row::Context));
        assert_eq!(row_at(2, true, 0, 1), None);
    }

    #[test]
    fn rows_are_counted_from_the_top_of_the_pane() {
        assert_eq!(row_at(2, false, 5, 4), None);
        assert_eq!(row_at(2, false, 5, 6), Some(1));
    }

    #[test]
    fn every_state_has_its_icon() {
        let icons: Vec<&str> = ["working", "blocked", "idle", "in-herdr", "stopped", ""].iter().map(|s| state_icon(s).0).collect();
        assert_eq!(icons, ["●", "◆", "○", "⏸", "■", "?"]);
        assert_eq!(state_icon("working").1, Color::Yellow);
        assert_eq!(state_icon("blocked").1, Color::Magenta);
        assert_eq!(state_icon("idle").1, Color::Green);
        assert_eq!(state_icon("stopped").1, Color::Red);
        assert_eq!(state_icon("bogus").0, "?");
    }
}
