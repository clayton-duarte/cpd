//! Pure parts of the session tree: which rows are visible, which row a click lands on, and the state icons.

use ratatui::style::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A Context, by group index.
    Context(usize),
    /// A session, by index into the sessions flattened in group order.
    Session(usize),
}

/// One Context group as the tree sees it: how many sessions it holds and whether it is collapsed.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    pub sessions: usize,
    pub collapsed: bool,
}

/// The visible rows, top to bottom: each Context row, then its sessions unless it is collapsed.
pub fn visible_rows(groups: &[Shape]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut next = 0;
    for (g, shape) in groups.iter().enumerate() {
        rows.push(Row::Context(g));
        if !shape.collapsed {
            rows.extend((next..next + shape.sessions).map(Row::Session));
        }
        next += shape.sessions;
    }
    rows
}

/// The index into `visible_rows` of a click on screen row `y`, when the tree's first row is at `top`.
pub fn row_at(groups: &[Shape], top: u16, y: u16) -> Option<usize> {
    let i = y.checked_sub(top)? as usize;
    (i < visible_rows(groups).len()).then_some(i)
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

    fn shapes(spec: &[(usize, bool)]) -> Vec<Shape> {
        spec.iter().map(|&(sessions, collapsed)| Shape { sessions, collapsed }).collect()
    }

    #[test]
    fn expanded_lists_each_context_then_its_sessions() {
        let g = shapes(&[(2, false), (1, false)]);
        assert_eq!(visible_rows(&g), vec![Row::Context(0), Row::Session(0), Row::Session(1), Row::Context(1), Row::Session(2)]);
    }

    #[test]
    fn collapsed_hides_only_its_own_sessions() {
        let g = shapes(&[(2, true), (1, false)]);
        assert_eq!(visible_rows(&g), vec![Row::Context(0), Row::Context(1), Row::Session(2)]);
    }

    #[test]
    fn a_click_lands_on_the_row_under_it() {
        let g = shapes(&[(2, false), (1, true)]);
        let rows = visible_rows(&g);
        assert_eq!(row_at(&g, 0, 1).map(|i| rows[i]), Some(Row::Session(0)));
        assert_eq!(row_at(&g, 0, 3).map(|i| rows[i]), Some(Row::Context(1)));
        assert_eq!(row_at(&g, 0, 4), None);
    }

    #[test]
    fn rows_are_counted_from_the_top_of_the_pane() {
        let g = shapes(&[(2, false)]);
        assert_eq!(row_at(&g, 5, 4), None);
        assert_eq!(row_at(&g, 5, 6), Some(1));
    }

    #[test]
    fn no_sessions_no_rows() {
        assert!(visible_rows(&[]).is_empty());
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
