//! The palette: the one place with colours and status icons. Every view takes them from here, so changing one meaning
//! (or one icon) changes it in the Plan pills, the Context column, the Dispatch rows and the legend alike.
//! `no_colour_outside_the_palette` fails when any other source file names a colour.

use ratatui::{
    style::{Color, Style},
    text::Span,
};

use crate::pr::Checks;

/// What a colour stands for. One colour per meaning, whatever view shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meaning {
    /// A queued Job.
    NotStarted,
    /// A running Job; phase `dispatch`; PR checks pending.
    InProgress,
    /// A Job in review; an open PR; phase `pr`.
    Review,
    /// A merged Job; a merged PR; phase `done`; checks passed.
    Done,
    /// A failed (blocked, escalated) Job; a closed PR; checks failed.
    Failed,
    /// Phase `plan`.
    Planning,
    /// Phase `context`.
    Grounding,
    /// A draft Job, a zero count, a separator, no Workflow, no checks, unknown.
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

/// One colour per meaning, with two deliberate exceptions: grounding (phase `context`) and not started share white,
/// since a Workflow that is still grounding has nothing running yet; done shares dark grey with nothing, the User's choice.
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

/// A Plan pill: black on the colour of a meaning.
pub fn pill(m: Meaning) -> Style {
    Style::new().fg(Color::Black).bg(color(m))
}

/// Colours that are not a status: the prompt and notes, the Worker view's roles.
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

/// Icon and meaning of a Job status (cpd's names; `closed` is merged). Anything else is `?`, nothing.
pub fn job_status(status: &str) -> (&'static str, Meaning) {
    match status {
        "queued" => ("○", Meaning::NotStarted),
        "running" => ("◐", Meaning::InProgress),
        "in_review" => ("⇥", Meaning::Review),
        "closed" | "merged" => ("✓", Meaning::Done),
        "failed" | "blocked" | "escalated" => ("✗", Meaning::Failed),
        "draft" => ("◌", Meaning::Nothing),
        _ => ("?", Meaning::Nothing),
    }
}

/// The Job statuses the legend lists: the name shown, and the status it stands for.
pub const JOB_LEGEND: [(&str, &str); 7] =
    [("queued", "queued"), ("running", "running"), ("merging", "in_review"), ("merged", "closed"), ("failed", "failed"), ("draft", "draft"), ("unknown", "?")];

/// Meaning of a Workflow phase (`None`: no Workflow; an unknown phase is nothing too).
pub fn phase(phase: Option<&str>) -> Meaning {
    match phase {
        Some("context") => Meaning::Grounding,
        Some("plan") => Meaning::Planning,
        Some("dispatch") => Meaning::InProgress,
        Some("pr") => Meaning::Review,
        Some("done") => Meaning::Done,
        _ => Meaning::Nothing,
    }
}

/// The phases the legend lists, with the name shown.
pub const PHASE_LEGEND: [(&str, Option<&str>); 6] =
    [("no Workflow", None), ("context", Some("context")), ("plan", Some("plan")), ("dispatch", Some("dispatch")), ("pr", Some("pr")), ("done", Some("done"))];

/// The Context dot: `●` when the session needs the User, `○` when not; coloured by the Workflow's phase.
pub fn dot(needs_you: bool, workflow_phase: Option<&str>) -> Span<'static> {
    Span::styled(if needs_you { "●" } else { "○" }, fg(phase(workflow_phase)))
}

/// The `⋅` between the Context counters.
pub fn separator() -> Span<'static> {
    Span::styled("⋅", fg(Meaning::Nothing))
}

/// Style of a Context counter: its status colour, or nothing when the count is zero.
pub fn count_style(status: &str, n: usize) -> Style {
    fg(if n == 0 { Meaning::Nothing } else { job_status(status).1 })
}

/// Meaning of a PR's state (`open`, `merged`, `closed`; anything else is nothing).
pub fn pr_state(state: &str) -> Meaning {
    match state {
        "open" => Meaning::Review,
        "merged" => Meaning::Done,
        "closed" => Meaning::Failed,
        _ => Meaning::Nothing,
    }
}

/// The PR states the legend lists.
pub const PR_LEGEND: [&str; 3] = ["open", "merged", "closed"];

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
pub const CHECKS_LEGEND: [(&str, Checks); 4] = [("passed", Checks::Passed), ("failed", Checks::Failed), ("pending", Checks::Pending), ("none", Checks::None)];

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
            ("queued", "○", Meaning::NotStarted),
            ("running", "◐", Meaning::InProgress),
            ("in_review", "⇥", Meaning::Review),
            ("closed", "✓", Meaning::Done),
            ("merged", "✓", Meaning::Done),
            ("failed", "✗", Meaning::Failed),
            ("blocked", "✗", Meaning::Failed),
            ("escalated", "✗", Meaning::Failed),
            ("draft", "◌", Meaning::Nothing),
            ("whatever", "?", Meaning::Nothing),
        ];
        for (status, icon, m) in table {
            assert_eq!(job_status(status), (icon, m), "{status}");
        }
    }

    #[test]
    fn every_phase_has_its_meaning() {
        let table = [
            (None, Meaning::Nothing),
            (Some("context"), Meaning::Grounding),
            (Some("plan"), Meaning::Planning),
            (Some("dispatch"), Meaning::InProgress),
            (Some("pr"), Meaning::Review),
            (Some("done"), Meaning::Done),
            (Some("unheard of"), Meaning::Nothing),
        ];
        for (p, m) in table {
            assert_eq!(phase(p), m, "{p:?}");
        }
    }

    #[test]
    fn the_dot_is_filled_when_the_session_needs_you_and_coloured_by_phase() {
        assert_eq!(dot(true, Some("dispatch")), Span::styled("●", Style::new().fg(Color::Green)));
        assert_eq!(dot(false, Some("pr")), Span::styled("○", Style::new().fg(Color::Yellow)));
        assert_eq!(dot(false, None), Span::styled("○", Style::new().fg(Color::DarkGray)));
    }

    #[test]
    fn counters_are_in_their_status_colour_and_nothing_when_zero() {
        assert_eq!(count_style("queued", 3), Style::new().fg(Color::White));
        assert_eq!(count_style("draft", 3), Style::new().fg(Color::DarkGray));
        assert_eq!(count_style("failed", 0), Style::new().fg(Color::DarkGray));
        assert_eq!(separator(), Span::styled("⋅", Style::new().fg(Color::DarkGray)));
    }

    #[test]
    fn every_pr_state_has_its_meaning() {
        let table = [("open", Meaning::Review), ("merged", Meaning::Done), ("closed", Meaning::Failed), ("draft", Meaning::Nothing)];
        for (s, m) in table {
            assert_eq!(pr_state(s), m, "{s}");
        }
        assert_eq!(PR_LEGEND, ["open", "merged", "closed"]);
    }

    #[test]
    fn every_check_has_its_icon_and_meaning() {
        let table = [(Checks::Passed, "✓", Meaning::Done), (Checks::Failed, "✗", Meaning::Failed), (Checks::Pending, "…", Meaning::InProgress), (Checks::None, "–", Meaning::Nothing)];
        for (c, icon, m) in table {
            assert_eq!(checks(c), (icon, m), "{c:?}");
        }
    }

    #[test]
    fn a_pill_is_black_on_the_colour() {
        assert_eq!(pill(Meaning::Review), Style::new().fg(Color::Black).bg(Color::Yellow));
    }

    #[test]
    fn ui_colours() {
        assert_eq!([Ui::Note, Ui::You, Ui::Agent, Ui::Tool].map(ui), [Color::Yellow, Color::Cyan, Color::Green, Color::Yellow]);
    }

    #[test]
    fn no_colour_outside_the_palette() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let shorthand = [".black()", ".red()", ".green()", ".yellow()", ".blue()", ".magenta()", ".cyan()", ".gray()", ".dark_gray()", ".white()", ".on_"];
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.file_name().is_some_and(|n| n == "palette.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for (n, line) in text.lines().enumerate() {
                if line.contains("Color::") || shorthand.iter().any(|s| line.contains(s)) {
                    offenders.push(format!("{}:{}: {}", path.file_name().unwrap().to_string_lossy(), n + 1, line.trim()));
                }
            }
        }
        assert!(offenders.is_empty(), "colours belong in palette.rs:\n{}", offenders.join("\n"));
    }
}
