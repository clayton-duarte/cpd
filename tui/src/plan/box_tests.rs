//! The Plan view's Workflow boxes are the box widget's (hs-d578, hs-d596): the chart pads nothing itself, `boxes.rs` draws no
//! border of its own, and a title follows the corner and one dash.

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use super::flow::{self, FlowJob, Live};
use super::*;
use crate::box_widget::spacing;
use crate::test_text::rows;

fn row(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn the_chart_has_no_padding_of_its_own() {
    let job = FlowJob {
        key: "a".into(),
        status: Live::Queued,
        tier: Some(cpd_core::config::Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        attempt: 1,
    };
    let chart = flow::layout(&[job], 0);
    let pill = chart.pills.first().expect("one Job, one pill").rect;
    let widest = chart.pills.iter().map(|p| p.rect.right()).max().unwrap();
    assert_eq!(pill.x, 0, "the first pill starts at the chart's left edge");
    assert_eq!(
        chart.buf.area.width, widest,
        "the buffer is as wide as its widest pill"
    );
}

#[test]
fn the_pr_boxes_are_drawn_by_the_box_widget() {
    let source = include_str!("boxes.rs");
    let product: String = source
        .lines()
        .take_while(|line| !line.starts_with("#[cfg(test)]"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        product.contains("BoxWidget"),
        "boxes.rs draws the Workflow boxes with BoxWidget"
    );
    for hand_built in [
        "Block::bordered",
        "BorderType::Rounded",
        "rect.inner(Margin",
    ] {
        assert!(
            !product.contains(hand_built),
            "boxes.rs still hand-builds a box: {hand_built}"
        );
    }
}

#[test]
fn a_pr_box_title_follows_the_corner_and_one_dash() {
    let centre = sample_centre(vec![sample_workflow("api")], Vec::new());
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).unwrap();
    let mut ui = Ui::default();
    terminal.draw(|f| ui.draw(f, Some(&centre))).unwrap();
    let buf = terminal.backend().buffer();
    // row 1 is the box's top: the margin column, then `╭`, the dash and the title
    assert_eq!(
        row(buf, 1).trim_end(),
        " ╭─ api · api-repo 󱦰 main · dispatch ─╮"
    );
    // the wall is one margin column in, the padding is the next column, and the body starts after both
    let wall = spacing::MARGIN_X;
    let body = wall + 1 + spacing::PAD_X;
    let cells = |y: u16| {
        buf.content[usize::from(y) * 40..usize::from(y + 1) * 40]
            .iter()
            .map(|c| c.symbol().to_owned())
            .collect::<Vec<_>>()
    };
    let under = cells(2);
    assert_eq!(under[usize::from(wall)], "│");
    assert_eq!(
        under[usize::from(wall + 1)..usize::from(body)]
            .concat()
            .trim(),
        "",
        "the padding column is blank"
    );
    let text: String = under[usize::from(body)..].concat();
    assert!(
        text.starts_with("no Jobs"),
        "`no Jobs` starts at column {body}: {text:?}"
    );
}

fn text(c: &flow::Chart) -> String {
    rows(&c.buf).join("\n")
}

#[test]
fn a_need_connector_reaches_the_top_border_with_no_arrowhead() {
    use cpd_core::ids::{JobKey, WorkflowKey};

    let mut web = sample_workflow("web");
    web.needs = vec![WorkflowKey::parse("api").unwrap()];
    let workflows = [sample_workflow("api"), web];
    let mut b = sample_job("api", "b");
    b.needs = vec![JobKey::parse("a").unwrap()];
    let jobs = [sample_job("api", "a"), b, sample_job("web", "c")];
    let c = boxes::layout(&workflows, &jobs, &read::NeedsUser::default(), 0).chart;

    let want = "\
╭─ api · api-repo 󱦰 main · dispatch ─╮
│ 󰝦 a│S1─────󰝦 b│S1              │
╰─────────────────┬──────────────────╯
                  ╰─────────────────╮
                                    │
╭─ web · web-repo 󱦰 main · dispatch ┴╮
│ 󰝦 c│S1                           │
╰────────────────────────────────────╯";
    let got = text(&c);
    assert_eq!(got, want, "\n{got}");
    assert!(
        !got.contains('\u{F19B3}'),
        "the 󱦳 arrowhead is gone: {got:?}"
    );
    let rows: Vec<&str> = got.lines().collect();
    assert_eq!(
        rows[4],
        format!("{}│", " ".repeat(36)),
        "the routed line reaches the cell above web's top border: {got:?}"
    );
    assert!(
        rows[0].contains('\u{F19B0}'),
        "the TO title arrow is still in api's top row"
    );
    assert!(
        rows[5].contains('\u{F19B0}'),
        "the TO title arrow is still in web's top row"
    );
}
