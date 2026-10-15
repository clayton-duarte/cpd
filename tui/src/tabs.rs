//! The Plan view's top row: only the Context's name and its tabs (one per Lead session), over a separator line.
//! The active tab is reversed; it is the only marker (the Context tree has its own `⏵`). A click on a tab switches to it.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::separator;

pub struct Tab {
    pub id: String,
    pub label: String,
    pub active: bool,
}

/// The rows the top takes: the tabs and the separator under them.
pub const HEIGHT: u16 = 2;

/// Draw the top row into `area` (`HEIGHT` rows). Returns the row of the tabs and each tab's `[from, to)` columns, for clicks.
pub fn draw(f: &mut Frame, area: Rect, context: Option<&str>, tabs: &[Tab]) -> (Rect, Vec<(u16, u16, String)>) {
    let [row, line] = Layout::vertical([Constraint::Length(HEIGHT - separator::HEIGHT), Constraint::Length(separator::HEIGHT)]).areas(area);
    separator::draw(f, line);
    let mut spans: Vec<Span> = Vec::new();
    let mut ranges = Vec::new();
    match context {
        Some(name) => {
            spans.push(format!(" {name} ▸ ").dim());
            let mut x = row.x + Line::from(spans.clone()).width() as u16;
            for t in tabs {
                let label = format!(" {} ", t.label);
                let w = Line::from(label.as_str()).width() as u16;
                ranges.push((x, x + w, t.id.clone()));
                spans.push(if t.active { label.reversed().bold() } else { label.into() });
                spans.push(" ".into());
                x += w + 1;
            }
        }
        None => spans.push(" no Lead session in the centre".dim()),
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row);
    (row, ranges)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::{self, Meaning};
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};

    fn render(context: Option<&str>, tabs: &[Tab], width: u16) -> (Buffer, Rect, Vec<(u16, u16, String)>) {
        let mut terminal = Terminal::new(TestBackend::new(width, 3)).unwrap();
        let mut out = None;
        terminal.draw(|f| out = Some(draw(f, Rect::new(0, 0, width, HEIGHT), context, tabs))).unwrap();
        let (row, ranges) = out.unwrap();
        (terminal.backend().buffer().clone(), row, ranges)
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    fn tab(id: &str, label: &str, active: bool) -> Tab {
        Tab { id: id.into(), label: label.into(), active }
    }

    #[test]
    fn the_row_holds_only_the_context_name_and_the_tabs() {
        let (buf, row, _) = render(Some("cpd"), &[tab("a", "v1", false), tab("b", "cpd-ui", true)], 30);
        assert_eq!(row.y, 0);
        assert_eq!(text(&buf, 0), " cpd ▸  v1   cpd-ui");
    }

    #[test]
    fn a_separator_line_runs_under_the_tabs_across_the_pane_in_the_dim_colour() {
        let (buf, _, _) = render(Some("cpd"), &[tab("a", "v1", true)], 12);
        assert_eq!(text(&buf, 1), "─".repeat(12));
        assert_eq!(buf[(3, 1)].fg, palette::color(Meaning::Nothing));
        assert_eq!(text(&buf, 2), "");
    }

    #[test]
    fn only_the_active_tab_is_reversed_and_there_is_no_play_mark() {
        let (buf, _, ranges) = render(Some("cpd"), &[tab("a", "v1", false), tab("b", "cpd-ui", true)], 30);
        let reversed = |x: u16| buf[(x, 0)].modifier.contains(Modifier::REVERSED);
        assert_eq!(ranges, [(7, 11, "a".into()), (12, 20, "b".into())]);
        assert!(!(ranges[0].0..ranges[0].1).any(reversed));
        assert!((ranges[1].0..ranges[1].1).all(reversed));
        assert!(!text(&buf, 0).contains(crate::context::ACTIVE_MARK));
    }

    #[test]
    fn without_a_centre_the_row_says_so() {
        let (buf, _, ranges) = render(None, &[], 40);
        assert_eq!(text(&buf, 0), " no Lead session in the centre");
        assert!(ranges.is_empty());
    }
}
