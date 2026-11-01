//! The layout rows of the section titles (the legend's, the logs popup's) and of the rows under
//! the logs popup's and the legend's titles, which have no box, only the container's margin.

use crate::layout_tests::{Sample, SectionCase, check};

const WIDTHS: [u16; 4] = [40, 20, 12, 6];
const HEIGHT: u16 = 12;

fn rows(subject: &str, case: SectionCase, sample: fn(u16, u16, SectionCase) -> Sample) {
    for width in WIDTHS {
        let drawn = sample(width, HEIGHT, case);
        check(
            &format!("{subject}, {case:?}, {width}x{HEIGHT}"),
            width,
            &drawn,
        );
    }
}

#[test]
fn layout_section_titles() {
    rows(
        "legend title",
        SectionCase::Title,
        crate::widgets::legend::layout_sample,
    );
    rows("logs title", SectionCase::Title, crate::logs::layout_sample);
}

#[test]
fn layout_logs_rows() {
    rows("logs events", SectionCase::Rows, crate::logs::layout_sample);
}

#[test]
fn layout_legend_rows() {
    rows(
        "legend body",
        SectionCase::Rows,
        crate::widgets::legend::layout_sample,
    );
}
