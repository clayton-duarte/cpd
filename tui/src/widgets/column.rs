//! The right column's drawing (commands.md §13.2), a port of the reference's `draw_dispatch`: the machine (hostname, CPU %,
//! one % per GPU, memory %) under one blank row, then the Inbox section directly under it, taking the rest of the column.
//! Pure: [`draw`] only writes to the frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph};

use super::host::Usage;
use crate::link::Links;
use crate::palette;
use super::inbox::{self, Shape};
use cpd_core::inbox::model::Item;
use time::OffsetDateTime;

/// Nerd Font `nf-oct-cpu` and `nf-md-memory`: one cell each.
const CPU_ICON: &str = "\u{F4BC}";
const GPU_ICON: &str = "\u{E266}";
const MEM_ICON: &str = "\u{EFC5}";

/// The machine the column names: its hostname and its CPU % and memory %.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Machine {
    pub name: String,
    pub usage: Usage,
}

impl Machine {
    /// ` <cpu icon> N%  <gpu icon> A% B%  <mem icon> M%`, one % per GPU and no GPU part when there is none; a value not known (the
    /// CPU until a second sample, or one that cannot be read) is `–`.
    fn usage_line(&self) -> String {
        let known = |v: Option<u8>| v.map_or("–".to_owned(), |v| v.to_string());
        let gpus = &self.usage.gpus;
        let gpu = if gpus.is_empty() { String::new() } else { format!("{GPU_ICON} {}  ", gpus.iter().map(|g| format!("{}%", known(*g))).collect::<Vec<_>>().join(" ")) };
        format!(" {CPU_ICON} {}%  {gpu}{MEM_ICON} {}%", known(self.usage.cpu), known(self.usage.mem))
    }
}

fn bold(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), palette::read_only_text().add_modifier(Modifier::BOLD))
}


/// Which items a scrolling list draws, and the cues around them (commands.md §13.2; the Inbox section
/// scrolls with this code, one row an item).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Window {
    /// The first item drawn, after clamping.
    pub first: usize,
    /// How many are drawn.
    pub shown: usize,
    /// A `▲ <first> more` row (right-aligned, `scroll::cue`) opens the list.
    pub cue_above: bool,
    /// How many are hidden below: a `▼ <n> more` row ends the list when not 0.
    pub below: usize,
}

impl Window {
    /// The window at offset `scroll` of `count` items in `rows` list rows (the two cue lines are not in `rows`): `scroll` is
    /// clamped to [`Window::max_scroll`].
    pub(crate) fn new(count: usize, rows: usize, scroll: usize) -> Window {
        let first = scroll.min(Window::max_scroll(count, rows));
        let shown = (count - first).min(rows);
        Window { first, shown, cue_above: first > 0, below: count - first - shown }
    }

    /// The largest offset: the last item reaches the bottom of the `rows`; 0 when every one fits.
    pub(crate) fn max_scroll(count: usize, rows: usize) -> usize {
        count.saturating_sub(rows)
    }
}

/// The largest offset for `count` items of `item_rows` rows each in a list of `height` rows: the last one reaches the
/// bottom (under the `▲` cue, which takes a row). 0 when every one fits.
pub(crate) fn max_scroll(count: usize, height: usize, item_rows: usize) -> usize {
    if count * item_rows <= height {
        return 0;
    }
    count.saturating_sub((height.saturating_sub(1)) / item_rows).min(count.saturating_sub(1))
}

/// The window at offset `scroll` (clamped to [`max_scroll`]). A cue takes one row of the list, and an item is whole.
pub(crate) fn window(count: usize, height: usize, item_rows: usize, scroll: usize) -> Window {
    let first = scroll.min(max_scroll(count, height, item_rows));
    let cue_above = first > 0 && height > 0;
    let room = height - usize::from(cue_above);
    let left = count - first;
    if left * item_rows <= room {
        return Window { first, shown: left, cue_above, below: 0 };
    }
    let shown = room.saturating_sub(1) / item_rows;
    Window { first, shown, cue_above, below: left - shown }
}

/// What [`draw`] reports to the view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drawn {
    /// Every link drawn (the Inbox rows' labels): a click on one opens its url.
    pub links: Links,
    /// The Inbox section as drawn, directly under the machine (its first row is the empty one): the wheel there scrolls it, a
    /// click on its toggle row folds or unfolds it.
    pub inbox: Rect,
    /// The Inbox section as drawn: capped by the column's rows and scrolled ([`Shape::hit`], [`Shape::wheeled`]).
    pub inbox_shape: Shape,
}

/// Draws the column in `area`: a blank row, the machine, then the Inbox section from the first row under them (commands.md
/// §13.2, pro-d130), not pinned to the bottom. The Inbox draws `inbox`, folded or `expanded`, from the item `inbox_scroll`, capped
/// at the rows below the machine; its ages are taken at `now`, the spinner in the glyph cell of a `pr-ready` item whose pull
/// request is in `merging` at `frame` (`palette::frame`). It draws no note (commands.md §17 "No notes"); the snapshot's error is
/// [`draw_error`]'s.
pub fn draw(
    f: &mut Frame,
    area: Rect,
    machine: &Machine,
    frame: usize,
    inbox: &[Item],
    inbox_scroll: usize,
    now: OffsetDateTime,
    merging: &std::collections::BTreeSet<String>,
) -> Drawn {
    let [_top, home, rest] = Layout::vertical([Constraint::Length(1), Constraint::Length(2), Constraint::Min(0)]).areas(area);
    let shape = Shape::new(inbox.len()).capped(rest.height).scrolled(inbox_scroll);
    let inbox_area = Rect { height: shape.height().min(rest.height), ..rest };
    let links = inbox::draw(f, inbox_area, inbox, shape, now, &|item| inbox::merges(merging, item).then_some(frame));
    f.render_widget(Paragraph::new(vec![Line::from(bold(format!(" {}", machine.name))), Line::styled(machine.usage_line(), palette::read_only_text())]), home);
    Drawn { links, inbox: inbox_area, inbox_shape: shape }
}


/// The snapshot's `error`, a lasting state of the column (commands.md §17 "No notes", rule (b)): it replaces the last row
/// of `area`, in `palette::error()`, over what [`draw`] drew.
pub fn draw_error(f: &mut Frame, area: Rect, error: &str) {
    let last = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
    f.render_widget(Clear, last);
    f.render_widget(Paragraph::new(Span::styled(error.to_owned(), palette::error())), last);
}


#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::test_text::row;

    use super::*;

    fn machine() -> Machine {
        Machine { name: "homeserver".to_owned(), usage: Usage { cpu: Some(23), gpus: vec![], mem: Some(41) } }
    }

    #[test]
    fn a_value_not_known_is_a_dash() {
        let usage = |cpu, mem| Machine { name: "homeserver".to_owned(), usage: Usage { cpu, gpus: vec![], mem } }.usage_line();
        assert_eq!(usage(None, Some(100)), " \u{F4BC} –%  \u{EFC5} 100%", "the CPU until a second sample");
        assert_eq!(usage(Some(7), None), " \u{F4BC} 7%  \u{EFC5} –%", "a memory that cannot be read");
    }

    #[test]
    fn a_snapshot_error_replaces_the_last_row() {
        let mut terminal = Terminal::new(TestBackend::new(30, 15)).unwrap();
        terminal
            .draw(|f| {
                draw(f, f.area(), &machine(), 0, &[], 0, OffsetDateTime::UNIX_EPOCH, &Default::default());
                draw_error(f, f.area(), "no_cockpit: nothing here");
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();
        assert_eq!(row(&buf, 14), "no_cockpit: nothing here");
        assert_eq!(buf[(0, 14)].fg, palette::error().fg.unwrap());
        assert!(row(&buf, 4).contains("Inbox"), "the rest is as drawn");
    }
}
