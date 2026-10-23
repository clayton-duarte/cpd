//! The ratatui pill atom: a pure function, not a Widget (hs-d596).
use cpd_core::pill::spec;
use ratatui::style::Style;
use ratatui::text::Span;
use std::borrow::Cow;

/// The columns the two caps add to a pill's text.
pub const CAPS_COLUMNS: u16 = 2;

/// The style of a cap: the block's foreground and nothing else.
pub fn cap_style(block: Style) -> Style {
    let _ = (block, spec());
    unimplemented!()
}

/// The three spans of a pill: left cap, text in `block`, right cap.
pub fn pill<'a>(text: impl Into<Cow<'a, str>>, block: Style) -> Vec<Span<'a>> {
    let _ = (text.into(), block);
    unimplemented!()
}
