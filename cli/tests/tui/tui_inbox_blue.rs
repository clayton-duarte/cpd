//! The Inbox's blue tier, "external support": glyph, colour and legend line (commands.md §13.2 "Inbox section", §17 legend).
//! The legend is drawn with ratatui's `TestBackend`.

use cpd_core::inbox::model::{Kind, Urgency};
use cpd_tui::palette;
use cpd_tui::widgets::legend;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

const BLUE: &str = "\u{F0368}";

#[test]
fn the_blue_tier_glyph_is_f0368_in_bright_blue_12() {
    let span = palette::tier_span(Urgency::Blue);
    assert_eq!(span.content, BLUE);
    assert_eq!(
        span.style.fg,
        Some(Color::LightBlue),
        "bright blue (12): a clickable Inbox row"
    );
    assert_eq!(
        palette::item_span(Kind::ReviewRequested, Urgency::Blue),
        ratatui::text::Span::styled("\u{F12F2}", span.style)
    );
    for other in [Urgency::Cyan, Urgency::Yellow] {
        let o = palette::tier_span(other);
        assert!(o.content != span.content, "{other:?} glyph differs");
        assert!(o.style.fg != span.style.fg, "{other:?} colour differs");
    }
}

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
fn the_legends_inbox_section_has_the_blue_tier_line_between_the_magenta_and_cyan_lines() {
    let buf = legend_buffer();
    let lines = rows(&buf);
    let at = |text: &str| {
        lines
            .iter()
            .position(|r| r.contains(text))
            .unwrap_or_else(|| panic!("no line {text:?}:\n{}", lines.join("\n")))
    };
    let blue = at("external support: a review asked of you");
    assert!(
        lines[blue].contains("blue"),
        "the line names the tier: {:?}",
        lines[blue]
    );
    let glyph = (0..buf.area.width)
        .find(|x| buf[(*x, blue as u16)].symbol() == BLUE)
        .expect("U+F0368 on the blue line");
    assert_eq!(buf[(glyph, blue as u16)].fg, Color::LightBlue);
    assert!(
        at("a decision before the work runs") < blue,
        "below the magenta line"
    );
    assert!(
        blue < at("a cpd notice: curation, nothing blocks"),
        "above the cyan line"
    );
}
