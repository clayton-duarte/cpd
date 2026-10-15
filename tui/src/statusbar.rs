//! The StatusBar: the one-row pane across the bottom of the cockpit. Items go in a left area (from the left edge, in
//! order) and a right area (in order, ending at the right edge); the space between them expands.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    text::Line,
    widgets::Paragraph,
};

/// What a click on an item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Legend,
    ResetLayout,
}

/// One entry of the bar: its text, and what a click on it does (`None`: not clickable).
pub struct Item {
    pub line: Line<'static>,
    pub action: Option<Action>,
}

/// Draw the bar in `area`: `left` items, an expanding middle, `right` items. Returns the `Rect` of each clickable item.
pub fn draw(f: &mut Frame, area: Rect, left: &[Item], right: &[Item]) -> Vec<(Rect, Action)> {
    let width = |i: &Item| Constraint::Length(i.line.width() as u16);
    let constraints: Vec<Constraint> = left.iter().map(width).chain([Constraint::Fill(1)]).chain(right.iter().map(width)).collect();
    let rects = Layout::horizontal(constraints).spacing(1).split(area);
    let (left_rects, rest) = rects.split_at(left.len());
    let items = left.iter().zip(left_rects).chain(right.iter().zip(&rest[1..])); // rest[0] is the blank middle
    let mut clickable = Vec::new();
    for (item, rect) in items {
        f.render_widget(Paragraph::new(item.line.clone()), *rect);
        if let Some(action) = item.action {
            clickable.push((*rect, action));
        }
    }
    clickable
}

#[cfg(test)]
mod tests {
    use super::*;

    use ratatui::{Terminal, backend::TestBackend};

    fn item(text: &'static str, action: Option<Action>) -> Item {
        Item { line: Line::from(text), action }
    }

    fn row(width: u16, left: &[Item], right: &[Item]) -> (String, Vec<(Rect, Action)>) {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        let mut hits = Vec::new();
        terminal.draw(|f| hits = draw(f, f.area(), left, right)).unwrap();
        let buf = terminal.backend().buffer();
        ((0..width).map(|x| buf[(x, 0)].symbol()).collect(), hits)
    }

    #[test]
    fn left_items_start_at_column_0_and_right_items_end_at_the_last_column() {
        for width in [80u16, 215] {
            let left = [item(" ? legend ", Some(Action::Legend)), item("L2", None)];
            let right = [item("R1", None), item("workers 1/4", None)];
            let (text, hits) = row(width, &left, &right);
            assert!(text.starts_with(" ? legend  L2"), "{width}: {text:?}");
            assert!(text.ends_with("R1 workers 1/4"), "{width}: {text:?}");
            let middle = &text[" ? legend  L2".len()..text.len() - "R1 workers 1/4".len()];
            assert!(!middle.is_empty() && middle.trim().is_empty(), "{width}: blank middle, got {middle:?}");
            assert_eq!(hits, [(Rect::new(0, 0, 10, 1), Action::Legend)]);
        }
    }
}
