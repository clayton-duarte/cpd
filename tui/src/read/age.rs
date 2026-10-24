//! The age text, derived once for the tree's Inbox and the plan view's Pulse.

/// An age in the largest whole unit: `42s`, `5m`, `3h`, `2d`; a negative age is `0s`.
pub fn age_text(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}
