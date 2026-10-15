//! The Plan's PR in the top row, ported from the cockpit's `pr.rs`: `#n state checks reviews comments`. The cockpit
//! asked `gh pr view` for it; cpd 2 reads what `pr poll` stored (commands.md §9, §17), so there is no live data to go
//! stale and nothing here runs a command.

use ratatui::{
    style::Stylize,
    text::{Line, Span},
};

use crate::palette;
use crate::read::{Pr, PrState};

fn count(n: usize, noun: &str) -> String {
    format!("{n} {noun}{}", if n == 1 { "" } else { "s" })
}

/// The PR of the Plan, or `no PR`. A PR that `pr poll` has not polled yet is dimmed and says so: its state and checks
/// are the ones it was opened with.
pub fn line(pr: Option<&Pr>) -> Line<'static> {
    let Some(pr) = pr else { return Line::from("no PR").dim() };
    let state = match pr.state {
        PrState::Open => "open",
        PrState::Closed => "closed",
        PrState::Merged => "merged",
    };
    let (sym, checks) = palette::checks(pr.checks_summary());
    let reviews = if pr.reviews == 0 { "no review".to_owned() } else { count(pr.reviews, "review") };
    let mut spans: Vec<Span> = vec![
        format!("#{}", pr.number).into(),
        " ".into(),
        state.fg(palette::color(palette::pr_state(pr.state))),
        " ".into(),
        sym.fg(palette::color(checks)),
        " ".into(),
        reviews.into(),
    ];
    if pr.comments > 0 {
        spans.push(format!(", {}", count(pr.comments, "comment")).into());
    }
    if pr.polled_at.is_none() {
        spans.push(" not polled yet".into());
        return Line::from(spans).dim();
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ratatui::style::Modifier;

    use super::*;
    use crate::palette::Meaning;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn pr(state: PrState, checks: &[(&str, &str)], reviews: usize, comments: usize, polled: bool) -> Pr {
        Pr {
            number: 7,
            url: "https://example.test/pull/7".into(),
            opened_at: "2026-01-05T00:00:00.000Z".into(),
            polled_at: polled.then(|| "2026-01-06T00:00:00.000Z".into()),
            state,
            checks: checks.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            reviews,
            comments,
        }
    }

    #[test]
    fn a_row_shows_number_state_checks_reviews_and_comments() {
        let l = line(Some(&pr(PrState::Open, &[("ci", "FAILURE"), ("lint", "SUCCESS")], 1, 2, true)));
        assert_eq!(text(&l), "#7 open ✗ 1 review, 2 comments");
        let l = line(Some(&pr(PrState::Merged, &[("ci", "SUCCESS")], 0, 0, true)));
        assert_eq!(text(&l), "#7 merged ✓ no review");
        let l = line(Some(&pr(PrState::Closed, &[], 2, 1, true)));
        assert_eq!(text(&l), "#7 closed – 2 reviews, 1 comment");
    }

    #[test]
    fn a_row_takes_its_colours_from_the_palette() {
        let colours = |state, checks: &[(&str, &str)]| {
            let spans = line(Some(&pr(state, checks, 0, 0, true))).spans;
            (spans[2].style.fg.unwrap(), spans[4].style.fg.unwrap())
        };
        let c = palette::color;
        assert_eq!(colours(PrState::Open, &[("ci", "SUCCESS")]), (c(Meaning::Review), c(Meaning::Done)));
        assert_eq!(colours(PrState::Merged, &[("ci", "FAILURE")]), (c(Meaning::Done), c(Meaning::Failed)));
        assert_eq!(colours(PrState::Closed, &[]), (c(Meaning::Failed), c(Meaning::Nothing)));
        assert_eq!(colours(PrState::Open, &[("ci", "IN_PROGRESS")]), (c(Meaning::Review), c(Meaning::InProgress)));
    }

    #[test]
    fn no_pr_is_dim() {
        let l = line(None);
        assert_eq!(text(&l), "no PR");
        assert!(l.style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn a_pr_not_polled_yet_is_dim_and_says_so() {
        let l = line(Some(&pr(PrState::Open, &[], 0, 0, false)));
        assert_eq!(text(&l), "#7 open – no review not polled yet");
        assert!(l.style.add_modifier.contains(Modifier::DIM));
        assert!(!line(Some(&pr(PrState::Open, &[], 0, 0, true))).style.add_modifier.contains(Modifier::DIM));
    }
}
