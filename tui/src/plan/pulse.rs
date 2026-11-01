//! Pulse in the Plan view (commands.md §13.2): one row per Plan of the Manager's Context, each with a sparkline of the
//! Plan's activity, drawn from the read model of [`crate::read::pulse`]. Everything here is pure: the rows' text and
//! styles, the bucket count, the wheel offset and the row under a click.
//!
//! A row, inside the view's margin: `<plan id padded>  <sparkline>  <age in 9>`. No icon (User, hs-d327): each bar is
//! coloured by the kind of activity in its slot, an empty slot is a grey `▁`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::dialog::scroll::Scroll;
use crate::palette::{self, Meaning};
use crate::read::{Bucket, Kind, Pulse, PulseRow, QUIET_MINUTES, age_text};

/// The fewest buckets a row draws, whatever the width.
const MIN_BUCKETS: usize = 12;
/// Columns of the age cell.
const AGE_WIDTH: usize = 9;
/// The bars, level 1 to 8.
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The buckets a row draws in `width` columns (inside the margin) for ids `id_width` wide: what the other cells leave,
/// at least [`MIN_BUCKETS`].
pub fn buckets(width: usize, id_width: usize) -> usize {
    // id, two spaces, [sparkline], two spaces, age
    width
        .saturating_sub(id_width + 2 + 2 + AGE_WIDTH)
        .max(MIN_BUCKETS)
}

/// The width of the longest Plan id.
fn id_width(pulse: &Pulse) -> usize {
    pulse
        .rows
        .iter()
        .map(|r| r.plan.as_str().chars().count())
        .max()
        .unwrap_or(0)
}

/// The glyph of a bucket on the scale `max` (the largest count over all rows): a bar in the colour of its kind, or a grey
/// `▁` when nothing happened in it.
pub fn glyph(bucket: &Bucket, max: u32) -> Span<'static> {
    match bucket.kind {
        Some(kind) if bucket.count > 0 => {
            let level = (u64::from(bucket.count) * 8)
                .div_ceil(u64::from(max.max(bucket.count)))
                .clamp(1, 8) as usize;
            bar(level, kind)
        }
        _ => Span::styled(BARS[0].to_string(), palette::fg(Meaning::Nothing)),
    }
}

/// A bar of `level` (1..=8) in the colour of `kind`.
pub fn bar(level: usize, kind: Kind) -> Span<'static> {
    Span::styled(
        BARS[level.clamp(1, 8) - 1].to_string(),
        Style::new().fg(palette::pulse_kind(kind)),
    )
}

/// The age cell, [`AGE_WIDTH`] wide, right-aligned: muted, as is `quiet <age>` (hs-d534: no longer a yellow).
pub(crate) fn age_cell(row: &PulseRow) -> Span<'static> {
    let age = row.age_minutes.map(|m| age_text(m * 60));
    if row.quiet {
        let text = age.map_or("quiet".to_owned(), |a| format!("quiet {a}"));
        return Span::styled(format!("{text:>AGE_WIDTH$}"), palette::muted_text());
    }
    Span::styled(
        format!("{:>AGE_WIDTH$}", age.unwrap_or_default()),
        palette::muted_text(),
    )
}

/// One row's line with `n` buckets: the last `n` of the row's, blank on the left when it has fewer.
pub fn row_line(row: &PulseRow, id_width: usize, n: usize, max: u32) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{:<id_width$}  ", row.plan.as_str()),
        palette::clickable_text(),
    )];
    let shown = &row.buckets[row.buckets.len().saturating_sub(n)..];
    if shown.len() < n {
        spans.push(Span::raw(" ".repeat(n - shown.len())));
    }
    spans.extend(shown.iter().map(|b| glyph(b, max)));
    spans.push(Span::raw("  "));
    spans.push(age_cell(row));
    Line::from(spans)
}

/// The row under the cell (`x`, `y`) of the screen when `area` shows the rows from `offset`.
pub fn row_at(area: Rect, offset: usize, rows: usize, x: u16, y: u16) -> Option<usize> {
    if !area.contains((x, y).into()) {
        return None;
    }
    let row = offset + usize::from(y - area.y);
    (row < rows).then_some(row)
}

/// Draws the rows of `pulse` from `scroll`'s offset in `area` (inside the margin), clamping `scroll` to the rows shown.
pub fn draw(buf: &mut Buffer, area: Rect, pulse: &Pulse, scroll: &mut Scroll) {
    scroll.resize(pulse.rows.len(), usize::from(area.height));
    let offset = scroll.offset();
    let (id_width, n) = (
        id_width(pulse),
        buckets(usize::from(area.width), id_width(pulse)),
    );
    for (i, row) in pulse
        .rows
        .iter()
        .skip(offset)
        .take(usize::from(area.height))
        .enumerate()
    {
        let line = Rect::new(area.x, area.y + i as u16, area.width, 1);
        row_line(row, id_width, n, pulse.max).render(line, buf);
    }
}

/// What the legend shows for `quiet`: the label of a quiet Plan's age cell.
pub fn quiet_sample() -> Span<'static> {
    Span::styled("quiet 41m", palette::muted_text())
}

/// The legend's entries: the sample and its meaning. The kinds are in the order of precedence.
pub fn legend() -> Vec<(Span<'static>, String)> {
    let sample = |kind| bar(4, kind);
    vec![
        (
            Span::raw("▂▄▆"),
            "activity per 5 min, first colour wins".to_owned(),
        ),
        (sample(Kind::Error), "an error, or a Job failed".to_owned()),
        (sample(Kind::Plan), "a Plan or ask Event".to_owned()),
        (sample(Kind::PrOpened), "a pull request opened".to_owned()),
        (sample(Kind::Escalation), "an escalation".to_owned()),
        (
            sample(Kind::Merged),
            "a Job or a Workflow merged".to_owned(),
        ),
        (sample(Kind::Other), "any other activity".to_owned()),
        (glyph(&Bucket::default(), 8), "no activity".to_owned()),
        (
            quiet_sample(),
            format!("queued or running work with no activity for {QUIET_MINUTES} min"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read::Dot;
    use cpd_core::ids::{PlanId, SessionId};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use ratatui::style::Color;

    fn b(count: u32) -> Bucket {
        of(count, Kind::Other)
    }

    fn of(count: u32, kind: Kind) -> Bucket {
        Bucket {
            count,
            kind: Some(kind),
            job_merged: false,
            pr_merged: false,
            worker_ran: false,
        }
    }

    fn row(plan: &str, buckets: Vec<Bucket>) -> PulseRow {
        PulseRow {
            plan: PlanId::parse(plan).unwrap(),
            lead: SessionId::parse(&format!("l-{plan}")).unwrap(),
            dot: Dot::Idle,
            buckets,
            merges: 0,
            age_minutes: None,
            quiet: false,
        }
    }

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_row_reads_id_sparkline_and_age_with_no_icon() {
        let mut r = row("api", vec![Bucket::default(), b(1), b(4), b(8)]);
        r.merges = 17;
        r.age_minutes = Some(4);
        let line = row_line(&r, 6, 4, 8);
        assert_eq!(text(&line), "api     ▁▁▄█  ".to_owned() + "       4m");
        assert!(
            !text(&line).contains('󰗠') && !text(&line).contains("17"),
            "no 󰗠 and no merge count"
        );
        assert!(!text(&line).contains(palette::NEEDS_YOU), "no dot");
    }

    #[test]
    fn a_rows_cells_carry_the_kind_colours_and_the_grey_zero() {
        let kinds = [
            Kind::Error,
            Kind::Plan,
            Kind::PrOpened,
            Kind::Escalation,
            Kind::Merged,
            Kind::Other,
        ];
        let mut buckets: Vec<Bucket> = kinds.iter().map(|k| of(8, *k)).collect();
        buckets.insert(3, Bucket::default());
        let spans = row_line(&row("api", buckets), 3, 7, 8).spans;
        let styles: Vec<(String, Option<Color>)> = spans[1..8]
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();
        let grey = palette::fg(Meaning::Nothing).fg;
        let expect = |g: &str, k| (g.to_owned(), Some(palette::pulse_kind(k)));
        assert_eq!(
            styles,
            [
                expect("█", Kind::Error),
                expect("█", Kind::Plan),
                expect("█", Kind::PrOpened),
                ("▁".to_owned(), grey),
                expect("█", Kind::Escalation),
                expect("█", Kind::Merged),
                expect("█", Kind::Other),
            ]
        );
        assert_eq!(spans[0].content, "api  ");
        assert_eq!(
            (spans[8].content.as_ref(), spans[9].content.as_ref()),
            ("  ", "         "),
            "two spaces, then the age cell"
        );
    }

    #[test]
    fn bars_are_on_the_shared_scale_and_at_least_one_eighth() {
        let level = |count, max| glyph(&b(count), max).content.to_string();
        assert_eq!(level(8, 8), "█");
        assert_eq!(level(1, 8), "▁");
        assert_eq!(level(1, 100), "▁");
        assert_eq!(level(4, 8), "▄");
        assert_eq!(level(5, 8), "▅");
        assert_eq!(level(3, 10), "▃", "ceil(24 / 10) = 3");
    }

    #[test]
    fn a_bar_is_the_colour_of_its_kind_and_an_empty_slot_a_grey_low_bar() {
        for kind in [
            Kind::Error,
            Kind::Plan,
            Kind::PrOpened,
            Kind::Escalation,
            Kind::Merged,
            Kind::Other,
        ] {
            let g = glyph(&of(3, kind), 8);
            assert_eq!(
                (g.content.as_ref(), g.style),
                ("▃", Style::new().fg(palette::pulse_kind(kind))),
                "{kind:?}"
            );
        }
        let g = glyph(&Bucket::default(), 8);
        assert_eq!(
            (g.content.as_ref(), g.style),
            ("▁", palette::fg(Meaning::Nothing))
        );
        // the Job and Workflow merged flags and a running Worker draw nothing of their own
        let g = glyph(
            &Bucket {
                count: 0,
                kind: None,
                job_merged: true,
                pr_merged: true,
                worker_ran: true,
            },
            8,
        );
        assert_eq!(
            (g.content.as_ref(), g.style),
            ("▁", palette::fg(Meaning::Nothing))
        );
    }

    #[test]
    fn an_age_cell_is_blank_without_activity_and_otherwise_muted() {
        let mut r = row("api", vec![]);
        assert_eq!(
            text(&Line::from(age_cell(&r))),
            "         ",
            "no activity ever: blank"
        );
        r.age_minutes = Some(0);
        let cell = age_cell(&r);
        assert_eq!(
            (cell.content.as_ref(), cell.style),
            ("       0s", palette::muted_text())
        );
    }

    #[test]
    fn a_quiet_row_reads_quiet_and_its_age_muted() {
        let mut r = row("api", vec![]);
        r.quiet = true;
        r.age_minutes = Some(41);
        let cell = age_cell(&r);
        assert_eq!(
            (cell.content.as_ref(), cell.style),
            ("quiet 41m", palette::muted_text())
        );
    }

    #[test]
    fn the_buckets_fill_125_columns_per_the_formula() {
        // 125 − 2·2 margin − (id + 2 + 2 + 9): the dot and the 󰗠N cell's 8 columns went to the sparkline
        assert_eq!(buckets(121, 13), 95);
        assert_eq!(buckets(121, 0), 108);
        assert_eq!(buckets(30, 13), 12, "at least twelve, cut at the edge");
        let pulse = Pulse {
            context: cpd_core::ids::ContextId::parse("ctx").unwrap(),
            rows: vec![row("cross-repo-plan", vec![b(1); 120])],
            max: 1,
            events_cursor: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(125, 4)).unwrap();
        terminal
            .draw(|f| {
                draw(
                    f.buffer_mut(),
                    Rect::new(2, 1, 121, 2),
                    &pulse,
                    &mut Scroll::default(),
                );
            })
            .unwrap();
        let line: String = (0..125)
            .map(|x| terminal.backend().buffer()[(x, 1)].symbol().to_owned())
            .collect();
        let n = buckets(121, 15);
        assert_eq!(line.chars().filter(|c| *c == '█').count(), n);
        assert_eq!(
            4 + (15 + 2 + 2 + 9) + n,
            125,
            "the cells fill the pane to its margin"
        );
        assert!(line.ends_with("  "), "{line:?}");
        assert!(
            line.starts_with("  cross-repo-plan  █"),
            "{line:?}: the id first, no dot"
        );
    }

    /// The offset after one wheel notch of `total` rows seen through 5, starting at `offset`.
    fn wheel(total: usize, offset: usize, kind: MouseEventKind) -> usize {
        let mut scroll = Scroll::default();
        scroll.resize(total, 5);
        scroll.by(isize::try_from(offset).unwrap_or(isize::MAX));
        assert!(scroll.mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE
        }));
        scroll.offset()
    }

    #[test]
    fn the_wheel_clamps_and_a_click_maps_to_the_row_under_it() {
        let area = Rect::new(2, 1, 40, 5);
        assert_eq!(wheel(8, 0, MouseEventKind::ScrollDown), 3);
        assert_eq!(
            wheel(8, 3, MouseEventKind::ScrollDown),
            3,
            "no offset past the last row"
        );
        assert_eq!(wheel(8, 1, MouseEventKind::ScrollUp), 0);
        assert_eq!(wheel(3, 0, MouseEventKind::ScrollDown), 0, "all rows fit");
        assert_eq!(row_at(area, 0, 8, 5, 1), Some(0));
        assert_eq!(row_at(area, 3, 8, 5, 3), Some(5));
        assert_eq!(row_at(area, 3, 8, 5, 5), Some(7));
        assert_eq!(row_at(area, 0, 2, 5, 4), None, "below the last row");
        assert_eq!(row_at(area, 0, 8, 1, 1), None, "in the margin");
        assert_eq!(row_at(area, 0, 8, 5, 0), None, "above the rows");
    }

    #[test]
    fn the_legend_has_an_entry_per_kind_in_precedence_order_then_grey_zero_and_quiet() {
        let legend = legend();
        let shown: Vec<(&str, Option<Color>)> = legend
            .iter()
            .map(|(s, _)| (s.content.as_ref(), s.style.fg))
            .collect();
        assert_eq!(
            shown,
            [
                ("▂▄▆", None),
                ("▄", Some(palette::pulse_kind(Kind::Error))),
                ("▄", Some(palette::pulse_kind(Kind::Plan))),
                ("▄", Some(palette::pulse_kind(Kind::PrOpened))),
                ("▄", Some(palette::pulse_kind(Kind::Escalation))),
                ("▄", Some(palette::pulse_kind(Kind::Merged))),
                ("▄", Some(palette::pulse_kind(Kind::Other))),
                ("▁", palette::fg(Meaning::Nothing).fg),
                ("quiet 41m", palette::muted_text().fg)
            ]
        );
    }
}
