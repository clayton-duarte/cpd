//! The Workers indicator, ported from the cockpit's `workers.rs`: `workers N/M`. `N` is the Attempts holding a
//! concurrency slot (not `ended`, machine-wide) and `M` the config's `concurrency`, both from the read model
//! (`Slots`), where the cockpit read `slots/*.json` and probed pids.

use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use super::palette::{self, Meaning};
use crate::tui::read::Slots;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Workers {
    /// Attempts holding a slot now.
    pub running: usize,
    /// The machine-wide limit.
    pub limit: u32,
}

impl Workers {
    pub fn of(slots: &Slots) -> Workers {
        Workers { running: slots.used(), limit: slots.limit }
    }

    /// `workers N/M` and `N/M`, widest first. `N` is in the "in progress" colour above 0, else "nothing"; `workers`
    /// and `/M` are dim.
    pub fn forms(&self) -> [Line<'static>; 2] {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let n = Span::styled(
            self.running.to_string(),
            palette::fg(if self.running > 0 { Meaning::InProgress } else { Meaning::Nothing }),
        );
        let limit = Span::styled(format!("/{}", self.limit), dim);
        [Line::from(vec![Span::styled("workers ", dim), n.clone(), limit.clone()]), Line::from(vec![n, limit])]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(l: &Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn the_long_form_has_a_coloured_count_and_dim_words() {
        let [long, short] = Workers { running: 3, limit: 10 }.forms();
        assert_eq!((text(&long), text(&short)), ("workers 3/10".to_owned(), "3/10".to_owned()));
        assert_eq!(long.spans[1].style.fg, Some(palette::color(Meaning::InProgress)), "N is in progress above 0");
        assert!(long.spans[0].style.add_modifier.contains(Modifier::DIM));
        assert!(long.spans[2].style.add_modifier.contains(Modifier::DIM));
        let [idle, _] = Workers { running: 0, limit: 10 }.forms();
        assert_eq!(idle.spans[1].style.fg, Some(palette::color(Meaning::Nothing)), "N is nothing at 0");
    }
}
