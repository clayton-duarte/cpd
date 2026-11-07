//! The code of the Dango palette (the 16 terminal colour indices of contract §6.1.1, hs-d591): the one place with
//! colours and status icons. The module is still called `palette` because that is the module path of every
//! `palette::...` call under `tui/` and it holds more than the colours (the status icons too); renaming it would touch
//! every view for no behaviour. Ported from the
//! spike's `palette.rs` (branch `cpd/cpd-ui`) on cpd 2's typed statuses. Every
//! view takes them from here, so changing one meaning (or one icon) changes it
//! in the tree, the Plan chart, the Dispatch column and the legend alike.
//! `no_colour_outside_the_palette` fails when another file under `tui/` names a colour.
//!
//! Changes from the spike: statuses are the read model's enums, not strings, so
//! there is no "unknown" icon; cpd 2 has no `in_review` Job status, and `merging`
//! (a running Job whose Attempt is checking or merging: "landing", yellow) takes its place and its icon.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};

use super::read::{AttemptStatus, Checks, Dot, JobStatus, Kind, PrState, Work};
use cpd_core::inbox::model::Urgency;
use cpd_core::pill::spec;
use cpd_core::plan::Phase;

/// What a colour stands for. One colour per meaning, whatever view shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meaning {
    /// A queued Job.
    NotStarted,
    /// A running Job; phase `dispatch`; PR checks pending.
    InProgress,
    /// A landing Job (checking or merging); an open PR; phase `deploy`.
    Review,
    /// A merged Job; a merged Workflow; phase `done`; checks passed.
    Done,
    /// What needs the User: a stuck Job (`job-stuck`), a closed PR its Lead asked about, a conflict or failed checks on a PR
    /// no Plan owns. A failure an agent handles is [`Meaning::Waiting`] instead (hs-d303).
    Failed,
    /// Phase `plan`; a draft Job.
    Planning,
    /// Phase `grounding`.
    Grounding,
    /// A zero count, a separator, a cancelled Job, no Plan, no checks, nothing to show.
    Nothing,
    /// A Session that waits for another agent, not for the User (hs-d303): the waiting dot.
    Waiting,
    /// A cpd notice that blocks nothing: the cyan Inbox tier and its Session dot (hs-d570, pro-d16). Not in [`Meaning::ALL`].
    Notice,
    /// External support: the blue Inbox tier, a review asked of you (B1). Not in [`Meaning::ALL`].
    External,
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
            Meaning::Notice => "a cpd notice",
            Meaning::External => "external support",
        }
    }
}

/// The one place a colour is named: the terminal colour of an index 1..=15 (`Color::Red`..`Color::White` are 1..=15 in order).
fn index_color(i: u8) -> Color {
    match i {
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        8 => Color::DarkGray,
        9 => Color::LightRed,
        10 => Color::LightGreen,
        11 => Color::LightYellow,
        12 => Color::LightBlue,
        13 => Color::LightMagenta,
        14 => Color::LightCyan,
        15 => Color::White,
        _ => unreachable!("a colour index is 1..=15 (0 is never drawn), the spec parser checks it"),
    }
}

/// The index 0..=15 of a colour made by [`index_color`].
fn color_index(c: Color) -> u8 {
    (1..=15)
        .find(|&i| index_color(i) == c)
        .expect("a palette colour has an index")
}

/// The spec's index of a meaning (`pi/lib/pill-spec.json`, `meanings`).
fn meaning_index(m: Meaning) -> u8 {
    let s = &spec().meanings;
    match m {
        Meaning::NotStarted | Meaning::Grounding => s.plain,
        Meaning::InProgress | Meaning::Waiting => s.in_progress,
        Meaning::Review => s.review,
        Meaning::Done => s.done,
        Meaning::Failed => s.failed,
        Meaning::Planning => s.planning,
        Meaning::Nothing => s.nothing,
        // The notice tier shares cyan with `Waiting`: two meanings, one colour, because the Inbox tier is read from its glyph and
        // position and the Inbox draws no waiting state (pro-d16).
        Meaning::Notice => s.in_progress,
        // Blue amends "blue is focus and selection only" for this one Inbox tier (B1).
        Meaning::External => s.focus,
    }
}

/// The base colour of a meaning, on what cannot be clicked (hs-d534, §6.1.1 rule (1)), read from the pill spec; [`color_clickable`] is its bright pair.
/// Grounding has no colour of its own: grey, the default when no meaning applies, like not started. Waiting is cyan, not blue: blue is focus and selection only.
pub fn color(m: Meaning) -> Color {
    index_color(meaning_index(m))
}

/// Foreground in the base colour of a meaning: for what cannot be clicked.
pub fn fg(m: Meaning) -> Style {
    Style::new().fg(color(m))
}

/// Foreground in the bright pair of a meaning: for every coloured mark inside a clickable area.
pub fn fg_clickable(m: Meaning) -> Style {
    Style::new().fg(color_clickable(m))
}

/// The colour of a Pulse bar by the kind of activity in its slot (commands.md §13.2, Kinds; User, hs-d327). Pulse rows are clickable, so the bars are the bright pairs; activity, PR-opened and escalation bars are all cyan.
pub fn pulse_kind(kind: Kind) -> Color {
    match kind {
        Kind::Error => color_clickable(Meaning::Failed),
        Kind::Plan => color_clickable(Meaning::Planning),
        Kind::PrOpened | Kind::Escalation | Kind::Other => color_clickable(Meaning::InProgress),
        Kind::Merged => color_clickable(Meaning::Done),
    }
}

/// Colours that are not a status: notes and prompts, the session renderer's roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ui {
    /// A dialog's input refusal.
    Note,
    /// The User's lines in the session renderer.
    You,
    /// The agent's lines in the session renderer.
    Agent,
    /// Tool calls in the session renderer.
    Tool,
}

pub fn ui(u: Ui) -> Color {
    match u {
        Ui::Note => index_color(3),
        Ui::Tool => index_color(6),
        Ui::You => index_color(7),
        Ui::Agent => index_color(6),
    }
}

/// The one colour for in focus and selection: blue, as the cockpit's `pane-active-border-style fg=blue`.
fn focus() -> Color {
    index_color(spec().meanings.focus)
}

/// A cursor and a selection (hs-d534, §6.1.1): the blue block, background 4, text inverted, in place of reverse video.
pub fn selected() -> Style {
    block(focus())
}

/// A block (a pill, a selection): reverse video, the colour as foreground over the terminal's own background.
fn block(c: Color) -> Style {
    Style::new()
        .fg(c)
        .bg(Color::Reset)
        .add_modifier(Modifier::REVERSED)
}

/// The selected tree row's name: blue text (hs-d351, hs-d534), where [`selected`] is the block of a cursor.
pub fn selected_name() -> Style {
    Style::new().fg(focus())
}

/// The bright pair of a base colour, as the spec's `clickable` says (n+8 on 1..=7, so `Red` -> `LightRed`, `Gray` -> `White`); the focus blue stays 4 (hs-d351) and 8 has no pair.
pub fn bright(c: Color) -> Color {
    if c == focus() {
        return c;
    }
    index_color(spec().clickable(color_index(c)))
}

/// The colour of a meaning on a clickable mark: its bright pair. The notice cyan is 14 on a clickable mark (pro-d16); `Waiting` shares it.
pub fn color_clickable(m: Meaning) -> Color {
    index_color(spec().clickable(meaning_index(m)))
}

/// Clickable text, colour 15.
pub fn clickable_text() -> Style {
    Style::new().fg(index_color(15))
}

/// Read-only text, colour 7.
pub fn read_only_text() -> Style {
    Style::new().fg(index_color(7))
}

/// An asleep Session's row, colour 7.
pub fn asleep_text() -> Style {
    read_only_text()
}

/// The fold toggles `╰󰬦 expand` / `╰󰬬 collapse`, colour 7.
pub fn toggle_text() -> Style {
    read_only_text()
}

/// The input box's border (hs-d575, hs-d576): colour 15 unfocused, bright blue 12 focused.
pub fn input_border(focused: bool) -> Style {
    Style::new().fg(if focused {
        index_color(12)
    } else {
        index_color(15)
    })
}

/// Muted text and lines (separators, ages, times, scroll cues, placeholders, rails), colour 8; it replaces `DIM`.
pub fn muted_text() -> Style {
    Style::new().fg(index_color(8))
}

/// An archived row, colour 8.
pub fn archived_text() -> Style {
    muted_text()
}

/// The pill of a Job (§6.1.1 rule (3)): not clickable, so its status colour's base, text inverted. A draft is magenta.
/// `merging` is a running Job whose Attempt is landing (checking or merging); a failed Job's pill is always red (hs-d586); `_stuck` no longer matters.
pub fn job_pill(status: JobStatus, merging: bool, _stuck: bool) -> Style {
    pill(job_status(status, merging).1)
}

/// The error line of a view that cannot load or read its data (hs-d534: red, blocking, not clickable).
pub fn error() -> Style {
    fg(Meaning::Failed)
}

/// A dialog's input refusal (hs-d534: yellow).
pub fn note() -> Style {
    Style::new().fg(ui(Ui::Note))
}

/// A pill: text inverted on the colour of a meaning, its base colour (read-only); its caps are drawn in the background colour.
pub fn pill(m: Meaning) -> Style {
    block(color(m))
}

/// A read-only pill (hs-d534 rule (3)): colour 7 (`Gray`), text inverted; its caps are drawn in its background colour.
pub fn pill_static() -> Style {
    block(index_color(7))
}

/// An interactive pill (hs-d534 rule (3)): background 15 (`White`), text inverted; its caps are drawn in its background colour.
pub fn pill_interactive() -> Style {
    block(index_color(15))
}

/// The selected button's pill (hs-d578, hs-d534): text inverted on bright blue 12, its caps drawn in its background colour.
pub fn pill_selected() -> Style {
    block(index_color(12))
}

/// The input box's label pill (hs-d578 F9): text inverted on the colour of the box's border (15 unfocused, 12 focused), its caps
/// drawn in that colour.
pub fn label_pill(focused: bool) -> Style {
    block(input_border(focused).fg.expect("a border has a colour"))
}

/// The focused pane's title pill: the blue block, as [`selected`].
pub fn pill_focus() -> Style {
    selected()
}

/// The terminal colour numbers of hs-d534, for the tests of `pill_colours` and `palette::colours`, which may not name a colour themselves.
#[cfg(test)]
pub(crate) mod numbered {
    use ratatui::style::Color;
    pub(crate) const C1: Color = Color::Red;
    pub(crate) const C2: Color = Color::Green;
    pub(crate) const C3: Color = Color::Yellow;
    pub(crate) const C4: Color = Color::Blue;
    pub(crate) const C5: Color = Color::Magenta;
    pub(crate) const C6: Color = Color::Cyan;
    pub(crate) const C7: Color = Color::Gray;
    pub(crate) const C8: Color = Color::DarkGray;
    pub(crate) const C9: Color = Color::LightRed;
    pub(crate) const C10: Color = Color::LightGreen;
    pub(crate) const C11: Color = Color::LightYellow;
    pub(crate) const C12: Color = Color::LightBlue;
    pub(crate) const C13: Color = Color::LightMagenta;
    pub(crate) const C14: Color = Color::LightCyan;
    pub(crate) const C15: Color = Color::White;
}

/// What the views' tests ask of a drawn cell, by the palette's own styles (no colour named outside this file).
#[cfg(test)]
pub(crate) mod check {
    use ratatui::buffer::Cell;

    /// The cell is part of a blue block: colour 4, reversed (a cursor or a selection).
    pub(crate) fn is_selected(cell: &Cell) -> bool {
        let block = super::selected();
        (Some(cell.bg), Some(cell.fg)) == (block.bg, block.fg)
            && cell.modifier.contains(ratatui::style::Modifier::REVERSED)
    }

    /// The cell's text is muted, colour 8.
    pub(crate) fn is_muted(cell: &Cell) -> bool {
        Some(cell.fg) == super::muted_text().fg
    }
}

// Job pill icons (Nerd Fonts, Material Design; hs-d235). https://www.nerdfonts.com/cheat-sheet
/// A queued Job: `nf-md-circle_outline` (U+F0766).
pub const QUEUED: &str = "\u{F0766}";
/// A landing Job: `nf-md-download_circle` (U+F1464).
pub const MERGING: &str = "\u{F1464}";
/// A merged Job: `nf-md-check_circle` (U+F05E0).
pub const MERGED: &str = "\u{F05E0}";
/// A failed Job: `nf-md-close_circle` (U+F0159).
pub const FAILED: &str = "\u{F0159}";
/// A draft Job: `nf-fa-circle_question` (U+F059).
pub const DRAFT: &str = "\u{F059}";
/// A cancelled Job: `nf-md-close_circle` (U+F0159).
pub const CANCELLED: &str = "\u{F0159}";

// Workflow check icons (hs-d235). Passed and failed share the glyphs of merged and failed Jobs.
/// Checks passed: `nf-md-check_circle` (U+F05E0).
pub const CHECKS_PASSED: &str = MERGED;
/// Checks failed: `nf-md-close_circle` (U+F0159).
pub const CHECKS_FAILED: &str = FAILED;
/// Checks pending: `nf-md-eye_circle` (U+F0B94).
pub const CHECKS_PENDING: &str = "\u{F0B94}";
/// No checks: `nf-md-minus` (U+F0374).
pub const CHECKS_NONE: &str = "\u{F0374}";

/// Icon and meaning of a Job's status; `merging` is a `running` Job whose
/// Attempt is landing, checking or merging (`JobCounts` counts it apart).
pub fn job_status(status: JobStatus, merging: bool) -> (&'static str, Meaning) {
    match (status, merging) {
        (JobStatus::Queued, _) => (QUEUED, Meaning::NotStarted),
        (JobStatus::Running, false) => (SPINNER[HOLLOW], Meaning::InProgress),
        (JobStatus::Running, true) => (MERGING, Meaning::Review),
        (JobStatus::Merged, _) => (MERGED, Meaning::Done),
        (JobStatus::Failed, _) => (FAILED, Meaning::Failed),
        (JobStatus::Draft, _) => (DRAFT, Meaning::Planning),
        (JobStatus::Cancelled, _) => (CANCELLED, Meaning::Nothing),
    }
}

/// The Job statuses the legend lists: the name shown, the status and whether it is `merging` (landing).
pub const JOB_LEGEND: [(&str, JobStatus, bool); 6] = [
    ("queued", JobStatus::Queued, false),
    ("running", JobStatus::Running, false),
    ("landing", JobStatus::Running, true),
    ("merged", JobStatus::Merged, false),
    ("failed", JobStatus::Failed, false),
    ("draft", JobStatus::Draft, false),
];

/// Meaning of an Attempt's status, for the slots it holds.
pub fn attempt_status(status: AttemptStatus) -> Meaning {
    match status {
        AttemptStatus::Starting => Meaning::NotStarted,
        AttemptStatus::Running => Meaning::InProgress,
        AttemptStatus::Checking | AttemptStatus::Merging => Meaning::Review,
        AttemptStatus::Ended => Meaning::Done,
    }
}

/// Meaning of a Plan's phase (`None`: no Plan).
pub fn phase(phase: Option<Phase>) -> Meaning {
    match phase {
        // A Lead-less shaping Plan has no Lead and has not started (contract §3.1, air-d19).
        Some(Phase::Shaping) => Meaning::NotStarted,
        Some(Phase::Grounding) => Meaning::Grounding,
        Some(Phase::Plan) => Meaning::Planning,
        Some(Phase::Dispatch) => Meaning::InProgress,
        Some(Phase::Deploy) => Meaning::Review,
        Some(Phase::Done) => Meaning::Done,
        None => Meaning::Nothing,
    }
}

/// The 28 `moon_alt` phases of Nerd Fonts 3.5.1 (weather): U+E3E3 new, U+E3C8..U+E3CD waxing crescent 1-6, U+E3CE first quarter,
/// U+E3CF..U+E3D4 waxing gibbous 1-6, U+E3D5 full, U+E3D6..U+E3DB waning gibbous 1-6, U+E3DC third quarter, U+E3DD..U+E3E2 waning
/// crescent 1-6: the one loader of pi and the cockpit (hs-d616), one per [`frame`] of [`FRAME_MS`]. The working dot and a running
/// Job's pill show one per frame; the needs-you dot is the first (the filled disc), the idle dot the one at [`HOLLOW`] (the hollow ring).
pub const SPINNER: [&str; 28] = [
    "\u{E3E3}", "\u{E3C8}", "\u{E3C9}", "\u{E3CA}", "\u{E3CB}", "\u{E3CC}", "\u{E3CD}", "\u{E3CE}",
    "\u{E3CF}", "\u{E3D0}", "\u{E3D1}", "\u{E3D2}", "\u{E3D3}", "\u{E3D4}", "\u{E3D5}", "\u{E3D6}",
    "\u{E3D7}", "\u{E3D8}", "\u{E3D9}", "\u{E3DA}", "\u{E3DB}", "\u{E3DC}", "\u{E3DD}", "\u{E3DE}",
    "\u{E3DF}", "\u{E3E0}", "\u{E3E1}", "\u{E3E2}",
];

/// The index of the hollow ring (U+E3D5, `moon_alt_full`) in [`SPINNER`]; frame 0 (U+E3E3, `moon_alt_new`) is the filled disc.
const HOLLOW: usize = 14;
/// An idle Session: the hollow ring, nothing is needed.
pub const IDLE: &str = SPINNER[HOLLOW];
/// A stopped Session: the hollow ring, drawn dim.
pub const STOPPED: &str = IDLE;
/// A Session that waits for another agent (hs-d303): `nf-md-circle_opacity` (U+F1853), in `Meaning::Waiting`.
pub const WAITING: &str = "\u{F1853}";
/// A Session that needs the User (coloured by case): the filled disc (U+E3E3, `moon_alt_new`), frame 0.
pub const NEEDS_YOU: &str = SPINNER[0];

/// The Inbox's tier glyphs (hs-d483), one per [`Urgency`], drawn in that urgency's colour; the state dots stay the Context tree's.
/// White: an unread reply.
pub const TIER_WHITE: &str = "\u{F0366}";
/// Magenta: a decision before the work runs (the generic system item), U+F173A (hs-d570 returns it from U+F06F1).
pub const TIER_MAGENTA: &str = "\u{F173A}";
/// A record to retire (an Inbox item of kind `curate-retire`), cyan.
pub const ITEM_RETIRE: &str = "\u{F116E}";
/// Blue: external support, a review asked of you, U+F0368 (md-message_reply_text).
pub const TIER_BLUE: &str = "\u{F0368}";
/// Cyan: a cpd notice that blocks nothing (the generic system item), U+F06F1 (hs-d570).
pub const TIER_CYAN: &str = "\u{F06F1}";
/// Yellow: pull-request work.
pub const TIER_YELLOW: &str = "\u{F069A}";
/// Red: a blocker only the User can clear.
pub const TIER_RED: &str = "\u{F0362}";

/// The glyph of a `deploy` item, U+F15A9 (yellow tier).
pub const ITEM_DEPLOY: &str = "\u{F15A9}";
/// The glyph of a `pr-ready` item, U+F12F4 (yellow tier).
pub const ITEM_PR_READY: &str = "\u{F12F4}";
/// The glyph of a `review-requested` item, U+F12F2 (blue tier).
pub const ITEM_REVIEW_REQUESTED: &str = "\u{F12F2}";

/// The tier glyph of `urgency`, in the bright colour of its urgency (the Inbox row is clickable; the colours of the tree's dots).
pub fn tier_span(urgency: Urgency) -> Span<'static> {
    let (glyph, meaning) = match urgency {
        Urgency::Red => (TIER_RED, Meaning::Failed),
        Urgency::Yellow => (TIER_YELLOW, Meaning::Review),
        Urgency::Magenta => (TIER_MAGENTA, Meaning::Planning),
        Urgency::White => (TIER_WHITE, Meaning::NotStarted),
        Urgency::Cyan => (TIER_CYAN, Meaning::Notice),
        Urgency::Blue => (TIER_BLUE, Meaning::External),
    };
    Span::styled(glyph, fg_clickable(meaning))
}

/// The glyph of an Inbox item of `kind` and `urgency` (hs-d569 CD5): its tier's glyph, except the two curation kinds, which
/// draw their own, in the white colour.
pub fn item_span(kind: cpd_core::inbox::model::Kind, urgency: Urgency) -> Span<'static> {
    use cpd_core::inbox::model::Kind as Item;
    match kind {
        Item::CurateRetire => Span::styled(ITEM_RETIRE, fg_clickable(Meaning::NotStarted)),
        Item::Deploy => Span::styled(ITEM_DEPLOY, fg_clickable(Meaning::Review)),
        Item::PrReady => {
            let meaning = match urgency {
                Urgency::Red => Meaning::Failed,
                Urgency::Yellow => Meaning::Review,
                Urgency::Cyan => Meaning::Notice,
                Urgency::White => Meaning::NotStarted,
                Urgency::Magenta => Meaning::Planning,
                Urgency::Blue => Meaning::External,
            };
            Span::styled(ITEM_PR_READY, fg_clickable(meaning))
        }
        Item::ReviewRequested => {
            Span::styled(ITEM_REVIEW_REQUESTED, fg_clickable(Meaning::External))
        }
        _ => tier_span(urgency),
    }
}

/// How long one frame of the spinner lasts, in milliseconds.
pub const FRAME_MS: u64 = 50;

/// The spinner frame at `now`: `(unix ms / FRAME_MS) % 28`, one clock for every spinner, so they all turn together.
pub fn frame(now: SystemTime) -> usize {
    let ms = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
    ((ms / u128::from(FRAME_MS)) % SPINNER.len() as u128) as usize
}

/// The tree's dot (commands.md §17), on a clickable row so every colour is the bright one (hs-d534): [`NEEDS_YOU`] red (9),
/// yellow (11), magenta (13) or white (15); [`SPINNER`] (frame `frame`) in the bright pair of [`Meaning::InProgress`]
/// (cyan 14) for a working Session that leads no Plan and of the Plan's phase for a working Plan Lead (hs-d622:
/// grounding 15, plan 13, dispatch 14, deploy 11, done 10), red (9) when [`Dot::Fixing`] (a Plan Lead working on a Plan
/// with a failed Job); [`WAITING`] in the colour of [`Meaning::Waiting`]; [`IDLE`] 15; [`STOPPED`] 8. An `archived` row
/// keeps its glyph, never animated (the hollow ring when working or fixing), in colour 8.
pub fn dot_span(dot: Dot, frame: usize, archived: bool) -> Span<'static> {
    let (glyph, style) = match dot {
        Dot::Red => (NEEDS_YOU, fg_clickable(Meaning::Failed)),
        Dot::Yellow => (NEEDS_YOU, fg_clickable(Meaning::Review)),
        Dot::Magenta => (NEEDS_YOU, fg_clickable(Meaning::Planning)),
        Dot::Cyan => (NEEDS_YOU, fg_clickable(Meaning::Notice)),
        Dot::White => (NEEDS_YOU, clickable_text()),
        Dot::Working(work) => (
            SPINNER[if archived {
                HOLLOW
            } else {
                frame % SPINNER.len()
            }],
            match work {
                Some(Work::Planning) => fg_clickable(Meaning::Planning),
                Some(Work::Dispatch) => fg_clickable(Meaning::InProgress),
                Some(Work::Review) => fg_clickable(Meaning::Review),
                None => clickable_text(),
            },
        ),
        Dot::Fixing => (
            SPINNER[if archived {
                HOLLOW
            } else {
                frame % SPINNER.len()
            }],
            fg_clickable(Meaning::Failed),
        ),
        Dot::Waiting => (WAITING, fg_clickable(Meaning::Waiting)),
        Dot::Idle => (IDLE, clickable_text()),
        Dot::Stopped => (STOPPED, muted_text()),
    };
    Span::styled(glyph, if archived { archived_text() } else { style })
}

/// The `⋅` between the tree's counters.
pub fn separator() -> Span<'static> {
    Span::styled("⋅", muted_text())
}

/// Style of a counter (the tree's, inside a clickable row): its status colour's bright pair, or colour 8 when the count is zero. A
/// failed count is red (hs-d580).
pub fn count_style(status: JobStatus, merging: bool, n: usize) -> Style {
    match (n, status) {
        (0, _) => muted_text(),
        (_, JobStatus::Failed) => fg_clickable(Meaning::Failed),
        _ => fg_clickable(job_status(status, merging).1),
    }
}

/// Meaning of a Workflow's state.
pub fn pr_state(state: PrState) -> Meaning {
    match state {
        // an open pull request is colourless (grey) until it is ready to merge: `pr_open`
        PrState::Open => Meaning::NotStarted,
        PrState::Merged => Meaning::Done,
        // its Lead handles a closed pull request (asks the User only if it must): waiting, see `PR_CLOSED_ASKED`
        PrState::Closed => Meaning::Waiting,
    }
}

/// Meaning of an open pull request's `open` word: yellow only when it is `clean`, ready to merge (hs-d534, pro-d118), else colourless.
pub fn pr_open(clean: bool) -> Meaning {
    if clean {
        Meaning::Review
    } else {
        Meaning::NotStarted
    }
}

/// A closed-unmerged pull request whose Plan's Lead has an open `dialog` item (it asks the User about it), or that no Plan
/// owns: red (audit Q4), where [`pr_state`] gives `closed` the waiting colour.
pub const PR_CLOSED_ASKED: (&str, Meaning) = ("closed", Meaning::Failed);

/// The Workflow states the legend lists, with the name shown.
pub const PR_LEGEND: [(&str, PrState); 3] = [
    ("open", PrState::Open),
    ("merged", PrState::Merged),
    ("closed", PrState::Closed),
];

/// What an open pull request with a merge conflict reads instead of `open` (its `mergeable` is `conflicting`), and its
/// meaning: the plan view's Workflow box title and the legend's Workflow states.
pub const PR_CONFLICT: (&str, Meaning) = ("conflict", Meaning::Failed);

/// What a conflicting open pull request that a cpd Plan owns reads: its Lead fixes it (hs-d302), so it is `Waiting`, not
/// red. [`PR_CONFLICT`] is the one no Plan owns, the User's to fix.
pub const PR_CONFLICT_OWNED: (&str, Meaning) = ("conflict", Meaning::Waiting);

/// What a conflicting pull request reads while its Workflow has an update Job that is not ended (hs-d532): the running colour
/// while the Job runs, the waiting colour while it is queued.
pub const PR_UPDATING_RUNNING: (&str, Meaning) = ("updating", Meaning::InProgress);
pub const PR_UPDATING_QUEUED: (&str, Meaning) = ("updating", Meaning::Waiting);

/// [`checks`] of a pull request that is `owned` by a Plan or not: owned failed checks are `Waiting` (the Lead fixes
/// them, hs-d302), not red; every other value is [`checks`]'.
pub fn checks_of(c: Checks, owned: bool) -> (&'static str, Meaning) {
    match (c, owned) {
        (Checks::Failed, true) => (CHECKS_FAILED, Meaning::Waiting),
        _ => checks(c),
    }
}

/// Icon and meaning of a Workflow's checks.
pub fn checks(c: Checks) -> (&'static str, Meaning) {
    match c {
        Checks::Passed => (CHECKS_PASSED, Meaning::Done),
        Checks::Failed => (CHECKS_FAILED, Meaning::Failed),
        Checks::Pending => (CHECKS_PENDING, Meaning::InProgress),
        Checks::None => (CHECKS_NONE, Meaning::Nothing),
    }
}

/// The checks the legend lists, with the name shown.
pub const CHECKS_LEGEND: [(&str, Checks); 4] = [
    ("passed", Checks::Passed),
    ("failed", Checks::Failed),
    ("pending", Checks::Pending),
    ("none", Checks::None),
];

#[cfg(test)]
mod colours;

#[cfg(test)]
mod tier_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_meaning_has_its_colour() {
        let table = [
            (Meaning::NotStarted, Color::Gray),
            (Meaning::InProgress, Color::Cyan),
            (Meaning::Review, Color::Yellow),
            (Meaning::Done, Color::Green),
            (Meaning::Failed, Color::Red),
            (Meaning::Planning, Color::Magenta),
            (Meaning::Grounding, Color::Gray),
            (Meaning::Nothing, Color::DarkGray),
            (Meaning::Waiting, Color::Cyan),
        ];
        assert_eq!(table.map(|(m, _)| m), Meaning::ALL);
        for (m, c) in table {
            assert_eq!(color(m), c, "{m:?}");
        }
    }

    /// hs-d570 BL3: the notice colour is cyan 6, bright 14 on a clickable row (pro-d16); the focus block stays 4.
    #[test]
    fn the_notice_meaning_is_cyan_6_and_bright_14_on_a_clickable_row() {
        assert_eq!(color(Meaning::Notice), Color::Cyan);
        assert_eq!(color_clickable(Meaning::Notice), Color::LightCyan, "14");
        assert_eq!(
            selected().fg,
            Some(numbered::C4),
            "the focus block is still 4"
        );
        assert_eq!(
            selected().bg,
            Some(Color::Reset),
            "a block is reversed over the terminal's background"
        );
        assert_eq!(
            tier_span(Urgency::Cyan),
            Span::styled(TIER_CYAN, fg_clickable(Meaning::Notice))
        );
        assert_eq!(
            tier_span(Urgency::Blue),
            Span::styled(TIER_BLUE, fg_clickable(Meaning::External))
        );
        assert_eq!(color(Meaning::External), Color::Blue);
        assert_eq!(color_clickable(Meaning::External), Color::LightBlue, "12");
        assert_eq!(
            dot_span(Dot::Cyan, 0, false),
            Span::styled(NEEDS_YOU, fg_clickable(Meaning::Notice))
        );
        assert_eq!(
            dot_span(Dot::Cyan, 0, true),
            Span::styled(NEEDS_YOU, Style::new().fg(Color::DarkGray)),
            "archived: 8"
        );
    }

    #[test]
    fn every_job_status_has_its_icon_and_meaning() {
        let table = [
            (JobStatus::Queued, false, QUEUED, Meaning::NotStarted),
            (JobStatus::Running, false, "\u{E3D5}", Meaning::InProgress),
            (JobStatus::Running, true, MERGING, Meaning::Review),
            (JobStatus::Merged, false, MERGED, Meaning::Done),
            (JobStatus::Failed, false, FAILED, Meaning::Failed),
            (JobStatus::Draft, false, DRAFT, Meaning::Planning),
            (JobStatus::Cancelled, false, CANCELLED, Meaning::Nothing),
        ];
        for (status, merging, icon, m) in table {
            assert_eq!(
                job_status(status, merging),
                (icon, m),
                "{status:?} {merging}"
            );
        }
    }

    #[test]
    fn every_attempt_status_and_phase_has_its_meaning() {
        let attempts = [
            (AttemptStatus::Starting, Meaning::NotStarted),
            (AttemptStatus::Running, Meaning::InProgress),
            (AttemptStatus::Checking, Meaning::Review),
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
    fn a_tree_dot_is_its_bright_colour_and_a_stopped_or_archived_one_is_muted() {
        for (dot, colour) in [
            (Dot::Red, Color::LightRed),
            (Dot::Yellow, Color::LightYellow),
            (Dot::Magenta, Color::LightMagenta),
            (Dot::Cyan, Color::LightCyan),
            (Dot::White, Color::White),
        ] {
            assert_eq!(
                dot_span(dot, 0, false),
                Span::styled(NEEDS_YOU, Style::new().fg(colour)),
                "{dot:?}"
            );
        }
        assert_eq!(
            dot_span(Dot::Idle, 0, false),
            Span::styled(IDLE, Style::new().fg(Color::White)),
            "clickable text, 15"
        );
        assert_eq!(
            dot_span(Dot::Waiting, 0, false),
            Span::styled(WAITING, Style::new().fg(Color::LightCyan)),
            "cyan, never the focus blue: not red, not the yellow of attention, and not a filled dot"
        );
        assert_ne!(WAITING, NEEDS_YOU);
        assert_ne!(color(Meaning::Waiting), color(Meaning::Failed));
        assert_ne!(color(Meaning::Waiting), color(Meaning::Review));
        assert_eq!(
            dot_span(Dot::Stopped, 0, false),
            Span::styled(STOPPED, Style::new().fg(Color::DarkGray)),
            "stopped: 8"
        );
    }

    #[test]
    fn the_spinner_is_28_frames_and_the_static_dots_are_its_ends() {
        assert_eq!(SPINNER.len(), 28);
        assert_eq!((SPINNER[0], SPINNER[14]), ("\u{E3E3}", "\u{E3D5}"));
        assert_eq!(
            (IDLE, STOPPED, NEEDS_YOU),
            (SPINNER[14], SPINNER[14], SPINNER[0])
        );
    }

    /// pro-d200: the five spinner colours: what the Lead works on.
    #[test]
    fn dots_palette_working_dot_colours() {
        use crate::read::Work;
        let table = [
            (
                Dot::Working(Some(Work::Planning)),
                numbered::C13,
                "planning",
            ),
            (
                Dot::Working(Some(Work::Dispatch)),
                numbered::C14,
                "dispatch",
            ),
            (Dot::Working(Some(Work::Review)), numbered::C11, "review"),
            (Dot::Working(None), numbered::C15, "no Plan"),
            (Dot::Fixing, numbered::C9, "fixing"),
        ];
        for (dot, colour, what) in table {
            for f in 0..2 * SPINNER.len() + 1 {
                let span = dot_span(dot, f, false);
                assert_eq!(span.content, SPINNER[f % 28], "{what} frame {f}");
                assert_eq!(
                    span.style,
                    Style::new().fg(colour),
                    "{what} frame {f}: fg only, no modifier"
                );
            }
            for f in 0..SPINNER.len() {
                assert_eq!(
                    dot_span(dot, f, true),
                    Span::styled("\u{E3D5}", Style::new().fg(numbered::C8)),
                    "{what} archived frame {f}"
                );
            }
        }
        assert_eq!(
            dot_span(Dot::Working(None), 0, false).style,
            clickable_text()
        );
    }

    #[test]
    fn every_dot_glyph_is_one_cell_wide() {
        for glyph in SPINNER
            .into_iter()
            .chain([NEEDS_YOU, WAITING, IDLE, STOPPED])
        {
            assert_eq!(
                Span::raw(glyph).width(),
                1,
                "U+{:X}",
                glyph.chars().next().unwrap() as u32
            );
            assert_eq!(glyph.chars().count(), 1, "{glyph:?}");
        }
    }

    #[test]
    fn every_pill_and_check_glyph_is_one_cell_wide_and_the_one_hs_d235_picked() {
        let glyphs = [
            (QUEUED, 0xF0766),
            (MERGING, 0xF1464),
            (MERGED, 0xF05E0),
            (FAILED, 0xF0159),
            (DRAFT, 0xF059),
            (CANCELLED, 0xF0159),
            (CHECKS_PASSED, 0xF05E0),
            (CHECKS_FAILED, 0xF0159),
            (CHECKS_PENDING, 0xF0B94),
            (CHECKS_NONE, 0xF0374),
        ];
        for (glyph, code) in glyphs {
            assert_eq!(
                glyph.chars().collect::<Vec<_>>(),
                [char::from_u32(code).unwrap()],
                "U+{code:X}"
            );
            assert_eq!(Span::raw(glyph).width(), 1, "U+{code:X}");
        }
    }

    #[test]
    fn an_archived_dot_keeps_its_glyph_in_colour_8_and_never_turns() {
        let grey = Style::new().fg(Color::DarkGray);
        for (dot, glyph) in [
            (Dot::Red, NEEDS_YOU),
            (Dot::Yellow, NEEDS_YOU),
            (Dot::Magenta, NEEDS_YOU),
            (Dot::Cyan, NEEDS_YOU),
            (Dot::White, NEEDS_YOU),
            (Dot::Waiting, WAITING),
            (Dot::Idle, IDLE),
            (Dot::Stopped, STOPPED),
        ] {
            assert_eq!(dot_span(dot, 0, true), Span::styled(glyph, grey), "{dot:?}");
        }
        for f in 0..SPINNER.len() {
            assert_eq!(
                dot_span(Dot::Working(None), f, true),
                Span::styled("\u{E3D5}", grey),
                "frame {f}"
            );
        }
    }

    #[test]
    fn the_frame_is_the_unix_time_in_twentieths_of_a_second_modulo_28() {
        let at = |ms: u64| frame(UNIX_EPOCH + std::time::Duration::from_millis(ms));
        assert_eq!(FRAME_MS, 50);
        assert_eq!(
            [0, 49, 50, 99, 100, 1399, 1400, 1450, 1500].map(at),
            [0, 0, 1, 1, 2, 27, 0, 1, 2]
        );
        assert_eq!(
            frame(UNIX_EPOCH - std::time::Duration::from_secs(1)),
            0,
            "before the epoch: the first frame"
        );
    }

    #[test]
    fn counters_are_in_their_status_colour_and_nothing_when_zero() {
        assert_eq!(
            count_style(JobStatus::Queued, false, 3),
            Style::new().fg(Color::White)
        );
        assert_eq!(
            count_style(JobStatus::Draft, false, 3),
            Style::new().fg(Color::LightMagenta)
        );
        assert_eq!(
            count_style(JobStatus::Cancelled, false, 3),
            Style::new().fg(Color::DarkGray)
        );
        assert_eq!(
            count_style(JobStatus::Failed, false, 0),
            Style::new().fg(Color::DarkGray)
        );
        assert_eq!(
            separator(),
            Span::styled("⋅", Style::new().fg(Color::DarkGray))
        );
    }

    #[test]
    fn every_pr_state_and_check_has_its_icon_and_meaning() {
        let states = [
            (PrState::Open, Meaning::NotStarted),
            (PrState::Merged, Meaning::Done),
            (PrState::Closed, Meaning::Waiting),
        ];
        for (s, m) in states {
            assert_eq!(pr_state(s), m, "{s:?}");
        }
        assert_eq!(PR_LEGEND.map(|(_, s)| s), states.map(|(s, _)| s));
        assert_eq!(pr_open(true), Meaning::Review);
        assert_eq!(pr_open(false), Meaning::NotStarted);
        assert_eq!(PR_CONFLICT, ("conflict", Meaning::Failed));
        assert_eq!(PR_CLOSED_ASKED, ("closed", Meaning::Failed));
        assert_eq!(PR_CONFLICT_OWNED, ("conflict", Meaning::Waiting));
        assert_eq!(
            checks_of(Checks::Failed, true),
            (CHECKS_FAILED, Meaning::Waiting)
        );
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
    fn a_pulse_bar_is_the_bright_red_magenta_green_or_cyan_by_its_kind() {
        let table = [
            (Kind::Error, Color::LightRed),
            (Kind::Plan, Color::LightMagenta),
            (Kind::PrOpened, Color::LightCyan),
            (Kind::Escalation, Color::LightCyan),
            (Kind::Merged, Color::LightGreen),
            (Kind::Other, Color::LightCyan),
        ];
        for (kind, colour) in table {
            assert_eq!(pulse_kind(kind), colour, "{kind:?}");
        }
        assert_eq!(
            color(Meaning::Nothing),
            Color::DarkGray,
            "an empty slot's ▁ is grey"
        );
    }

    /// No other file of the cockpit's views names a colour. `moon_tests.rs` is exempt: it pins the dots' colours (hs-d579) and draws nothing; so are `pill_colours.rs` and `palette/`, which pin the palette's own styles.
    #[test]
    fn no_colour_outside_the_palette() {
        fn walk(dir: &std::path::Path, offenders: &mut Vec<String>) {
            let shorthand = [
                ".black()",
                ".red()",
                ".green()",
                ".yellow()",
                ".blue()",
                ".magenta()",
                ".cyan()",
                ".gray()",
                ".dark_gray()",
                ".white()",
                ".on_",
            ];
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, offenders);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| {
                        n != "palette.rs" && n != "moon_tests.rs" && n != "pill_colours.rs"
                    })
                    && !path
                        .ancestors()
                        .any(|a| a.file_name().is_some_and(|n| n == "palette"))
                {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for (n, line) in text.lines().enumerate() {
                        if line.contains("Color::") || shorthand.iter().any(|s| line.contains(s)) {
                            offenders.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                n + 1,
                                line.trim()
                            ));
                        }
                    }
                }
            }
        }
        let mut offenders = Vec::new();
        walk(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
            &mut offenders,
        );
        assert!(
            offenders.is_empty(),
            "colours belong in tui/palette.rs:\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn a_pill_is_the_colour_inverted() {
        assert_eq!(pill(Meaning::Review), block(Color::Yellow));
    }

    #[test]
    fn every_ui_role_has_its_colour() {
        let table = [
            (Ui::Note, Color::Yellow),
            (Ui::You, Color::Gray),
            (Ui::Agent, Color::Cyan),
            (Ui::Tool, Color::Cyan),
        ];
        for (u, c) in table {
            assert_eq!(ui(u), c, "{u:?}");
        }
    }

    #[test]
    fn a_note_is_yellow_and_an_error_red() {
        assert_eq!(note(), Style::new().fg(Color::Yellow));
        assert_eq!(error(), Style::new().fg(Color::Red));
    }
}
