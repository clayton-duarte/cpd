//! Red tests of the Workflow cost on the bottom border of a PR box (Context record pro-d179 item 3): the text
//! ` <duration> · <cost> `, its width, style and exit `┬`, beside the active-time counter (`counter_tests`).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::lines_tests::violations;
use super::{boxes, sample_pr};
use crate::read::{NeedsUser, PrNode};
use crate::test_text::rows;
use cpd_core::ids::PrKey;

fn chars(s: &str) -> Vec<String> {
    s.chars().map(String::from).collect()
}

fn canvas(prs: &[PrNode]) -> Buffer {
    boxes::layout(prs, &[], &NeedsUser::default(), 0).chart.buf
}

/// The first row of `buf` that starts with `╰`, as cells (trailing blanks trimmed).
fn bottom(buf: &Buffer) -> (usize, Vec<String>) {
    let r = rows(buf);
    let y = r
        .iter()
        .position(|l| l.starts_with('╰'))
        .expect("a bottom border");
    (y, chars(r[y].trim_end()))
}

fn with(key: &str, secs: Option<u64>, cost: Option<f64>) -> PrNode {
    PrNode {
        elapsed_seconds: secs,
        cost_usd: cost,
        ..sample_pr(key)
    }
}

/// Box `a` of repo `r` and base `m`: 26 cells wide with no Jobs.
fn small(secs: Option<u64>, cost: Option<f64>) -> PrNode {
    let mut p = with("a", secs, cost);
    p.repo_name = "r".into();
    p.base = "m".into();
    p
}

#[test]
fn a_box_with_a_time_and_a_cost_reads_both_on_its_bottom_border() {
    let buf = canvas(&[small(Some(3723), Some(0.449))]);
    let (_, row) = bottom(&buf);
    let line = row.concat();
    assert_eq!(line, "╰─ 1h·2m·3s · $0.449 ────╯");
    assert_eq!(row.len(), buf.area.width as usize, "{line}");
    assert_eq!(row[2], " ", "the text starts at column 2: {line}");
    assert_eq!(row[3], "1", "{line}");
}

#[test]
fn the_cost_is_formatted_as_everywhere_else() {
    for (cost, want) in [
        (0.004, "$0.004"),
        (0.0, "$0.000"),
        (0.449, "$0.449"),
        (4.5, "$4.500"),
        (1234.0, "$1.23k"),
    ] {
        let (_, row) = bottom(&canvas(&[small(Some(5), Some(cost))]));
        let line = row.concat();
        assert!(
            line.starts_with(&format!("╰─ 5s · {want} ─")),
            "{cost}: {line}"
        );
    }
}

#[test]
fn only_a_workflow_with_cost_data_shows_a_cost_after_the_counter() {
    let buf = canvas(&[
        with("a", Some(60), None),
        with("b", Some(60), Some(0.0)),
        with("c", None, None),
    ]);
    let (_, row) = bottom(&buf);
    let mut boxes_: Vec<String> = Vec::new();
    let mut open = None;
    for (x, c) in row.iter().enumerate() {
        match c.as_str() {
            "╰" => open = Some(x),
            "╯" => boxes_.push(row[open.take().unwrap()..=x].concat()),
            _ => {}
        }
    }
    assert_eq!(boxes_.len(), 3, "{}", row.concat());
    assert!(boxes_[0].starts_with("╰─ 1m·0s ─"), "{}", boxes_[0]);
    assert!(
        !boxes_[0].contains(" · ") && !boxes_[0].contains('$'),
        "{}",
        boxes_[0]
    );
    assert!(
        boxes_[1].starts_with("╰─ 1m·0s · $0.000 ─"),
        "{}",
        boxes_[1]
    );
    let inner: Vec<String> = chars(&boxes_[2]);
    assert!(
        inner[1..inner.len() - 1].iter().all(|c| c == "─"),
        "{}",
        boxes_[2]
    );
}

#[test]
fn the_time_and_the_cost_are_drawn_in_the_muted_text_colour() {
    let buf = canvas(&[small(Some(3723), Some(0.449))]);
    let (y, row) = bottom(&buf);
    assert!(
        row.concat().starts_with("╰─ 1h·2m·3s · $0.449 ─"),
        "no cost drawn: {}",
        row.concat()
    );
    let want = crate::palette::muted_text();
    for x in 2..20u16 {
        let c = &buf[(x, y as u16)];
        assert_eq!(
            (c.fg, c.bg, c.modifier),
            (want.fg.unwrap(), want.bg.unwrap_or(c.bg), c.modifier),
            "column {x}"
        );
        assert_eq!(c.style().add_modifier, want.add_modifier, "column {x}");
    }
    let last = buf.area.width - 1;
    let corner = |x: u16| {
        let c = &buf[(x, y as u16)];
        (c.fg, c.bg, c.modifier)
    };
    let top = &buf[(0, 0)];
    assert_eq!(corner(0), (top.fg, top.bg, top.modifier));
    assert_eq!(corner(last), (top.fg, top.bg, top.modifier));
}

#[test]
fn a_box_is_widened_to_hold_the_time_and_the_cost_plus_four() {
    let (_, row) = bottom(&canvas(&[small(Some(u64::MAX), Some(1234.0))]));
    assert_eq!(row.concat(), "╰─ 5124095576030431h·0m·15s · $1.23k ─╯");
    assert_eq!(row.len(), 39);
}

/// The `┬` columns of box `a`'s bottom border, that another PR `b` needs; and the canvas, the row, the box's width.
fn exit(secs: Option<u64>, cost: Option<f64>) -> (Buffer, usize, usize, Vec<usize>) {
    let web = PrNode {
        needs: vec![PrKey::parse("a").unwrap()],
        ..sample_pr("b")
    };
    let buf = canvas(&[small(secs, cost), web]);
    let r = rows(&buf);
    let y = r.iter().position(|l| l.starts_with('╰')).unwrap();
    let row = chars(&r[y]);
    let width = row.iter().position(|c| c == "╯").unwrap() + 1;
    let tees = (0..width).filter(|&x| row[x] == "┬").collect();
    (buf, y, width, tees)
}

#[test]
fn the_cost_moves_the_exit_when_it_covers_the_middle_column() {
    let (buf, y, width, tees) = exit(Some(5), None);
    assert_eq!((width, tees), (26, vec![12]));
    assert!(rows(&buf)[y].starts_with("╰─ 5s ───"), "{}", rows(&buf)[y]);
    let (buf, y, width, tees) = exit(Some(5), Some(0.449));
    assert_eq!(
        rows(&buf)[y].chars().take(16).collect::<String>(),
        "╰─ 5s · $0.449 ┬"
    );
    assert_eq!((width, tees), (26, vec![15]));
}

#[test]
fn a_need_line_leaves_a_box_past_the_time_and_the_cost() {
    let (buf, y, width, tees) = exit(Some(u64::MAX), Some(1234.0));
    let r = rows(&buf);
    assert_eq!(
        tees,
        [2 + 35],
        "the first `─` after the text\n{}",
        r.join("\n")
    );
    assert_eq!(
        chars(&r[y])[..38].concat(),
        "╰─ 5124095576030431h·0m·15s · $1.23k ┬",
        "{}",
        r.join("\n")
    );
    let below = chars(&r[y + 1]);
    assert_ne!(
        below[37],
        " ",
        "a line cell under the exit\n{}",
        r.join("\n")
    );
    let b_top = y
        + 1
        + r[y + 1..]
            .iter()
            .position(|l| l.starts_with('╭'))
            .expect("b's top border");
    let landed: Vec<usize> = chars(&r[b_top])
        .iter()
        .enumerate()
        .filter(|(_, c)| *c == "┴")
        .map(|(x, _)| x)
        .collect();
    assert_eq!(landed.len(), 1, "{}", r.join("\n"));
    // b's title is 30 cells, so b lands at 2 + 30 = 32, left of the exit (37): the line turns left
    assert_eq!(landed[0], 32, "{}", r.join("\n"));
    let b_bottom = b_top
        + r[b_top..]
            .iter()
            .position(|l| l.starts_with('╰'))
            .expect("b's bottom border");
    let boxes_: Vec<Rect> = vec![
        Rect::new(0, 0, width as u16, y as u16 + 1),
        Rect::new(
            0,
            b_top as u16,
            buf.area.width,
            (b_bottom - b_top) as u16 + 1,
        ),
    ];
    assert!(violations(&buf, &[], &boxes_, buf.area).is_empty());
}
