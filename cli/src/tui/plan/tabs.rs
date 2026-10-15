//! The plan view's top row, ported from the cockpit's `tabs.rs` and `separator.rs`: only the Context's id and its tabs
//! (one per Lead Session of the Context with a pane in the cockpit), over a separator line. Nothing at the right.
//! The active tab is reversed and bold; it is the only marker. A click on a tab opens that Lead in the chat slot.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::tui::palette::{self, Meaning};
use crate::ids::SessionId;

pub struct Tab {
    pub id: SessionId,
    pub label: String,
    pub active: bool,
}

/// The rows the top takes: the tabs and the separator under them.
pub const HEIGHT: u16 = 2;

/// The row of the tabs and each tab's `[from, to)` columns, for clicks.
pub type Hits = Vec<(u16, u16, SessionId)>;

/// Draw the top row into `area` (`HEIGHT` rows). Returns the tabs' row and their columns.
pub fn draw(f: &mut Frame, area: Rect, context: Option<&str>, tabs: &[Tab]) -> (Rect, Hits) {
    let [row, line] = Layout::vertical([Constraint::Length(HEIGHT - 1), Constraint::Length(1)]).areas(area);
    // `Borders::TOP` of a one-row block is the line itself.
    f.render_widget(Block::new().borders(Borders::TOP).border_style(palette::fg(Meaning::Nothing)), line);
    let mut spans: Vec<Span> = Vec::new();
    let mut hits = Vec::new();
    match context {
        Some(name) => {
            spans.push(format!(" {name} ▸ ").dim());
            let mut x = row.x + Line::from(spans.clone()).width() as u16;
            for t in tabs {
                let label = format!(" {} ", t.label);
                let w = Line::from(label.as_str()).width() as u16;
                hits.push((x, x + w, t.id.clone()));
                spans.push(if t.active { label.reversed().bold() } else { label.into() });
                spans.push(" ".into());
                x += w + 1;
            }
        }
        None => spans.push(" no Lead session in the centre".dim()),
    }
    f.render_widget(Paragraph::new(Line::from(spans)), row);
    (row, hits)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};

    use super::*;

    fn render(context: Option<&str>, tabs: &[Tab], width: u16) -> (Buffer, Rect, Hits) {
        let mut terminal = Terminal::new(TestBackend::new(width, 3)).unwrap();
        let mut out = None;
        terminal.draw(|f| out = Some(draw(f, Rect::new(0, 0, width, HEIGHT), context, tabs))).unwrap();
        let (row, hits) = out.unwrap();
        (terminal.backend().buffer().clone(), row, hits)
    }

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    fn tab(id: &str, label: &str, active: bool) -> Tab {
        Tab { id: SessionId::parse(id).unwrap(), label: label.into(), active }
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
        let (buf, _, hits) = render(Some("cpd"), &[tab("a", "v1", false), tab("b", "cpd-ui", true)], 30);
        let reversed = |x: u16| buf[(x, 0)].modifier.contains(Modifier::REVERSED);
        let ids: Vec<(u16, u16, &str)> = hits.iter().map(|(a, b, id)| (*a, *b, id.as_str())).collect();
        assert_eq!(ids, [(7, 11, "a"), (12, 20, "b")]);
        assert!(!(hits[0].0..hits[0].1).any(reversed));
        assert!((hits[1].0..hits[1].1).all(reversed));
        assert!(!text(&buf, 0).contains("⏵")); // the tree's `ACTIVE_MARK`
    }

    #[test]
    fn the_active_tab_is_bold_too() {
        let (buf, _, hits) = render(Some("cpd"), &[tab("a", "v1", false), tab("b", "cpd-ui", true)], 30);
        let bold = |x: u16| buf[(x, 0)].modifier.contains(Modifier::BOLD);
        assert!(!(hits[0].0..hits[0].1).any(bold));
        assert!((hits[1].0..hits[1].1).all(bold));
    }

    #[test]
    fn without_a_centre_the_row_says_so() {
        let (buf, _, hits) = render(None, &[], 40);
        assert_eq!(text(&buf, 0), " no Lead session in the centre");
        assert!(hits.is_empty());
    }
}
