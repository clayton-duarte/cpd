//! The Inbox's notice tier is cyan; blue is only for focus (commands.md §13.2 "Inbox section", §17 legend; contract §6.1.1;
//! pro-d16). The legend is drawn with ratatui's `TestBackend`.

use cpd_tui::palette::{self, Meaning};
use cpd_tui::widgets::legend;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::Color;

/// The tier glyphs, repeated here as the tests of `tui_tree.rs` do: cyan.
const WHITE: &str = "\u{F0366}";

#[test]
fn the_notice_tiers_colour_is_cyan_6_read_only_and_14_clickable_never_blue() {
    assert_eq!(
        palette::color(Meaning::Notice),
        Color::Cyan,
        "base, read-only: 6"
    );
    assert_eq!(
        palette::color_clickable(Meaning::Notice),
        Color::LightCyan,
        "bright, clickable: 14"
    );
    assert_eq!(palette::fg(Meaning::Notice).fg, Some(Color::Cyan));
    for blue in [Color::Blue, Color::LightBlue] {
        assert!(
            palette::color(Meaning::Notice) != blue
                && palette::color_clickable(Meaning::Notice) != blue
        );
    }
}

// ---------- the legend ----------

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
fn the_legends_inbox_section_has_the_cyan_tier_line_in_bright_cyan_and_none_for_blue() {
    let buf = legend_buffer();
    let lines = rows(&buf);
    let notice = lines
        .iter()
        .position(|r| r.contains("a cpd notice: curation, nothing blocks"))
        .expect("the notice tier's line");
    let glyph = (0..buf.area.width)
        .find(|x| buf[(*x, notice as u16)].symbol() == WHITE)
        .expect("U+F0366 on the notice line");
    assert_eq!(
        buf[(glyph, notice as u16)].fg,
        palette::color_clickable(Meaning::NotStarted),
        "bright white (15): the legend's sample is drawn as the Inbox row is"
    );
    assert!(
        lines[notice].contains("white"),
        "the line names the tier: {:?}",
        lines[notice]
    );
    // the other four tiers keep their lines
    for text in [
        "a blocker only you can clear",
        "pull-request work",
        "a decision before the work runs",
        "cyan, an unread reply",
    ] {
        assert!(lines.iter().any(|r| r.contains(text)), "{text}");
    }
    assert!(
        !lines.iter().any(|r| r.contains("blue:")),
        "no line for blue as a tier or a dot:\n{}",
        lines.join("\n")
    );
    assert!(
        lines.iter().any(|r| r.contains("cyan:")),
        "the Session Dots' cyan line:\n{}",
        lines.join("\n")
    );
}

#[test]
fn the_session_dots_precedence_line_reads_red_yellow_magenta_cyan_white() {
    let lines = rows(&legend_buffer());
    assert!(
        lines
            .iter()
            .any(|r| r.contains("red > yellow > magenta > cyan > white > ")),
        "{}",
        lines.join("\n")
    );
    assert!(
        !lines.iter().any(|r| r.contains("magenta > blue")),
        "{}",
        lines.join("\n")
    );
}
