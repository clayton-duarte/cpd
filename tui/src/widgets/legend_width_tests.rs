//! Red tests for audit-plan F1 in the legend: its fixed texts are measured in display columns, not `char`s.

use ratatui::text::Line;

use crate::cells;

use super::Section;

#[test]
fn the_legend_measures_its_texts_in_columns() {
    // Five `計` (U+8A08): five `char`s, ten display columns. The header is fixed text; the section measures it in
    // columns, so the first column is ten wide, not five.
    let wide = "計計計計計";
    let section = Section {
        title: "Jobs",
        header: Some([wide, "status", "counter"]),
        rows: vec![vec![Line::from("x"), Line::from("y"), Line::from("z")]],
    };
    assert_eq!(
        section.column_widths(),
        vec![cells::width(wide) as u16, 6, 7],
        "the header is measured in columns"
    );
    // The title is measured the same way: the widest thing of a title-only section is its own display width.
    let titled = Section {
        title: wide,
        header: None,
        rows: Vec::new(),
    };
    assert_eq!(
        titled.width(),
        cells::width(wide) as u16,
        "the title is measured in columns"
    );
}
