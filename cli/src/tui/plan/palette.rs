//! The plan view's palette, ported from the cockpit's `palette.rs` (branch `cpd/cpd-ui`): the one place in this view
//! with colours and status icons. `no_colour_outside_the_palette` fails when another file of the view names a colour.

use ratatui::style::{Color, Style};

use crate::tui::read::{Checks, PrState};

/// What a colour stands for. One colour per meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Meaning {
    /// A queued Job.
    NotStarted,
    /// A running Job; PR checks pending.
    InProgress,
    /// A Job in its merge; an open PR.
    Review,
    /// A merged Job; a merged PR; checks passed.
    Done,
    /// A failed Job; a closed PR; checks failed.
    Failed,
    /// A draft Job, a zero count, a separator, no PR, no checks.
    Nothing,
}

/// One colour per meaning; done shares dark grey with nothing, the User's choice (cpd-ui `done-dark-grey`).
pub fn color(m: Meaning) -> Color {
    match m {
        Meaning::NotStarted => Color::White,
        Meaning::InProgress => Color::Green,
        Meaning::Review => Color::Yellow,
        Meaning::Done => Color::DarkGray,
        Meaning::Failed => Color::Red,
        Meaning::Nothing => Color::DarkGray,
    }
}

/// Foreground in the colour of a meaning.
pub fn fg(m: Meaning) -> Style {
    Style::new().fg(color(m))
}

/// A Job pill: black on the colour of a meaning.
pub fn pill(m: Meaning) -> Style {
    Style::new().fg(Color::Black).bg(color(m))
}

/// Notes at the foot of the view.
pub fn note() -> Style {
    Style::new().fg(Color::Yellow)
}

/// Icon and meaning of a Job as the chart shows it.
pub fn job(status: super::flow::Live) -> (&'static str, Meaning) {
    use super::flow::Live::*;
    match status {
        Draft => ("◌", Meaning::Nothing),
        Queued => ("○", Meaning::NotStarted),
        Running => ("◐", Meaning::InProgress),
        Merging => ("⇥", Meaning::Review),
        Merged => ("✓", Meaning::Done),
        Failed => ("✗", Meaning::Failed),
    }
}

/// Meaning of a PR's state.
pub fn pr_state(state: PrState) -> Meaning {
    match state {
        PrState::Open => Meaning::Review,
        PrState::Merged => Meaning::Done,
        PrState::Closed => Meaning::Failed,
    }
}

/// Icon and meaning of a PR's checks.
pub fn checks(c: Checks) -> (&'static str, Meaning) {
    match c {
        Checks::Passed => ("✓", Meaning::Done),
        Checks::Failed => ("✗", Meaning::Failed),
        Checks::Pending => ("…", Meaning::InProgress),
        Checks::None => ("–", Meaning::Nothing),
    }
}

#[cfg(test)]
mod tests {
    use super::super::flow::Live;
    use super::*;

    #[test]
    fn every_meaning_has_its_colour() {
        let table = [
            (Meaning::NotStarted, Color::White),
            (Meaning::InProgress, Color::Green),
            (Meaning::Review, Color::Yellow),
            (Meaning::Done, Color::DarkGray),
            (Meaning::Failed, Color::Red),
            (Meaning::Nothing, Color::DarkGray),
        ];
        for (m, c) in table {
            assert_eq!(color(m), c, "{m:?}");
        }
    }

    #[test]
    fn every_job_status_has_its_icon_and_meaning() {
        let table = [
            (Live::Draft, "◌", Meaning::Nothing),
            (Live::Queued, "○", Meaning::NotStarted),
            (Live::Running, "◐", Meaning::InProgress),
            (Live::Merging, "⇥", Meaning::Review),
            (Live::Merged, "✓", Meaning::Done),
            (Live::Failed, "✗", Meaning::Failed),
        ];
        for (status, icon, m) in table {
            assert_eq!(job(status), (icon, m), "{status:?}");
        }
    }

    #[test]
    fn every_pr_state_and_check_has_its_meaning() {
        assert_eq!(
            [PrState::Open, PrState::Merged, PrState::Closed].map(pr_state),
            [Meaning::Review, Meaning::Done, Meaning::Failed]
        );
        let table = [
            (Checks::Passed, "✓", Meaning::Done),
            (Checks::Failed, "✗", Meaning::Failed),
            (Checks::Pending, "…", Meaning::InProgress),
            (Checks::None, "–", Meaning::Nothing),
        ];
        for (c, icon, m) in table {
            assert_eq!(checks(c), (icon, m), "{c:?}");
        }
    }

    #[test]
    fn a_pill_is_black_on_the_colour() {
        assert_eq!(pill(Meaning::Review), Style::new().fg(Color::Black).bg(Color::Yellow));
    }

    #[test]
    fn no_colour_outside_the_palette() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tui/plan");
        let shorthand = [
            ".black()", ".red()", ".green()", ".yellow()", ".blue()", ".magenta()", ".cyan()", ".gray()", ".dark_gray()",
            ".white()", ".on_",
        ];
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") || path.file_name().is_some_and(|n| n == "palette.rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // The tests of a file may name a colour to check one; its code may not.
            let code = text.split("#[cfg(test)]").next().unwrap();
            for (n, line) in code.lines().enumerate() {
                if line.contains("Color::") || shorthand.iter().any(|s| line.contains(s)) {
                    offenders.push(format!("{}:{}: {}", path.file_name().unwrap().to_string_lossy(), n + 1, line.trim()));
                }
            }
        }
        assert!(offenders.is_empty(), "colours belong in palette.rs:\n{}", offenders.join("\n"));
    }
}
