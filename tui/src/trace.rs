//! The frame trace (commands.md §17 "Frame trace", hs-d588). Pure: every time is an argument (a `Duration`
//! since an epoch the view picks), nothing reads a clock; only the caller reads the environment value.

use std::time::Duration;

use crate::viewlog::ViewLog;

/// The trace of one view's main loop.
pub struct Trace {}

impl Trace {
    /// On only when `value` (`CPD_TUI_TRACE`) is exactly `1`.
    pub fn new(_value: Option<&str>) -> Trace {
        todo!()
    }

    pub fn is_on(&self) -> bool {
        todo!()
    }

    /// Adds time spent in a read since the previous draw ended.
    pub fn add_read(&mut self, _spent: Duration) {
        todo!()
    }

    /// Adds time spent in a layout since the previous draw ended.
    pub fn add_layout(&mut self, _spent: Duration) {
        todo!()
    }

    /// Ends a draw that began at `start` and took `draw`: the line, `None` when off. Resets the sums.
    pub fn line(&mut self, _start: Duration, _draw: Duration, _frame: usize) -> Option<String> {
        todo!()
    }

    /// [`Trace::line`], appended to `log`; nothing is written (no file) when off.
    pub fn record(&mut self, _log: &ViewLog, _start: Duration, _draw: Duration, _frame: usize) -> Result<(), String> {
        todo!()
    }
}
