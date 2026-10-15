//! The tree's colours and icons, from the spike's `palette.rs` (branch
//! `cpd/cpd-ui`): the one place in this view that names a colour. Only the
//! meanings the tree uses are here; the other views have their own.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::plan::Phase;

/// What a colour stands for. One colour per meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meaning {
    /// A queued Job.
    NotStarted,
    /// A running Job; phase `dispatch`; a working Session.
    InProgress,
    /// A Job in review (merging); phase `pr`; a blocked Session.
    Review,
    /// A merged Job; phase `done`.
    Done,
    /// A failed Job.
    Failed,
    /// Phase `plan`.
    Planning,
    /// Phase `grounding`.
    Grounding,
    /// A draft Job, a zero count, a guide, a stopped Session.
    Nothing,
}

/// One colour per meaning, with the spike's two deliberate exceptions:
/// grounding and not started share white, done shares dark grey with nothing.
pub fn color(m: Meaning) -> Color {
    match m {
        Meaning::NotStarted | Meaning::Grounding => Color::White,
        Meaning::InProgress => Color::Green,
        Meaning::Review => Color::Yellow,
        Meaning::Done | Meaning::Nothing => Color::DarkGray,
        Meaning::Failed => Color::Red,
        Meaning::Planning => Color::Magenta,
    }
}

/// Foreground in the colour of a meaning.
pub fn fg(m: Meaning) -> Style {
    Style::new().fg(color(m))
}

/// Notes and prompts at the foot of the view (the spike's `Ui::Note`).
pub fn note() -> Style {
    Style::new().fg(Color::Yellow)
}

/// The meaning of a Plan's phase.
pub fn phase(phase: Phase) -> Meaning {
    match phase {
        Phase::Grounding => Meaning::Grounding,
        Phase::Plan => Meaning::Planning,
        Phase::Dispatch => Meaning::InProgress,
        Phase::Pr => Meaning::Review,
        Phase::Done => Meaning::Done,
    }
}

/// The dot of a row: `●` when it needs the User, `○` when not, in the colour of a phase.
pub fn dot(needs_you: bool, phase: Option<Phase>) -> Span<'static> {
    Span::styled(if needs_you { "●" } else { "○" }, fg(phase.map_or(Meaning::Nothing, self::phase)))
}

/// The `⋅` between the counters.
pub fn separator() -> Span<'static> {
    Span::styled("⋅", fg(Meaning::Nothing))
}

/// The colours of the five counters, in the order of `JobCounts::counters`:
/// the first is the queued Jobs, in phase `plan` the drafts.
pub fn counter(index: usize, phase: Phase) -> Meaning {
    match index {
        0 if phase == Phase::Plan => Meaning::Nothing,
        0 => Meaning::NotStarted,
        1 => Meaning::InProgress,
        2 => Meaning::Review,
        3 => Meaning::Failed,
        _ => Meaning::Done,
    }
}

/// Style of a counter: its status colour, or nothing when the count is zero.
pub fn count_style(index: usize, phase: Phase, n: usize) -> Style {
    fg(if n == 0 { Meaning::Nothing } else { counter(index, phase) })
}
