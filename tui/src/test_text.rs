//! The one reading of a drawn [`Buffer`] as text, for the tests that compare what was drawn
//! (hs-d599, plan F7).
//!
//! [`row`] and [`rows`] trim the trailing blanks of every row: that is the rule the trimmed test
//! helpers shared. A test that needs the full width, every cell including the blanks, reads the
//! cells itself and keeps its own helper.

use ratatui::buffer::Buffer;

/// The row of `buf` at `y`, its trailing blanks trimmed.
pub(crate) fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Every row of `buf`, each with its trailing blanks trimmed.
pub(crate) fn rows(buf: &Buffer) -> Vec<String> {
    (0..buf.area.height).map(|y| row(buf, y)).collect()
}
