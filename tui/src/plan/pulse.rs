//! Pulse in the Plan view (commands.md §13.2): one row per Plan of the Manager's Context, each with a sparkline of the
//! Plan's activity, drawn from the read model of [`crate::read::pulse`]. Everything here is pure: the rows' text and
//! styles, the bucket count, the wheel offset and the row under a click.
//!
//! A row, inside the view's margin: `<dot> <plan id padded>  <sparkline>  <󰗠N in 4>  <age in 9>`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::palette::{self, Meaning};
use crate::read::{Bucket, JobStatus, Pulse, PulseRow, QUIET_MINUTES};

/// The fewest buckets a row draws, whatever the width.
const MIN_BUCKETS: usize = 12;
/// Columns of the `󰗠N` cell and of the age cell.
const MERGES_WIDTH: usize = 4;
const AGE_WIDTH: usize = 9;
/// Rows a wheel notch scrolls.
pub const NOTCH: usize = 3;
/// The bars, level 1 to 8.
const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// The merged glyph: the palette's, for a merged Job, so Pulse follows it.
pub fn merged_icon() -> &'static str {
    palette::job_status(JobStatus::Merged, false).0
}

/// The buckets a row draws in `width` columns (inside the margin) for ids `id_width` wide: what the other cells leave,
/// at least [`MIN_BUCKETS`].
pub fn buckets(width: usize, id_width: usize) -> usize {
    // dot, space, id, two spaces, [sparkline], two spaces, 󰗠N, two spaces, age
    width.saturating_sub(1 + 1 + id_width + 2 + 2 + MERGES_WIDTH + 2 + AGE_WIDTH).max(MIN_BUCKETS)
}

/// The width of the longest Plan id.
fn id_width(pulse: &Pulse) -> usize {
    pulse.rows.iter().map(|r| r.plan.as_str().chars().count()).max().unwrap_or(0)
}

/// The glyph of a bucket on the scale `max` (the largest count over all rows).
pub fn glyph(bucket: &Bucket, max: u32) -> Span<'static> {
    if bucket.pr_merged {
        Span::styled(merged_icon(), palette::fg(Meaning::Done))
    } else if bucket.count > 0 {
        let level = (u64::from(bucket.count) * 8).div_ceil(u64::from(max.max(bucket.count))).clamp(1, 8) as usize;
        bar(level, bucket.job_merged)
    } else if bucket.worker_ran {
        Span::styled("▁", palette::fg(Meaning::Nothing))
    } else {
        Span::raw(" ")
    }
}

/// A bar of `level` (1..=8), green when a Job merged in it.
pub fn bar(level: usize, merged: bool) -> Span<'static> {
    let style = if merged { palette::fg(Meaning::Done) } else { Style::new() };
    Span::styled(BARS[level.clamp(1, 8) - 1].to_string(), style)
}

/// `Nm` under 60 minutes, `Nh` under 48 hours, else `Nd`.
pub fn age_text(minutes: i64) -> String {
    let minutes = minutes.max(0);
    if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 48 * 60 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / (24 * 60))
    }
}

/// The age cell, [`AGE_WIDTH`] wide, right-aligned: dim, or `quiet <age>` in the Review colour.
fn age_cell(row: &PulseRow) -> Span<'static> {
    let age = row.age_minutes.map(age_text);
    if row.quiet {
        let text = age.map_or("quiet".to_owned(), |a| format!("quiet {a}"));
        return Span::styled(format!("{text:>AGE_WIDTH$}"), palette::fg(Meaning::Review));
    }
    Span::styled(format!("{:>AGE_WIDTH$}", age.unwrap_or_default()), Style::new().add_modifier(Modifier::DIM))
}

/// One row's line with `n` buckets: the last `n` of the row's, blank on the left when it has fewer.
pub fn row_line(row: &PulseRow, id_width: usize, n: usize, max: u32, frame: usize) -> Line<'static> {
    let mut spans = vec![
        palette::dot_span(row.dot, frame, false),
        Span::raw(format!(" {:<id_width$}  ", row.plan.as_str())),
    ];
    let shown = &row.buckets[row.buckets.len().saturating_sub(n)..];
    if shown.len() < n {
        spans.push(Span::raw(" ".repeat(n - shown.len())));
    }
    spans.extend(shown.iter().map(|b| glyph(b, max)));
    spans.push(Span::raw("  "));
    spans.push(if row.merges > 0 {
        Span::styled(format!("{:>MERGES_WIDTH$}", format!("{}{}", merged_icon(), row.merges)), palette::fg(Meaning::Done))
    } else {
        Span::raw(" ".repeat(MERGES_WIDTH))
    });
    spans.push(Span::raw("  "));
    spans.push(age_cell(row));
    Line::from(spans)
}

/// The largest offset: the last row at the foot of `height` rows.
fn max_offset(rows: usize, height: usize) -> usize {
    rows.saturating_sub(height)
}

/// `offset` clamped so no row scrolls past the last.
pub fn clamp(offset: usize, rows: usize, height: usize) -> usize {
    offset.min(max_offset(rows, height))
}

/// The offset after one wheel notch, `down` or up, clamped.
pub fn scroll(offset: usize, down: bool, rows: usize, height: usize) -> usize {
    clamp(if down { offset.saturating_add(NOTCH) } else { offset.saturating_sub(NOTCH) }, rows, height)
}

/// The row under the cell (`x`, `y`) of the screen when `area` shows the rows from `offset`.
pub fn row_at(area: Rect, offset: usize, rows: usize, x: u16, y: u16) -> Option<usize> {
    if !area.contains((x, y).into()) {
        return None;
    }
    let row = offset + usize::from(y - area.y);
    (row < rows).then_some(row)
}

/// Draws the rows of `pulse` from `offset` in `area` (inside the margin); returns the offset, clamped.
pub fn draw(buf: &mut Buffer, area: Rect, pulse: &Pulse, offset: usize, frame: usize) -> usize {
    let offset = clamp(offset, pulse.rows.len(), usize::from(area.height));
    let (id_width, n) = (id_width(pulse), buckets(usize::from(area.width), id_width(pulse)));
    for (i, row) in pulse.rows.iter().skip(offset).take(usize::from(area.height)).enumerate() {
        let line = Rect::new(area.x, area.y + i as u16, area.width, 1);
        row_line(row, id_width, n, pulse.max, frame).render(line, buf);
    }
    offset
}

/// What the legend shows for `quiet`: the label of a quiet Plan's age cell.
pub fn quiet_sample() -> Span<'static> {
    Span::styled("quiet 41m", palette::fg(Meaning::Review))
}

/// The legend's five entries: the sample and its meaning.
pub fn legend() -> Vec<(Span<'static>, String)> {
    let bucket = |count, job_merged, pr_merged, worker_ran| Bucket { count, job_merged, pr_merged, worker_ran };
    vec![
        (glyph(&bucket(4, false, false, false), 8), "activity per 5 minutes".to_owned()),
        (glyph(&bucket(4, true, false, false), 8), "a Job merged".to_owned()),
        (glyph(&bucket(1, false, true, false), 8), "a PR merged".to_owned()),
        (glyph(&bucket(0, false, false, true), 8), "a Worker running".to_owned()),
        (quiet_sample(), format!("queued or running work with no activity for {QUIET_MINUTES} min")),
    ]
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    use super::*;
    use crate::read::Dot;
    use cpd_core::ids::{PlanId, SessionId};

    fn b(count: u32) -> Bucket {
        Bucket { count, job_merged: false, pr_merged: false, worker_ran: false }
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
    fn a_row_reads_dot_id_sparkline_merges_and_age() {
        let mut r = row("api", vec![b(0), b(1), b(4), b(8)]);
        r.merges = 17;
        r.age_minutes = Some(4);
        let line = row_line(&r, 6, 4, 8, 0);
        let dot = palette::dot_span(Dot::Idle, 0, false).content;
        let expected = format!("{dot} api     {}", [" ▁▄█", "  ", " ", merged_icon(), "17", "  ", "       4m"].concat());
        assert_eq!(text(&line), expected);
        assert_eq!(line.spans[0], palette::dot_span(Dot::Idle, 0, false));
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
    fn glyph_colours_and_blanks() {
        let g = glyph(&Bucket { count: 2, job_merged: false, pr_merged: true, worker_ran: true }, 8);
        assert_eq!((g.content.as_ref(), g.style), (merged_icon(), palette::fg(Meaning::Done)));
        let g = glyph(&Bucket { count: 2, job_merged: true, pr_merged: false, worker_ran: false }, 8);
        assert_eq!(g.style, palette::fg(Meaning::Done));
        assert_eq!(glyph(&b(2), 8).style, Style::new(), "the default foreground");
        let g = glyph(&Bucket { count: 0, job_merged: false, pr_merged: false, worker_ran: true }, 8);
        assert_eq!((g.content.as_ref(), g.style), ("▁", palette::fg(Meaning::Nothing)));
        assert_eq!(glyph(&b(0), 8).content, " ");
    }

    #[test]
    fn merges_are_green_and_blank_at_zero() {
        let mut r = row("api", vec![b(0)]);
        r.merges = 6;
        let spans = row_line(&r, 3, 1, 1, 0).spans;
        let cell = &spans[spans.len() - 3];
        assert_eq!((cell.content.as_ref(), cell.style), (format!("  {}6", merged_icon()).as_str(), palette::fg(Meaning::Done)));
        r.merges = 0;
        let spans = row_line(&r, 3, 1, 1, 0).spans;
        assert_eq!(spans[spans.len() - 3].content, "    ");
    }

    #[test]
    fn ages_are_minutes_hours_or_days() {
        for (m, t) in [(0, "0m"), (59, "59m"), (60, "1h"), (47 * 60 + 59, "47h"), (48 * 60, "2d"), (5 * 24 * 60, "5d")] {
            assert_eq!(age_text(m), t, "{m}");
        }
        let mut r = row("api", vec![]);
        assert_eq!(text(&Line::from(age_cell(&r))), "         ", "no activity ever: blank");
        r.age_minutes = Some(0);
        let cell = age_cell(&r);
        assert_eq!((cell.content.as_ref(), cell.style.add_modifier), ("       0m", Modifier::DIM));
    }

    #[test]
    fn a_quiet_row_reads_quiet_and_its_age_in_the_review_colour() {
        let mut r = row("api", vec![]);
        r.quiet = true;
        r.age_minutes = Some(41);
        let cell = age_cell(&r);
        assert_eq!((cell.content.as_ref(), cell.style), ("quiet 41m", palette::fg(Meaning::Review)));
    }

    #[test]
    fn the_dot_is_the_palettes_for_the_rows_dot() {
        for dot in [Dot::Stopped, Dot::Idle, Dot::Working, Dot::Red, Dot::White] {
            let mut r = row("api", vec![]);
            r.dot = dot;
            assert_eq!(row_line(&r, 3, 0, 1, 2).spans[0], palette::dot_span(dot, 2, false));
        }
    }

    #[test]
    fn the_buckets_fill_125_columns_per_the_formula() {
        // 125 − 2·2 margin − (1 + 1 + 13 + 2 + 2 + 4 + 2 + 9)
        assert_eq!(buckets(121, 13), 87);
        assert_eq!(buckets(121, 0), 100);
        assert_eq!(buckets(30, 13), 12, "at least twelve, cut at the edge");
        let pulse = Pulse {
            context: cpd_core::ids::ContextId::parse("ctx").unwrap(),
            rows: vec![row("cross-repo-plan", vec![b(1); 120])],
            max: 1,
            events_cursor: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(125, 4)).unwrap();
        terminal.draw(|f| { draw(f.buffer_mut(), Rect::new(2, 1, 121, 2), &pulse, 0, 0); }).unwrap();
        let line: String = (0..125).map(|x| terminal.backend().buffer()[(x, 1)].symbol().to_owned()).collect();
        let n = buckets(121, 15);
        assert_eq!(line.chars().filter(|c| *c == '█').count(), n);
        assert_eq!(4 + (1 + 1 + 15 + 2 + 2 + 4 + 2 + 9) + n, 125, "the cells fill the pane to its margin");
        assert!(line.ends_with("  "), "{line:?}");
    }

    #[test]
    fn the_wheel_clamps_and_a_click_maps_to_the_row_under_it() {
        let area = Rect::new(2, 1, 40, 5);
        assert_eq!(scroll(0, true, 8, 5), 3);
        assert_eq!(scroll(3, true, 8, 5), 3, "no offset past the last row");
        assert_eq!(scroll(1, false, 8, 5), 0);
        assert_eq!(scroll(0, true, 3, 5), 0, "all rows fit");
        assert_eq!(row_at(area, 0, 8, 5, 1), Some(0));
        assert_eq!(row_at(area, 3, 8, 5, 3), Some(5));
        assert_eq!(row_at(area, 3, 8, 5, 5), Some(7));
        assert_eq!(row_at(area, 0, 2, 5, 4), None, "below the last row");
        assert_eq!(row_at(area, 0, 8, 1, 1), None, "in the margin");
        assert_eq!(row_at(area, 0, 8, 5, 0), None, "above the rows");
    }

    #[test]
    fn the_legend_has_the_five_entries_in_their_colours() {
        let legend = legend();
        let shown: Vec<(&str, Option<Color>)> = legend.iter().map(|(s, _)| (s.content.as_ref(), s.style.fg)).collect();
        let done = palette::fg(Meaning::Done).fg;
        assert_eq!(
            shown,
            [
                ("▄", None),
                ("▄", done),
                (merged_icon(), done),
                ("▁", palette::fg(Meaning::Nothing).fg),
                ("quiet 41m", palette::fg(Meaning::Review).fg)
            ]
        );
    }
}
