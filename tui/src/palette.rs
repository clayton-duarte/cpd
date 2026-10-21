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

use super::read::{AttemptStatus, Checks, Dot, JobStatus, Kind, PrState};
use cpd_core::inbox::model::Urgency;
use cpd_core::plan::Phase;

/// What a colour stands for. One colour per meaning, whatever view shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meaning {
    /// A queued Job.
    NotStarted,
    /// A running Job; phase `dispatch`; PR checks pending.
    InProgress,
    /// A merging Job; an open PR; phase `deploy`.
    Review,
    /// A merged Job; a merged PR; phase `done`; checks passed.
    Done,
    /// What needs the User: a stuck Job (`job-stuck`), a closed PR its Lead asked about, a conflict or failed checks on a PR
    /// no Plan owns. A failure an agent handles is [`Meaning::Waiting`] instead (hs-d303).
    Failed,
    /// Phase `plan`.
    Planning,
    /// Phase `grounding`.
    Grounding,
    /// A draft Job, a zero count, a separator, no Plan, no checks, nothing to show.
    Nothing,
    /// A Session that waits for another agent, not for the User (hs-d303): the waiting dot.
    Waiting,
}

impl Meaning {
    pub const ALL: [Meaning; 9] = [
        Meaning::NotStarted,
        Meaning::InProgress,
        Meaning::Review,
        Meaning::Done,
        Meaning::Failed,
        Meaning::Planning,
        Meaning::Grounding,
        Meaning::Nothing,
        Meaning::Waiting,
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
            Meaning::Waiting => "waiting on an agent",
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
        // Cyan, not blue: blue means focus only (hs-d352); it is neither the red of a failure nor the yellow of attention.
        Meaning::Waiting => Color::Cyan,
    }
}

/// Foreground in the colour of a meaning.
pub fn fg(m: Meaning) -> Style {
    Style::new().fg(color(m))
}

/// The colour of a Pulse bar by the kind of activity in its slot (commands.md §13.2, Kinds; User, hs-d327). Green is not
/// [`Meaning::Done`], which shares dark grey with nothing.
pub fn pulse_kind(kind: Kind) -> Color {
    match kind {
        Kind::Error => Color::Red,
        Kind::Plan => Color::Magenta,
        Kind::PrOpened => Color::Yellow,
        Kind::Escalation => Color::Blue,
        Kind::Merged => Color::Green,
        Kind::Other => Color::White,
    }
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

/// The one colour for in focus and selected (hs-d351), as the cockpit's `pane-active-border-style fg=blue`.
const FOCUS: Color = Color::Blue;

/// The selected row's name in the tree (hs-d351).
pub fn selected() -> Style {
    Style::new().fg(FOCUS)
}

/// Notes and prompts at the foot of a view.
pub fn note() -> Style {
    Style::new().fg(ui(Ui::Note))
}

/// A Plan pill: black on the colour of a meaning.
pub fn pill(m: Meaning) -> Style {
    Style::new().fg(Color::Black).bg(color(m))
}

/// A non-interactive pill (hs-d530 R1): background 8 (`DarkGray`), text 7 (`Gray`); its caps are drawn in its background colour.
pub fn pill_static() -> Style {
    todo!("hs-d530 R1: bg DarkGray, fg Gray")
}

/// An interactive pill (hs-d530 R2): background 15 (`White`), text 0 (`Black`); its caps are drawn in its background colour.
pub fn pill_interactive() -> Style {
    todo!("hs-d530 R2: bg White, fg Black")
}

/// The focused pane's title pill (hs-d530 R3): background 4 (`Blue`), text 0 (`Black`).
pub fn pill_focus() -> Style {
    todo!("hs-d530 R3: bg Blue, fg Black")
}

/// The terminal colour numbers of hs-d530, for the tests of `pill_colours`, which may not name a colour themselves.
#[cfg(test)]
pub(crate) mod numbered {
    use ratatui::style::Color;
    pub(crate) const C0: Color = Color::Black;
    pub(crate) const C4: Color = Color::Blue;
    pub(crate) const C7: Color = Color::Gray;
    pub(crate) const C8: Color = Color::DarkGray;
    pub(crate) const C15: Color = Color::White;
}

// Job pill icons (Nerd Fonts, Material Design; hs-d235). https://www.nerdfonts.com/cheat-sheet
/// A queued Job: `nf-md-circle_outline` (U+F0766).
pub const QUEUED: &str = "\u{F0766}";
/// A merging Job: `nf-md-eye_circle_outline` (U+F0B95).
pub const MERGING: &str = "\u{F0B95}";
/// A merged Job: `nf-md-check_circle` (U+F05E0).
pub const MERGED: &str = "\u{F05E0}";
/// A failed Job: `nf-md-close_circle` (U+F0159).
pub const FAILED: &str = "\u{F0159}";
/// A draft Job: `nf-md-progress_pencil` (U+F1787).
pub const DRAFT: &str = "\u{F1787}";
/// A cancelled Job: `nf-md-cancel` (U+F073A).
pub const CANCELLED: &str = "\u{F073A}";

// PR check icons (hs-d235). Passed and failed share the glyphs of merged and failed Jobs.
/// Checks passed: `nf-md-check_circle` (U+F05E0).
pub const CHECKS_PASSED: &str = MERGED;
/// Checks failed: `nf-md-close_circle` (U+F0159).
pub const CHECKS_FAILED: &str = FAILED;
/// Checks pending: `nf-md-progress_clock` (U+F0996).
pub const CHECKS_PENDING: &str = "\u{F0996}";
/// No checks: `nf-md-minus` (U+F0374).
pub const CHECKS_NONE: &str = "\u{F0374}";

/// Icon and meaning of a Job's status; `merging` is a `running` Job whose
/// Attempt is merging (`JobCounts` counts it apart).
pub fn job_status(status: JobStatus, merging: bool) -> (&'static str, Meaning) {
    match (status, merging) {
        (JobStatus::Queued, _) => (QUEUED, Meaning::NotStarted),
        (JobStatus::Running, false) => (SPINNER[0], Meaning::InProgress),
        (JobStatus::Running, true) => (MERGING, Meaning::Review),
        (JobStatus::Merged, _) => (MERGED, Meaning::Done),
        // an agent handles a failed Job (its Lead retries or cancels it): waiting; it is red only once `job-stuck` is open
        (JobStatus::Failed, _) => (FAILED, Meaning::Waiting),
        (JobStatus::Draft, _) => (DRAFT, Meaning::Nothing),
        (JobStatus::Cancelled, _) => (CANCELLED, Meaning::Nothing),
    }
}

/// A failed Job that needs the User: its `job-stuck` item is open (audit Q3). Red, where [`job_status`] gives `failed` the
/// waiting colour.
pub const JOB_STUCK: (&str, Meaning) = (FAILED, Meaning::Failed);

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
        Some(Phase::Deploy) => Meaning::Review,
        Some(Phase::Done) => Meaning::Done,
        None => Meaning::Nothing,
    }
}

/// Nerd Font `nf-md-circle_outline` (U+F0766), then `nf-md-circle_slice_1` … `8` (U+F0A9E..U+F0AA5): the 9 frames of one
/// animation. The working dot and a running Job's pill show one per [`frame`]; the other dots are its first and last.
/// https://www.nerdfonts.com/cheat-sheet
pub const SPINNER: [&str; 9] =
    ["\u{F0766}", "\u{F0A9E}", "\u{F0A9F}", "\u{F0AA0}", "\u{F0AA1}", "\u{F0AA2}", "\u{F0AA3}", "\u{F0AA4}", "\u{F0AA5}"];

/// An idle Session: the first frame.
pub const IDLE: &str = SPINNER[0];
/// A stopped Session: the first frame, drawn dim.
pub const STOPPED: &str = IDLE;
/// A Session that waits for another agent (hs-d303): `nf-md-circle_slice_4`, the half circle, in `Meaning::Waiting`.
pub const WAITING: &str = SPINNER[4];
/// A Session that needs the User (coloured by case): the last frame.
pub const NEEDS_YOU: &str = SPINNER[8];

/// The Inbox's tier glyphs (hs-d483), one per [`Urgency`], drawn in that urgency's colour; the state dots stay the Context tree's.
/// White: an unread reply.
pub const TIER_WHITE: &str = "\u{F0366}";
/// Magenta: a decision before the work runs.
pub const TIER_MAGENTA: &str = "\u{F173A}";
/// Yellow: pull-request work.
pub const TIER_YELLOW: &str = "\u{F069A}";
/// Red: a blocker only the User can clear.
pub const TIER_RED: &str = "\u{F0362}";

/// The tier glyph of `urgency`, in the colour of its urgency (the colours of the tree's dots).
pub fn tier_span(urgency: Urgency) -> Span<'static> {
    let (glyph, meaning) = match urgency {
        Urgency::Red => (TIER_RED, Meaning::Failed),
        Urgency::Yellow => (TIER_YELLOW, Meaning::Review),
        Urgency::Magenta => (TIER_MAGENTA, Meaning::Planning),
        Urgency::White => (TIER_WHITE, Meaning::NotStarted),
    };
    Span::styled(glyph, fg(meaning))
}

/// How long one frame of the spinner lasts, in milliseconds.
const FRAME_MS: u128 = 100;

/// The spinner frame at `now`: `(unix ms / 100) % 9`, one clock for every spinner, so they all turn together.
pub fn frame(now: SystemTime) -> usize {
    let ms = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    ((ms / FRAME_MS) % SPINNER.len() as u128) as usize
}

/// The tree's dot (commands.md §17): [`NEEDS_YOU`] red, yellow, magenta or white; [`SPINNER`] (frame `frame`) in the
/// terminal's foreground when working; [`WAITING`] in the colour of [`Meaning::Waiting`]; [`IDLE`] in the terminal's foreground; [`STOPPED`] dim. An `archived` row keeps its glyph, never
/// animated (the first frame when working), in the colour of [`Meaning::Nothing`].
pub fn dot_span(dot: Dot, frame: usize, archived: bool) -> Span<'static> {
    let (glyph, style) = match dot {
        Dot::Red => (NEEDS_YOU, Style::new().fg(Color::Red)),
        Dot::Yellow => (NEEDS_YOU, Style::new().fg(Color::Yellow)),
        Dot::Magenta => (NEEDS_YOU, Style::new().fg(Color::Magenta)),
        Dot::White => (NEEDS_YOU, Style::new().fg(Color::White)),
        Dot::Working => (SPINNER[if archived { 0 } else { frame % SPINNER.len() }], Style::new()),
        Dot::Waiting => (WAITING, fg(Meaning::Waiting)),
        Dot::Idle => (IDLE, Style::new()),
        Dot::Stopped => (STOPPED, Style::new().add_modifier(Modifier::DIM)),
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

/// Style of the failed counter: [`count_style`]'s waiting colour while an agent handles the failed Jobs, red when one of
/// them is `stuck` (a `job-stuck` item is open, audit Q3), nothing when the count is zero.
pub fn failed_count_style(n: usize, stuck: bool) -> Style {
    if stuck && n > 0 { fg(JOB_STUCK.1) } else { count_style(JobStatus::Failed, false, n) }
}

/// Meaning of a PR's state.
pub fn pr_state(state: PrState) -> Meaning {
    match state {
        PrState::Open => Meaning::Review,
        PrState::Merged => Meaning::Done,
        // its Lead handles a closed pull request (asks the User only if it must): waiting, see `PR_CLOSED_ASKED`
        PrState::Closed => Meaning::Waiting,
    }
}

/// A closed-unmerged pull request whose Plan's Lead has an open `dialog` item (it asks the User about it), or that no Plan
/// owns: red (audit Q4), where [`pr_state`] gives `closed` the waiting colour.
pub const PR_CLOSED_ASKED: (&str, Meaning) = ("closed", Meaning::Failed);

/// The PR states the legend lists, with the name shown.
pub const PR_LEGEND: [(&str, PrState); 3] = [("open", PrState::Open), ("merged", PrState::Merged), ("closed", PrState::Closed)];

/// What an open pull request with a merge conflict reads instead of `open` (its `mergeable` is `conflicting`), and its
/// meaning: the plan view's PR box title and the legend's PR states.
pub const PR_CONFLICT: (&str, Meaning) = ("conflict", Meaning::Failed);

/// What a conflicting open pull request that a cpd Plan owns reads: its Lead fixes it (hs-d302), so it is `Waiting`, not
/// red. [`PR_CONFLICT`] is the one no Plan owns, the User's to fix.
pub const PR_CONFLICT_OWNED: (&str, Meaning) = ("conflict", Meaning::Waiting);

/// [`checks`] of a pull request that is `owned` by a Plan or not: owned failed checks are `Waiting` (the Lead fixes
/// them, hs-d302), not red; every other value is [`checks`]'.
pub fn checks_of(c: Checks, owned: bool) -> (&'static str, Meaning) {
    match (c, owned) {
        (Checks::Failed, true) => (CHECKS_FAILED, Meaning::Waiting),
        _ => checks(c),
    }
}

/// Icon and meaning of a PR's checks.
pub fn checks(c: Checks) -> (&'static str, Meaning) {
    match c {
        Checks::Passed => (CHECKS_PASSED, Meaning::Done),
        Checks::Failed => (CHECKS_FAILED, Meaning::Failed),
        Checks::Pending => (CHECKS_PENDING, Meaning::InProgress),
        Checks::None => (CHECKS_NONE, Meaning::Nothing),
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
            (Meaning::Waiting, Color::Cyan),
        ];
        assert_eq!(table.map(|(m, _)| m), Meaning::ALL);
        for (m, c) in table {
            assert_eq!(color(m), c, "{m:?}");
        }
    }

    #[test]
    fn every_job_status_has_its_icon_and_meaning() {
        let table = [
            (JobStatus::Queued, false, QUEUED, Meaning::NotStarted),
            (JobStatus::Running, false, SPINNER[0], Meaning::InProgress),
            (JobStatus::Running, true, MERGING, Meaning::Review),
            (JobStatus::Merged, false, MERGED, Meaning::Done),
            (JobStatus::Failed, false, FAILED, Meaning::Waiting),
            (JobStatus::Draft, false, DRAFT, Meaning::Nothing),
            (JobStatus::Cancelled, false, CANCELLED, Meaning::Nothing),
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
            (Some(Phase::Deploy), Meaning::Review),
            (Some(Phase::Done), Meaning::Done),
        ];
        for (p, m) in phases {
            assert_eq!(phase(p), m, "{p:?}");
        }
    }

    #[test]
    fn a_needs_you_dot_is_filled_and_coloured_by_its_case_and_nothing_else_is() {
        for (dot, colour) in [(Dot::Red, Color::Red), (Dot::Yellow, Color::Yellow), (Dot::Magenta, Color::Magenta), (Dot::White, Color::White)] {
            assert_eq!(dot_span(dot, 0, false), Span::styled(NEEDS_YOU, Style::new().fg(colour)), "{dot:?}");
        }
        assert_eq!(dot_span(Dot::Idle, 0, false), Span::styled(IDLE, Style::new()), "the terminal's foreground");
        assert_eq!(
            dot_span(Dot::Waiting, 0, false),
            Span::styled(WAITING, Style::new().fg(Color::Cyan)),
            "cyan, never the focus blue: not red, not the yellow of attention, and not a filled dot"
        );
        assert_ne!(WAITING, NEEDS_YOU);
        assert_ne!(color(Meaning::Waiting), color(Meaning::Failed));
        assert_ne!(color(Meaning::Waiting), color(Meaning::Review));
        assert_eq!(dot_span(Dot::Stopped, 0, false), Span::styled(STOPPED, Style::new().add_modifier(Modifier::DIM)), "dim, no colour");
    }

    #[test]
    fn a_working_dot_turns_with_the_frame_in_the_terminals_foreground() {
        let glyphs: Vec<_> = (0..10).map(|f| dot_span(Dot::Working, f, false).content.into_owned()).collect();
        assert_eq!(glyphs, [SPINNER.to_vec(), vec![SPINNER[0]]].concat());
        assert_eq!(dot_span(Dot::Working, 2, false).style, Style::new());
        assert_eq!(SPINNER.len(), 9);
        assert_eq!((SPINNER[0], SPINNER[8]), ("\u{F0766}", "\u{F0AA5}"));
        assert_eq!((IDLE, STOPPED, NEEDS_YOU), (SPINNER[0], SPINNER[0], SPINNER[8]));
    }

    #[test]
    fn every_dot_glyph_is_one_cell_wide() {
        for glyph in SPINNER.into_iter().chain([NEEDS_YOU, WAITING, IDLE, STOPPED]) {
            assert_eq!(Span::raw(glyph).width(), 1, "U+{:X}", glyph.chars().next().unwrap() as u32);
            assert_eq!(glyph.chars().count(), 1, "{glyph:?}");
        }
    }

    #[test]
    fn every_pill_and_check_glyph_is_one_cell_wide_and_the_one_hs_d235_picked() {
        let glyphs = [
            (QUEUED, 0xF0766),
            (MERGING, 0xF0B95),
            (MERGED, 0xF05E0),
            (FAILED, 0xF0159),
            (DRAFT, 0xF1787),
            (CANCELLED, 0xF073A),
            (CHECKS_PASSED, 0xF05E0),
            (CHECKS_FAILED, 0xF0159),
            (CHECKS_PENDING, 0xF0996),
            (CHECKS_NONE, 0xF0374),
        ];
        for (glyph, code) in glyphs {
            assert_eq!(glyph.chars().collect::<Vec<_>>(), [char::from_u32(code).unwrap()], "U+{code:X}");
            assert_eq!(Span::raw(glyph).width(), 1, "U+{code:X}");
        }
    }

    #[test]
    fn an_archived_dot_keeps_its_glyph_in_the_darker_grey_and_never_turns() {
        let grey = Style::new().fg(Color::DarkGray);
        for (dot, glyph) in [
            (Dot::Red, NEEDS_YOU),
            (Dot::Yellow, NEEDS_YOU),
            (Dot::Magenta, NEEDS_YOU),
            (Dot::White, NEEDS_YOU),
            (Dot::Waiting, WAITING),
            (Dot::Idle, IDLE),
            (Dot::Stopped, STOPPED),
        ] {
            assert_eq!(dot_span(dot, 0, true), Span::styled(glyph, grey), "{dot:?}");
        }
        for f in 0..SPINNER.len() {
            assert_eq!(dot_span(Dot::Working, f, true), Span::styled(SPINNER[0], grey), "frame {f}");
        }
    }

    #[test]
    fn the_frame_is_the_unix_time_in_tenths_of_a_second_modulo_nine() {
        let at = |ms: u64| frame(UNIX_EPOCH + std::time::Duration::from_millis(ms));
        assert_eq!([0, 99, 100, 199, 200, 800, 899, 900, 1000].map(at), [0, 0, 1, 1, 2, 8, 8, 0, 1]);
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
    fn a_failed_job_and_its_counter_are_waiting_until_it_is_stuck() {
        assert_eq!(job_status(JobStatus::Failed, false), (FAILED, Meaning::Waiting), "its Lead handles it");
        assert_eq!(JOB_STUCK, (FAILED, Meaning::Failed), "job-stuck: the User's");
        assert_eq!(failed_count_style(2, false), Style::new().fg(Color::Cyan));
        assert_eq!(failed_count_style(2, true), Style::new().fg(Color::Red));
        assert_eq!(failed_count_style(0, true), Style::new().fg(Color::DarkGray), "nothing failed: nothing to show");
        assert_eq!(count_style(JobStatus::Failed, false, 1), Style::new().fg(Color::Cyan));
    }

    #[test]
    fn every_pr_state_and_check_has_its_icon_and_meaning() {
        let states = [(PrState::Open, Meaning::Review), (PrState::Merged, Meaning::Done), (PrState::Closed, Meaning::Waiting)];
        for (s, m) in states {
            assert_eq!(pr_state(s), m, "{s:?}");
        }
        assert_eq!(PR_LEGEND.map(|(_, s)| s), states.map(|(s, _)| s));
        assert_eq!(PR_CONFLICT, ("conflict", Meaning::Failed));
        assert_eq!(PR_CLOSED_ASKED, ("closed", Meaning::Failed));
        assert_eq!(PR_CONFLICT_OWNED, ("conflict", Meaning::Waiting));
        assert_eq!(checks_of(Checks::Failed, true), (CHECKS_FAILED, Meaning::Waiting));
        assert_eq!(checks_of(Checks::Failed, false), checks(Checks::Failed));
        for c in [Checks::Passed, Checks::Pending, Checks::None] {
            assert_eq!(checks_of(c, true), checks(c), "{c:?}");
        }
        let table = [
            (Checks::Passed, CHECKS_PASSED, Meaning::Done),
            (Checks::Failed, CHECKS_FAILED, Meaning::Failed),
            (Checks::Pending, CHECKS_PENDING, Meaning::InProgress),
            (Checks::None, CHECKS_NONE, Meaning::Nothing),
        ];
        for (c, icon, m) in table {
            assert_eq!(checks(c), (icon, m), "{c:?}");
        }
        assert_eq!(CHECKS_LEGEND.map(|(_, c)| c), table.map(|(c, _, _)| c));
    }

    #[test]
    fn a_pulse_bar_is_red_magenta_yellow_blue_green_or_white_by_its_kind() {
        let table = [
            (Kind::Error, Color::Red),
            (Kind::Plan, Color::Magenta),
            (Kind::PrOpened, Color::Yellow),
            (Kind::Escalation, Color::Blue),
            (Kind::Merged, Color::Green),
            (Kind::Other, Color::White),
        ];
        for (kind, colour) in table {
            assert_eq!(pulse_kind(kind), colour, "{kind:?}");
        }
        assert_eq!(color(Meaning::Nothing), Color::DarkGray, "an empty slot's ▁ is grey");
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
