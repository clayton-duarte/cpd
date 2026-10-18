//! The Inbox counter at the right of the StatusBar (commands.md §13.2): `inbox <n>`, `<n>` the open, visible items
//! (the count of the tree's Inbox header), in the colour of the most urgent of them; dim `inbox 0` when there is none.

use ratatui::{
    style::{Modifier, Style},
    text::Line,
};
use rusqlite::Connection;

use cpd_core::error::CpdError;
use cpd_core::inbox::model::{self, Filter, State, Urgency};
use cpd_core::store::Store;

use crate::palette;
use crate::read::Dot;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inbox {
    /// The open items that are neither snoozed nor dismissed.
    pub count: usize,
    /// The urgency of the most urgent of them; `None` when `count` is 0.
    pub urgency: Option<Urgency>,
}

impl Inbox {
    /// The visible items of every Context, as `cpd inbox list` reads them.
    pub fn read(store: &mut Store) -> Result<Inbox, CpdError> {
        store.read(Inbox::of)
    }

    fn of(conn: &Connection) -> Result<Inbox, CpdError> {
        let now: String = conn.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| r.get(0))?;
        let items = model::list(conn, &Filter { context: None, state: State::Visible { now } })?;
        Ok(Inbox { count: items.len(), urgency: items.iter().map(|i| i.urgency).max() })
    }

    /// `inbox <n>` in the colour of the most urgent item; dim when there is none.
    pub fn line(&self) -> Line<'static> {
        let style = match self.urgency {
            Some(urgency) => palette::dot_span(Dot::from(urgency), 0, false).style,
            None => Style::new().add_modifier(Modifier::DIM),
        };
        Line::styled(format!("inbox {}", self.count), style)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    #[test]
    fn the_counter_is_in_the_colour_of_the_most_urgent_item() {
        for (urgency, color) in
            [(Urgency::Red, Color::Red), (Urgency::Yellow, Color::Yellow), (Urgency::Magenta, Color::Magenta), (Urgency::White, Color::White)]
        {
            let line = Inbox { count: 3, urgency: Some(urgency) }.line();
            assert_eq!(line.to_string(), "inbox 3");
            assert_eq!(line.style.fg, Some(color), "{urgency:?}");
        }
    }

    #[test]
    fn with_no_item_it_is_a_dim_zero() {
        let line = Inbox { count: 0, urgency: None }.line();
        assert_eq!(line.to_string(), "inbox 0");
        assert_eq!((line.style.fg, line.style.add_modifier.contains(Modifier::DIM)), (None, true));
    }
}
