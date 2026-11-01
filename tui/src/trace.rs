//! The frame trace (commands.md §17 "Frame trace", hs-d588). Pure: every time is an argument (a `Duration`
//! since an epoch the view picks), nothing reads a clock; only the caller reads the environment value.

use std::time::{Duration, Instant};

use crate::viewlog::ViewLog;

/// The trace of one view's main loop.
pub struct Trace {
    on: bool,
    read: Duration,
    layout: Duration,
    last_start: Option<Duration>,
}

impl Trace {
    /// On only when `value` (`CPD_TUI_TRACE`) is exactly `1`.
    pub fn new(value: Option<&str>) -> Trace {
        Trace {
            on: value == Some("1"),
            read: Duration::ZERO,
            layout: Duration::ZERO,
            last_start: None,
        }
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// Adds time spent in a read since the previous draw ended.
    pub fn add_read(&mut self, spent: Duration) {
        self.read += spent;
    }

    /// Adds time spent in a layout since the previous draw ended.
    pub fn add_layout(&mut self, spent: Duration) {
        self.layout += spent;
    }

    /// Ends a draw that began at `start` and took `draw`: the line, `None` when off. Resets the sums.
    pub fn line(&mut self, start: Duration, draw: Duration, frame: usize) -> Option<String> {
        if !self.on {
            return None;
        }
        let gap = self
            .last_start
            .map_or(0, |last| start.saturating_sub(last).as_micros());
        self.last_start = Some(start);
        let line = format!(
            "trace gap_us={gap} read_us={} layout_us={} draw_us={} frame={frame}",
            self.read.as_micros(),
            self.layout.as_micros(),
            draw.as_micros()
        );
        (self.read, self.layout) = (Duration::ZERO, Duration::ZERO);
        Some(line)
    }

    /// [`Trace::line`], appended to `log`; nothing is written (no file) when off.
    pub fn record(
        &mut self,
        log: &ViewLog,
        start: Duration,
        draw: Duration,
        frame: usize,
    ) -> Result<(), String> {
        match self.line(start, draw, frame) {
            Some(line) => log.append(&line),
            None => Ok(()),
        }
    }

    /// Runs `f`, adding its time to the reads; with the trace off, just `f`.
    pub fn read<T>(&mut self, f: impl FnOnce() -> T) -> T {
        if !self.on {
            return f();
        }
        let began = Instant::now();
        let out = f();
        self.add_read(began.elapsed());
        out
    }

    /// Runs `draw` (a terminal draw), then appends the line to `log`; with the trace off, just `draw`. The
    /// `epoch` is the instant the view's times count from.
    pub fn draw<T>(
        &mut self,
        epoch: Instant,
        log: &ViewLog,
        frame: usize,
        draw: impl FnOnce() -> T,
    ) -> (T, Result<(), String>) {
        if !self.on {
            return (draw(), Ok(()));
        }
        let began = Instant::now();
        let out = draw();
        let took = began.elapsed();
        let written = self.record(log, began.duration_since(epoch), took, frame);
        (out, written)
    }
}
