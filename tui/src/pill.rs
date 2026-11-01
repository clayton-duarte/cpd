//! The ratatui pill atom: a pure function, not a Widget (hs-d596).
use cpd_core::pill::spec;
use ratatui::style::Style;
use ratatui::text::Span;
use std::borrow::Cow;

/// The columns the two caps add to a pill's text.
pub const CAPS_COLUMNS: u16 = 2;

/// The style of a cap: the block's foreground and nothing else.
pub fn cap_style(block: Style) -> Style {
    Style::new().fg(block.fg.expect("a pill block has a foreground"))
}

/// The three spans of a pill: left cap, text in `block`, right cap.
pub fn pill<'a>(text: impl Into<Cow<'a, str>>, block: Style) -> Vec<Span<'a>> {
    let caps = &spec().caps;
    let cap = cap_style(block);
    vec![
        Span::styled(caps.left.as_str(), cap),
        Span::styled(text, block),
        Span::styled(caps.right.as_str(), cap),
    ]
}
