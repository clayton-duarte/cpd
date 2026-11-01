//! The time, as contract §1.3 writes it.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub use jiff::tz::TimeZone;
use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// `at` as contract §1.3 writes times: RFC 3339 UTC with milliseconds.
pub fn timestamp(at: OffsetDateTime) -> String {
    at.to_offset(UtcOffset::UTC)
        .format(format_description!(
            "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"
        ))
        .expect("a UTC time always formats")
}

/// Now, as §1.3 writes times.
pub fn now() -> String {
    timestamp(OffsetDateTime::now_utc())
}

fn zoned(at: OffsetDateTime, zone: &TimeZone) -> jiff::Zoned {
    let ts = jiff::Timestamp::from_nanosecond(at.unix_timestamp_nanos())
        .expect("a time in jiff's range");
    ts.to_zoned(zone.clone())
}

/// `at` as prose for a person (`YYYY-MM-DD HH:MM ZZZ`) in `zone`: the abbreviation when letters only, else the numeric offset.
pub fn prose(at: OffsetDateTime, zone: &TimeZone) -> String {
    let z = zoned(at, zone);
    let abbr = zone
        .to_offset_info(z.timestamp())
        .abbreviation()
        .to_string();
    let label = if !abbr.is_empty() && abbr.chars().all(|c| c.is_ascii_alphabetic()) {
        abbr
    } else {
        z.strftime("%:z").to_string()
    };
    format!("{} {label}", z.strftime("%Y-%m-%d %H:%M"))
}

/// The local `HH:MM:SS` of `at` in `zone`, bare.
pub fn time_of_day(at: OffsetDateTime, zone: &TimeZone) -> String {
    zoned(at, zone).strftime("%H:%M:%S").to_string()
}

/// The time a wait reads and the nap it takes; a test passes a [`ManualClock`].
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
    fn sleep(&self, d: Duration);
    /// The UTC wall time: the system's for [`SystemClock`], a fixed epoch plus the elapsed time for [`ManualClock`].
    fn utc(&self) -> OffsetDateTime;
    /// Which implementation this is, by name, for a test of `Ports` (`"SystemClock"` for the real one).
    fn kind(&self) -> &'static str {
        "other"
    }
    /// The zone prose times are written in: the system's, except for [`ManualClock`].
    fn zone(&self) -> TimeZone {
        TimeZone::system()
    }
}

/// The real time: `Instant::now` and `thread::sleep`.
pub struct SystemClock;

impl Clock for SystemClock {
    fn kind(&self) -> &'static str {
        "SystemClock"
    }
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, d: Duration) {
        std::thread::sleep(d);
    }
    fn utc(&self) -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }
}

/// The UTC time a [`ManualClock`] starts at: fixed, so a stamp a test reads is the same on every run.
const EPOCH: OffsetDateTime = time::macros::datetime!(2026-01-01 00:00:00 UTC);

#[derive(Default)]
struct State {
    elapsed: Duration,
    naps: Vec<Duration>,
}

/// Virtual time: `sleep` records the nap and advances `now()` by it. A clone shares the state.
#[derive(Clone)]
pub struct ManualClock {
    start: Instant,
    state: Arc<Mutex<State>>,
}

impl ManualClock {
    pub fn new() -> Self {
        ManualClock {
            start: Instant::now(),
            state: Arc::default(),
        }
    }
    fn state(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .expect("the clock's state is never held across a panic")
    }
    /// Moves virtual time forward without recording a nap.
    pub fn advance(&self, d: Duration) {
        self.state().elapsed += d;
    }
    /// The virtual time passed since construction.
    pub fn elapsed(&self) -> Duration {
        self.state().elapsed
    }
    /// Every `sleep`, in order.
    pub fn naps(&self) -> Vec<Duration> {
        self.state().naps.clone()
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        ManualClock::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.start + self.elapsed()
    }
    fn sleep(&self, d: Duration) {
        let mut state = self.state();
        state.naps.push(d);
        state.elapsed += d;
    }
    fn utc(&self) -> OffsetDateTime {
        EPOCH + self.elapsed()
    }
    fn zone(&self) -> TimeZone {
        TimeZone::posix("EST5EDT,M3.2.0,M11.1.0").expect("a valid rule")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_time_is_written_in_utc_with_milliseconds() {
        assert_eq!(
            timestamp(datetime!(2026-05-01 12:34:56.789_123 +02:00)),
            "2026-05-01T10:34:56.789Z"
        );
    }
}
