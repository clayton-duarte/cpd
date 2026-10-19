//! The time, as contract §1.3 writes it.

use time::macros::format_description;
use time::{OffsetDateTime, UtcOffset};

/// `at` as contract §1.3 writes times: RFC 3339 UTC with milliseconds.
pub fn timestamp(at: OffsetDateTime) -> String {
    at.to_offset(UtcOffset::UTC)
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .expect("a UTC time always formats")
}

/// Now, as §1.3 writes times.
pub fn now() -> String {
    timestamp(OffsetDateTime::now_utc())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn a_time_is_written_in_utc_with_milliseconds() {
        assert_eq!(timestamp(datetime!(2026-05-01 12:34:56.789_123 +02:00)), "2026-05-01T10:34:56.789Z");
    }
}
