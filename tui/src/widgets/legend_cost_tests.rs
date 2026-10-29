//! The legend's `cost` row (pro-d179, commands.md legend paragraph, F6): the last row of the section
//! `Jobs: Plan Pills And Context Counters`, after the `counters row` row.

use ratatui::style::Style;
use ratatui::text::Line;

use super::legend::sections;
use crate::palette;

const TITLE: &str = "Jobs: Plan Pills And Context Counters";

fn text(line: &Line) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn cost_row() -> Vec<Line<'static>> {
    let all = sections();
    let jobs = all.iter().find(|s| s.title == TITLE).expect("the Jobs section");
    let names: Vec<String> = jobs.rows.iter().map(|r| text(&r[0])).collect();
    let counters = names.iter().position(|n| n == "counters row").expect("the counters row");
    assert_eq!(names.last().map(String::as_str), Some("cost"), "the cost row is the last: {names:?}");
    assert_eq!(counters + 1, names.len() - 1, "one row after the counters row: {names:?}");
    jobs.rows[counters + 1].clone()
}

fn colour_of(line: &Line) -> Vec<Option<ratatui::style::Color>> {
    line.spans.iter().map(|s| Style::from(s.style).fg.or(line.style.fg)).collect()
}

#[test]
fn the_legend_cost_row_describes_the_new_format() {
    let row = cost_row();
    let cells: Vec<String> = row.iter().map(text).collect();
    assert_eq!(cells, ["cost", "$0.120", "$0.012 ⋅ $1.23k ⋅ omitted when unknown"]);
}

#[test]
fn the_cost_sample_is_the_formatters_output_in_colour_8() {
    let row = cost_row();
    assert_eq!(text(&row[1]), "$0.120");
    let muted = palette::muted_text().fg;
    assert!(colour_of(&row[1]).iter().all(|c| *c == muted), "the sample is colour 8: {:?}", colour_of(&row[1]));
    assert!(colour_of(&row[2]).iter().all(|c| *c == muted), "the note is colour 8: {:?}", colour_of(&row[2]));
}
