//! The palette: the one place with colours and status icons, ported from the
//! spike's `palette.rs` (branch `cpd/cpd-ui`) on cpd 2's typed statuses. Every
//! view takes them from here, so changing one meaning (or one icon) changes it
//! in the tree, the Plan chart, the Dispatch column and the legend alike.
//! `no_colour_outside_the_palette` fails when another file under `tui/` names a colour.
//!
//! Changes from the spike: statuses are the read model's enums, not strings, so
//! there is no "unknown" icon; cpd 2 has no `in_review` Job status, and `merging`
//! (a running Job whose Attempt is merging) takes its place and its icon.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

use super::read::{AttemptStatus, Checks, Dot, JobStatus, PrState};
use cpd_core::plan::Phase;

/// What a colour stands for. One colour per meaning, whatever view shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meaning {
    /// A queued Job.
    NotStarted,
    /// A running Job; phase `dispatch`; PR checks pending.
    InProgress,
    /// A merging Job; an open PR; phase `pr`.
    Review,
    /// A merged Job; a merged PR; phase `done`; checks passed.
    Done,
    /// A failed Job; a closed PR; checks failed.
    Failed,
    /// Phase `plan`.
    Planning,
    /// Phase `grounding`.
    Grounding,
    /// A draft Job, a zero count, a separator, no Plan, no checks, nothing to show.
    Nothing,
}

impl Meaning {
    pub const ALL: [Meaning; 8] = [
        Meaning::NotStarted,
        Meaning::InProgress,
        Meaning::Review,
        Meaning::Done,
        Meaning::Failed,
        Meaning::Planning,
        Meaning::Grounding,
        Meaning::Nothing,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Meaning::NotStarted => "not started",
            Meaning::InProgress => "in progress",
            Meaning::Review => "waiting on review",
            Meaning::Done => "done",
            Meaning::Failed => "failed",
            Meaning::Planning => "planning",
            Meaning::Grounding => "grounding",
            Meaning::Nothing => "nothing / n/a / zero",
        }
    }
}

/// One colour per meaning, with two deliberate exceptions: grounding and not
/// started share white, since a Plan that is still grounding has nothing
/// running yet; done shares dark grey with nothing, the User's choice.
pub fn color(m: Meaning) -> Color {
    match m {
        Meaning::NotStarted => Color::White,
        Meaning::InProgress => Color::Green,
        Meaning::Review => Color::Yellow,
        Meaning::Done => Color::DarkGray,
        Meaning::Failed => Color::Red,
        Meaning::Planning => Color::Magenta,
        Meaning::Grounding => Color::White,
        Meaning::Nothing => Color::DarkGray,
    }
}

/// Foreground in the colour of a meaning.
pub fn fg(m: Meaning) -> Style {
    Style::new().fg(color(m))
}

/// Colours that are not a status: notes and prompts, the Worker view's roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ui {
    /// Notes and prompts at the foot of a view.
    Note,
    /// The User's lines in the Worker view.
    You,
    /// The agent's lines in the Worker view.
    Agent,
    /// Tool calls in the Worker view.
    Tool,
}

pub fn ui(u: Ui) -> Color {
    match u {
        Ui::Note | Ui::Tool => Color::Yellow,
        Ui::You => Color::Cyan,
        Ui::Agent => Color::Green,
    }
}

/// Notes and prompts at the foot of a view.
pub fn note() -> Style {
    Style::new().fg(ui(Ui::Note))
}

/// A Plan pill: black on the colour of a meaning.
pub fn pill(m: Meaning) -> Style {
    Style::new().fg(Color::Black).bg(color(m))
}

/// Icon and meaning of a Job's status; `merging` is a `running` Job whose
/// Attempt is merging (`JobCounts` counts it apart).
pub fn job_status(status: JobStatus, merging: bool) -> (&'static str, Meaning) {
    match (status, merging) {
        (JobStatus::Queued, _) => ("○", Meaning::NotStarted),
        (JobStatus::Running, false) => ("◜", Meaning::InProgress),
        (JobStatus::Running, true) => ("⇥", Meaning::Review),
        (JobStatus::Merged, _) => ("✓", Meaning::Done),
        (JobStatus::Failed, _) => ("✗", Meaning::Failed),
        (JobStatus::Draft, _) => ("◌", Meaning::Nothing),
        (JobStatus::Cancelled, _) => ("⊘", Meaning::Nothing),
    }
}

/// The Job statuses the legend lists: the name shown, the status and whether it is `merging`.
pub const JOB_LEGEND: [(&str, JobStatus, bool); 6] = [
    ("queued", JobStatus::Queued, false),
    ("running", JobStatus::Running, false),
    ("merging", JobStatus::Running, true),
    ("merged", JobStatus::Merged, false),
    ("failed", JobStatus::Failed, false),
    ("draft", JobStatus::Draft, false),
];

/// Meaning of an Attempt's status, for the slots it holds.
pub fn attempt_status(status: AttemptStatus) -> Meaning {
    match status {
        AttemptStatus::Starting => Meaning::NotStarted,
        AttemptStatus::Running => Meaning::InProgress,
        AttemptStatus::Merging => Meaning::Review,
        AttemptStatus::Ended => Meaning::Done,
    }
}

/// Meaning of a Plan's phase (`None`: no Plan).
pub fn phase(phase: Option<Phase>) -> Meaning {
    match phase {
        Some(Phase::Grounding) => Meaning::Grounding,
        Some(Phase::Plan) => Meaning::Planning,
        Some(Phase::Dispatch) => Meaning::InProgress,
        Some(Phase::Pr) => Meaning::Review,
        Some(Phase::Done) => Meaning::Done,
        None => Meaning::Nothing,
    }
}

/// The four frames of the working dot, one per [`frame`].
pub const SPINNER: [&str; 4] = ["◜", "◝", "◞", "◟"];

/// How long one frame of the spinner lasts, in milliseconds.
const FRAME_MS: u128 = 250;

/// The spinner frame at `now`: `(unix ms / 250) % 4`, one clock for every spinner, so they all turn together.
pub fn frame(now: SystemTime) -> usize {
    let ms = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    ((ms / FRAME_MS) % SPINNER.len() as u128) as usize
}

/// The tree's dot (commands.md §17): `●` red, yellow, magenta or white; `◜◝◞◟` (frame `frame`) in the terminal's
/// foreground when working; `○` in the terminal's foreground; `◌` dim. An `archived` row keeps its glyph, never
/// animated (the first frame when working), in the colour of [`Meaning::Nothing`].
pub fn dot_span(dot: Dot, frame: usize, archived: bool) -> Span<'static> {
    let (glyph, style) = match dot {
        Dot::Red => ("●", Style::new().fg(Color::Red)),
        Dot::Yellow => ("●", Style::new().fg(Color::Yellow)),
        Dot::Magenta => ("●", Style::new().fg(Color::Magenta)),
        Dot::White => ("●", Style::new().fg(Color::White)),
        Dot::Working => (SPINNER[if archived { 0 } else { frame % SPINNER.len() }], Style::new()),
        Dot::Idle => ("○", Style::new()),
        Dot::Stopped => ("◌", Style::new().add_modifier(Modifier::DIM)),
    };
    Span::styled(glyph, if archived { fg(Meaning::Nothing) } else { style })
}

/// The `⋅` between the tree's counters.
pub fn separator() -> Span<'static> {
    Span::styled("⋅", fg(Meaning::Nothing))
}

/// Style of a counter: its status colour, or nothing when the count is zero.
pub fn count_style(status: JobStatus, merging: bool, n: usize) -> Style {
    fg(if n == 0 { Meaning::Nothing } else { job_status(status, merging).1 })
}

/// Meaning of a PR's state.
pub fn pr_state(state: PrState) -> Meaning {
    match state {
        PrState::Open => Meaning::Review,
        PrState::Merged => Meaning::Done,
        PrState::Closed => Meaning::Failed,
    }
}

/// The PR states the legend lists, with the name shown.
pub const PR_LEGEND: [(&str, PrState); 3] = [("open", PrState::Open), ("merged", PrState::Merged), ("closed", PrState::Closed)];

/// Icon and meaning of a PR's checks.
pub fn checks(c: Checks) -> (&'static str, Meaning) {
    match c {
        Checks::Passed => ("✓", Meaning::Done),
        Checks::Failed => ("✗", Meaning::Failed),
        Checks::Pending => ("…", Meaning::InProgress),
        Checks::None => ("–", Meaning::Nothing),
    }
}

/// The checks the legend lists, with the name shown.
pub const CHECKS_LEGEND: [(&str, Checks); 4] =
    [("passed", Checks::Passed), ("failed", Checks::Failed), ("pending", Checks::Pending), ("none", Checks::None)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_meaning_has_its_colour() {
        let table = [
            (Meaning::NotStarted, Color::White),
            (Meaning::InProgress, Color::Green),
            (Meaning::Review, Color::Yellow),
            (Meaning::Done, Color::DarkGray),
            (Meaning::Failed, Color::Red),
            (Meaning::Planning, Color::Magenta),
            (Meaning::Grounding, Color::White),
            (Meaning::Nothing, Color::DarkGray),
        ];
        assert_eq!(table.map(|(m, _)| m), Meaning::ALL);
        for (m, c) in table {
            assert_eq!(color(m), c, "{m:?}");
        }
    }

    #[test]
    fn every_job_status_has_its_icon_and_meaning() {
        let table = [
            (JobStatus::Queued, false, "○", Meaning::NotStarted),
            (JobStatus::Running, false, "◜", Meaning::InProgress),
            (JobStatus::Running, true, "⇥", Meaning::Review),
            (JobStatus::Merged, false, "✓", Meaning::Done),
            (JobStatus::Failed, false, "✗", Meaning::Failed),
            (JobStatus::Draft, false, "◌", Meaning::Nothing),
            (JobStatus::Cancelled, false, "⊘", Meaning::Nothing),
        ];
        for (status, merging, icon, m) in table {
            assert_eq!(job_status(status, merging), (icon, m), "{status:?} {merging}");
        }
    }

    #[test]
    fn every_attempt_status_and_phase_has_its_meaning() {
        let attempts = [
            (AttemptStatus::Starting, Meaning::NotStarted),
            (AttemptStatus::Running, Meaning::InProgress),
            (AttemptStatus::Merging, Meaning::Review),
            (AttemptStatus::Ended, Meaning::Done),
        ];
        for (s, m) in attempts {
            assert_eq!(attempt_status(s), m, "{s:?}");
        }
        let phases = [
            (None, Meaning::Nothing),
            (Some(Phase::Grounding), Meaning::Grounding),
            (Some(Phase::Plan), Meaning::Planning),
            (Some(Phase::Dispatch), Meaning::InProgress),
            (Some(Phase::Pr), Meaning::Review),
            (Some(Phase::Done), Meaning::Done),
        ];
        for (p, m) in phases {
            assert_eq!(phase(p), m, "{p:?}");
        }
    }

    #[test]
    fn a_needs_you_dot_is_filled_and_coloured_by_its_case_and_nothing_else_is() {
        for (dot, colour) in [(Dot::Red, Color::Red), (Dot::Yellow, Color::Yellow), (Dot::Magenta, Color::Magenta), (Dot::White, Color::White)] {
            assert_eq!(dot_span(dot, 0, false), Span::styled("●", Style::new().fg(colour)), "{dot:?}");
        }
        assert_eq!(dot_span(Dot::Idle, 0, false), Span::styled("○", Style::new()), "the terminal's foreground");
        assert_eq!(dot_span(Dot::Stopped, 0, false), Span::styled("◌", Style::new().add_modifier(Modifier::DIM)), "dim, no colour");
    }

    #[test]
    fn a_working_dot_turns_with_the_frame_in_the_terminals_foreground() {
        let glyphs: Vec<_> = (0..5).map(|f| dot_span(Dot::Working, f, false).content.into_owned()).collect();
        assert_eq!(glyphs, ["◜", "◝", "◞", "◟", "◜"]);
        assert_eq!(dot_span(Dot::Working, 2, false).style, Style::new());
        assert_eq!(SPINNER, ["◜", "◝", "◞", "◟"]);
    }

    #[test]
    fn an_archived_dot_keeps_its_glyph_in_the_darker_grey_and_never_turns() {
        let grey = Style::new().fg(Color::DarkGray);
        for (dot, glyph) in [(Dot::Red, "●"), (Dot::Yellow, "●"), (Dot::Magenta, "●"), (Dot::White, "●"), (Dot::Idle, "○"), (Dot::Stopped, "◌")] {
            assert_eq!(dot_span(dot, 0, true), Span::styled(glyph, grey), "{dot:?}");
        }
        for f in 0..4 {
            assert_eq!(dot_span(Dot::Working, f, true), Span::styled("◜", grey), "frame {f}");
        }
    }

    #[test]
    fn the_frame_is_the_unix_time_in_quarter_seconds_modulo_four() {
        let at = |ms: u64| frame(UNIX_EPOCH + std::time::Duration::from_millis(ms));
        assert_eq!([0, 249, 250, 499, 500, 750, 999, 1000, 1250].map(at), [0, 0, 1, 1, 2, 3, 3, 0, 1]);
        assert_eq!(frame(UNIX_EPOCH - std::time::Duration::from_secs(1)), 0, "before the epoch: the first frame");
    }

    #[test]
    fn counters_are_in_their_status_colour_and_nothing_when_zero() {
        assert_eq!(count_style(JobStatus::Queued, false, 3), Style::new().fg(Color::White));
        assert_eq!(count_style(JobStatus::Draft, false, 3), Style::new().fg(Color::DarkGray));
        assert_eq!(count_style(JobStatus::Failed, false, 0), Style::new().fg(Color::DarkGray));
        assert_eq!(separator(), Span::styled("⋅", Style::new().fg(Color::DarkGray)));
    }

    #[test]
    fn every_pr_state_and_check_has_its_icon_and_meaning() {
        let states = [(PrState::Open, Meaning::Review), (PrState::Merged, Meaning::Done), (PrState::Closed, Meaning::Failed)];
        for (s, m) in states {
            assert_eq!(pr_state(s), m, "{s:?}");
        }
        assert_eq!(PR_LEGEND.map(|(_, s)| s), states.map(|(s, _)| s));
        let table = [
            (Checks::Passed, "✓", Meaning::Done),
            (Checks::Failed, "✗", Meaning::Failed),
            (Checks::Pending, "…", Meaning::InProgress),
            (Checks::None, "–", Meaning::Nothing),
        ];
        for (c, icon, m) in table {
            assert_eq!(checks(c), (icon, m), "{c:?}");
        }
        assert_eq!(CHECKS_LEGEND.map(|(_, c)| c), table.map(|(c, _, _)| c));
    }

    #[test]
    fn a_pill_is_black_on_the_colour() {
        assert_eq!(pill(Meaning::Review), Style::new().fg(Color::Black).bg(Color::Yellow));
    }

    /// No other file of the cockpit's views names a colour.
    #[test]
    fn no_colour_outside_the_palette() {
        fn walk(dir: &std::path::Path, offenders: &mut Vec<String>) {
            let shorthand = [
                ".black()", ".red()", ".green()", ".yellow()", ".blue()", ".magenta()", ".cyan()", ".gray()", ".dark_gray()",
                ".white()", ".on_",
            ];
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, offenders);
                } else if path.extension().is_some_and(|e| e == "rs") && path.file_name().is_some_and(|n| n != "palette.rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (n, line) in text.lines().enumerate() {
                        if line.contains("Color::") || shorthand.iter().any(|s| line.contains(s)) {
                            offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                        }
                    }
                }
            }
        }
        let mut offenders = Vec::new();
        walk(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut offenders);
        assert!(offenders.is_empty(), "colours belong in tui/palette.rs:\n{}", offenders.join("\n"));
    }

    #[test]
    fn every_ui_role_has_its_colour() {
        let table = [(Ui::Note, Color::Yellow), (Ui::You, Color::Cyan), (Ui::Agent, Color::Green), (Ui::Tool, Color::Yellow)];
        for (u, c) in table {
            assert_eq!(ui(u), c, "{u:?}");
        }
    }

    #[test]
    fn a_note_is_yellow() {
        assert_eq!(note(), Style::new().fg(Color::Yellow));
    }
}
