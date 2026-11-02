//! The retired `curate-docs` kind in the cockpit: its glyph is its tier's and the legend has no row for it
//! (commands.md §13.2, §17; drop-curate-docs).

use cpd_core::inbox::model::{Kind, Urgency};
use cpd_tui::palette;
use cpd_tui::widgets::legend;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

const DOCS_GLYPH: &str = "\u{F0653}";

fn legend_buffer() -> Buffer {
    let (w, h) = legend::size();
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| legend::draw(f, f.area())).unwrap();
    terminal.backend().buffer().clone()
}

fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_owned())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn the_legend_has_no_documentation_candidate_row() {
    let buf = legend_buffer();
    let lines = rows(&buf);
    let text = lines.join("\n");
    assert!(
        !lines.iter().any(|r| r.contains("documentation candidate")),
        "no documentation candidate row:\n{text}"
    );
    assert!(!text.contains(DOCS_GLYPH), "no U+F0653 cell:\n{text}");
    assert!(
        lines.iter().any(|r| r.contains("a record to retire")),
        "the retire row stays:\n{text}"
    );
}

#[test]
fn a_curate_docs_history_row_draws_its_tier_glyph() {
    let span = palette::item_span(Kind::CurateDocs, Urgency::Cyan);
    assert_eq!(span, palette::tier_span(Urgency::Cyan));
    assert_ne!(span.content, DOCS_GLYPH);
}
